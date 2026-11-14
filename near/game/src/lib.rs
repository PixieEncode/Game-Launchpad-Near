//! Proof-of-Play game contract for NEAR.
//!
//! One contract per game, deployed by the factory at its own sub-account. It
//! is the game's NEP-141 coin, the vault that backs it, the paid run sessions,
//! play-mining and the buy-only bonding curve, all in one. It has no owner and
//! no admin methods: everything is fixed at `new`.
//!
//! The rule that must never break: no action lowers vault ÷ supply.
//! - A run's vault share is held as `pending` until the run ends. At `end_run`
//!   the coins are priced at the floor *before* the deposit lands, then the
//!   deposit is added, so the floor never drops at either step.
//! - Buys cost at least floor + 10% and all of it goes to the vault.
//! - Redeems pay vault × amount ÷ supply, rounded down.
//! - Creator earnings are escrowed separately and never touch the vault.
//!
//! `vault` is a tracked number, never the account balance: the balance also
//! holds storage, prepaid run credits, open runs and the creator's escrow.

// `new` takes the seven launch fields the factory passes; the generated wrapper adds one more.
#![allow(clippy::too_many_arguments)]

mod events;
mod math;
pub mod score;

#[cfg(test)]
mod tests;

use near_contract_standards::fungible_token::events::{FtBurn, FtMint};
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::{
    FungibleToken, FungibleTokenCore, FungibleTokenResolver,
};
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::borsh::BorshSerialize;
use near_sdk::json_types::{U128, U64};
use near_sdk::store::{Lazy, LookupMap, Vector};
use near_sdk::{
    assert_one_yocto, env, near, require, AccountId, BorshStorageKey, CurveType, Gas, NearToken,
    PanicOnDefault, Promise, PromiseError, PromiseOrValue, PublicKey,
};

pub use events::{EventKind, GameEvent};
use events::{log_event, RING_SIZE};
use math::{mul_div_down, u, BPS, ONE_COIN};

const NEAR: u128 = 1_000_000_000_000_000_000_000_000;
const MILLI_NEAR: u128 = NEAR / 1_000;
const LEADERBOARD_SIZE: usize = 20;
const MAX_RUNS_PER_PURCHASE: u32 = 100;
const GAS_FOR_PARENT_FUND: Gas = Gas::from_tgas(5);
const GAS_FOR_PARENT_CALLBACK: Gas = Gas::from_tgas(5);

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Accounts,
    Sessions,
    Players,
    Leaderboard,
    Events,
}

/// Fixed at creation and readable by anyone through `get_params`.
/// Amounts are yoctoNEAR; coin amounts are coin units (18 decimals).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// yoctoNEAR per run.
    pub entry_fee: U128,
    /// Share of each fee that backs the coin.
    pub vault_bps: u16,
    /// Share of each fee sent to the parent game's vault (remixes only, taken from the creator's share).
    pub parent_bps: u16,
    /// yoctoNEAR per whole coin used to price mining while the floor is lower (or there is no supply).
    pub launch_price: U128,
    /// Score that earns a run's full mining budget.
    pub target_score: u32,
    /// Share of the budget a score of 0 still earns.
    pub min_share_bps: u16,
    /// Most coin units one account can mine.
    pub wallet_mining_cap: U128,
    /// Mining closes once this many coin units are mined...
    pub mining_supply_cap: U128,
    /// ...or after this many finished runs.
    pub mining_run_cap: u64,
    /// Curve price at zero sold, yoctoNEAR per whole coin.
    pub curve_base_price: U128,
    /// yoctoNEAR added to the curve price per whole coin sold.
    pub curve_slope: U128,
    /// Buyers pay at least floor × (1 + this).
    pub floor_premium_bps: u16,
    /// Unique-player counts that each unlock 25% of the creator's escrow.
    pub milestones: [u32; 4],
    /// After this long, anyone may expire an unfinished run; its deposit then joins the vault.
    pub max_run_secs: u64,
}

impl Params {
    /// The Monad defaults, in NEAR.
    pub fn standard() -> Self {
        Self {
            entry_fee: U128(20 * MILLI_NEAR),       // 0.02 NEAR
            vault_bps: 7_000,                       // 70%
            parent_bps: 1_000,                      // 10%, from the creator's 30%
            launch_price: U128(MILLI_NEAR / 10),    // 0.0001 NEAR per coin
            target_score: 40,
            min_share_bps: 2_500,                   // 25% at score 0
            wallet_mining_cap: U128(500 * ONE_COIN),
            mining_supply_cap: U128(10_000 * ONE_COIN),
            mining_run_cap: 300,
            curve_base_price: U128(MILLI_NEAR / 2), // 0.0005 NEAR per coin
            curve_slope: U128(MILLI_NEAR / 10 / 1_000), // +0.0001 NEAR per 1,000 coins sold
            floor_premium_bps: 1_000,               // floor + 10%
            milestones: [3, 10, 25, 50],
            max_run_secs: 12 * 60, // 10-minute cap on a run plus a 2-minute grace
        }
    }

    fn validate(&self) {
        require!(self.entry_fee.0 > 0, "entry_fee must be positive");
        require!(
            self.vault_bps as u32 + self.parent_bps as u32 <= BPS as u32,
            "vault_bps + parent_bps must not exceed 10000"
        );
        require!(self.launch_price.0 > 0, "launch_price must be positive");
        require!(self.target_score > 0, "target_score must be positive");
        require!(self.min_share_bps as u128 <= BPS, "min_share_bps must not exceed 10000");
        require!(self.mining_run_cap > 0, "mining_run_cap must be positive");
        require!(
            self.milestones.windows(2).all(|w| w[0] <= w[1]),
            "milestones must be ascending"
        );
        require!(self.max_run_secs > 0, "max_run_secs must be positive");
    }
}

/// An open run. Removed when it ends or expires, so storage does not grow with
/// every run; its result lives on in the events and the leaderboard.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub player: AccountId,
    pub started_at_ms: u64,
    pub seed: u32,
    /// yoctoNEAR of the fee that joins the vault when the run ends.
    pub deposit: U128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    /// Coin units this account has mined.
    pub mined: U128,
    /// Prepaid runs.
    pub credits: u32,
    /// Has finished at least one run.
    pub played: bool,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct LeaderEntry {
    pub player: AccountId,
    pub score: u32,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct StartedRun {
    pub session_id: u64,
    pub seed: u32,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub name: String,
    pub symbol: String,
    pub factory: AccountId,
    pub creator: AccountId,
    pub parent: Option<AccountId>,
    pub config_hash: String,
    pub verifier_pk: PublicKey,
    pub created_at_ms: u64,
    pub vault: U128,
    pub pending: U128,
    pub supply: U128,
    pub floor_price: U128,
    pub spot_price: U128,
    pub total_mined: U128,
    pub sold: U128,
    pub mining_closed: bool,
    pub session_count: u64,
    pub open_sessions: u64,
    pub finished_runs: u64,
    pub unique_players: u32,
    pub best_score: u32,
    pub best_player: Option<AccountId>,
    pub creator_earned: U128,
    pub creator_claimed: U128,
    pub creator_unlocked: U128,
    pub credits_held: U128,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Game {
    token: FungibleToken,
    name: String,
    symbol: String,
    factory: AccountId,
    creator: AccountId,
    parent: Option<AccountId>,
    verifier_pk: [u8; 32],
    config_hash: [u8; 32],
    params: Params,
    created_at_ms: u64,

    /// yoctoNEAR backing the coin. Floor = vault ÷ supply.
    vault: u128,
    /// Vault shares of runs that have started but not ended.
    pending: u128,
    /// Fees held for prepaid runs that have not started.
    credits_held: u128,
    creator_earned: u128,
    creator_claimed: u128,

    session_count: u64,
    open_sessions: u64,
    sessions: LookupMap<u64, Session>,
    players: LookupMap<AccountId, Player>,
    /// Bytes one player's records take (coin balance + player entry).
    registration_bytes: u64,
    unique_players: u32,
    total_mined: u128,
    finished_runs: u64,
    mining_closed: bool,
    /// Coin units minted through the curve.
    sold: u128,
    best_score: u32,
    best_player: Option<AccountId>,
    leaderboard: Lazy<Vec<LeaderEntry>>,
    events: Vector<GameEvent>,
    event_count: u64,
}

#[near]
impl Game {
    /// Called once by the factory in the same batch that deploys the contract.
    #[init]
    pub fn new(
        name: String,
        symbol: String,
        creator: AccountId,
        parent: Option<AccountId>,
        verifier_pk: PublicKey,
        config_hash: String,
        params: Params,
    ) -> Self {
        require!(!name.is_empty() && name.len() <= 64, "Name must be 1-64 characters");
        require!(!symbol.is_empty() && symbol.len() <= 12, "Symbol must be 1-12 characters");
        require!(
            verifier_pk.curve_type() == CurveType::ED25519,
            "Verifier key must be ed25519"
        );
        params.validate();
        let mut params = params;
        if parent.is_none() {
            params.parent_bps = 0;
        }
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&verifier_pk.as_bytes()[1..]);

        let mut this = Self {
            token: FungibleToken::new(StorageKey::Accounts),
            name,
            symbol,
            factory: env::predecessor_account_id(),
            creator,
            parent,
            verifier_pk: pk,
            config_hash: score::hex32(&config_hash),
            params,
            created_at_ms: env::block_timestamp_ms(),
            vault: 0,
            pending: 0,
            credits_held: 0,
            creator_earned: 0,
            creator_claimed: 0,
            session_count: 0,
            open_sessions: 0,
            sessions: LookupMap::new(StorageKey::Sessions),
            players: LookupMap::new(StorageKey::Players),
            registration_bytes: 0,
            unique_players: 0,
            total_mined: 0,
            finished_runs: 0,
            mining_closed: false,
            sold: 0,
            best_score: 0,
            best_player: None,
            leaderboard: Lazy::new(StorageKey::Leaderboard, Vec::new()),
            events: Vector::new(StorageKey::Events),
            event_count: 0,
        };
        this.measure_registration();
        this
    }

    // ------------------------------------------------------------------ runs

    /// Prepays `count` runs for the caller, or for `account_id`. Attach
    /// count × entry fee, plus `registration_cost` if that account is new.
    /// Any excess is refunded. Returns the account's credits.
    #[payable]
    pub fn buy_runs(&mut self, count: u32, account_id: Option<AccountId>) -> u32 {
        require!(
            count > 0 && count <= MAX_RUNS_PER_PURCHASE,
            "Buy between 1 and 100 runs"
        );
        let who = account_id.unwrap_or_else(env::predecessor_account_id);
        let fees = self.params.entry_fee.0 * count as u128;
        let need = fees + self.registration_needed(&who);
        let attached = env::attached_deposit().as_yoctonear();
        require!(
            attached >= need,
            format!("Attach {} yoctoNEAR for {} runs", need, count)
        );
        self.ensure_registered(&who);
        let mut player = self.players.get(&who).cloned().unwrap_or_default();
        player.credits += count;
        let credits = player.credits;
        self.players.insert(who, player);
        self.credits_held += fees;
        refund(attached - need);
        credits
    }

    /// Starts a run, paid by the attached fee (plus `registration_cost` for a
    /// new account) or else by one prepaid credit. The fee is split now; the
    /// vault share is held until the run ends. Returns the session id and the
    /// seed the game must be played with.
    #[payable]
    pub fn start_run(&mut self) -> StartedRun {
        let player = env::predecessor_account_id();
        let fee = self.params.entry_fee.0;
        let attached = env::attached_deposit().as_yoctonear();
        if attached > 0 {
            let need = fee + self.registration_needed(&player);
            require!(
                attached >= need,
                format!("Attach {} yoctoNEAR to start a run", need)
            );
            self.ensure_registered(&player);
            refund(attached - need);
        } else {
            let mut p = self
                .players
                .get(&player)
                .cloned()
                .unwrap_or_default();
            require!(
                p.credits > 0,
                "No run credits: attach the entry fee or call buy_runs first"
            );
            p.credits -= 1;
            self.players.insert(player.clone(), p);
            self.credits_held -= fee;
        }

        let to_vault = mul_div_down(fee, self.params.vault_bps as u128, BPS);
        let to_parent = mul_div_down(fee, self.params.parent_bps as u128, BPS);
        self.creator_earned += fee - to_vault - to_parent;
        self.pending += to_vault;

        self.session_count += 1;
        let session_id = self.session_count;
        let seed = draw_seed(session_id);
        self.sessions.insert(
            session_id,
            Session {
                player: player.clone(),
                started_at_ms: env::block_timestamp_ms(),
                seed,
                deposit: U128(to_vault),
            },
        );
        self.open_sessions += 1;

        if to_parent > 0 {
            if let Some(parent) = self.parent.clone() {
                Promise::new(parent)
                    .function_call(
                        "fund".to_string(),
                        Vec::new(),
                        NearToken::from_yoctonear(to_parent),
                        GAS_FOR_PARENT_FUND,
                    )
                    .then(
                        Self::ext(env::current_account_id())
                            .with_static_gas(GAS_FOR_PARENT_CALLBACK)
                            .on_parent_funded(U128(to_parent)),
                    )
                    .detach();
            }
        }

        self.record(EventKind::RunStart, &player, session_id, 0, 0, fee);
        StartedRun { session_id, seed }
    }

    /// Ends the caller's run with a score signed by the verifier, then mines.
    /// The signature covers this game, the player, the session, its seed, the
    /// game's config hash, the score and the input log's hash (see `score`).
    pub fn end_run(
        &mut self,
        session_id: u64,
        score: u32,
        log_hash: String,
        signature: String,
    ) -> U128 {
        let player = env::predecessor_account_id();
        let session = self
            .sessions
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("Unknown or finished session"));
        require!(session.player == player, "Not your session");

        let digest = score::score_digest(
            &env::current_account_id(),
            &player,
            session_id,
            session.seed,
            &self.config_hash,
            score,
            &score::hex32(&log_hash),
        );
        let sig = score::hex64(&signature);
        require!(
            env::ed25519_verify(&sig, digest, &self.verifier_pk),
            "Bad signature"
        );

        self.sessions.remove(&session_id);
        self.open_sessions -= 1;
        let deposit = session.deposit.0;
        self.pending -= deposit;
        self.finished_runs += 1;

        let mut p = self.players.get(&player).cloned().unwrap_or_default();
        if !p.played {
            p.played = true;
            self.unique_players += 1;
        }
        if score > self.best_score || self.best_player.is_none() {
            self.best_score = score;
            self.best_player = Some(player.clone());
        }
        self.update_leaderboard(&player, score);

        // Price the coins at the floor before this deposit lands, then add it.
        let mut mined = 0;
        if !self.mining_closed {
            mined = self.preview_mined_raw(deposit, score);
            mined = mined.min(self.params.wallet_mining_cap.0.saturating_sub(p.mined.0));
            mined = mined.min(self.params.mining_supply_cap.0.saturating_sub(self.total_mined));
        }
        self.vault += deposit;
        if mined > 0 {
            p.mined = U128(p.mined.0 + mined);
            self.total_mined += mined;
            self.mint(&player, mined, "mined");
        }
        self.players.insert(player.clone(), p);

        self.record(EventKind::RunEnd, &player, session_id, score, mined, deposit);
        if !self.mining_closed
            && (self.total_mined >= self.params.mining_supply_cap.0
                || self.finished_runs >= self.params.mining_run_cap)
        {
            self.mining_closed = true;
            let creator = self.creator.clone();
            self.record(EventKind::MiningClosed, &creator, 0, 0, self.total_mined, 0);
        }
        U128(mined)
    }

    /// Anyone may close a run that has passed `max_run_secs`. It mines
    /// nothing and its deposit joins the vault, which raises the floor.
    pub fn expire_run(&mut self, session_id: u64) {
        let session = self
            .sessions
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("Unknown or finished session"));
        let deadline = session.started_at_ms + self.params.max_run_secs * 1_000;
        require!(
            env::block_timestamp_ms() >= deadline,
            "The run can still be finished"
        );
        self.sessions.remove(&session_id);
        self.open_sessions -= 1;
        self.pending -= session.deposit.0;
        self.vault += session.deposit.0;
        self.record(
            EventKind::RunExpired,
            &session.player,
            session_id,
            0,
            0,
            session.deposit.0,
        );
    }

    // ---------------------------------------------------------------- market

    /// Burns `amount` of the caller's coins and pays vault × amount ÷ supply.
    /// Attach exactly 1 yoctoNEAR (it joins the vault), so a limited access
    /// key cannot redeem.
    #[payable]
    pub fn redeem(&mut self, amount: U128) -> U128 {
        assert_one_yocto();
        let amount = amount.0;
        require!(amount > 0, "Amount must be positive");
        let supply = self.token.total_supply;
        require!(supply > 0, "No coins exist");
        let holder = env::predecessor_account_id();
        let payout = mul_div_down(self.vault, amount, supply);
        self.token.internal_withdraw(&holder, amount);
        FtBurn {
            owner_id: &holder,
            amount: U128(amount),
            memo: Some("redeem"),
        }
        .emit();
        self.vault = self.vault - payout + 1;
        self.record(EventKind::Redeem, &holder, 0, 0, amount, payout);
        if payout > 0 {
            Promise::new(holder)
                .transfer(NearToken::from_yoctonear(payout))
                .detach();
        }
        U128(payout)
    }

    /// Buys `amount` coin units on the curve. Only after mining closes. All of
    /// the cost goes to the vault; excess is refunded. Attach `quote_buy`
    /// (plus `registration_cost` for a new account). Returns the cost.
    #[payable]
    pub fn buy(&mut self, amount: U128) -> U128 {
        require!(self.mining_closed, "Buying opens when mining closes");
        let amount = amount.0;
        require!(amount > 0, "Amount must be positive");
        let buyer = env::predecessor_account_id();
        let cost = self.quote_buy_raw(amount);
        let need = cost + self.registration_needed(&buyer);
        let attached = env::attached_deposit().as_yoctonear();
        require!(
            attached >= need,
            format!("Attach at least {} yoctoNEAR", need)
        );
        self.ensure_registered(&buyer);
        self.sold += amount;
        self.vault += cost;
        self.mint(&buyer, amount, "bought");
        self.record(EventKind::Buy, &buyer, 0, 0, amount, cost);
        refund(attached - need);
        U128(cost)
    }

    /// Adds the attached NEAR to the vault. Remixes call this to pay the parent.
    #[payable]
    pub fn fund(&mut self) {
        let amount = env::attached_deposit().as_yoctonear();
        require!(amount > 0, "Attach NEAR to fund the vault");
        self.vault += amount;
        self.record(EventKind::Fund, &env::predecessor_account_id(), 0, 0, 0, amount);
    }

    /// If the parent's `fund` failed, its share comes back here: keep it in this vault.
    #[private]
    pub fn on_parent_funded(&mut self, amount: U128) {
        if let Err(PromiseError::Failed) = env::promise_result_checked(0, 0) {
            self.vault += amount.0;
            env::log_str("Parent fund failed; share added to this vault");
        }
    }

    // --------------------------------------------------------------- creator

    /// Pays the creator the unlocked part of their escrow.
    pub fn claim_creator(&mut self) -> U128 {
        require!(
            env::predecessor_account_id() == self.creator,
            "Only the creator can claim"
        );
        let unlocked = self.creator_unlocked_raw();
        require!(unlocked > self.creator_claimed, "Nothing to claim yet");
        let amount = unlocked - self.creator_claimed;
        self.creator_claimed = unlocked;
        let creator = self.creator.clone();
        self.record(EventKind::CreatorClaim, &creator, 0, 0, 0, amount);
        Promise::new(creator)
            .transfer(NearToken::from_yoctonear(amount))
            .detach();
        U128(amount)
    }

    // ----------------------------------------------------------------- views

    /// yoctoNEAR per whole coin, rounded down. 0 while there is no supply.
    pub fn floor_price(&self) -> U128 {
        U128(self.floor_raw())
    }

    /// Price of the next whole coin on the curve: curve price, never below floor + premium.
    pub fn spot_price(&self) -> U128 {
        let curve = self.params.curve_base_price.0
            + mul_div_down(self.params.curve_slope.0, self.sold, ONE_COIN);
        let floor = math::mul_div_up(
            self.floor_raw(),
            BPS + self.params.floor_premium_bps as u128,
            BPS,
        );
        U128(curve.max(floor))
    }

    /// yoctoNEAR that buying `amount` coin units costs right now.
    pub fn quote_buy(&self, amount: U128) -> U128 {
        U128(self.quote_buy_raw(amount.0))
    }

    /// Coin units a run with this vault deposit and score would mine now, before caps.
    pub fn preview_mined(&self, deposit: U128, score: u32) -> U128 {
        U128(self.preview_mined_raw(deposit.0, score))
    }

    pub fn get_session(&self, session_id: u64) -> Option<Session> {
        self.sessions.get(&session_id).cloned()
    }

    pub fn get_player(&self, account_id: AccountId) -> Option<Player> {
        self.players.get(&account_id).cloned()
    }

    /// yoctoNEAR a new account adds to its first payment for its token storage.
    pub fn registration_cost(&self) -> U128 {
        U128(self.registration_cost_raw())
    }

    /// yoctoNEAR to attach to `start_run` for this account.
    pub fn run_cost(&self, account_id: AccountId) -> U128 {
        U128(self.params.entry_fee.0 + self.registration_needed(&account_id))
    }

    pub fn creator_unlocked(&self) -> U128 {
        U128(self.creator_unlocked_raw())
    }

    pub fn get_params(&self) -> Params {
        self.params.clone()
    }

    pub fn get_stats(&self) -> Stats {
        Stats {
            name: self.name.clone(),
            symbol: self.symbol.clone(),
            factory: self.factory.clone(),
            creator: self.creator.clone(),
            parent: self.parent.clone(),
            config_hash: score::to_hex(&self.config_hash),
            verifier_pk: PublicKey::from_parts(CurveType::ED25519, self.verifier_pk.to_vec())
                .unwrap(),
            created_at_ms: self.created_at_ms,
            vault: U128(self.vault),
            pending: U128(self.pending),
            supply: U128(self.token.total_supply),
            floor_price: self.floor_price(),
            spot_price: self.spot_price(),
            total_mined: U128(self.total_mined),
            sold: U128(self.sold),
            mining_closed: self.mining_closed,
            session_count: self.session_count,
            open_sessions: self.open_sessions,
            finished_runs: self.finished_runs,
            unique_players: self.unique_players,
            best_score: self.best_score,
            best_player: self.best_player.clone(),
            creator_earned: U128(self.creator_earned),
            creator_claimed: U128(self.creator_claimed),
            creator_unlocked: self.creator_unlocked(),
            credits_held: U128(self.credits_held),
        }
    }

    /// Best score per account, highest first, at most 20.
    pub fn leaderboard(&self) -> Vec<LeaderEntry> {
        self.leaderboard.get().clone()
    }

    /// The last events, newest first (at most 50).
    pub fn recent_events(&self, limit: Option<u32>) -> Vec<GameEvent> {
        let len = self.events.len();
        let n = limit.unwrap_or(RING_SIZE).min(len);
        (0..n as u64)
            .map(|i| {
                let idx = ((self.event_count - 1 - i) % RING_SIZE as u64) as u32;
                self.events[idx].clone()
            })
            .collect()
    }
}

// --------------------------------------------------------------- internals

impl Game {
    fn floor_raw(&self) -> u128 {
        let supply = self.token.total_supply;
        if supply == 0 {
            0
        } else {
            mul_div_down(self.vault, ONE_COIN, supply)
        }
    }

    /// Budget = deposit ÷ max(floor, launch price), share = 25% + 75% × min(score, target) ÷ target.
    /// Taken against the exact vault ÷ supply, rounded down, so minting it can never lower the floor.
    fn preview_mined_raw(&self, deposit: u128, score: u32) -> u128 {
        let p = &self.params;
        let supply = self.token.total_supply;
        let launch = p.launch_price.0;
        let below_launch = supply == 0 || u(self.vault) * u(ONE_COIN) < u(launch) * u(supply);
        let budget = if below_launch {
            mul_div_down(deposit, ONE_COIN, launch)
        } else {
            mul_div_down(deposit, supply, self.vault)
        };
        let skill = score.min(p.target_score) as u128;
        let min_share = p.min_share_bps as u128;
        let share = min_share + (BPS - min_share) * skill / p.target_score as u128;
        mul_div_down(budget, share, BPS)
    }

    /// The larger of the curve cost and floor × amount × (1 + premium), plus 1 yoctoNEAR.
    fn quote_buy_raw(&self, amount: u128) -> u128 {
        let p = &self.params;
        let s = u(self.sold);
        let a = u(amount);
        let one = u(ONE_COIN);
        let curve = u(p.curve_base_price.0) * a / one
            + u(p.curve_slope.0) * ((s + a) * (s + a) - s * s) / (one * one * u(2));
        let supply = self.token.total_supply;
        let floor_cost = if supply == 0 {
            math::U256::zero()
        } else {
            let n = u(self.vault) * a * u(BPS + p.floor_premium_bps as u128);
            let d = u(supply) * u(BPS);
            (n + d - math::U256::one()) / d
        };
        math::narrow(curve.max(floor_cost)) + 1
    }

    fn creator_unlocked_raw(&self) -> u128 {
        let reached = self
            .params
            .milestones
            .iter()
            .filter(|m| self.unique_players >= **m)
            .count() as u128;
        mul_div_down(self.creator_earned, reached, 4)
    }

    fn registration_cost_raw(&self) -> u128 {
        self.registration_bytes as u128 * env::storage_byte_cost().as_yoctonear()
    }

    fn registration_needed(&self, account: &AccountId) -> u128 {
        if self.players.contains_key(account) {
            0
        } else {
            self.registration_cost_raw()
        }
    }

    fn ensure_registered(&mut self, account: &AccountId) {
        if !self.players.contains_key(account) {
            if !self.token.accounts.contains_key(account) {
                self.token.internal_register_account(account);
            }
            self.players.insert(account.clone(), Player::default());
        }
    }

    /// Measures the storage of one account's coin balance and player entry,
    /// using the longest possible account id.
    fn measure_registration(&mut self) {
        let before = env::storage_usage();
        let tmp: AccountId = "a".repeat(64).parse().unwrap();
        self.players.insert(
            tmp.clone(),
            Player {
                mined: U128(u128::MAX),
                credits: u32::MAX,
                played: true,
            },
        );
        self.players.flush();
        let player_bytes = env::storage_usage() - before;
        self.players.remove(&tmp);
        self.players.flush();
        self.registration_bytes = self.token.account_storage_usage + player_bytes;
    }

    fn mint(&mut self, to: &AccountId, amount: u128, memo: &str) {
        self.token.internal_deposit(to, amount);
        FtMint {
            owner_id: to,
            amount: U128(amount),
            memo: Some(memo),
        }
        .emit();
    }

    fn update_leaderboard(&mut self, player: &AccountId, score: u32) {
        let board = self.leaderboard.get_mut();
        if let Some(i) = board.iter().position(|e| &e.player == player) {
            if board[i].score >= score {
                return;
            }
            board.remove(i);
        }
        // After every entry with an equal or higher score: earlier scores keep their place.
        let at = board.iter().position(|e| e.score < score).unwrap_or(board.len());
        if at < LEADERBOARD_SIZE {
            board.insert(
                at,
                LeaderEntry {
                    player: player.clone(),
                    score,
                },
            );
            board.truncate(LEADERBOARD_SIZE);
        }
    }

    fn record(
        &mut self,
        kind: EventKind,
        account: &AccountId,
        session_id: u64,
        score: u32,
        coins: u128,
        near: u128,
    ) {
        let e = GameEvent {
            kind,
            account: account.clone(),
            session_id: U64(session_id),
            score,
            coins: U128(coins),
            near: U128(near),
            floor: U128(self.floor_raw()),
            timestamp_ms: U64(env::block_timestamp_ms()),
        };
        log_event(&e);
        if self.events.len() < RING_SIZE {
            self.events.push(e);
        } else {
            let idx = (self.event_count % RING_SIZE as u64) as u32;
            self.events.set(idx, e);
        }
        self.event_count += 1;
    }
}

fn refund(amount: u128) {
    if amount > 0 {
        Promise::new(env::predecessor_account_id())
            .transfer(NearToken::from_yoctonear(amount))
            .detach();
    }
}

/// A 32-bit seed for the run's obstacle pattern, from the block's random seed and the session id.
fn draw_seed(session_id: u64) -> u32 {
    let mut buf = env::random_seed();
    buf.extend_from_slice(&session_id.to_le_bytes());
    let h = env::sha256(&buf);
    u32::from_le_bytes([h[0], h[1], h[2], h[3]])
}

// ------------------------------------------------------------- NEP-141

#[near]
impl FungibleTokenCore for Game {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.token.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        self.token.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for Game {
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let (used, burned) =
            self.token
                .internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned > 0 {
            // Supply falls with the vault unchanged: the floor rises.
            FtBurn {
                owner_id: &sender_id,
                amount: U128(burned),
                memo: Some("refund to unregistered sender"),
            }
            .emit();
        }
        used.into()
    }
}

#[near]
impl FungibleTokenMetadataProvider for Game {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: self.name.clone(),
            symbol: self.symbol.clone(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 18,
        }
    }
}

// ------------------------------------------------------------- NEP-145
// Registration costs a fixed amount (`registration_cost`) and is never
// withdrawn: a player's entry also holds their mining record, and removing it
// would reset their per-wallet mining cap.

#[near]
impl StorageManagement for Game {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let _ = registration_only;
        let account = account_id.unwrap_or_else(env::predecessor_account_id);
        let attached = env::attached_deposit().as_yoctonear();
        let need = self.registration_needed(&account);
        require!(
            attached >= need,
            "The attached deposit is less than the minimum storage balance"
        );
        self.ensure_registered(&account);
        refund(attached - need);
        self.storage_balance_of(account).unwrap()
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        assert_one_yocto();
        let account = env::predecessor_account_id();
        let balance = self
            .storage_balance_of(account)
            .unwrap_or_else(|| env::panic_str("The account is not registered"));
        if let Some(a) = amount {
            require!(a.is_zero(), "Nothing to withdraw: storage holds no excess");
        }
        balance
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        let _ = force;
        assert_one_yocto();
        env::panic_str("Unregistering is disabled: the entry holds this account's mining record")
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        let cost = NearToken::from_yoctonear(self.registration_cost_raw());
        StorageBalanceBounds {
            min: cost,
            max: Some(cost),
        }
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        if self.players.contains_key(&account_id) {
            Some(StorageBalance {
                total: NearToken::from_yoctonear(self.registration_cost_raw()),
                available: NearToken::from_yoctonear(0),
            })
        } else {
            None
        }
    }
}
