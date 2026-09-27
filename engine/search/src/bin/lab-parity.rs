//! Parity positions from played games (FF-parity-harness): plays games between the two teams of
//! a scenario and writes every decision after the lead turn as a one-decision scenario that
//! `enumerate.cjs` (Showdown) and `lab-check` (lab-engine) can both replay, so the engine's
//! parity is measured on real mid-game positions (statuses, boosts, volatiles, hazards, weather
//! timers, half-fainted teams, replacements) rather than on hand-built one-turn fixtures.
//!
//! Usage: lab-parity <scenario.json> --out-dir <dir> [--name <prefix>] [--games n] [--seed s]
//!                   [--policy random|nash] [--threads k] [--max-turns t] [--first-step i]
//!                   [--rolls median|extremes] [--eval heuristic|material]
//!
//! Each game starts from one of the scenario's initial states (drawn by probability) and runs
//! to its end. At every decision both sides choose by `--policy`: `random` draws uniformly over
//! every legal choice (`Pruning::All`: ally targets, switches, Mega Evolution included), `nash`
//! from the one-turn matrix-game equilibrium `lab-rollout` uses (opponent model ①, `--rolls`
//! and `--eval` shape it). Chance is drawn from the engine's `RollMode::Extremes` distribution
//! (damage rolls only at their minimum and maximum), so every recorded outcome is also an
//! outcome of Showdown's `--mode extremes` and `--mode full` enumerations.
//!
//! A position is the game's history up to one decision, written as a scenario: `startState`
//! pins the leads' switch-in outcome, `setupTurns` are the earlier decisions (a turn with the
//! mid-turn switch choices it asked for, or a replacement), `setupStates` pins each one's
//! outcome by its canonical state, `setupRolls: "extremes"`, and `turn`/`midTurn` are the
//! decision's own choices. Replaying such a scenario costs one enumeration per setup turn on
//! each side (pinned outcome kept), not the product of every setup branch. Team files are
//! referenced by paths relative to `--out-dir`. Files are named
//! `<name>.<policy>.g<game>.s<step>.json`; `<name>.<policy>.games.json` lists the games.
//!
//! Game `g` of seed `s` is deterministic given the engine and the policy.

use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use lab_engine::eval::{Evaluator, Heuristic, Material};
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{EnumerateOptions, RollMode, Suspension};
use lab_scenario::{
    advance_order, canonical_value, load_scenario_file, scenario_positions, LoadedScenario,
    PartyOrder, Position,
};
use lab_search::game::asked_slots;
use lab_search::{
    decision, format_choice, format_switches, legal_choices, transitions, Choice, Config, Decision,
    Pruning, Solver,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Policy {
    Random,
    Nash,
}

impl Policy {
    fn name(self) -> &'static str {
        match self {
            Policy::Random => "random",
            Policy::Nash => "nash",
        }
    }
}

/// One decision of a game as a scenario needs it.
struct Step {
    turn: u16,
    kind: &'static str,
    p1: String,
    p2: String,
    mid_turn: [Vec<String>; 2],
    /// Canonical state after the decision (after its mid-turn switches).
    after: Value,
}

struct GameRecord {
    index: usize,
    seed: u64,
    start: usize,
    start_state: Value,
    steps: Vec<Step>,
    ending: String,
    reason: Option<String>,
    elapsed: f64,
}

/// xorshift64* seeded through splitmix64 (as `lab-rollout`).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Rng((z ^ (z >> 31)).max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn weighted(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut u = self.unit() * total;
        for (i, w) in weights.iter().enumerate() {
            if u < *w {
                return i;
            }
            u -= w;
        }
        weights.len() - 1
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f64) as usize).min(n - 1)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-parity: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    games: usize,
    seed: u64,
    policy: Policy,
    max_turns: u16,
    first_step: usize,
    config: Config,
    game_threads: usize,
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scenario: Option<String> = None;
    let mut out_dir: Option<String> = None;
    let mut name: Option<String> = None;
    let mut games = 4usize;
    let mut seed = 1u64;
    let mut threads = 0usize;
    let mut max_turns = 40u16;
    let mut first_step = 1usize;
    let mut policy = Policy::Random;
    let mut rolls = RollMode::Median;
    let mut eval = "heuristic".to_owned();
    let mut i = 0;
    let number = |i: usize, flag: &str| -> Result<u64, String> {
        args.get(i)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("{flag} needs a number"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                i += 1;
                out_dir = args.get(i).cloned();
            }
            "--name" => {
                i += 1;
                name = args.get(i).cloned();
            }
            "--games" => {
                i += 1;
                games = number(i, "--games")? as usize;
            }
            "--seed" => {
                i += 1;
                seed = number(i, "--seed")?;
            }
            "--threads" => {
                i += 1;
                threads = number(i, "--threads")? as usize;
            }
            "--max-turns" => {
                i += 1;
                max_turns = number(i, "--max-turns")? as u16;
            }
            "--first-step" => {
                i += 1;
                first_step = number(i, "--first-step")? as usize;
            }
            "--policy" => {
                i += 1;
                policy = match args.get(i).map(String::as_str) {
                    Some("random") => Policy::Random,
                    Some("nash") => Policy::Nash,
                    _ => return Err("--policy needs random or nash".into()),
                };
            }
            "--rolls" => {
                i += 1;
                rolls = match args.get(i).map(String::as_str) {
                    Some("median") => RollMode::Median,
                    Some("extremes") => RollMode::Extremes,
                    _ => return Err("--rolls needs median or extremes".into()),
                };
            }
            "--eval" => {
                i += 1;
                eval = args.get(i).cloned().ok_or("--eval needs a value")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => scenario = Some(other.to_owned()),
        }
        i += 1;
    }
    let scenario = scenario.ok_or(
        "usage: lab-parity <scenario.json> --out-dir <dir> [--name prefix] [--games n] [--seed s] \
         [--policy random|nash] [--threads k] [--max-turns t] [--first-step i]",
    )?;
    let out_dir = PathBuf::from(out_dir.ok_or("--out-dir is required")?);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let name = name.unwrap_or_else(|| {
        Path::new(&scenario)
            .file_stem()
            .map_or("game".into(), |s| s.to_string_lossy().into_owned())
    });
    if games == 0 {
        return Err("--games must be at least 1".into());
    }

    let loaded = load_scenario_file(&scenario).map_err(|e| e.to_string())?;
    let positions = scenario_positions(&loaded)?;
    let start_weights: Vec<f64> = positions.iter().map(|p| p.probability).collect();
    let teams = team_refs(&scenario, &out_dir)?;

    let evaluator: Box<dyn Evaluator<2> + Sync> = match eval.as_str() {
        "heuristic" => Box::new(Heuristic),
        "material" => Box::new(Material),
        _ => return Err("--eval needs heuristic or material".into()),
    };
    let game_threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        threads
    }
    .min(games);
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = rolls;
    config.threads = if game_threads == 1 { 0 } else { 1 };
    let options = Options {
        games,
        seed,
        policy,
        max_turns,
        first_step,
        config,
        game_threads,
    };
    println!(
        "{}: {} game(s), seed {seed}, policy {}, extremes chance, {} initial state(s), {game_threads} game thread(s)",
        loaded.meta.description,
        games,
        policy.name(),
        positions.len()
    );

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let records: Mutex<Vec<GameRecord>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.game_threads {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::SeqCst);
                if g >= options.games {
                    break;
                }
                let record = play(
                    &loaded,
                    &positions,
                    &start_weights,
                    evaluator.as_ref(),
                    &options,
                    g,
                );
                println!(
                    "game {:>3} seed {:016x}: {} after {} decision(s){} ({:.1} s)",
                    record.index,
                    record.seed,
                    record.ending,
                    record.steps.len(),
                    record
                        .reason
                        .as_ref()
                        .map(|r| format!(": {r}"))
                        .unwrap_or_default(),
                    record.elapsed
                );
                records.lock().unwrap().push(record);
            });
        }
    });
    let mut records = records.into_inner().unwrap();
    records.sort_by_key(|r| r.index);

    let mut written = 0usize;
    let mut index = Vec::new();
    for record in &records {
        let mut files = Vec::new();
        for s in options.first_step..record.steps.len() {
            let file = format!(
                "{name}.{}.g{:03}.s{:02}.json",
                policy.name(),
                record.index,
                s
            );
            let scenario_json =
                position_scenario(&loaded, &scenario, &teams, &name, policy, record, s);
            let text = serde_json::to_string_pretty(&scenario_json).map_err(|e| e.to_string())?;
            std::fs::write(out_dir.join(&file), text).map_err(|e| format!("{file}: {e}"))?;
            files.push(file);
            written += 1;
        }
        index.push(json!({
            "game": record.index,
            "seed": format!("{:016x}", record.seed),
            "start": record.start,
            "ending": record.ending,
            "reason": record.reason,
            "decisions": record.steps.len(),
            "turns": record.steps.last().map(|s| s.turn),
            "positions": files,
            "elapsed_s": record.elapsed,
        }));
    }
    let games_file = out_dir.join(format!("{name}.{}.games.json", policy.name()));
    let summary = json!({
        "scenario": scenario,
        "name": name,
        "policy": policy.name(),
        "policy_rolls": format!("{rolls:?}"),
        "eval": eval,
        "chance": "extremes (RollMode::Extremes)",
        "seed": seed,
        "games": index,
        "positions_written": written,
        "elapsed_s": started.elapsed().as_secs_f64(),
        "lab_search_version": env!("CARGO_PKG_VERSION"),
    });
    std::fs::write(
        &games_file,
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", games_file.display()))?;
    println!(
        "{written} position(s) written to {} ({:.1} s)",
        out_dir.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Both sides' choices at a decision under the policy.
fn choose<E: Evaluator<2> + ?Sized + Sync>(
    solver: &mut Solver<'_, 2, E>,
    policy: Policy,
    rng: &mut Rng,
    state: &mut State<2>,
    suspension: Option<&Suspension>,
    decision: Decision,
) -> Result<[Choice<2>; 2], String> {
    match policy {
        Policy::Random => {
            let mut out = [Choice::WAIT; 2];
            for side in [SideId::One, SideId::Two] {
                let choices =
                    legal_choices(state, Ruleset::CHAMPIONS_MC, decision, side, Pruning::All);
                if choices.is_empty() {
                    return Err(format!("{side:?} has no legal choice"));
                }
                out[side.index()] = choices[rng.below(choices.len())];
            }
            Ok(out)
        }
        Policy::Nash => {
            let m = solver
                .analyse_mixed(state, suspension)
                .map_err(|e| format!("policy: {e:?}"))?;
            let rows: Vec<f64> = m.equilibrium.rows.iter().map(|&p| f64::from(p)).collect();
            let cols: Vec<f64> = m.equilibrium.cols.iter().map(|&p| f64::from(p)).collect();
            Ok([m.ours[rng.weighted(&rows)], m.theirs[rng.weighted(&cols)]])
        }
    }
}

/// `choice` as the Showdown choice string for `side` at `decision`.
fn describe(
    state: &State<2>,
    decision: Decision,
    side: SideId,
    order: &[PartyOrder; 2],
    choice: &Choice<2>,
) -> String {
    match choice {
        Choice::Turn(action) => format_choice(state, side, &order[side.index()], action),
        Choice::Switches(switches) => format_switches(
            &order[side.index()],
            &asked_slots(state, decision, side),
            switches,
        ),
    }
}

/// One outcome of the pair of choices, drawn from the Extremes distribution.
fn step_outcome(
    rng: &mut Rng,
    state: &mut State<2>,
    decision: Decision,
    suspension: Option<&Suspension>,
    choices: [Choice<2>; 2],
) -> Result<Outcome, String> {
    let options = EnumerateOptions {
        rolls: RollMode::Extremes,
    };
    let outcomes = transitions(
        state,
        Ruleset::CHAMPIONS_MC,
        options,
        decision,
        suspension,
        choices,
    )
    .map_err(|e| e.to_string())?;
    if outcomes.is_empty() {
        return Err("no outcome".into());
    }
    let weights: Vec<f64> = outcomes.iter().map(|o| o.probability).collect();
    let k = rng.weighted(&weights);
    Ok(outcomes.into_iter().nth(k).unwrap())
}

fn play(
    loaded: &LoadedScenario,
    positions: &[Position],
    start_weights: &[f64],
    evaluator: &(dyn Evaluator<2> + Sync),
    options: &Options,
    index: usize,
) -> GameRecord {
    let started = Instant::now();
    let seed =
        Rng::new(options.seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)).next_u64();
    let mut rng = Rng::new(seed);
    let start = rng.weighted(start_weights);
    let mut state = positions[start].state.clone();
    let mut order = positions[start].order.clone();
    let start_state = canonical_value(&state, &loaded.meta).unwrap_or(Value::Null);
    let mut solver = Solver::new(options.config, evaluator);
    let mut steps: Vec<Step> = Vec::new();
    let (ending, reason) = 'game: loop {
        let decision = match decision(&state, None) {
            Ok(d) => d,
            Err(e) => break ("aborted".to_owned(), Some(format!("decision: {e}"))),
        };
        if let Decision::Over(result) = decision {
            let ending = match result {
                BattleResult::Win(SideId::One) => "p1",
                BattleResult::Win(_) => "p2",
                BattleResult::Tie => "tie",
                BattleResult::Ongoing => "ongoing (bug)",
            };
            break (ending.to_owned(), None);
        }
        if state.turn > options.max_turns {
            break ("cutoff".to_owned(), None);
        }
        let turn = state.turn;
        let choices = match choose(
            &mut solver,
            options.policy,
            &mut rng,
            &mut state,
            None,
            decision,
        ) {
            Ok(c) => c,
            Err(e) => break ("aborted".to_owned(), Some(e)),
        };
        let p1 = describe(&state, decision, SideId::One, &order, &choices[0]);
        let p2 = describe(&state, decision, SideId::Two, &order, &choices[1]);
        let outcome = match step_outcome(&mut rng, &mut state, decision, None, choices) {
            Ok(o) => o,
            Err(e) => break ("aborted".to_owned(), Some(format!("turn {turn}: {e}"))),
        };
        state.apply(&outcome.instructions);
        advance_order(&mut order, &outcome.instructions);
        let mut suspension = outcome.suspension;
        let mut mid_turn: [Vec<String>; 2] = [Vec::new(), Vec::new()];
        while let Some(s) = suspension.take() {
            let mid = match lab_search::decision(&state, Some(&s)) {
                Ok(d) => d,
                Err(e) => break 'game ("aborted".to_owned(), Some(format!("mid-turn: {e}"))),
            };
            let switches = match choose(
                &mut solver,
                options.policy,
                &mut rng,
                &mut state,
                Some(&s),
                mid,
            ) {
                Ok(c) => c,
                Err(e) => break 'game ("aborted".to_owned(), Some(e)),
            };
            for side in [SideId::One, SideId::Two] {
                if !asked_slots(&state, mid, side).is_empty() {
                    mid_turn[side.index()].push(describe(
                        &state,
                        mid,
                        side,
                        &order,
                        &switches[side.index()],
                    ));
                }
            }
            let outcome = match step_outcome(&mut rng, &mut state, mid, Some(&s), switches) {
                Ok(o) => o,
                Err(e) => {
                    break 'game (
                        "aborted".to_owned(),
                        Some(format!("turn {turn} mid-turn: {e}")),
                    )
                }
            };
            state.apply(&outcome.instructions);
            advance_order(&mut order, &outcome.instructions);
            suspension = outcome.suspension;
        }
        let after = match canonical_value(&state, &loaded.meta) {
            Ok(v) => v,
            Err(e) => break ("aborted".to_owned(), Some(format!("canonical: {e}"))),
        };
        steps.push(Step {
            turn,
            kind: match decision {
                Decision::Replacement => "Replacement",
                _ => "Turn",
            },
            p1,
            p2,
            mid_turn,
            after,
        });
    };
    GameRecord {
        index,
        seed,
        start,
        start_state,
        steps,
        ending,
        reason,
        elapsed: started.elapsed().as_secs_f64(),
    }
}

/// The scenario of the game's decision `s` (its steps before `s` as pinned setup turns).
fn position_scenario(
    loaded: &LoadedScenario,
    scenario: &str,
    teams: &[(String, Option<String>); 2],
    name: &str,
    policy: Policy,
    record: &GameRecord,
    s: usize,
) -> Value {
    let step = &record.steps[s];
    let setup: Vec<Value> = record.steps[..s]
        .iter()
        .map(|t| {
            if t.mid_turn.iter().all(Vec::is_empty) {
                json!([t.p1, t.p2])
            } else {
                json!([t.p1, t.p2, {"p1": t.mid_turn[0], "p2": t.mid_turn[1]}])
            }
        })
        .collect();
    let states: Vec<Value> = record.steps[..s].iter().map(|t| t.after.clone()).collect();
    let mut out = json!({
        "description": format!(
            "FF parity position (FF-parity-harness): {name}, lab-parity game {} (seed {:016x}, policy {}, extremes chance), decision {s} = turn {} {}. Base scenario {scenario}. The {s} earlier decisions are setupTurns pinned by setupStates. Not a recommendation.",
            record.index, record.seed, policy.name(), step.turn, step.kind
        ),
        "format": loaded.meta.format,
        "p1": {"team": teams[0].0, "order": teams[0].1},
        "p2": {"team": teams[1].0, "order": teams[1].1},
        "startState": record.start_state,
        "setupRolls": "extremes",
        "setupTurns": setup,
        "setupStates": states,
        "turn": {"p1": step.p1, "p2": step.p2},
    });
    if step.mid_turn.iter().any(|m| !m.is_empty()) {
        out["midTurn"] = json!({"p1": step.mid_turn[0], "p2": step.mid_turn[1]});
    }
    out
}

/// The scenario's team paths rewritten relative to `out_dir`, with the team preview orders.
fn team_refs(scenario: &str, out_dir: &Path) -> Result<[(String, Option<String>); 2], String> {
    let text = std::fs::read_to_string(scenario).map_err(|e| format!("{scenario}: {e}"))?;
    let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{scenario}: {e}"))?;
    let base = Path::new(scenario).parent().unwrap_or(Path::new("."));
    let out_abs = std::path::absolute(out_dir).map_err(|e| e.to_string())?;
    let mut refs: [(String, Option<String>); 2] = Default::default();
    for (i, side) in ["p1", "p2"].into_iter().enumerate() {
        let team = json[side]["team"]
            .as_str()
            .ok_or_else(|| format!("{scenario}: {side}.team must be a path"))?;
        let abs = std::path::absolute(base.join(team)).map_err(|e| e.to_string())?;
        refs[i] = (
            relative(&abs, &out_abs),
            json[side]["order"].as_str().map(str::to_owned),
        );
    }
    Ok(refs)
}

/// `target` relative to the directory `from` (both absolute and normalized), with `/`.
fn relative(target: &Path, from: &Path) -> String {
    let t: Vec<Component> = target.components().collect();
    let f: Vec<Component> = from.components().collect();
    let common = t.iter().zip(&f).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..f.len() {
        parts.push("..".into());
    }
    for c in &t[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}
