//! Self-play rollouts under the one-turn equilibrium policy: at every decision both sides draw
//! their choice from the matrix-game equilibrium `lab-plan --solve nash` would print (opponent
//! model ①, the same evaluator on both sides), the turn is then played once with exact chance,
//! and the game runs to its end. The tally of wins compares two leads or two teams on game
//! results rather than on the evaluator's scale — the evaluator still shapes every choice, so
//! this is "how the two do under the same policy", not their true strength (DESIGN.md "탐색의
//! 용도와 정보 모델", AGENTS.md "대전과 비교 방법").
//!
//! Usage: lab-rollout <scenario.json> [--games n] [--seed s] [--threads k] [--max-turns t]
//!                    [--policy nash|deep-nash [--beam b] [--outcomes k]]
//!                    [--rolls median|extremes|quartiles|full|pessimistic]
//!                    [--eval material|heuristic|file:<weights.json>]
//!                    [--setup-rolls full|median|extremes|quartiles] [--position i]
//!                    [--out results.json] [--quiet]
//!
//! The start is the scenario's position (after switch-ins, setup turns and patch); with several
//! initial states one is drawn per game by its probability (`--position` fixes one). `--rolls`
//! is the damage-roll mode the policy's matrix game is solved with (default median); the game
//! itself always runs with exact chance (`sample_turn`). `--policy deep-nash` draws from the
//! depth-2 mixed equilibrium instead (`Solver::analyse_deep_mixed`: both sides' `--beam` best
//! choices plus their shallow supports, children worth their next-turn equilibrium over the
//! `--outcomes` most probable outcomes) — much slower per decision, less bound to the
//! evaluator's one-turn view. Game `g` of seed `s` is deterministic
//! given the engine, the evaluator and the policy. A game the solver cannot value (an effect the
//! engine does not implement) is aborted, a game still going after `--max-turns` turns is a
//! cutoff; both are reported outside the win tally (AGENTS.md: 중단 경기는 승패 집계에서 제외).
//! With `--threads k` (default: the machine's cores) `k` games run at once, each solving its
//! matrix games on one thread; `--threads 1` runs the games in turn, each solving on all cores.
//! The policy's strategies are cached across games by position (state and suspension hash), so
//! the opening decision is solved once per initial state and repeated positions are reused;
//! the policy is deterministic, so the cache changes nothing but the time.

use std::collections::HashMap;
use std::io::Write as _;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use lab_engine::eval::{Evaluator, Heuristic, Material, Weighted, FEATURE_COUNT, FEATURE_NAMES};
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{
    enumerate_replacements, sample_resume_turn, sample_turn, RollMode, Suspension,
};
use lab_scenario::{
    advance_order, load_scenario_file, scenario_positions_with, LoadedScenario, PartyOrder,
    Position,
};
use lab_search::solve::SearchError;
use lab_search::{format_choice, Choice, Config, Decision, Equilibrium, Solver};

/// Both sides' mixed strategies at one decision, as the policy produced them.
#[derive(Clone)]
struct Strategies {
    ours: Vec<Choice<2>>,
    theirs: Vec<Choice<2>>,
    equilibrium: Equilibrium,
}

/// A position: the state and what the turn still waits for.
type PositionKey = (State<2>, Option<Suspension>);

/// Strategies by position, shared by every game of the batch.
struct StrategyCache {
    /// Keyed by the position itself: a 64-bit hash alone would hand one position another's
    /// strategies on a collision (board B32).
    entries: Mutex<HashMap<PositionKey, Strategies>>,
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl StrategyCache {
    fn new() -> StrategyCache {
        StrategyCache {
            entries: Mutex::new(HashMap::new()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    fn key(state: &State<2>, suspension: Option<&Suspension>) -> PositionKey {
        (state.clone(), suspension.cloned())
    }

    fn get(&self, key: &PositionKey) -> Option<Strategies> {
        let found = self.entries.lock().unwrap().get(key).cloned();
        if found.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        found
    }

    fn put(&self, key: PositionKey, strategies: Strategies) {
        self.entries.lock().unwrap().insert(key, strategies);
    }
}

/// Which equilibrium the sides draw their choices from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Policy {
    /// The one-turn matrix game (`analyse_mixed`).
    Nash,
    /// The depth-2 matrix game over both beams (`analyse_deep_mixed`).
    DeepNash,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-rollout: {e}");
            ExitCode::FAILURE
        }
    }
}

/// How a game ended.
#[derive(Clone, Debug, PartialEq)]
enum Ending {
    Win(SideId),
    Tie,
    /// Still running after `--max-turns` turns.
    Cutoff,
    /// The solver or the engine refused (an unimplemented effect, an internal error).
    Aborted(String),
}

impl Ending {
    fn label(&self) -> &'static str {
        match self {
            Ending::Win(SideId::One) => "p1",
            Ending::Win(_) => "p2",
            Ending::Tie => "tie",
            Ending::Cutoff => "cutoff",
            Ending::Aborted(_) => "aborted",
        }
    }
}

struct GameRecord {
    index: usize,
    seed: u64,
    start: usize,
    ending: Ending,
    turns: u16,
    decisions: Vec<Value>,
    elapsed: f64,
}

/// xorshift64* seeded through splitmix64, so consecutive game numbers give unrelated streams.
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

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// An index drawn by `weights` (need not sum to 1; the last index absorbs rounding).
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
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scenario: Option<String> = None;
    let mut games: usize = 20;
    let mut seed: u64 = 1;
    let mut threads: usize = 0;
    let mut max_turns: u16 = 30;
    let mut rolls = RollMode::Median;
    let mut setup_rolls = RollMode::Full;
    let mut policy = Policy::Nash;
    let mut beam: usize = 3;
    let mut outcomes: Option<usize> = Some(2);
    let mut eval = "heuristic".to_owned();
    let mut position_index: Option<usize> = None;
    let mut out: Option<String> = None;
    let mut quiet = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--games" => {
                i += 1;
                games = parse(&args, i, "--games")?;
            }
            "--seed" => {
                i += 1;
                seed = parse(&args, i, "--seed")?;
            }
            "--threads" => {
                i += 1;
                threads = parse(&args, i, "--threads")?;
            }
            "--max-turns" => {
                i += 1;
                max_turns = parse(&args, i, "--max-turns")?;
            }
            "--position" => {
                i += 1;
                position_index = Some(parse(&args, i, "--position")?);
            }
            "--rolls" => {
                i += 1;
                rolls = roll_mode(args.get(i), "--rolls")?;
            }
            "--policy" => {
                i += 1;
                policy = match args.get(i).map(String::as_str) {
                    Some("nash") => Policy::Nash,
                    Some("deep-nash") => Policy::DeepNash,
                    _ => return Err("--policy needs nash or deep-nash".into()),
                };
            }
            "--beam" => {
                i += 1;
                beam = parse(&args, i, "--beam")?;
            }
            "--outcomes" => {
                i += 1;
                outcomes = Some(parse(&args, i, "--outcomes")?);
            }
            "--setup-rolls" => {
                i += 1;
                setup_rolls = roll_mode(args.get(i), "--setup-rolls")?;
            }
            "--eval" => {
                i += 1;
                eval = args
                    .get(i)
                    .cloned()
                    .ok_or("--eval needs material, heuristic or file:<weights.json>")?;
            }
            "--out" => {
                i += 1;
                out = Some(args.get(i).cloned().ok_or("--out needs a file")?);
            }
            "--quiet" => quiet = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => scenario = Some(other.to_owned()),
        }
        i += 1;
    }
    let scenario = scenario.ok_or(
        "usage: lab-rollout <scenario.json> [--games n] [--seed s] [--threads k] [--max-turns t] \
         [--rolls median|extremes|quartiles|full|pessimistic] [--eval material|heuristic|file:<w.json>] \
         [--setup-rolls ...] [--position i] [--out results.json] [--quiet]",
    )?;
    if games == 0 {
        return Err("--games must be at least 1".into());
    }
    let game_threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        threads
    }
    .min(games);

    let loaded = load_scenario_file(&scenario).map_err(|e| e.to_string())?;
    let setup_options = lab_engine::turn::EnumerateOptions { rolls: setup_rolls };
    let mut positions = scenario_positions_with(&loaded, setup_options)?;
    if let Some(index) = position_index {
        let n = positions.len();
        let chosen = positions
            .into_iter()
            .nth(index)
            .ok_or_else(|| format!("--position {index}: only {n} initial states"))?;
        positions = vec![Position {
            probability: 1.0,
            ..chosen
        }];
    }
    let start_weights: Vec<f64> = positions.iter().map(|p| p.probability).collect();

    let evaluator: Box<dyn Evaluator<2> + Sync> = if eval == "material" {
        Box::new(Material)
    } else if let Some(path) = eval.strip_prefix("file:") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut weights = [0.0f32; FEATURE_COUNT];
        for (k, name) in FEATURE_NAMES.iter().enumerate() {
            weights[k] = value["weights"][*name]
                .as_f64()
                .ok_or_else(|| format!("{path}: weights.{name} missing"))?
                as f32;
        }
        Box::new(Weighted { weights })
    } else if eval == "heuristic" {
        Box::new(Heuristic)
    } else {
        return Err("--eval needs material, heuristic or file:<weights.json>".into());
    };

    // The policy: opponent model ① at depth 1, mixed. Each game's solver works on one thread
    // when games run in parallel, on all cores when they run in turn.
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = rolls;
    config.threads = if game_threads == 1 { 0 } else { 1 };
    config.outcome_cap = outcomes;
    let policy_text = match policy {
        Policy::Nash => "nash depth 1".to_owned(),
        Policy::DeepNash => format!("deep-nash beam {beam} outcomes {outcomes:?}"),
    };

    let teams = team_files(&scenario)?;
    println!(
        "{}\nformat {}, {} initial state(s); {} games, seed {seed}, policy {policy_text} rolls {:?} eval {eval}, exact chance in play, max {max_turns} turns, {game_threads} game thread(s)",
        loaded.meta.description,
        loaded.meta.format,
        positions.len(),
        games,
        rolls
    );
    for (side, path, hash) in &teams {
        println!("  {side} team {path} (fnv1a64 {hash:016x})");
    }

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let cache = StrategyCache::new();
    let records: Mutex<Vec<GameRecord>> = Mutex::new(Vec::with_capacity(games));
    let stdout = Mutex::new(std::io::stdout());
    std::thread::scope(|scope| {
        for _ in 0..game_threads {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::SeqCst);
                if g >= games {
                    break;
                }
                let record = play_game(
                    &loaded,
                    &positions,
                    &start_weights,
                    config,
                    evaluator.as_ref(),
                    g,
                    seed,
                    max_turns,
                    policy,
                    beam,
                    &cache,
                );
                if !quiet {
                    let mut o = stdout.lock().unwrap();
                    let _ = writeln!(
                        o,
                        "game {:>3} seed {:016x} start {}: {} after {} turn(s) ({:.1} s){}",
                        record.index,
                        record.seed,
                        record.start,
                        match &record.ending {
                            Ending::Win(side) => format!("{} wins", side_name(*side)),
                            Ending::Tie => "tie".to_owned(),
                            Ending::Cutoff => "cutoff".to_owned(),
                            Ending::Aborted(_) => "aborted".to_owned(),
                        },
                        record.turns,
                        record.elapsed,
                        match &record.ending {
                            Ending::Aborted(why) => format!(": {why}"),
                            _ => String::new(),
                        }
                    );
                }
                records.lock().unwrap().push(record);
            });
        }
    });
    let elapsed = started.elapsed().as_secs_f64();
    let mut records = records.into_inner().unwrap();
    records.sort_by_key(|r| r.index);

    let count = |f: &dyn Fn(&Ending) -> bool| records.iter().filter(|r| f(&r.ending)).count();
    let p1 = count(&|e| *e == Ending::Win(SideId::One));
    let p2 = count(&|e| *e == Ending::Win(SideId::Two));
    let ties = count(&|e| *e == Ending::Tie);
    let cutoffs = count(&|e| *e == Ending::Cutoff);
    let aborted = count(&|e| matches!(e, Ending::Aborted(_)));
    let decided = p1 + p2 + ties;
    let (rate, lo, hi) = wilson(p1 as f64 + 0.5 * ties as f64, decided);
    let mean_turns = {
        let finished: Vec<f64> = records
            .iter()
            .filter(|r| matches!(r.ending, Ending::Win(_) | Ending::Tie))
            .map(|r| f64::from(r.turns))
            .collect();
        if finished.is_empty() {
            f64::NAN
        } else {
            finished.iter().sum::<f64>() / finished.len() as f64
        }
    };
    let (cache_hits, cache_misses) = (
        cache.hits.load(Ordering::Relaxed),
        cache.misses.load(Ordering::Relaxed),
    );
    println!(
        "\ntally: p1 {p1}, p2 {p2}, tie {ties} of {decided} decided; cutoff {cutoffs}, aborted {aborted}; p1 score {:.1}% (ties half; Wilson 95% {:.1}–{:.1}%), mean {:.1} turns, {:.1} s; strategy cache {cache_hits} hits / {cache_misses} solves",
        rate * 100.0,
        lo * 100.0,
        hi * 100.0,
        mean_turns,
        elapsed
    );
    let mut reasons: Vec<(&String, usize)> = Vec::new();
    for r in &records {
        if let Ending::Aborted(why) = &r.ending {
            match reasons.iter_mut().find(|(w, _)| *w == why) {
                Some(entry) => entry.1 += 1,
                None => reasons.push((why, 1)),
            }
        }
    }
    if !reasons.is_empty() {
        println!("aborted games:");
        for (why, n) in &reasons {
            println!("  {n} x {why}");
        }
    }

    if let Some(path) = out {
        let report = json!({
            "scenario": scenario,
            "description": loaded.meta.description,
            "format": loaded.meta.format,
            "teams": teams.iter().map(|(side, path, hash)| json!({"side": side, "file": path, "fnv1a64": format!("{hash:016x}")})).collect::<Vec<_>>(),
            "games": games,
            "seed": seed,
            "policy": {"solve": match policy { Policy::Nash => "nash", Policy::DeepNash => "deep-nash" }, "depth": match policy { Policy::Nash => 1, Policy::DeepNash => 2 }, "beam": match policy { Policy::Nash => Value::Null, Policy::DeepNash => json!(beam) }, "outcomes": outcomes, "rolls": format!("{rolls:?}"), "eval": eval, "chance_in_play": "exact (sample_turn)"},
            "max_turns": max_turns,
            "initial_states": positions.len(),
            "tally": {"p1": p1, "p2": p2, "tie": ties, "cutoff": cutoffs, "aborted": aborted, "decided": decided},
            "p1_score": rate,
            "wilson95": [lo, hi],
            "mean_turns": if mean_turns.is_nan() { Value::Null } else { json!(mean_turns) },
            "elapsed_s": elapsed,
            "strategy_cache": {"hits": cache_hits, "solves": cache_misses},
            "lab_search_version": env!("CARGO_PKG_VERSION"),
            "game_records": records.iter().map(|r| json!({
                "game": r.index,
                "seed": format!("{:016x}", r.seed),
                "start": r.start,
                "ending": r.ending.label(),
                "winner": match r.ending { Ending::Win(s) => Value::String(side_name(s).to_owned()), _ => Value::Null },
                "aborted_reason": match &r.ending { Ending::Aborted(why) => Value::String(why.clone()), _ => Value::Null },
                "turns": r.turns,
                "elapsed_s": r.elapsed,
                "decisions": r.decisions,
            })).collect::<Vec<_>>(),
        });
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
        println!("written {path}");
    }
    Ok(())
}

/// One game: the equilibrium policy on both sides, exact chance, until the battle ends.
#[allow(clippy::too_many_arguments)]
fn play_game(
    loaded: &LoadedScenario,
    positions: &[Position],
    start_weights: &[f64],
    config: Config,
    evaluator: &(dyn Evaluator<2> + Sync),
    index: usize,
    master_seed: u64,
    max_turns: u16,
    policy: Policy,
    beam: usize,
    cache: &StrategyCache,
) -> GameRecord {
    let started = Instant::now();
    let seed =
        Rng::new(master_seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)).next_u64();
    let mut rng = Rng::new(seed);
    let start = rng.weighted(start_weights);
    let mut state: State<2> = positions[start].state.clone();
    let mut order: [PartyOrder; 2] = positions[start].order.clone();
    let mut suspension: Option<Suspension> = None;
    let mut solver = Solver::new(config, evaluator);
    let mut decisions: Vec<Value> = Vec::new();
    let ending = loop {
        let decision = match lab_search::decision(&state, suspension.as_ref()) {
            Ok(d) => d,
            Err(e) => break Ending::Aborted(format!("decision: {e}")),
        };
        if let Decision::Over(result) = decision {
            break match result {
                BattleResult::Win(side) => Ending::Win(side),
                BattleResult::Tie => Ending::Tie,
                BattleResult::Ongoing => Ending::Aborted("over but ongoing (bug)".into()),
            };
        }
        if state.turn > max_turns {
            break Ending::Cutoff;
        }
        // The policy: both sides' mixed strategies at this decision, from the batch's cache when
        // another game reached the same position.
        let key = StrategyCache::key(&state, suspension.as_ref());
        let strategies = match cache.get(&key) {
            Some(s) => Ok(s),
            None => {
                let solved = match policy {
                    Policy::Nash => {
                        solver
                            .analyse_mixed(&mut state, suspension.as_ref())
                            .map(|m| Strategies {
                                ours: m.ours,
                                theirs: m.theirs,
                                equilibrium: m.equilibrium,
                            })
                    }
                    Policy::DeepNash => solver
                        .analyse_deep_mixed(&mut state, suspension.as_ref(), beam)
                        .map(|d| Strategies {
                            ours: d.ours,
                            theirs: d.theirs,
                            equilibrium: d.equilibrium,
                        }),
                };
                if let Ok(s) = &solved {
                    cache.put(key, s.clone());
                }
                solved
            }
        };
        let Strategies {
            ours: our_choices,
            theirs: their_choices,
            equilibrium,
        } = match strategies {
            Ok(s) => s,
            Err(SearchError::Unsupported(whys)) => {
                break Ending::Aborted(format!("unsupported: {}", whys.join("; ")));
            }
            Err(e) => break Ending::Aborted(format!("solver: {e}")),
        };
        let rows: Vec<f64> = equilibrium.rows.iter().map(|&p| f64::from(p)).collect();
        let cols: Vec<f64> = equilibrium.cols.iter().map(|&p| f64::from(p)).collect();
        let ours = our_choices[rng.weighted(&rows)];
        let theirs = their_choices[rng.weighted(&cols)];
        let [c1, c2] = if config.us == SideId::One {
            [ours, theirs]
        } else {
            [theirs, ours]
        };
        let turn_seed = rng.next_u64();
        let outcome: Result<Outcome, String> = match (decision, c1, c2) {
            (Decision::Turn, Choice::Turn(a), Choice::Turn(b)) => {
                sample_turn(&mut state, config.ruleset, [a, b], 1, turn_seed)
                    .map_err(|e| format!("turn: {e}"))
                    .map(|mut v| v.remove(0))
            }
            (Decision::MidTurn, Choice::Switches(a), Choice::Switches(b)) => {
                match suspension.as_ref() {
                    Some(s) => sample_resume_turn(&mut state, s, [a, b], 1, turn_seed)
                        .map_err(|e| format!("mid-turn: {e}"))
                        .map(|mut v| v.remove(0)),
                    None => Err("mid-turn decision without a suspension (bug)".into()),
                }
            }
            (Decision::Replacement, Choice::Switches(a), Choice::Switches(b)) => {
                enumerate_replacements(&mut state, [a, b])
                    .map_err(|e| format!("replacement: {e}"))
                    .and_then(|outcomes| {
                        if outcomes.is_empty() {
                            return Err("replacement: no outcome".into());
                        }
                        let weights: Vec<f64> = outcomes.iter().map(|o| o.probability).collect();
                        let k = rng.weighted(&weights);
                        Ok(outcomes.into_iter().nth(k).unwrap())
                    })
            }
            (d, a, b) => Err(format!("choices {a:?} / {b:?} do not fit {d:?} (bug)")),
        };
        let outcome = match outcome {
            Ok(o) => o,
            Err(why) => break Ending::Aborted(why),
        };
        // The choices are written against the state and party order they were made in.
        let turn = state.turn;
        let p1_text = describe(loaded, &state, SideId::One, &c1, &order);
        let p2_text = describe(loaded, &state, SideId::Two, &c2, &order);
        state.apply(&outcome.instructions);
        advance_order(&mut order, &outcome.instructions);
        decisions.push(json!({
            "turn": turn,
            "decision": format!("{decision:?}"),
            "p1": p1_text,
            "p2": p2_text,
            "value_p1": if config.us == SideId::One { equilibrium.value } else { -equilibrium.value },
            "exploitability": equilibrium.exploitability,
            "hp_after": hp_summary(loaded, &state),
        }));
        suspension = outcome.suspension;
    };
    GameRecord {
        index,
        seed,
        start,
        ending,
        turns: state.turn,
        decisions,
        elapsed: started.elapsed().as_secs_f64(),
    }
}

fn describe(
    loaded: &LoadedScenario,
    state: &State<2>,
    side: SideId,
    choice: &Choice<2>,
    order: &[PartyOrder; 2],
) -> String {
    match choice {
        Choice::Turn(action) => format_choice(state, side, &order[side.index()], action),
        Choice::Switches(switches) => {
            let meta = &loaded.meta.sides[side.index()];
            let names: Vec<String> = switches
                .iter()
                .map(|s| match s {
                    Some(p) => format!("switch {}", meta.name(*p).unwrap_or("?")),
                    None => "-".to_owned(),
                })
                .collect();
            if switches.iter().all(Option::is_none) {
                "(waits)".to_owned()
            } else {
                names.join(", ")
            }
        }
    }
}

fn hp_summary(loaded: &LoadedScenario, state: &State<2>) -> Value {
    let side = |id: SideId| -> Vec<String> {
        let s = state.side(id);
        let meta = &loaded.meta.sides[id.index()];
        s.party
            .iter()
            .enumerate()
            .filter(|(_, mon)| mon.max_hp > 0)
            .map(|(i, mon)| {
                format!(
                    "{}:{:.0}",
                    meta.name(i as u8).unwrap_or("?"),
                    100.0 * f64::from(mon.hp.max(0)) / f64::from(mon.max_hp)
                )
            })
            .collect()
    };
    json!({"p1": side(SideId::One), "p2": side(SideId::Two)})
}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        _ => "p2",
    }
}

fn parse<T: std::str::FromStr>(args: &[String], i: usize, flag: &str) -> Result<T, String> {
    args.get(i)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("{flag} needs a number"))
}

fn roll_mode(arg: Option<&String>, flag: &str) -> Result<RollMode, String> {
    match arg.map(String::as_str) {
        Some("full") => Ok(RollMode::Full),
        Some("extremes") => Ok(RollMode::Extremes),
        Some("quartiles") => Ok(RollMode::Quartiles),
        Some("median") => Ok(RollMode::Median),
        Some("pessimistic") => Ok(RollMode::Pessimistic(SideId::One)),
        _ => Err(format!(
            "{flag} needs full, extremes, quartiles, median or pessimistic"
        )),
    }
}

/// Wilson 95% interval of `successes / n` (0 and empty when nothing was decided).
fn wilson(successes: f64, n: usize) -> (f64, f64, f64) {
    if n == 0 {
        return (f64::NAN, f64::NAN, f64::NAN);
    }
    let n = n as f64;
    let z = 1.96f64;
    let p = successes / n;
    let denom = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    (p, (centre - half).max(0.0), (centre + half).min(1.0))
}

/// The scenario's team files (`p1.team`, `p2.team`, relative to the scenario) with an FNV-1a
/// 64 hash of their bytes, for the record.
fn team_files(scenario: &str) -> Result<Vec<(String, String, u64)>, String> {
    let text = std::fs::read_to_string(scenario).map_err(|e| format!("{scenario}: {e}"))?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("{scenario}: {e}"))?;
    let base = std::path::Path::new(scenario)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut out = Vec::new();
    for side in ["p1", "p2"] {
        let Some(path) = json[side]["team"].as_str() else {
            continue;
        };
        let full = base.join(path);
        let bytes = std::fs::read(&full).map_err(|e| format!("{}: {e}", full.display()))?;
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        out.push((side.to_owned(), path.to_owned(), h));
    }
    Ok(out)
}
