//! Floor simulation (Phase 3).
//!
//! - `fuzz`: thousands of random runs, buys, redeems, transfers, funds and
//!   abandoned runs across many accounts. After every step the harness
//!   asserts vault ÷ supply did not fall, and a ledger kept outside the
//!   contract asserts the contract can always pay what it owes.
//! - `scenario`: 60 days of play by one of four crowds (few players, many
//!   players, one whale, a bot farm), reported day by day.
//!
//! `cargo test` runs a 2,000-step fuzz and the small-crowd scenario. The full report in
//! `near/simulation/RESULTS.md` is written by
//! `cargo test --release -p pop-game write_simulation_results -- --ignored`.

use super::*;
use std::fmt::Write as _;

// ------------------------------------------------------------------ helpers

/// xorshift64*: small, seeded, reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
    fn between(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }
}

fn near_str(yocto: u128) -> String {
    format!("{:.4}", yocto as f64 / NEAR as f64)
}

fn floor_str(yocto_per_coin: u128) -> String {
    format!("{:.7}", yocto_per_coin as f64 / NEAR as f64)
}

fn coins_str(units: u128) -> String {
    format!("{:.1}", units as f64 / ONE_COIN as f64)
}

/// NEAR that went into and out of the contract, tracked independently.
#[derive(Default)]
struct Ledger {
    fees: u128,
    registrations: u128,
    buys: u128,
    funds: u128,
    yocto: u128,
    redeemed: u128,
    claimed: u128,
}

impl Ledger {
    /// What the contract must hold for its obligations: vault, pending runs,
    /// prepaid credits and the creator's unpaid escrow. Registration fees pay
    /// for storage and are not owed to anyone.
    fn check(&self, t: &T) {
        let held = self.fees + self.buys + self.funds + self.yocto - self.redeemed - self.claimed;
        let owed = t.game.vault + t.game.pending + t.game.credits_held + t.game.creator_earned
            - t.game.creator_claimed;
        assert_eq!(held, owed, "contract holds {held} but owes {owed}");
    }
}

/// Wraps the harness with the ledger and counts what happened.
struct Sim {
    t: T,
    ledger: Ledger,
    runs: u64,
    buys: u64,
    redeems: u64,
    transfers: u64,
    funds: u64,
    expired: u64,
    claims: u64,
    min_floor: u128,
    max_floor: u128,
}

impl Sim {
    fn new(params: Params) -> Sim {
        Sim {
            t: T::with(params, None),
            ledger: Ledger::default(),
            runs: 0,
            buys: 0,
            redeems: 0,
            transfers: 0,
            funds: 0,
            expired: 0,
            claims: 0,
            min_floor: u128::MAX,
            max_floor: 0,
        }
    }

    /// Reads a balance in a fresh call context, so many reads in a row do not run out of gas.
    fn bal(&mut self, who: &str) -> u128 {
        self.t.ctx("viewer.testnet", 0);
        self.t.balance(who)
    }

    fn after(&mut self) {
        self.ledger.check(&self.t);
        let f = self.t.floor();
        if self.t.game.token.total_supply > 0 {
            self.min_floor = self.min_floor.min(f);
            self.max_floor = self.max_floor.max(f);
        }
    }

    fn play(&mut self, who: &str, score: u32) -> u128 {
        let reg = self.t.game.registration_needed(&acc(who));
        self.ledger.fees += self.t.game.params.entry_fee.0;
        self.ledger.registrations += reg;
        let mined = self.t.play(who, score);
        self.runs += 1;
        self.after();
        mined
    }

    fn abandon(&mut self, who: &str) {
        let reg = self.t.game.registration_needed(&acc(who));
        self.ledger.fees += self.t.game.params.entry_fee.0;
        self.ledger.registrations += reg;
        let run = self.t.start(who);
        self.after();
        self.t.advance(self.t.game.params.max_run_secs);
        self.t.ctx("keeper.testnet", 0);
        self.t.game.expire_run(run.session_id);
        self.t.check_floor("expire_run");
        self.expired += 1;
        self.after();
    }

    fn buy(&mut self, who: &str, amount: u128) -> u128 {
        let reg = self.t.game.registration_needed(&acc(who));
        let cost = self.t.buy(who, amount);
        self.ledger.buys += cost;
        self.ledger.registrations += reg;
        self.buys += 1;
        self.after();
        cost
    }

    fn redeem(&mut self, who: &str, amount: u128) -> u128 {
        let paid = self.t.redeem(who, amount);
        self.ledger.redeemed += paid;
        self.ledger.yocto += 1;
        self.redeems += 1;
        self.after();
        paid
    }

    fn transfer(&mut self, from: &str, to: &str, amount: u128) {
        let reg = self.t.game.registration_needed(&acc(to));
        if reg > 0 {
            self.t.ctx(to, reg);
            self.t.game.storage_deposit(None, None);
            self.ledger.registrations += reg;
        }
        self.t.ctx(from, 1);
        self.t.game.ft_transfer(acc(to), U128(amount), None);
        self.t.check_floor("ft_transfer");
        // The 1 yoctoNEAR an ft_transfer needs stays in the contract, owed to no one.
        self.transfers += 1;
        self.after();
    }

    fn fund(&mut self, amount: u128) {
        self.t.ctx("donor.testnet", amount);
        self.t.game.fund();
        self.t.check_floor("fund");
        self.ledger.funds += amount;
        self.funds += 1;
        self.after();
    }

    fn claim(&mut self) {
        if self.t.game.creator_unlocked_raw() > self.t.game.creator_claimed {
            self.t.ctx(CREATOR, 0);
            let paid = self.t.game.claim_creator().0;
            self.ledger.claimed += paid;
            self.claims += 1;
            self.after();
        }
    }
}

// --------------------------------------------------------------------- fuzz

pub struct FuzzReport {
    pub steps: u64,
    pub seed: u64,
    pub accounts: usize,
    pub runs: u64,
    pub buys: u64,
    pub redeems: u64,
    pub transfers: u64,
    pub funds: u64,
    pub expired: u64,
    pub claims: u64,
    pub mining_closed_at_run: Option<u64>,
    pub start_floor: u128,
    pub min_floor: u128,
    pub end_floor: u128,
    pub supply: u128,
    pub vault: u128,
}

/// Random steps across `accounts` players. Mining closes after `run_cap` runs, so both phases are covered.
pub fn fuzz(steps: u64, seed: u64, accounts: usize, run_cap: u64) -> FuzzReport {
    let mut s = Sim::new(Params {
        mining_run_cap: run_cap,
        ..Params::standard()
    });
    let mut r = Rng(seed);
    let names: Vec<String> = (0..accounts).map(|i| format!("fz{i}.testnet")).collect();
    let mut closed_at = None;
    let mut start_floor = None;
    for _ in 0..steps {
        let who = names[r.below(accounts as u64) as usize].clone();
        let roll = r.below(100);
        let open = !s.t.game.mining_closed;
        match roll {
            // Runs, weighted towards play; scores 0..=80 so both ends of the curve are hit.
            0..=54 => {
                s.play(&who, r.below(81) as u32);
            }
            55..=57 => s.abandon(&who),
            58..=74 => {
                let bal = s.bal(&who);
                if bal > 0 {
                    let part = r.between(1, 100) as u128;
                    s.redeem(&who, (bal * part / 100).max(1));
                }
            }
            75..=89 if !open => {
                s.buy(&who, r.between(1, 400) as u128 * ONE_COIN / r.between(1, 4) as u128);
            }
            90..=94 => {
                let bal = s.bal(&who);
                if bal > 1 {
                    let to = names[r.below(accounts as u64) as usize].clone();
                    if to != who {
                        s.transfer(&who, &to, bal / 2);
                    }
                }
            }
            95..=96 => s.fund(r.between(1, 1_000) as u128 * MILLI_NEAR / 100),
            97..=99 => s.claim(),
            _ => {
                s.play(&who, r.below(81) as u32);
            }
        }
        if start_floor.is_none() && s.t.game.token.total_supply > 0 {
            start_floor = Some(s.t.floor());
        }
        if closed_at.is_none() && s.t.game.mining_closed {
            closed_at = Some(s.runs);
        }
    }
    FuzzReport {
        steps,
        seed,
        accounts,
        runs: s.runs,
        buys: s.buys,
        redeems: s.redeems,
        transfers: s.transfers,
        funds: s.funds,
        expired: s.expired,
        claims: s.claims,
        mining_closed_at_run: closed_at,
        start_floor: start_floor.unwrap_or(0),
        min_floor: s.min_floor,
        end_floor: s.t.floor(),
        supply: s.t.game.token.total_supply,
        vault: s.t.game.vault,
    }
}

#[test]
fn floor_never_falls_across_2000_random_steps() {
    let r = fuzz(2_000, 0x5eed_0001, 40, 600);
    assert!(r.runs > 1_000);
    assert!(r.buys > 20 && r.redeems > 80 && r.transfers > 20 && r.expired > 20);
    assert!(r.mining_closed_at_run.is_some());
    assert!(r.end_floor >= r.start_floor);
}

// ---------------------------------------------------------------- scenarios

#[derive(Clone, Copy, PartialEq)]
pub enum Crowd {
    Few,
    Many,
    Whale,
    BotFarm,
}

impl Crowd {
    pub fn title(&self) -> &'static str {
        match self {
            Crowd::Few => "Few players",
            Crowd::Many => "Many players",
            Crowd::Whale => "One whale",
            Crowd::BotFarm => "Bot farm",
        }
    }
    pub fn description(&self) -> &'static str {
        match self {
            Crowd::Few => "6 regulars, each 2 runs a day at their own skill (scores 10-45).",
            Crowd::Many => "120 players; each plays on 15% of days, 1-3 runs, skill-based scores. After mining closes some buy and some redeem every day.",
            Crowd::Whale => "30 casual players plus one whale who plays 30 runs a day at score 45, buys 3,000 coins the day after mining closes, and redeems half on day 40.",
            Crowd::BotFarm => "60 bot wallets playing 2 runs a day each at scores 0-3, next to 30 honest players.",
        }
    }
}

pub struct Day {
    pub day: u32,
    pub runs: u64,
    pub total_runs: u64,
    pub mining_open: bool,
    pub supply: u128,
    pub vault: u128,
    pub floor: u128,
    pub spot: u128,
}

/// What one group of accounts put in, took out and holds at the end.
#[derive(Default, Clone)]
pub struct Group {
    pub name: &'static str,
    pub accounts: Vec<String>,
    pub spent: u128,
    pub received: u128,
    pub mined: u128,
    pub bought: u128,
}

pub struct ScenarioReport {
    pub crowd: Crowd,
    pub days: Vec<Day>,
    pub groups: Vec<Group>,
    pub closed_on_day: Option<u32>,
    pub end_floor: u128,
    pub supply: u128,
    pub creator_earned: u128,
    pub runs: u64,
    /// Coin units each group holds at the end, in `groups` order.
    pub held: Vec<u128>,
}

struct Player {
    name: String,
    group: usize,
    skill: u32,
}

fn score_for(r: &mut Rng, skill: u32) -> u32 {
    let lo = skill.saturating_sub(10) as u64;
    r.between(lo, skill as u64 + 10) as u32
}

pub fn scenario(crowd: Crowd, seed: u64) -> ScenarioReport {
    let mut s = Sim::new(Params::standard());
    let mut r = Rng(seed);
    let mut groups: Vec<Group> = vec![];
    let mut players: Vec<Player> = vec![];
    let add_group = |groups: &mut Vec<Group>, players: &mut Vec<Player>, name: &'static str, n: usize, skill: (u32, u32), r: &mut Rng| {
        let g = groups.len();
        let mut accounts = vec![];
        for i in 0..n {
            let account = format!("{}{}.testnet", name.to_lowercase().replace(' ', ""), i);
            accounts.push(account.clone());
            players.push(Player {
                name: account,
                group: g,
                skill: r.between(skill.0 as u64, skill.1 as u64) as u32,
            });
        }
        groups.push(Group {
            name,
            accounts,
            ..Default::default()
        });
    };
    match crowd {
        Crowd::Few => add_group(&mut groups, &mut players, "Regulars", 6, (10, 45), &mut r),
        Crowd::Many => add_group(&mut groups, &mut players, "Players", 120, (5, 50), &mut r),
        Crowd::Whale => {
            add_group(&mut groups, &mut players, "Casuals", 30, (5, 40), &mut r);
            add_group(&mut groups, &mut players, "Whale", 1, (45, 45), &mut r);
        }
        Crowd::BotFarm => {
            add_group(&mut groups, &mut players, "Honest", 30, (15, 45), &mut r);
            add_group(&mut groups, &mut players, "Bots", 60, (1, 1), &mut r);
        }
    }

    let mut days = vec![];
    let mut closed_on_day = None;
    for day in 1..=60u32 {
        let runs_before = s.runs;
        let open = !s.t.game.mining_closed;
        // Who plays today.
        let mut plays: Vec<(usize, u32)> = vec![];
        for (i, p) in players.iter().enumerate() {
            let g = groups[p.group].name;
            let n = match (crowd, g) {
                (Crowd::Few, _) => 2,
                (Crowd::Many, _) => {
                    if r.chance(if open { 150 } else { 60 }) {
                        r.between(1, 3)
                    } else {
                        0
                    }
                }
                (Crowd::Whale, "Whale") => {
                    if open {
                        30
                    } else {
                        0
                    }
                }
                (Crowd::Whale, _) => {
                    if r.chance(300) {
                        r.between(1, 2)
                    } else {
                        0
                    }
                }
                (Crowd::BotFarm, "Bots") => {
                    if open {
                        2
                    } else {
                        0
                    }
                }
                (Crowd::BotFarm, _) => {
                    if r.chance(400) {
                        r.between(1, 2)
                    } else {
                        0
                    }
                }
            };
            for _ in 0..n {
                let score = if g == "Bots" { r.below(4) as u32 } else { score_for(&mut r, p.skill) };
                plays.push((i, score));
            }
        }
        // Interleave the day's runs.
        for k in (1..plays.len()).rev() {
            let j = r.below(k as u64 + 1) as usize;
            plays.swap(k, j);
        }
        for (i, score) in plays {
            let p = &players[i];
            let fee = s.t.game.params.entry_fee.0 + s.t.game.registration_needed(&acc(&p.name));
            let mined = s.play(&p.name, score);
            let g = &mut groups[p.group];
            g.spent += fee;
            g.mined += mined;
        }

        // Market activity once mining has closed.
        if s.t.game.mining_closed {
            if closed_on_day.is_none() {
                closed_on_day = Some(day);
            }
            for p in &players {
                let (name, gi) = (p.name.clone(), p.group);
                let g = groups[gi].name;
                let (buy_pm, redeem_pm) = match crowd {
                    Crowd::Few => (150, 100),
                    Crowd::Many => (50, 30),
                    Crowd::Whale if g == "Whale" => (0, 0),
                    Crowd::Whale => (60, 40),
                    Crowd::BotFarm if g == "Bots" => (0, 200),
                    Crowd::BotFarm => (60, 40),
                };
                if r.chance(buy_pm) {
                    let amount = r.between(10, 100) as u128 * ONE_COIN;
                    let reg = s.t.game.registration_needed(&acc(&name));
                    let cost = s.buy(&name, amount);
                    groups[gi].spent += cost + reg;
                    groups[gi].bought += amount;
                }
                let bal = s.bal(&name);
                if bal > 0 && r.chance(redeem_pm) {
                    let paid = s.redeem(&name, bal / 4);
                    groups[gi].received += paid;
                }
            }
            if crowd == Crowd::Whale {
                let whale = groups[1].accounts[0].clone();
                if closed_on_day == Some(day - 1) || (closed_on_day == Some(day) && day == 60) {
                    let reg = s.t.game.registration_needed(&acc(&whale));
                    let cost = s.buy(&whale, 3_000 * ONE_COIN);
                    groups[1].spent += cost + reg;
                    groups[1].bought += 3_000 * ONE_COIN;
                }
                if day == 40 {
                    let bal = s.bal(&whale);
                    let paid = s.redeem(&whale, bal / 2);
                    groups[1].received += paid;
                }
            }
        }
        if day % 7 == 0 {
            s.claim();
        }
        s.t.advance(86_400);
        days.push(Day {
            day,
            runs: s.runs - runs_before,
            total_runs: s.runs,
            mining_open: !s.t.game.mining_closed,
            supply: s.t.game.token.total_supply,
            vault: s.t.game.vault,
            floor: s.t.floor(),
            spot: s.t.game.spot_price().0,
        });
    }
    let held = groups
        .iter()
        .map(|g| g.accounts.iter().map(|a| s.bal(a)).sum())
        .collect();
    ScenarioReport {
        crowd,
        days,
        groups,
        closed_on_day,
        end_floor: s.t.floor(),
        supply: s.t.game.token.total_supply,
        creator_earned: s.t.game.creator_earned,
        runs: s.runs,
        held,
    }
}

#[test]
fn a_small_crowd_keeps_the_floor_for_60_days() {
    // The other crowds run in write_simulation_results.
    let r = scenario(Crowd::Few, 7);
    assert!(r.days.windows(2).all(|w| w[1].floor >= w[0].floor || w[0].supply == 0));
    assert!(r.end_floor >= Params::standard().launch_price.0);
}

// ------------------------------------------------------------------- report

fn render(fz: &[FuzzReport], scenarios: &[ScenarioReport]) -> String {
    let mut o = String::new();
    let p = Params::standard();
    writeln!(o, "# Floor simulation results\n").unwrap();
    writeln!(
        o,
        "Generated by `cargo test --release -p pop-game write_simulation_results -- --ignored` \
         (source: `near/game/src/tests/sim.rs`). Every step runs the real contract code in \
         near-sdk's mocked runtime. After **every** action the harness asserts that vault ÷ \
         supply did not fall, and an independent ledger of NEAR in and out asserts the contract \
         holds exactly what it owes (vault, open runs, prepaid runs and the creator's unpaid \
         escrow). Any failure stops the run, so these numbers only exist because no step failed.\n"
    )
    .unwrap();
    writeln!(o, "Params: the standard game (0.02 NEAR fee, 70% to the vault, launch price 0.0001 NEAR, \
        target score 40, wallet cap 500, mining closes at 10,000 coins or the run cap).\n").unwrap();

    writeln!(o, "## Random fuzz\n").unwrap();
    writeln!(o, "| Seed | Steps | Accounts | Runs | Abandoned | Buys | Redeems | Transfers | Funds | Claims | Mining closed at run | First floor (NEAR/coin) | Lowest floor | Final floor | Final supply (coins) | Final vault (NEAR) |").unwrap();
    writeln!(o, "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
    for f in fz {
        writeln!(
            o,
            "| `{:#x}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            f.seed,
            f.steps,
            f.accounts,
            f.runs,
            f.expired,
            f.buys,
            f.redeems,
            f.transfers,
            f.funds,
            f.claims,
            f.mining_closed_at_run.map(|r| r.to_string()).unwrap_or("-".into()),
            floor_str(f.start_floor),
            floor_str(f.min_floor),
            floor_str(f.end_floor),
            coins_str(f.supply),
            near_str(f.vault)
        )
        .unwrap();
    }
    let total: u64 = fz.iter().map(|f| f.steps).sum();
    writeln!(o, "\n{total} random steps in all; the floor never fell and the ledger always balanced.\n").unwrap();

    writeln!(o, "## 60-day scenarios\n").unwrap();
    writeln!(o, "Each day's runs are shuffled together. Once mining closes, players buy on the curve \
        and redeem against the vault at the rates described. The creator claims weekly.\n").unwrap();
    for s in scenarios {
        writeln!(o, "### {}\n", s.crowd.title()).unwrap();
        writeln!(o, "{}\n", s.crowd.description()).unwrap();
        writeln!(
            o,
            "{} runs; mining closed on {}; final floor **{} NEAR** per coin ({:.1}× the launch price); \
             {} coins in supply; creator earned {} NEAR.\n",
            s.runs,
            s.closed_on_day.map(|d| format!("day {d}")).unwrap_or("no day (still open)".into()),
            floor_str(s.end_floor),
            s.end_floor as f64 / p.launch_price.0 as f64,
            coins_str(s.supply),
            near_str(s.creator_earned)
        )
        .unwrap();
        writeln!(o, "| Group | Accounts | NEAR in | NEAR out | Coins mined | Coins bought | Coins held | Held × floor (NEAR) | Net (NEAR) | Mined per NEAR in |").unwrap();
        writeln!(o, "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
        for (g, h) in s.groups.iter().zip(&s.held) {
            let value = mul_div_down(*h, s.end_floor, ONE_COIN);
            let net = (g.received + value) as i128 - g.spent as i128;
            writeln!(
                o,
                "| {} | {} | {} | {} | {} | {} | {} | {} | {}{} | {:.1} |",
                g.name,
                g.accounts.len(),
                near_str(g.spent),
                near_str(g.received),
                coins_str(g.mined),
                coins_str(g.bought),
                coins_str(*h),
                near_str(value),
                if net < 0 { "-" } else { "+" },
                near_str(net.unsigned_abs()),
                if g.spent == 0 { 0.0 } else { (g.mined as f64 / ONE_COIN as f64) / (g.spent as f64 / NEAR as f64) }
            )
            .unwrap();
        }
        writeln!(o, "\n<details><summary>Day by day</summary>\n").unwrap();
        writeln!(o, "| Day | Runs | Total runs | Mining | Supply (coins) | Vault (NEAR) | Floor (NEAR/coin) | Next coin on the curve |").unwrap();
        writeln!(o, "| --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
        for d in &s.days {
            writeln!(
                o,
                "| {} | {} | {} | {} | {} | {} | {} | {} |",
                d.day,
                d.runs,
                d.total_runs,
                if d.mining_open { "open" } else { "closed" },
                coins_str(d.supply),
                near_str(d.vault),
                floor_str(d.floor),
                floor_str(d.spot)
            )
            .unwrap();
        }
        writeln!(o, "\n</details>\n").unwrap();
    }
    writeln!(o, "## Reading the bot farm\n").unwrap();
    if let Some(b) = scenarios.iter().find(|s| s.crowd == Crowd::BotFarm) {
        let (honest, bots) = (&b.groups[0], &b.groups[1]);
        let bot_net = (bots.received + mul_div_down(b.held[1], b.end_floor, ONE_COIN)) as i128
            - bots.spent as i128;
        writeln!(
            o,
            "Bots at scores 0-3 mine only 25-31% of each run's budget, and the rest of every fee \
             stays in the vault, so their runs raise the floor for everyone. But mining closes \
             after 300 runs, so a farm can use up the mining phase: here it closed on day {}, and \
             the {} bot wallets mined {} of the {} coins mined in all. The farm still ended {} \
             NEAR {} after redeeming. The per-wallet cap does not stop one person with many \
             wallets; that is a stated limit, and Phase 6 caps free runs per login.",
            b.closed_on_day.unwrap_or(0),
            bots.accounts.len(),
            coins_str(bots.mined),
            coins_str(bots.mined + honest.mined),
            near_str(bot_net.unsigned_abs()),
            if bot_net < 0 { "down" } else { "up" },
        )
        .unwrap();
    }
    o
}

/// Writes near/simulation/RESULTS.md. Run with --release; it takes a minute or two.
#[test]
#[ignore]
fn write_simulation_results() {
    let fz = vec![
        fuzz(3_000, 0x5eed_0001, 60, 1_000),
        fuzz(3_000, 0x5eed_0002, 150, 2_000),
        fuzz(3_000, 0x5eed_0003, 25, 300),
    ];
    let scenarios: Vec<ScenarioReport> = [Crowd::Few, Crowd::Many, Crowd::Whale, Crowd::BotFarm]
        .into_iter()
        .enumerate()
        .map(|(i, crowd)| scenario(crowd, 100 + i as u64))
        .collect();
    let out = render(&fz, &scenarios);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../simulation");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("RESULTS.md"), out).unwrap();
}
