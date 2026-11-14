//! Sandbox tests: the real contracts on a local NEAR node with real accounts.
//!
//! Build both contracts first (they are read from near/target/near), then:
//!   cargo test -p pop-sandbox-tests
//! The NEAR sandbox runs on Linux and macOS; CI runs these on Linux.

use ed25519_dalek::{Signer, SigningKey};
use near_workspaces::network::Sandbox;
use near_workspaces::types::{AccessKey, Gas, KeyType, NearToken, SecretKey};
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const GAME_WASM: &str = "../target/near/pop_game/pop_game.wasm";
const FACTORY_WASM: &str = "../target/near/pop_factory/pop_factory.wasm";
const CONFIG_HASH: &str = "4f77cdd9d12d104492eef8a22635f73db8d1ab7a8030b1431f8bea2ab6c8e599";
const LOG_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const MILLI: u128 = 1_000_000_000_000_000_000_000;

fn verifier() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

fn verifier_pk() -> String {
    hex::encode(verifier().verifying_key().to_bytes())
}

/// The score message from near/game/src/score.rs, rebuilt independently here:
/// sha256(borsh(game, player, session_id, seed, config_hash, score, log_hash)).
fn sign_score(game: &str, player: &str, session_id: u64, seed: u32, score: u32) -> String {
    let mut m = Vec::new();
    for s in [game, player] {
        m.extend_from_slice(&(s.len() as u32).to_le_bytes());
        m.extend_from_slice(s.as_bytes());
    }
    m.extend_from_slice(&session_id.to_le_bytes());
    m.extend_from_slice(&seed.to_le_bytes());
    m.extend_from_slice(&hex::decode(CONFIG_HASH).unwrap());
    m.extend_from_slice(&score.to_le_bytes());
    m.extend_from_slice(&hex::decode(LOG_HASH).unwrap());
    let digest = Sha256::digest(&m);
    hex::encode(verifier().sign(&digest).to_bytes())
}

fn u128_of(v: &Value) -> u128 {
    v.as_str().unwrap().parse().unwrap()
}

struct Env {
    worker: Worker<Sandbox>,
    root: Account,
    factory: Contract,
}

async fn setup() -> anyhow::Result<Env> {
    let worker = near_workspaces::sandbox().await?;
    let root = worker.root_account()?;
    let factory_wasm = std::fs::read(FACTORY_WASM)?;
    let factory_account = root
        .create_subaccount("factory")
        .initial_balance(NearToken::from_near(10))
        .transact()
        .await?
        .into_result()?;
    let factory = factory_account.deploy(&factory_wasm).await?.into_result()?;
    factory
        .call("new")
        .args_json(json!({ "owner": root.id(), "verifier_pk": verifier_pk() }))
        .transact()
        .await?
        .into_result()?;
    Ok(Env {
        worker,
        root,
        factory,
    })
}

async fn user(env: &Env, name: &str) -> anyhow::Result<Account> {
    Ok(env
        .root
        .create_subaccount(name)
        .initial_balance(NearToken::from_near(10))
        .transact()
        .await?
        .into_result()?)
}

/// Launches a game through the factory and returns its account id.
async fn launch(env: &Env, creator: &Account, symbol: &str, parent_id: u64) -> anyhow::Result<String> {
    let cost: Value = env.factory.view("game_storage_cost").await?.json()?;
    let id: u64 = creator
        .call(env.factory.id(), "create_game")
        .args_json(json!({
            "name": format!("{symbol} game"),
            "symbol": symbol,
            "config_hash": CONFIG_HASH,
            "parent_id": parent_id,
        }))
        .deposit(NearToken::from_yoctonear(u128_of(&cost)))
        .max_gas()
        .transact()
        .await?
        .into_result()?
        .json()?;
    let game: Value = env
        .factory
        .view("get_game")
        .args_json(json!({ "id": id }))
        .await?
        .json()?;
    Ok(game["account"].as_str().expect("game was recorded").to_string())
}

async fn stats(env: &Env, game: &str) -> anyhow::Result<Value> {
    Ok(env
        .worker
        .view(&game.parse()?, "get_stats")
        .await?
        .json()?)
}

/// Starts and ends one paid run; returns the coins mined.
async fn play(env: &Env, player: &Account, game: &str, score: u32) -> anyhow::Result<u128> {
    let cost: Value = env
        .worker
        .view(&game.parse()?, "run_cost")
        .args_json(json!({ "account_id": player.id() }))
        .await?
        .json()?;
    let run: Value = player
        .call(&game.parse()?, "start_run")
        .deposit(NearToken::from_yoctonear(u128_of(&cost)))
        .max_gas()
        .transact()
        .await?
        .into_result()?
        .json()?;
    end(player, game, &run, score).await
}

async fn end(player: &Account, game: &str, run: &Value, score: u32) -> anyhow::Result<u128> {
    let session_id = run["session_id"].as_u64().unwrap();
    let seed = run["seed"].as_u64().unwrap() as u32;
    let sig = sign_score(game, player.id().as_str(), session_id, seed, score);
    let mined: Value = player
        .call(&game.parse()?, "end_run")
        .args_json(json!({
            "session_id": session_id,
            "score": score,
            "log_hash": LOG_HASH,
            "signature": sig,
        }))
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?
        .json()?;
    Ok(u128_of(&mined))
}

#[tokio::test]
async fn factory_creates_a_game_and_the_registry_lists_it() -> anyhow::Result<()> {
    let env = setup().await?;
    let alice = user(&env, "alice").await?;
    let game = launch(&env, &alice, "FROG", 0).await?;
    assert_eq!(game, format!("frog-1.{}", env.factory.id()));

    let count: u64 = env.factory.view("game_count").await?.json()?;
    assert_eq!(count, 1);
    let list: Value = env
        .factory
        .view("list_games")
        .args_json(json!({ "offset": 0, "limit": 10 }))
        .await?
        .json()?;
    assert_eq!(list[0]["account"], game);
    assert_eq!(list[0]["creator"], alice.id().as_str());
    let by_account: Option<u64> = env
        .factory
        .view("game_id_by_account")
        .args_json(json!({ "account_id": game }))
        .await?
        .json()?;
    assert_eq!(by_account, Some(1));

    // The game works: a first run at score 40 mines 140 coins at a 0.0001 NEAR floor.
    let s = stats(&env, &game).await?;
    assert_eq!(s["creator"], alice.id().as_str());
    let bob = user(&env, "bob").await?;
    assert_eq!(play(&env, &bob, &game, 40).await?, 140 * 10u128.pow(18));
    let s = stats(&env, &game).await?;
    assert_eq!(u128_of(&s["floor_price"]), MILLI / 10);

    // The game account has no access keys: nobody can change it.
    let keys = env.worker.view_access_keys(&game.parse()?).await?;
    assert!(keys.is_empty(), "a launched game must have no access keys");
    Ok(())
}

#[tokio::test]
async fn a_remix_run_pays_the_parents_vault() -> anyhow::Result<()> {
    let env = setup().await?;
    let alice = user(&env, "alice").await?;
    let bob = user(&env, "bob").await?;
    let parent = launch(&env, &alice, "FROG", 0).await?;
    play(&env, &alice, &parent, 40).await?;
    let before = u128_of(&stats(&env, &parent).await?["vault"]);

    let remix = launch(&env, &bob, "FROG2", 1).await?;
    assert_eq!(stats(&env, &remix).await?["parent"], parent);
    play(&env, &bob, &remix, 40).await?;

    let after = u128_of(&stats(&env, &parent).await?["vault"]);
    assert_eq!(after - before, 2 * MILLI, "10% of the 0.02 NEAR fee");
    let r = stats(&env, &remix).await?;
    assert_eq!(u128_of(&r["vault"]), 14 * MILLI);
    assert_eq!(u128_of(&r["creator_earned"]), 4 * MILLI);
    Ok(())
}

#[tokio::test]
async fn a_failed_parent_payment_stays_in_the_remix_vault() -> anyhow::Result<()> {
    // A game deployed directly whose parent account has no contract: its `fund` call fails.
    let env = setup().await?;
    let nocontract = user(&env, "nocontract").await?;
    let orphan_account = user(&env, "orphan").await?;
    let orphan = orphan_account
        .deploy(&std::fs::read(GAME_WASM)?)
        .await?
        .into_result()?;
    orphan
        .call("new")
        .args_json(json!({
            "name": "Orphan",
            "symbol": "ORF",
            "creator": orphan_account.id(),
            "parent": nocontract.id(),
            "verifier_pk": verifier_pk(),
            "config_hash": CONFIG_HASH,
            "params": env.factory.view("get_default_params").await?.json::<Value>()?,
        }))
        .transact()
        .await?
        .into_result()?;

    let bob = user(&env, "bob").await?;
    let cost: Value = orphan
        .view("run_cost")
        .args_json(json!({ "account_id": bob.id() }))
        .await?
        .json()?;
    let outcome = bob
        .call(orphan.id(), "start_run")
        .deposit(NearToken::from_yoctonear(u128_of(&cost)))
        .max_gas()
        .transact()
        .await?;
    assert!(outcome.is_success(), "start_run itself succeeds");
    assert!(
        outcome.receipt_failures().len() == 1,
        "only the parent's fund call fails"
    );
    let s: Value = orphan.view("get_stats").await?.json()?;
    // 70% waits for the run to end; the parent's failed 10% came back into this vault.
    assert_eq!(u128_of(&s["pending"]), 14 * MILLI);
    assert_eq!(u128_of(&s["vault"]), 2 * MILLI);
    Ok(())
}

#[tokio::test]
async fn a_limited_key_plays_prepaid_runs_but_cannot_redeem() -> anyhow::Result<()> {
    let env = setup().await?;
    let alice = user(&env, "alice").await?;
    let game = launch(&env, &alice, "FROG", 0).await?;
    let game_id: near_workspaces::AccountId = game.parse()?;
    let player = user(&env, "player").await?;

    // One normal approval: prepay two runs (plus token storage for a new account).
    let reg: Value = env.worker.view(&game_id, "registration_cost").await?.json()?;
    player
        .call(&game_id, "buy_runs")
        .args_json(json!({ "count": 2 }))
        .deposit(NearToken::from_yoctonear(2 * 20 * MILLI + u128_of(&reg)))
        .max_gas()
        .transact()
        .await?
        .into_result()?;

    // A function-call key limited to start_run and end_run on this game.
    let sk = SecretKey::from_random(KeyType::ED25519);
    player
        .batch(player.id())
        .add_key(
            sk.public_key(),
            AccessKey::function_call_access(
                &game_id,
                &["start_run", "end_run"],
                Some(NearToken::from_millinear(250)),
            ),
        )
        .transact()
        .await?
        .into_result()?;
    let limited = Account::from_secret_key(player.id().clone(), sk, &env.worker);

    // The limited key plays a prepaid run start to finish.
    // Modest gas, as a session key would send: 300 Tgas would exceed the key's allowance.
    let run: Value = limited
        .call(&game_id, "start_run")
        .gas(Gas::from_tgas(50))
        .transact()
        .await?
        .into_result()?
        .json()?;
    let mined = end(&limited, &game, &run, 40).await?;
    assert_eq!(mined, 140 * 10u128.pow(18));

    // It cannot redeem: the method is not on the key, and redeem needs 1 yoctoNEAR,
    // which a function-call key cannot attach.
    let redeem = limited
        .call(&game_id, "redeem")
        .args_json(json!({ "amount": "1000000000000000000" }))
        .deposit(NearToken::from_yoctonear(1))
        .max_gas()
        .transact()
        .await;
    assert!(
        redeem.is_err() || redeem.unwrap().is_failure(),
        "a limited key must not redeem"
    );
    let redeem = limited
        .call(&game_id, "redeem")
        .args_json(json!({ "amount": "1000000000000000000" }))
        .max_gas()
        .transact()
        .await;
    assert!(redeem.is_err() || redeem.unwrap().is_failure());

    // The full-access key still can.
    player
        .call(&game_id, "redeem")
        .args_json(json!({ "amount": "1000000000000000000" }))
        .deposit(NearToken::from_yoctonear(1))
        .max_gas()
        .transact()
        .await?
        .into_result()?;
    Ok(())
}
