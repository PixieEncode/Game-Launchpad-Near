//! Unit tests for the factory's own logic. The launch batch itself (account
//! creation, deploy, `new`) runs on testnet: see near/README.md.

use super::*;
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{testing_env, PromiseResult, RuntimeFeesConfig};
use std::panic::{catch_unwind, AssertUnwindSafe};

const FACTORY: &str = "factory.popnear.testnet";
const OWNER: &str = "popnear.testnet";
const PK: &str = "aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00aa00";
const HASH: &str = "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00";

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn ctx(who: &str, deposit: NearToken, results: Vec<PromiseResult>) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(acc(FACTORY))
            .predecessor_account_id(acc(who))
            .attached_deposit(deposit)
            .account_balance(NearToken::from_near(100))
            .build(),
        near_sdk::test_vm_config(),
        RuntimeFeesConfig::test(),
        Default::default(),
        results,
    );
}

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

fn factory() -> Factory {
    ctx(OWNER, NearToken::from_yoctonear(0), vec![]);
    Factory::new(acc(OWNER), PK.into(), None)
}

/// Calls create_game, then its callback with `ok`, and returns the reserved id.
fn launch(f: &mut Factory, who: &str, symbol: &str, parent_id: u64, ok: bool) -> u64 {
    let cost = f.game_storage_cost();
    ctx(who, cost, vec![]);
    let id = f.create_game(format!("{symbol} game"), symbol.into(), HASH.into(), parent_id);
    let record = GameRecord {
        id,
        creator: acc(who),
        account: game_account(symbol, id, &acc(FACTORY)),
        parent_id,
        config_hash: HASH.into(),
        verifier_pk: PK.into(),
        name: format!("{symbol} game"),
        symbol: symbol.into(),
        created_at_ms: 0,
    };
    let result = if ok {
        PromiseResult::Successful(vec![])
    } else {
        PromiseResult::Failed
    };
    ctx(FACTORY, NearToken::from_yoctonear(0), vec![result]);
    f.on_game_created(record, U128(cost.as_yoctonear()));
    id
}

#[test]
fn create_game_batches_account_funding_deploy_and_init() {
    let mut f = factory();
    let cost = f.game_storage_cost();
    ctx("alice.testnet", cost, vec![]);
    let id = f.create_game("Frog Jump".into(), "FROG".into(), HASH.into(), 0);
    assert_eq!(id, 1);
    let receipts = get_created_receipts();
    let batch = &receipts[0];
    assert_eq!(batch.receiver_id, acc("frog-1.factory.popnear.testnet"));
    let mut kinds = vec![];
    for a in &batch.actions {
        match a {
            MockAction::CreateAccount { .. } => kinds.push("create".to_string()),
            MockAction::Transfer { deposit, .. } => {
                assert_eq!(*deposit, cost);
                kinds.push("transfer".into())
            }
            MockAction::DeployContract { code, .. } => {
                assert_eq!(code.len(), GAME_WASM.len());
                kinds.push("deploy".into())
            }
            MockAction::FunctionCallWeight { method_name, args, .. } => {
                assert_eq!(method_name, b"new");
                let v: serde_json::Value = serde_json::from_slice(args).unwrap();
                assert_eq!(v["creator"], "alice.testnet");
                assert_eq!(v["parent"], serde_json::Value::Null);
                assert_eq!(v["verifier_pk"], PK);
                assert_eq!(v["params"]["mining_run_cap"], 300);
                kinds.push("new".into())
            }
            _ => {}
        }
    }
    assert_eq!(kinds, vec!["create", "transfer", "deploy", "new"]);
    // Not listed until the callback confirms the launch.
    assert_eq!(f.game_count(), 0);
}

#[test]
fn the_callback_records_successful_launches_newest_first() {
    let mut f = factory();
    launch(&mut f, "alice.testnet", "FROG", 0, true);
    assert!(get_logs().iter().any(|l| l.contains("\"event\":\"game_created\"")));
    launch(&mut f, "bob.testnet", "Neon Rush!", 0, true);
    launch(&mut f, "carol.testnet", "HOP", 1, true);
    assert_eq!(f.game_count(), 3);
    let list: Vec<String> = f.list_games(0, 10).iter().map(|g| g.account.to_string()).collect();
    assert_eq!(
        list,
        vec![
            "hop-3.factory.popnear.testnet",
            "neonrush-2.factory.popnear.testnet",
            "frog-1.factory.popnear.testnet"
        ]
    );
    assert_eq!(f.list_games(1, 1)[0].id, 2);
    assert!(f.list_games(5, 10).is_empty());
    assert_eq!(f.get_game(3).unwrap().parent_id, 1);
    assert_eq!(
        f.game_id_by_account(acc("frog-1.factory.popnear.testnet")),
        Some(1)
    );
}

#[test]
fn a_failed_launch_is_not_recorded_and_is_refunded() {
    let mut f = factory();
    let id = launch(&mut f, "alice.testnet", "FROG", 0, false);
    assert_eq!(f.game_count(), 0);
    assert!(f.get_game(id).is_none());
    let refund: Vec<(AccountId, NearToken)> = get_created_receipts()
        .into_iter()
        .flat_map(|r| {
            let to = r.receiver_id.clone();
            r.actions.into_iter().filter_map(move |a| match a {
                MockAction::Transfer { deposit, .. } => Some((to.clone(), deposit)),
                _ => None,
            })
        })
        .collect();
    assert_eq!(refund, vec![(acc("alice.testnet"), f.game_storage_cost())]);
    // The next launch gets a fresh id.
    assert_eq!(launch(&mut f, "alice.testnet", "FROG", 0, true), 2);
}

#[test]
fn remixes_pass_the_parent_account_to_the_game() {
    let mut f = factory();
    launch(&mut f, "alice.testnet", "FROG", 0, true);
    ctx("bob.testnet", f.game_storage_cost(), vec![]);
    f.create_game("Frog 2".into(), "FROG2".into(), HASH.into(), 1);
    let receipts = get_created_receipts();
    let args = receipts[0]
        .actions
        .iter()
        .find_map(|a| match a {
            MockAction::FunctionCallWeight { args, .. } => Some(args.clone()),
            _ => None,
        })
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(v["parent"], "frog-1.factory.popnear.testnet");
}

#[test]
fn create_game_rejects_unknown_parents_low_deposits_and_bad_hashes() {
    let mut f = factory();
    let cost = f.game_storage_cost();
    ctx("alice.testnet", cost, vec![]);
    assert!(fails(|| f.create_game("A".into(), "A".into(), HASH.into(), 9)).contains("Unknown parent"));
    ctx("alice.testnet", cost.saturating_sub(NearToken::from_yoctonear(1)), vec![]);
    assert!(fails(|| f.create_game("A".into(), "A".into(), HASH.into(), 0)).contains("Attach at least"));
    ctx("alice.testnet", cost, vec![]);
    assert!(fails(|| f.create_game("A".into(), "A".into(), "abc".into(), 0)).contains("config_hash"));
}

#[test]
fn storage_cost_covers_the_game_code() {
    let f = factory();
    let bytes = GAME_WASM.len() as u128 + GAME_STATE_MARGIN_BYTES as u128;
    assert_eq!(
        f.game_storage_cost().as_yoctonear(),
        bytes * env::storage_byte_cost().as_yoctonear()
    );
    assert_eq!(f.game_code_hash().len(), 64);
}

#[test]
fn only_the_owner_changes_defaults_and_only_for_future_games() {
    let mut f = factory();
    launch(&mut f, "alice.testnet", "FROG", 0, true);
    ctx("mallory.testnet", NearToken::from_yoctonear(0), vec![]);
    assert!(fails(|| f.set_verifier_pk(PK.into())).contains("Only the owner"));
    assert!(fails(|| f.set_default_params(Params::standard())).contains("Only the owner"));

    let new_pk = "bb".repeat(32);
    ctx(OWNER, NearToken::from_yoctonear(0), vec![]);
    f.set_verifier_pk(new_pk.clone());
    f.set_default_params(Params {
        mining_run_cap: 10,
        ..Params::standard()
    });
    assert_eq!(f.get_default_params().mining_run_cap, 10);
    // The launched game's record keeps the key it launched with.
    assert_eq!(f.get_game(1).unwrap().verifier_pk, PK);
    ctx("bob.testnet", f.game_storage_cost(), vec![]);
    f.create_game("B".into(), "B".into(), HASH.into(), 0);
    let args = get_created_receipts()[0]
        .actions
        .iter()
        .find_map(|a| match a {
            MockAction::FunctionCallWeight { args, .. } => Some(args.clone()),
            _ => None,
        })
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(v["verifier_pk"], new_pk);
    assert_eq!(v["params"]["mining_run_cap"], 10);
}

#[test]
fn game_account_names_are_valid_and_bounded() {
    let f = acc(FACTORY);
    assert_eq!(game_account("FROG", 7, &f).as_str(), "frog-7.factory.popnear.testnet");
    assert_eq!(game_account("$$$", 2, &f).as_str(), "game-2.factory.popnear.testnet");
    assert_eq!(
        game_account("averyveryverylongsymbolname", 3, &f).as_str(),
        "averyveryverylon-3.factory.popnear.testnet"
    );
}
