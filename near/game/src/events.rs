//! NEP-297 events. Every run start, run end, buy, redeem, creator claim, fund
//! and mining close is logged as `EVENT_JSON:{...}` and also kept in a ring
//! buffer of the last 50, which the web app polls through `recent_events`.

use near_sdk::json_types::{U128, U64};
use near_sdk::{env, near, AccountId};

pub const EVENT_STANDARD: &str = "proof_of_play";
pub const EVENT_VERSION: &str = "1.0.0";
pub const RING_SIZE: u32 = 50;

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStart,
    RunEnd,
    RunExpired,
    Buy,
    Redeem,
    CreatorClaim,
    Fund,
    MiningClosed,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventKind::RunStart => "run_start",
            EventKind::RunEnd => "run_end",
            EventKind::RunExpired => "run_expired",
            EventKind::Buy => "buy",
            EventKind::Redeem => "redeem",
            EventKind::CreatorClaim => "creator_claim",
            EventKind::Fund => "fund",
            EventKind::MiningClosed => "mining_closed",
        }
    }
}

/// One flat record for every kind; fields a kind does not use are zero.
/// - `coins`: coin units (18 decimals) mined, bought or redeemed.
/// - `near`: yoctoNEAR paid in (fee, buy cost, fund) or out (redeem, claim).
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEvent {
    pub kind: EventKind,
    pub account: AccountId,
    pub session_id: U64,
    pub score: u32,
    pub coins: U128,
    pub near: U128,
    /// Floor after the action, yoctoNEAR per whole coin.
    pub floor: U128,
    pub timestamp_ms: U64,
}

pub fn log_event(e: &GameEvent) {
    let data = near_sdk::serde_json::to_string(e).unwrap_or_default();
    env::log_str(&format!(
        "EVENT_JSON:{{\"standard\":\"{}\",\"version\":\"{}\",\"event\":\"{}\",\"data\":[{}]}}",
        EVENT_STANDARD,
        EVENT_VERSION,
        e.kind.as_str(),
        data
    ));
}
