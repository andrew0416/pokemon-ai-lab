//! Self-play rollouts under an equilibrium policy (the library side of `lab-rollout`, board
//! PY3a): at every decision both sides draw their choice from the matrix-game equilibrium
//! (one-turn [`Policy::Nash`] or depth-2 [`Policy::DeepNash`], opponent model ①, the same
//! evaluator on both sides), the turn is played once with exact chance (`sample_turn`), and
//! the game runs to its end. Game `g` of seed `s` is deterministic given the engine, the
//! evaluator and the policy. The policy's strategies are cached across games by position
//! ([`StrategyCache`]); the policy is deterministic, so the cache changes nothing but the time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use lab_engine::eval::Evaluator;
use lab_engine::instruction::Outcome;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{enumerate_replacements, sample_resume_turn, sample_turn, Suspension};
use lab_scenario::{advance_order, LoadedScenario, PartyOrder, Position};

use crate::model::side_name;
use crate::nash::Equilibrium;
use crate::solve::{Config, SearchError, Solver};
use crate::{format_choice, Choice, Decision};

/// Both sides' mixed strategies at one decision, as the policy produced them.
#[derive(Clone, Debug)]
pub struct Strategies {
    pub ours: Vec<Choice<2>>,
    pub theirs: Vec<Choice<2>>,
    pub equilibrium: Equilibrium,
}

/// A position: the state and what the turn still waits for.
pub type PositionKey = (State<2>, Option<Suspension>);

/// Strategies by position, shared by every game of a batch.
pub struct StrategyCache {
    /// Keyed by the position itself: a 64-bit hash alone would hand one position another's
    /// strategies on a collision (board B32).
    entries: Mutex<HashMap<PositionKey, Strategies>>,
    pub hits: AtomicUsize,
    pub misses: AtomicUsize,
}

impl Default for StrategyCache {
    fn default() -> Self {
        StrategyCache::new()
    }
}

impl StrategyCache {
    pub fn new() -> StrategyCache {
        StrategyCache {
            entries: Mutex::new(HashMap::new()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    pub fn key(state: &State<2>, suspension: Option<&Suspension>) -> PositionKey {
        (state.clone(), suspension.cloned())
    }

    pub fn get(&self, key: &PositionKey) -> Option<Strategies> {
        let found = self.entries.lock().unwrap().get(key).cloned();
        if found.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        found
    }

    pub fn put(&self, key: PositionKey, strategies: Strategies) {
        self.entries.lock().unwrap().insert(key, strategies);
    }
}

/// Which equilibrium the sides draw their choices from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// The one-turn matrix game (`analyse_mixed`).
    Nash,
    /// The depth-2 matrix game over both beams (`analyse_deep_mixed`).
    DeepNash,
}

/// How a game ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Ending {
    Win(SideId),
    Tie,
    /// Still running after `max_turns` turns.
    Cutoff,
    /// The solver or the engine refused (an unimplemented effect, an internal error).
    Aborted(String),
}

impl Ending {
    pub fn label(&self) -> &'static str {
        match self {
            Ending::Win(SideId::One) => "p1",
            Ending::Win(_) => "p2",
            Ending::Tie => "tie",
            Ending::Cutoff => "cutoff",
            Ending::Aborted(_) => "aborted",
        }
    }
}

/// One game's record.
#[derive(Clone, Debug)]
pub struct GameRecord {
    pub index: usize,
    pub seed: u64,
    /// The start position's index.
    pub start: usize,
    pub ending: Ending,
    pub turns: u16,
    /// Per decision: turn, decision kind, both choices, the equilibrium value from p1's side,
    /// exploitability and HP after the turn.
    pub decisions: Vec<Value>,
    pub elapsed: f64,
}

/// xorshift64* seeded through splitmix64, so consecutive game numbers give unrelated streams.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        Rng((z ^ (z >> 31)).max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// An index drawn by `weights` (need not sum to 1; the last index absorbs rounding).
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
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

/// What every game of a batch shares.
#[derive(Clone, Copy, Debug)]
pub struct RolloutSettings {
    /// The policy's solver configuration (its `us` is side one; `threads` per game).
    pub config: Config,
    pub max_turns: u16,
    pub policy: Policy,
    /// [`Policy::DeepNash`]'s beam.
    pub beam: usize,
    pub master_seed: u64,
}

/// One game: the equilibrium policy on both sides, exact chance, until the battle ends. The
/// start position is drawn from `positions` by `start_weights`.
pub fn play_game<E: Evaluator<2> + ?Sized + Sync>(
    loaded: &LoadedScenario,
    positions: &[Position],
    start_weights: &[f64],
    settings: &RolloutSettings,
    evaluator: &E,
    index: usize,
    cache: &StrategyCache,
) -> GameRecord {
    let config = settings.config;
    let started = Instant::now();
    let seed = Rng::new(settings.master_seed ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .next_u64();
    let mut rng = Rng::new(seed);
    let start = rng.weighted(start_weights);
    let mut state: State<2> = positions[start].state.clone();
    let mut order: [PartyOrder; 2] = positions[start].order.clone();
    let mut suspension: Option<Suspension> = None;
    let mut solver = Solver::new(config, evaluator);
    let mut decisions: Vec<Value> = Vec::new();
    let ending = loop {
        let decision = match crate::decision(&state, suspension.as_ref()) {
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
        if state.turn > settings.max_turns {
            break Ending::Cutoff;
        }
        // The policy: both sides' mixed strategies at this decision, from the batch's cache when
        // another game reached the same position.
        let key = StrategyCache::key(&state, suspension.as_ref());
        let strategies = match cache.get(&key) {
            Some(s) => Ok(s),
            None => {
                let solved = match settings.policy {
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
                        .analyse_deep_mixed(&mut state, suspension.as_ref(), settings.beam)
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

/// `games` games on `game_threads` threads (each game solving with `settings.config.threads`),
/// sharing one [`StrategyCache`]; `on_game` sees each record as it finishes (in finishing
/// order). Returns the records sorted by game index and the cache.
#[allow(clippy::too_many_arguments)]
pub fn run_games<E: Evaluator<2> + ?Sized + Sync>(
    loaded: &LoadedScenario,
    positions: &[Position],
    start_weights: &[f64],
    settings: &RolloutSettings,
    evaluator: &E,
    games: usize,
    game_threads: usize,
    on_game: &(dyn Fn(&GameRecord) + Sync),
) -> (Vec<GameRecord>, StrategyCache) {
    let next = AtomicUsize::new(0);
    let cache = StrategyCache::new();
    let records: Mutex<Vec<GameRecord>> = Mutex::new(Vec::with_capacity(games));
    std::thread::scope(|scope| {
        for _ in 0..game_threads.max(1) {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::SeqCst);
                if g >= games {
                    break;
                }
                let record = play_game(
                    loaded,
                    positions,
                    start_weights,
                    settings,
                    evaluator,
                    g,
                    &cache,
                );
                on_game(&record);
                records.lock().unwrap().push(record);
            });
        }
    });
    let mut records = records.into_inner().unwrap();
    records.sort_by_key(|r| r.index);
    (records, cache)
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

/// Both sides' HP percentages by name (`Name:pct`).
pub fn hp_summary(loaded: &LoadedScenario, state: &State<2>) -> Value {
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

/// Wilson 95% interval of `successes / n`: `(rate, low, high)`, NaN when nothing was decided.
pub fn wilson(successes: f64, n: usize) -> (f64, f64, f64) {
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
pub fn team_files(scenario: &str) -> Result<Vec<(String, String, u64)>, String> {
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

/// `side_name` for the records.
pub fn winner_label(ending: &Ending) -> Value {
    match ending {
        Ending::Win(s) => Value::String(side_name(*s).to_owned()),
        _ => Value::Null,
    }
}
