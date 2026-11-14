//! Proof-of-Play factory for NEAR. Replaces GameFactory.sol.
//!
//! `create_game` launches each game at its own sub-account, such as
//! `hop-7.factory.popnear.testnet`: one batch creates the account, funds its
//! storage with the attached NEAR, deploys the embedded game contract and calls
//! `new`. The callback records the game only if that batch succeeded, and
//! refunds the caller otherwise.
//!
//! The game account gets no access key, so nobody, the factory owner
//! included, can change a launched game. The owner may only change the
//! defaults (params and verifier key) used for future games.

#[cfg(test)]
mod tests;

use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError,
};

/// The game contract, built by `cargo near build` in `near/game` before this crate.
pub const GAME_WASM: &[u8] = include_bytes!("../../target/near/pop_game/pop_game.wasm");

/// Room for the game's own state beyond its code (events ring, leaderboard,
/// open sessions, player entries are paid by players). Bytes.
pub const GAME_STATE_MARGIN_BYTES: u64 = 30_000;
const GAS_FOR_NEW: Gas = Gas::from_tgas(40);
const GAS_FOR_CALLBACK: Gas = Gas::from_tgas(15);

#[derive(near_sdk::borsh::BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Games,
    Order,
    ByAccount,
}

/// Same fields and JSON as the game contract's `Params`; passed through to `new`.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub entry_fee: U128,
    pub vault_bps: u16,
    pub parent_bps: u16,
    pub launch_price: U128,
    pub target_score: u32,
    pub min_share_bps: u16,
    pub wallet_mining_cap: U128,
    pub mining_supply_cap: U128,
    pub mining_run_cap: u64,
    pub curve_base_price: U128,
    pub curve_slope: U128,
    pub floor_premium_bps: u16,
    pub milestones: [u32; 4],
    pub max_run_secs: u64,
}

impl Params {
    /// The Monad defaults, in NEAR (see the game contract).
    pub fn standard() -> Self {
        const MILLI: u128 = 1_000_000_000_000_000_000_000;
        const COIN: u128 = 1_000_000_000_000_000_000;
        Self {
            entry_fee: U128(20 * MILLI),
            vault_bps: 7_000,
            parent_bps: 1_000,
            launch_price: U128(MILLI / 10),
            target_score: 40,
            min_share_bps: 2_500,
            wallet_mining_cap: U128(500 * COIN),
            mining_supply_cap: U128(10_000 * COIN),
            mining_run_cap: 300,
            curve_base_price: U128(MILLI / 2),
            curve_slope: U128(MILLI / 10 / 1_000),
            floor_premium_bps: 1_000,
            milestones: [3, 10, 25, 50],
            max_run_secs: 12 * 60,
        }
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub struct GameRecord {
    pub id: u64,
    pub creator: AccountId,
    pub account: AccountId,
    /// 0 for an original game, else the id of the game it remixes.
    pub parent_id: u64,
    /// SHA-256 of the game config JSON (stored off-chain), 64 hex characters.
    pub config_hash: String,
    /// Verifier key the game was launched with (hex). Lets the app flag dev-verifier games.
    pub verifier_pk: String,
    pub name: String,
    pub symbol: String,
    pub created_at_ms: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Factory {
    owner: AccountId,
    verifier_pk: String,
    default_params: Params,
    /// Ids are reserved when `create_game` is called, so a failed launch leaves a gap.
    next_id: u64,
    games: LookupMap<u64, GameRecord>,
    /// Ids of launched games, oldest first.
    order: Vector<u64>,
    by_account: LookupMap<AccountId, u64>,
}

#[near]
impl Factory {
    #[init]
    pub fn new(owner: AccountId, verifier_pk: String, params: Option<Params>) -> Self {
        check_hex(&verifier_pk, 64, "verifier_pk");
        Self {
            owner,
            verifier_pk,
            default_params: params.unwrap_or_else(Params::standard),
            next_id: 1,
            games: LookupMap::new(StorageKey::Games),
            order: Vector::new(StorageKey::Order),
            by_account: LookupMap::new(StorageKey::ByAccount),
        }
    }

    /// Launches a game at `<symbol>-<id>.<this account>`. Attach at least
    /// `game_storage_cost`; all of it goes to the game account. Returns the
    /// reserved id; the game is listed once the launch succeeds.
    #[payable]
    pub fn create_game(
        &mut self,
        name: String,
        symbol: String,
        config_hash: String,
        parent_id: u64,
    ) -> u64 {
        check_hex(&config_hash, 64, "config_hash");
        let attached = env::attached_deposit();
        let need = self.game_storage_cost();
        require!(
            attached >= need,
            format!(
                "Attach at least {} yoctoNEAR ({} bytes of contract and state at {} yoctoNEAR per byte)",
                need.as_yoctonear(),
                GAME_WASM.len() as u64 + GAME_STATE_MARGIN_BYTES,
                env::storage_byte_cost().as_yoctonear()
            )
        );
        let parent = if parent_id == 0 {
            None
        } else {
            let p = self
                .games
                .get(&parent_id)
                .unwrap_or_else(|| env::panic_str("Unknown parent game"));
            Some(p.account.clone())
        };

        let id = self.next_id;
        self.next_id += 1;
        let account = game_account(&symbol, id, &env::current_account_id());
        let creator = env::predecessor_account_id();

        let args = json!({
            "name": name,
            "symbol": symbol,
            "creator": creator,
            "parent": parent,
            "verifier_pk": self.verifier_pk,
            "config_hash": config_hash,
            "params": self.default_params,
        });
        let record = GameRecord {
            id,
            creator: creator.clone(),
            account: account.clone(),
            parent_id,
            config_hash,
            verifier_pk: self.verifier_pk.clone(),
            name,
            symbol,
            created_at_ms: env::block_timestamp_ms(),
        };

        Promise::new(account)
            .create_account()
            .transfer(attached)
            .deploy_contract(GAME_WASM.to_vec())
            .function_call(
                "new".to_string(),
                serde_json::to_vec(&args).unwrap(),
                NearToken::from_yoctonear(0),
                GAS_FOR_NEW,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_CALLBACK)
                    .on_game_created(record, U128(attached.as_yoctonear())),
            )
            .detach();
        id
    }

    /// Records the game if the launch batch succeeded; refunds the creator otherwise.
    #[private]
    pub fn on_game_created(
        &mut self,
        record: GameRecord,
        deposit: U128,
    ) -> bool {
        // `new` returns nothing, so success carries an empty result.
        if let Err(PromiseError::Failed) = env::promise_result_checked(0, 0) {
            env::log_str(&format!("Launch of {} failed; deposit refunded", record.account));
            Promise::new(record.creator)
                .transfer(NearToken::from_yoctonear(deposit.0))
                .detach();
            return false;
        }
        self.by_account.insert(record.account.clone(), record.id);
        self.order.push(record.id);
        env::log_str(&format!(
            "EVENT_JSON:{}",
            json!({
                "standard": "proof_of_play",
                "version": "1.0.0",
                "event": "game_created",
                "data": [record],
            })
        ));
        self.games.insert(record.id, record);
        true
    }

    // ---------------------------------------------------------------- owner

    /// Changes the params future games launch with. Launched games are unaffected.
    pub fn set_default_params(&mut self, params: Params) {
        self.assert_owner();
        self.default_params = params;
    }

    /// Changes the verifier key future games launch with. Launched games are unaffected.
    pub fn set_verifier_pk(&mut self, verifier_pk: String) {
        self.assert_owner();
        check_hex(&verifier_pk, 64, "verifier_pk");
        self.verifier_pk = verifier_pk;
    }

    // ---------------------------------------------------------------- views

    /// yoctoNEAR `create_game` needs: the game's code plus a state margin, at the storage price.
    pub fn game_storage_cost(&self) -> NearToken {
        env::storage_byte_cost()
            .saturating_mul((GAME_WASM.len() as u64 + GAME_STATE_MARGIN_BYTES) as u128)
    }

    pub fn game_count(&self) -> u64 {
        self.order.len() as u64
    }

    pub fn get_game(&self, id: u64) -> Option<GameRecord> {
        self.games.get(&id).cloned()
    }

    /// Newest first: `limit` games starting `offset` from the newest.
    pub fn list_games(&self, offset: u64, limit: u64) -> Vec<GameRecord> {
        let n = self.order.len() as u64;
        (offset..n.min(offset.saturating_add(limit)))
            .map(|i| {
                let id = self.order[(n - 1 - i) as u32];
                self.games[&id].clone()
            })
            .collect()
    }

    pub fn game_id_by_account(&self, account_id: AccountId) -> Option<u64> {
        self.by_account.get(&account_id).copied()
    }

    pub fn get_owner(&self) -> AccountId {
        self.owner.clone()
    }

    pub fn get_verifier_pk(&self) -> String {
        self.verifier_pk.clone()
    }

    pub fn get_default_params(&self) -> Params {
        self.default_params.clone()
    }

    /// SHA-256 of the embedded game contract, hex.
    pub fn game_code_hash(&self) -> String {
        to_hex(&env::sha256(GAME_WASM))
    }
}

impl Factory {
    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner,
            "Only the owner can change defaults"
        );
    }
}

/// `<symbol, lowercase letters and digits only, at most 16>-<id>.<factory>`.
pub fn game_account(symbol: &str, id: u64, factory: &AccountId) -> AccountId {
    let prefix: String = symbol
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(16)
        .collect();
    let prefix = if prefix.is_empty() { "game".to_string() } else { prefix };
    format!("{prefix}-{id}.{factory}")
        .parse()
        .unwrap_or_else(|_| env::panic_str("The game account name would be too long"))
}

fn check_hex(s: &str, len: usize, what: &str) {
    require!(
        s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()),
        format!("{what} must be {len} hex characters")
    );
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
