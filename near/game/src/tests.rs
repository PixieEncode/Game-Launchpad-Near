//! Unit tests. The first seven port contracts/test/ProofOfPlay.test.ts from the
//! Monad version; the rest cover each method. Every helper that changes state
//! asserts afterwards that vault ÷ supply did not fall.

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::testing_env;
use std::panic::{catch_unwind, AssertUnwindSafe};

const GAME: &str = "hop-1.factory.testnet";
const FACTORY: &str = "factory.testnet";
const CREATOR: &str = "creator.testnet";
const PARENT: &str = "hop-0.factory.testnet";
const CONFIG_HASH: &str = "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00";
const LOG_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn milli(x: u128) -> u128 {
    x * MILLI_NEAR
}

fn coins(x: u128) -> u128 {
    x * ONE_COIN
}

/// Runs `f`, expects it to panic, and returns the panic message.
fn fails<R>(f: impl FnOnce() -> R) -> String {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(prev);
    match r {
        Ok(_) => panic!("expected a panic"),
        Err(e) => e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default(),
    }
}

struct T {
    game: Game,
    key: SigningKey,
    now_ms: u64,
    nonce: u8,
    /// Last seen vault ÷ supply as a fraction (vault, supply), checked after every action.
    last: (u128, u128),
}

impl T {
    fn new() -> T {
        T::with(Params::standard(), None)
    }

    fn with(params: Params, parent: Option<&str>) -> T {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let pk = PublicKey::from_parts(CurveType::ED25519, key.verifying_key().to_bytes().to_vec())
            .unwrap();
        let now_ms = 1_800_000_000_000;
        testing_env!(VMContextBuilder::new()
            .current_account_id(acc(GAME))
            .predecessor_account_id(acc(FACTORY))
            .block_timestamp(now_ms * 1_000_000)
            .build());
        let game = Game::new(
            "Neon Hopper".into(),
            "HOP".into(),
            acc(CREATOR),
            parent.map(acc),
            pk,
            CONFIG_HASH.into(),
            params,
        );
        T {
            game,
            key,
            now_ms,
            nonce: 0,
            last: (0, 0),
        }
    }

    fn ctx(&mut self, who: &str, deposit: u128) {
        self.nonce = self.nonce.wrapping_add(1);
        let mut seed = [0u8; 32];
        seed[0] = self.nonce;
        let c = VMContextBuilder::new()
            .current_account_id(acc(GAME))
            .predecessor_account_id(acc(who))
            .signer_account_id(acc(who))
            .attached_deposit(NearToken::from_yoctonear(deposit))
            .account_balance(NearToken::from_near(1_000))
            .block_timestamp(self.now_ms * 1_000_000)
            .random_seed(seed)
            .build();
        testing_env!(c);
    }

    fn advance(&mut self, secs: u64) {
        self.now_ms += secs * 1_000;
    }

    /// Asserts vault ÷ supply did not fall since the last check (skipped while supply is 0).
    fn check_floor(&mut self, what: &str) {
        let now = (self.game.vault, self.game.token.total_supply);
        let (v0, s0) = self.last;
        if s0 > 0 && now.1 > 0 {
            // v1/s1 >= v0/s0  <=>  v1*s0 >= v0*s1
            assert!(
                u(now.0) * u(s0) >= u(v0) * u(now.1),
                "{what}: floor fell from {v0}/{s0} to {}/{}",
                now.0,
                now.1
            );
        }
        self.last = now;
    }

    fn sign_for(&self, game: &str, player: &str, session_id: u64, seed: u32, score: u32) -> String {
        let digest = score::score_digest(
            &acc(game),
            &acc(player),
            session_id,
            seed,
            &score::hex32(CONFIG_HASH),
            score,
            &score::hex32(LOG_HASH),
        );
        score::to_hex(&self.key.sign(&digest).to_bytes())
    }

    fn sign(&self, player: &str, run: &StartedRun, score: u32) -> String {
        self.sign_for(GAME, player, run.session_id, run.seed, score)
    }

    fn start(&mut self, who: &str) -> StartedRun {
        let cost = self.game.run_cost(acc(who)).0;
        self.ctx(who, cost);
        let r = self.game.start_run();
        self.check_floor("start_run");
        r
    }

    fn start_with_credit(&mut self, who: &str) -> StartedRun {
        self.ctx(who, 0);
        let r = self.game.start_run();
        self.check_floor("start_run (credit)");
        r
    }

    fn end(&mut self, who: &str, run: &StartedRun, score: u32) -> u128 {
        let sig = self.sign(who, run, score);
        self.ctx(who, 0);
        let mined = self.game.end_run(run.session_id, score, LOG_HASH.into(), sig).0;
        self.check_floor("end_run");
        mined
    }

    fn play(&mut self, who: &str, score: u32) -> u128 {
        let run = self.start(who);
        self.end(who, &run, score)
    }

    fn redeem(&mut self, who: &str, amount: u128) -> u128 {
        self.ctx(who, 1);
        let paid = self.game.redeem(U128(amount)).0;
        self.check_floor("redeem");
        paid
    }

    fn buy(&mut self, who: &str, amount: u128) -> u128 {
        let need = self.game.quote_buy(U128(amount)).0
            + self.game.registration_needed(&acc(who));
        self.ctx(who, need);
        let cost = self.game.buy(U128(amount)).0;
        self.check_floor("buy");
        cost
    }

    fn balance(&self, who: &str) -> u128 {
        self.game.ft_balance_of(acc(who)).0
    }

    fn floor(&self) -> u128 {
        self.game.floor_price().0
    }
}

/// (receiver, yoctoNEAR) of every plain transfer created by the last call.
fn transfers() -> Vec<(AccountId, u128)> {
    get_created_receipts()
        .into_iter()
        .flat_map(|r| {
            let to = r.receiver_id.clone();
            r.actions.into_iter().filter_map(move |a| match a {
                MockAction::Transfer { deposit, .. } => Some((to.clone(), deposit.as_yoctonear())),
                _ => None,
            })
        })
        .collect()
}

/// (receiver, method, yoctoNEAR) of every function call created by the last call.
fn calls() -> Vec<(AccountId, String, u128)> {
    get_created_receipts()
        .into_iter()
        .flat_map(|r| {
            let to = r.receiver_id.clone();
            r.actions.into_iter().filter_map(move |a| match a {
                MockAction::FunctionCallWeight {
                    method_name,
                    attached_deposit,
                    ..
                } => Some((
                    to.clone(),
                    String::from_utf8(method_name).unwrap(),
                    attached_deposit.as_yoctonear(),
                )),
                _ => None,
            })
        })
        .collect()
}

fn small_caps(runs: u64) -> Params {
    Params {
        mining_run_cap: runs,
        ..Params::standard()
    }
}

// ===================================================== ported Monad scenarios

#[test]
fn mines_from_a_floor_neutral_budget_scaled_by_score() {
    let mut t = T::new();
    // First run: no supply yet, so the budget is 0.014 NEAR / 0.0001 launch price = 140 coins.
    assert_eq!(t.play("alice.testnet", 40), coins(140));
    assert_eq!(t.floor(), milli(1) / 10);
    // Score 0 earns the 25% minimum of the next budget; the floor rises.
    let f0 = t.floor();
    let low = t.play("bob.testnet", 0);
    assert!(low > 0);
    assert!(t.floor() > f0);
    // Scores above the target earn no more than the target.
    assert_eq!(
        t.game.preview_mined(U128(milli(14)), 40),
        t.game.preview_mined(U128(milli(14)), 400)
    );
}

#[test]
fn never_lowers_the_floor_across_60_random_runs_buys_and_redeems() {
    // Mining closes on the run cap after the 60 runs (the Monad test used closeMining).
    let mut t = T::with(small_caps(60), None);
    let players: Vec<String> = (2..10).map(|i| format!("player{i}.testnet")).collect();
    let mut seed: u64 = 42;
    let mut rnd = |n: u64| {
        seed = seed * 16807 % 2147483647;
        seed % n
    };
    for i in 0..60 {
        let p = players[rnd(players.len() as u64) as usize].clone();
        let score = rnd(120) as u32;
        t.play(&p, score);
        let bal = t.balance(&p);
        if i % 7 == 3 && bal > 0 {
            t.redeem(&p, bal / 3);
        }
    }
    assert!(t.game.mining_closed);
    for amount in [coins(10), coins(500), coins(2_000)] {
        t.buy(&players[0], amount);
    }
}

#[test]
fn escrows_the_creators_fee_share_and_unlocks_it_by_unique_players() {
    let mut t = T::new();
    t.play("alice.testnet", 10);
    t.play("bob.testnet", 10);
    t.ctx(CREATOR, 0);
    assert!(fails(|| t.game.claim_creator()).contains("Nothing to claim"));
    t.play("carol.testnet", 10);
    // 3 runs × 30% of 0.02 = 0.018 NEAR earned; 3 players unlock 25%.
    assert_eq!(t.game.creator_earned, milli(18));
    let vault = t.game.vault;
    t.ctx(CREATOR, 0);
    assert_eq!(t.game.claim_creator().0, milli(18) / 4);
    assert_eq!(transfers(), vec![(acc(CREATOR), milli(18) / 4)]);
    assert_eq!(t.game.vault, vault, "claims never touch the vault");
    t.ctx(CREATOR, 0);
    assert!(fails(|| t.game.claim_creator()).contains("Nothing to claim"));
}

#[test]
fn splits_fees_70_vault_30_creator_and_10_to_the_parent_on_remixes() {
    let mut t = T::with(Params::standard(), Some(PARENT));
    let run = t.start("bob.testnet");
    assert_eq!(
        calls()[0],
        (acc(PARENT), "fund".to_string(), milli(2)),
        "10% of the fee goes to the parent's vault"
    );
    assert_eq!(t.game.pending, milli(14), "70% is held for this vault until the run ends");
    assert_eq!(t.game.creator_earned, milli(4), "the creator keeps 20%");
    t.end("bob.testnet", &run, 40);
    assert_eq!(t.game.vault, milli(14));
    // An original game has no parent share: the creator keeps the full 30%.
    let mut o = T::new();
    o.start("bob.testnet");
    assert!(calls().is_empty());
    assert_eq!(o.game.creator_earned, milli(6));
}

#[test]
fn closes_mining_after_the_run_cap() {
    assert_eq!(Params::standard().mining_run_cap, 300);
    let mut t = T::with(small_caps(3), None);
    assert!(!t.game.mining_closed);
    t.play("alice.testnet", 5);
    t.play("bob.testnet", 5);
    assert!(!t.game.mining_closed);
    t.play("carol.testnet", 5);
    assert!(t.game.mining_closed);
    assert!(get_logs().iter().any(|l| l.contains("\"event\":\"mining_closed\"")));
}

#[test]
fn rejects_forged_scores_other_players_sessions_and_double_claims() {
    let mut t = T::new();
    let run = t.start("alice.testnet");
    let sig = t.sign("alice.testnet", &run, 10);
    // A raised score.
    t.ctx("alice.testnet", 0);
    let e = fails(|| t.game.end_run(run.session_id, 999, LOG_HASH.into(), sig.clone()));
    assert!(e.contains("Bad signature"), "{e}");
    // Another player's session.
    t.ctx("bob.testnet", 0);
    let e = fails(|| t.game.end_run(run.session_id, 10, LOG_HASH.into(), sig.clone()));
    assert!(e.contains("Not your session"), "{e}");
    // The honest claim, then the same claim again.
    t.ctx("alice.testnet", 0);
    t.game.end_run(run.session_id, 10, LOG_HASH.into(), sig.clone());
    t.ctx("alice.testnet", 0);
    let e = fails(|| t.game.end_run(run.session_id, 10, LOG_HASH.into(), sig.clone()));
    assert!(e.contains("Unknown or finished session"), "{e}");
}

#[test]
fn keeps_the_curve_closed_during_mining_and_prices_buys_at_or_above_the_floor() {
    let mut t = T::with(small_caps(2), None);
    t.play("alice.testnet", 40);
    t.ctx("alice.testnet", NEAR);
    assert!(fails(|| t.game.buy(U128(ONE_COIN))).contains("Buying opens when mining closes"));
    t.play("bob.testnet", 10);
    assert!(t.game.mining_closed);
    assert!(t.game.spot_price().0 >= t.floor() * 11 / 10);
    assert_eq!(t.game.best_player, Some(acc("alice.testnet")));
}

// ============================================================ end_run checks

#[test]
fn end_run_rejects_a_signature_for_a_different_game_account() {
    let mut t = T::new();
    let run = t.start("alice.testnet");
    let sig = t.sign_for("hop-2.factory.testnet", "alice.testnet", run.session_id, run.seed, 10);
    t.ctx("alice.testnet", 0);
    let e = fails(|| t.game.end_run(run.session_id, 10, LOG_HASH.into(), sig));
    assert!(e.contains("Bad signature"), "{e}");
}

#[test]
fn end_run_rejects_a_different_log_or_seed() {
    let mut t = T::new();
    let run = t.start("alice.testnet");
    let sig = t.sign("alice.testnet", &run, 10);
    t.ctx("alice.testnet", 0);
    let other_log = "2222222222222222222222222222222222222222222222222222222222222222";
    let e = fails(|| t.game.end_run(run.session_id, 10, other_log.into(), sig));
    assert!(e.contains("Bad signature"), "{e}");
    let wrong_seed = t.sign_for(GAME, "alice.testnet", run.session_id, run.seed ^ 1, 10);
    t.ctx("alice.testnet", 0);
    let e = fails(|| t.game.end_run(run.session_id, 10, LOG_HASH.into(), wrong_seed));
    assert!(e.contains("Bad signature"), "{e}");
}

#[test]
fn end_run_rejects_malformed_hex() {
    let mut t = T::new();
    let run = t.start("alice.testnet");
    t.ctx("alice.testnet", 0);
    let e = fails(|| t.game.end_run(run.session_id, 10, "zz".into(), "00".into()));
    assert!(e.contains("hex"), "{e}");
}

// ================================================================== mining

#[test]
fn first_run_at_score_40_mines_140_and_sets_the_floor_to_the_launch_price() {
    let mut t = T::new();
    assert_eq!(t.play("alice.testnet", 40), coins(140));
    assert_eq!(t.game.vault, milli(14));
    assert_eq!(t.game.token.total_supply, coins(140));
    assert_eq!(t.floor(), Params::standard().launch_price.0);
}

#[test]
fn score_0_mines_a_quarter_of_its_budget_and_scores_above_40_mine_as_40() {
    let mut t = T::new();
    assert_eq!(t.play("alice.testnet", 0), coins(35));
    let mut u = T::new();
    assert_eq!(u.play("alice.testnet", 400), coins(140));
    let mut v = T::new();
    // 25% + 75% × 20/40 = 62.5%
    assert_eq!(v.play("alice.testnet", 20), coins(140) * 625 / 1000);
}

#[test]
fn the_wallet_cap_cuts_the_mint() {
    let mut t = T::with(
        Params {
            wallet_mining_cap: U128(coins(200)),
            ..Params::standard()
        },
        None,
    );
    assert_eq!(t.play("alice.testnet", 40), coins(140));
    assert_eq!(t.play("alice.testnet", 40), coins(60));
    assert_eq!(t.play("alice.testnet", 40), 0);
    assert_eq!(t.game.get_player(acc("alice.testnet")).unwrap().mined.0, coins(200));
    assert!(t.play("bob.testnet", 40) > 0, "the cap is per wallet");
}

#[test]
fn the_supply_cap_cuts_the_mint_and_closes_mining() {
    let mut t = T::with(
        Params {
            mining_supply_cap: U128(coins(200)),
            ..Params::standard()
        },
        None,
    );
    assert_eq!(t.play("alice.testnet", 40), coins(140));
    assert!(!t.game.mining_closed);
    assert_eq!(t.play("bob.testnet", 40), coins(60));
    assert!(t.game.mining_closed);
    assert_eq!(t.game.total_mined, coins(200));
}

#[test]
fn after_mining_closes_runs_mint_nothing_and_raise_the_floor() {
    let mut t = T::with(small_caps(1), None);
    t.play("alice.testnet", 40);
    assert!(t.game.mining_closed);
    let supply = t.game.token.total_supply;
    for _ in 0..3 {
        let f = t.floor();
        assert_eq!(t.play("bob.testnet", 40), 0);
        assert!(t.floor() > f);
    }
    assert_eq!(t.game.token.total_supply, supply);
}

#[test]
fn concurrent_runs_never_lower_the_floor() {
    let mut t = T::new();
    t.play("alice.testnet", 40);
    let a = t.start("bob.testnet");
    let b = t.start("carol.testnet");
    let c = t.start("dave.testnet");
    assert_eq!(t.game.open_sessions, 3);
    t.end("carol.testnet", &b, 40);
    t.end("bob.testnet", &a, 0);
    t.end("dave.testnet", &c, 25);
    assert_eq!(t.game.pending, 0);
    assert_eq!(t.game.open_sessions, 0);
}

// =================================================================== runs

#[test]
fn start_run_charges_registration_once_and_refunds_excess() {
    let mut t = T::new();
    let fee = Params::standard().entry_fee.0;
    let reg = t.game.registration_cost().0;
    assert!(reg > 0 && reg < milli(5), "registration costs {reg}");
    assert_eq!(t.game.run_cost(acc("alice.testnet")).0, fee + reg);
    t.ctx("alice.testnet", fee + reg + 7);
    t.game.start_run();
    assert_eq!(transfers(), vec![(acc("alice.testnet"), 7)]);
    assert_eq!(t.game.run_cost(acc("alice.testnet")).0, fee);
    assert!(t.game.storage_balance_of(acc("alice.testnet")).is_some());
    // Too little: refused.
    t.ctx("alice.testnet", fee - 1);
    assert!(fails(|| t.game.start_run()).contains("Attach"));
}

#[test]
fn start_run_records_the_session_and_its_seed() {
    let mut t = T::new();
    let r1 = t.start("alice.testnet");
    let r2 = t.start("alice.testnet");
    assert_eq!((r1.session_id, r2.session_id), (1, 2));
    let s = t.game.get_session(1).unwrap();
    assert_eq!(s.player, acc("alice.testnet"));
    assert_eq!(s.seed, r1.seed);
    assert_eq!(s.deposit.0, milli(14));
    assert_ne!(r1.seed, r2.seed, "each run draws its own seed");
    assert!(get_logs().iter().any(|l| l.contains("\"event\":\"run_start\"")));
}

#[test]
fn buy_runs_prepays_credits_for_self_or_another_account() {
    let mut t = T::new();
    let fee = Params::standard().entry_fee.0;
    let reg = t.game.registration_cost().0;
    // A relayer buys three runs for a new player.
    t.ctx("relayer.testnet", 3 * fee + reg + 5);
    assert_eq!(t.game.buy_runs(3, Some(acc("newbie.testnet"))), 3);
    assert_eq!(transfers(), vec![(acc("relayer.testnet"), 5)]);
    assert_eq!(t.game.credits_held, 3 * fee);
    // The player plays them with no deposit (as a session key would).
    let run = t.start_with_credit("newbie.testnet");
    assert_eq!(t.game.get_player(acc("newbie.testnet")).unwrap().credits, 2);
    assert_eq!(t.game.credits_held, 2 * fee);
    assert_eq!(t.end("newbie.testnet", &run, 40), coins(140));
    // Buying for yourself once registered costs only the fees.
    t.ctx("newbie.testnet", fee);
    assert_eq!(t.game.buy_runs(1, None), 3);
    // Too little, zero runs, or no credits: refused.
    t.ctx("newbie.testnet", fee - 1);
    assert!(fails(|| t.game.buy_runs(1, None)).contains("Attach"));
    t.ctx("newbie.testnet", 0);
    assert!(fails(|| t.game.buy_runs(0, None)).contains("between 1 and 100"));
    t.ctx("stranger.testnet", 0);
    assert!(fails(|| t.game.start_run()).contains("No run credits"));
}

#[test]
fn expire_run_moves_an_abandoned_deposit_into_the_vault() {
    let mut t = T::new();
    t.play("alice.testnet", 40);
    let run = t.start("bob.testnet");
    t.ctx("anyone.testnet", 0);
    assert!(fails(|| t.game.expire_run(run.session_id)).contains("can still be finished"));
    t.advance(Params::standard().max_run_secs);
    let f = t.floor();
    t.ctx("anyone.testnet", 0);
    t.game.expire_run(run.session_id);
    t.check_floor("expire_run");
    assert!(t.floor() > f);
    assert_eq!(t.game.pending, 0);
    assert_eq!(t.game.vault, milli(28));
    assert!(t.game.get_session(run.session_id).is_none());
    // The player can no longer claim it.
    let sig = t.sign("bob.testnet", &run, 40);
    t.ctx("bob.testnet", 0);
    assert!(fails(|| t.game.end_run(run.session_id, 40, LOG_HASH.into(), sig))
        .contains("Unknown or finished session"));
}

// ================================================================= market

#[test]
fn redeem_pays_the_exact_share_and_keeps_the_floor() {
    let mut t = T::new();
    t.play("alice.testnet", 40);
    t.play("bob.testnet", 13);
    let (vault, supply, floor) = (t.game.vault, t.game.token.total_supply, t.floor());
    let amount = t.balance("bob.testnet") / 3;
    let paid = t.redeem("bob.testnet", amount);
    assert_eq!(paid, mul_div_down(vault, amount, supply));
    assert_eq!(transfers(), vec![(acc("bob.testnet"), paid)]);
    assert!(t.floor() >= floor);
    // Redeeming needs exactly 1 yoctoNEAR, more coins than held is refused.
    t.ctx("bob.testnet", 0);
    assert!(fails(|| t.game.redeem(U128(1))).contains("exactly 1 yoctoNEAR"));
    t.ctx("bob.testnet", 1);
    assert!(fails(|| t.game.redeem(U128(coins(1_000_000)))).contains("balance"));
}

#[test]
fn redeeming_everything_empties_the_vault_without_lowering_the_floor() {
    let mut t = T::new();
    t.play("alice.testnet", 40);
    let all = t.balance("alice.testnet");
    assert_eq!(t.redeem("alice.testnet", all), milli(14));
    assert_eq!(t.game.token.total_supply, 0);
    // The next run prices at the launch price again.
    assert_eq!(t.play("bob.testnet", 40), coins(140));
}

#[test]
fn buy_charges_at_least_floor_plus_10_percent_and_raises_the_floor() {
    let mut t = T::with(small_caps(2), None);
    t.play("alice.testnet", 40);
    t.play("bob.testnet", 40);
    let floor = t.floor();
    let amount = coins(10);
    let quote = t.game.quote_buy(U128(amount)).0;
    assert!(quote > floor * 10 * 11 / 10);
    let vault = t.game.vault;
    let cost = t.buy("carol.testnet", amount);
    assert_eq!(cost, quote);
    assert_eq!(t.game.vault, vault + cost);
    assert_eq!(t.balance("carol.testnet"), amount);
    assert!(t.floor() > floor);
    assert_eq!(t.game.sold, amount);
}

#[test]
fn buy_follows_the_curve_when_it_is_above_the_floor_and_refunds_excess() {
    let mut t = T::with(small_caps(1), None);
    t.play("alice.testnet", 40); // floor 0.0001, curve starts at 0.0005
    let amount = coins(1_000);
    // 1,000 coins from 0 sold: 0.0005 × 1000 + 0.0001 × 1000 / 2 = 0.55 NEAR (+1 yocto).
    assert_eq!(t.game.quote_buy(U128(amount)).0, milli(550) + 1);
    t.ctx("alice.testnet", NEAR);
    let cost = t.game.buy(U128(amount)).0;
    assert_eq!(transfers(), vec![(acc("alice.testnet"), NEAR - cost)]);
    // The next 1,000 coins cost more.
    assert!(t.game.quote_buy(U128(amount)).0 > cost);
    t.ctx("alice.testnet", 1);
    assert!(fails(|| t.game.buy(U128(amount))).contains("Attach at least"));
}

#[test]
fn fund_adds_to_the_vault_and_a_failed_parent_payment_stays_here() {
    let mut t = T::new();
    t.play("alice.testnet", 40);
    let f = t.floor();
    t.ctx("remix.factory.testnet", milli(2));
    t.game.fund();
    t.check_floor("fund");
    assert_eq!(t.game.vault, milli(16));
    assert!(t.floor() > f);
    t.ctx("remix.factory.testnet", 0);
    assert!(fails(|| t.game.fund()).contains("Attach NEAR"));

    // Callback: success leaves the vault alone; failure keeps the share here.
    let mut r = T::with(Params::standard(), Some(PARENT));
    r.start("bob.testnet");
    let c = VMContextBuilder::new()
        .current_account_id(acc(GAME))
        .predecessor_account_id(acc(GAME))
        .build();
    testing_env!(
        c.clone(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![near_sdk::PromiseResult::Successful(vec![])],
    );
    r.game.on_parent_funded(U128(milli(2)));
    assert_eq!(r.game.vault, 0);
    testing_env!(
        c,
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![near_sdk::PromiseResult::Failed],
    );
    r.game.on_parent_funded(U128(milli(2)));
    assert_eq!(r.game.vault, milli(2));
}

// ================================================================ creator

#[test]
fn creator_escrow_unlocks_a_quarter_at_3_10_25_and_50_players() {
    let mut t = T::new();
    let mut unlocked = vec![];
    for i in 1..=50 {
        t.play(&format!("p{i}.testnet"), 1);
        if [3, 10, 25, 50].contains(&i) {
            unlocked.push(t.game.creator_unlocked().0);
        }
    }
    let earned = t.game.creator_earned;
    assert_eq!(earned, 50 * milli(6));
    // Each milestone's figure is a quarter of what had been earned by then.
    assert_eq!(unlocked[0], 3 * milli(6) / 4);
    assert_eq!(unlocked[1], 10 * milli(6) / 2);
    assert_eq!(unlocked[2], 25 * milli(6) * 3 / 4);
    assert_eq!(unlocked[3], earned);
    let vault = t.game.vault;
    t.ctx(CREATOR, 0);
    assert_eq!(t.game.claim_creator().0, earned);
    assert_eq!(t.game.vault, vault);
    t.ctx("alice.testnet", 0);
    assert!(fails(|| t.game.claim_creator()).contains("Only the creator"));
}

#[test]
fn repeat_runs_do_not_count_as_new_players() {
    let mut t = T::new();
    for _ in 0..5 {
        t.play("alice.testnet", 1);
    }
    assert_eq!(t.game.unique_players, 1);
    assert_eq!(t.game.creator_unlocked().0, 0);
}

// ============================================================ token + views

#[test]
fn coins_are_nep141_transferable_between_registered_accounts() {
    let mut t = T::new();
    t.play("alice.testnet", 40);
    let reg = t.game.registration_cost().0;
    t.ctx("bob.testnet", reg);
    t.game.storage_deposit(None, None);
    t.ctx("alice.testnet", 1);
    t.game.ft_transfer(acc("bob.testnet"), U128(coins(40)), None);
    t.check_floor("ft_transfer");
    assert_eq!(t.balance("bob.testnet"), coins(40));
    assert_eq!(t.balance("alice.testnet"), coins(100));
    let m = t.game.ft_metadata();
    assert_eq!((m.symbol.as_str(), m.decimals), ("HOP", 18));
    // Storage registration is fixed and cannot be withdrawn or removed.
    let b = t.game.storage_balance_bounds();
    assert_eq!(b.min.as_yoctonear(), reg);
    t.ctx("bob.testnet", 1);
    assert!(fails(|| t.game.storage_unregister(Some(true))).contains("disabled"));
}

#[test]
fn leaderboard_keeps_each_players_best_score_highest_first() {
    let mut t = T::new();
    t.play("alice.testnet", 10);
    t.play("bob.testnet", 30);
    t.play("alice.testnet", 50);
    t.play("alice.testnet", 20); // lower than her best: ignored
    t.play("carol.testnet", 30); // ties keep the earlier score first
    let board: Vec<(String, u32)> = t
        .game
        .leaderboard()
        .into_iter()
        .map(|e| (e.player.to_string(), e.score))
        .collect();
    assert_eq!(
        board,
        vec![
            ("alice.testnet".into(), 50),
            ("bob.testnet".into(), 30),
            ("carol.testnet".into(), 30)
        ]
    );
    for i in 0..25 {
        t.play(&format!("p{i}.testnet"), 100 + i);
    }
    let board = t.game.leaderboard();
    assert_eq!(board.len(), 20);
    assert_eq!(board[0].score, 124);
    assert_eq!(t.game.best_score, 124);
}

#[test]
fn recent_events_returns_the_last_50_newest_first() {
    let mut t = T::new();
    t.play("alice.testnet", 7);
    let ev = t.game.recent_events(Some(2));
    assert_eq!(ev[0].kind, EventKind::RunEnd);
    assert_eq!(ev[0].score, 7);
    assert_eq!(ev[0].coins.0, t.balance("alice.testnet"));
    assert_eq!(ev[1].kind, EventKind::RunStart);
    for _ in 0..40 {
        t.play("bob.testnet", 1);
    }
    let ev = t.game.recent_events(None);
    assert_eq!(ev.len(), 50);
    assert_eq!(t.game.event_count, 82);
    assert_eq!(ev[0].kind, EventKind::RunEnd);
    assert_eq!(ev[0].account, acc("bob.testnet"));
    assert!(ev
        .windows(2)
        .all(|w| w[0].timestamp_ms.0 >= w[1].timestamp_ms.0));
}

#[test]
fn stats_and_params_are_readable() {
    let mut t = T::with(Params::standard(), Some(PARENT));
    t.play("alice.testnet", 40);
    let s = t.game.get_stats();
    assert_eq!(s.symbol, "HOP");
    assert_eq!(s.parent, Some(acc(PARENT)));
    assert_eq!(s.factory, acc(FACTORY));
    assert_eq!(s.config_hash, CONFIG_HASH);
    assert_eq!(s.unique_players, 1);
    assert_eq!(s.floor_price.0, milli(1) / 10);
    assert_eq!(t.game.get_params().parent_bps, 1_000);
    // An original game ignores parent_bps.
    assert_eq!(T::new().game.get_params().parent_bps, 0);
}

#[test]
fn new_rejects_bad_params() {
    let bad = Params {
        vault_bps: 9_500,
        parent_bps: 1_000,
        ..Params::standard()
    };
    let e = fails(|| T::with(bad, Some(PARENT)));
    assert!(e.contains("must not exceed 10000"), "{e}");
    let e = fails(|| {
        T::with(
            Params {
                target_score: 0,
                ..Params::standard()
            },
            None,
        )
    });
    assert!(e.contains("target_score"), "{e}");
}
