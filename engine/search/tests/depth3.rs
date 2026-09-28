//! Depth-3 deep mixed analysis (board S24c): with every choice in every beam and every outcome
//! followed, `analyse_deep_mixed_levels` over two levels is the brute-force recursion — the
//! root's matrix over all pairs, each cell the expected depth-2 equilibrium value of the
//! pair's outcomes, each of those the expected one-turn equilibrium value of its own pairs'
//! outcomes, and so on down to the leaf evaluation; a replacement or mid-turn decision inside
//! the tree does not use a level. Also: one level is `analyse_deep_mixed`, the thread count
//! and the deep table change nothing but the time, and double oracle stays within tolerance.

use std::path::PathBuf;

use lab_engine::eval::{Evaluator, Heuristic};
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId};
use lab_engine::turn::{EnumerateOptions, RollMode, Suspension, TurnError};
use lab_engine::Doubles;
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::nash::{self, Matrix};
use lab_search::{
    decision, legal_choices, transitions, Choice, Config, Decision, DeepLevel, Pruning, Solver, WIN,
};

const OPTIONS: EnumerateOptions = EnumerateOptions {
    rolls: RollMode::Median,
};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn state(name: &str) -> Doubles {
    let loaded =
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    scenario_positions_with(&loaded, OPTIONS)
        .unwrap()
        .remove(0)
        .state
}

/// The recursion written out, side one's view, no beams, no caps, no table.
struct Brute {
    /// Non-turn decisions met below the root (the level-preserving case).
    inner_non_turn: usize,
}

impl Brute {
    fn terminal(result: BattleResult, depth: u32) -> f32 {
        match result {
            BattleResult::Win(SideId::One) => WIN + depth as f32,
            BattleResult::Win(_) => -(WIN + depth as f32),
            _ => 0.0,
        }
    }

    fn choices(state: &Doubles, decision: Decision, side: SideId) -> Vec<Choice<2>> {
        legal_choices(
            state,
            Ruleset::CHAMPIONS_MC,
            decision,
            side,
            Pruning::Sensible,
        )
    }

    /// The expected value of `child` over the pair's outcomes (NaN: unevaluable).
    fn expect(
        &mut self,
        state: &mut Doubles,
        decision: Decision,
        suspension: Option<&Suspension>,
        pair: [Choice<2>; 2],
        child: &mut dyn FnMut(&mut Self, &mut Doubles, Option<&Suspension>) -> f32,
    ) -> f32 {
        let outcomes = match transitions(
            state,
            Ruleset::CHAMPIONS_MC,
            OPTIONS,
            decision,
            suspension,
            pair,
        ) {
            Ok(outcomes) => outcomes,
            Err(TurnError::Unsupported(_)) => return f32::NAN,
            Err(e) => panic!("{e}"),
        };
        let mut sum = 0.0f64;
        for outcome in &outcomes {
            state.apply(&outcome.instructions);
            let v = child(self, state, outcome.suspension.as_ref());
            state.reverse(&outcome.instructions);
            if v.is_nan() {
                return f32::NAN;
            }
            sum += outcome.probability * v as f64;
        }
        sum as f32
    }

    /// Pure maximin with leaf evaluation after `depth` turns.
    fn maximin(&mut self, state: &mut Doubles, suspension: Option<&Suspension>, depth: u32) -> f32 {
        let d = decision(state, suspension).unwrap();
        if let Decision::Over(result) = d {
            return Self::terminal(result, depth);
        }
        if depth == 0 {
            return Heuristic.evaluate(state);
        }
        let next = if d == Decision::Turn {
            depth - 1
        } else {
            depth
        };
        let mut best = f32::NEG_INFINITY;
        for a in Self::choices(state, d, SideId::One) {
            let mut worst = f32::INFINITY;
            for b in Self::choices(state, d, SideId::Two) {
                let v = self.expect(state, d, suspension, [a, b], &mut |s, st, su| {
                    s.maximin(st, su, next)
                });
                if !v.is_nan() {
                    worst = worst.min(v);
                }
            }
            if worst != f32::INFINITY {
                best = best.max(worst);
            }
        }
        if best == f32::NEG_INFINITY {
            f32::NAN
        } else {
            best
        }
    }

    /// The equilibrium value of a payoff matrix after dropping unevaluable columns, then rows.
    fn equilibrium(n: usize, m: usize, values: &[f32]) -> f32 {
        let keep_col: Vec<bool> = (0..m)
            .map(|c| (0..n).all(|r| !values[r * m + c].is_nan()))
            .collect();
        let keep_row: Vec<bool> = (0..n)
            .map(|r| (0..m).all(|c| !keep_col[c] || !values[r * m + c].is_nan()))
            .collect();
        let mut dense = Vec::new();
        for r in (0..n).filter(|&r| keep_row[r]) {
            for c in (0..m).filter(|&c| keep_col[c]) {
                dense.push(values[r * m + c]);
            }
        }
        let (rows, cols) = (
            keep_row.iter().filter(|k| **k).count(),
            keep_col.iter().filter(|k| **k).count(),
        );
        if rows == 0 || cols == 0 {
            return f32::NAN;
        }
        nash::solve(&Matrix::new(rows, cols, dense), 20_000, 0.01).value
    }

    /// `levels` more deep levels below this position (0: its one-turn equilibrium, whose
    /// cells look through a replacement to the next turn). `root`: the level is used whatever
    /// the decision.
    fn deep(
        &mut self,
        state: &mut Doubles,
        suspension: Option<&Suspension>,
        levels: u32,
        root: bool,
    ) -> f32 {
        let d = decision(state, suspension).unwrap();
        if let Decision::Over(result) = d {
            return Self::terminal(result, 0);
        }
        if !root && d != Decision::Turn {
            self.inner_non_turn += 1;
        }
        let ours = Self::choices(state, d, SideId::One);
        let theirs = Self::choices(state, d, SideId::Two);
        let mut values = Vec::new();
        for &a in &ours {
            for &b in &theirs {
                let v = if levels == 0 {
                    let next = if d == Decision::Turn { 0 } else { 1 };
                    self.expect(state, d, suspension, [a, b], &mut |s, st, su| {
                        s.maximin(st, su, next)
                    })
                } else {
                    let rest = if root || d == Decision::Turn {
                        levels - 1
                    } else {
                        levels
                    };
                    self.expect(state, d, suspension, [a, b], &mut |s, st, su| {
                        s.deep(st, su, rest, false)
                    })
                };
                values.push(v);
            }
        }
        Self::equilibrium(ours.len(), theirs.len(), &values)
    }
}

fn exact_config(threads: usize) -> Config {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = threads;
    config.double_oracle = false;
    config.dominance = false;
    config
}

const FULL: DeepLevel = DeepLevel {
    beam: 1000,
    outcomes: None,
};

#[test]
fn depth3_full_beams_match_brute_force() {
    for name in ["eject-button-uturn", "ee-recoil"] {
        let original = state(name);
        let mut brute = Brute { inner_non_turn: 0 };
        let mut s = original.clone();
        let expected = brute.deep(&mut s, None, 2, true);
        assert_eq!(s, original);
        assert!(
            brute.inner_non_turn > 0,
            "{name}: no replacement or mid-turn decision inside the tree"
        );
        for threads in [1, 4] {
            let evaluator = Heuristic;
            let mut solver = Solver::new(exact_config(threads), &evaluator);
            let mut s = original.clone();
            let deep = solver
                .analyse_deep_mixed_levels(&mut s, None, &[FULL, FULL])
                .unwrap();
            assert_eq!(s, original);
            assert_eq!(deep.levels.len(), 2);
            assert!(
                (deep.equilibrium.value - expected).abs() < 1e-3,
                "{name} threads {threads}: depth 3 {} vs brute force {expected}",
                deep.equilibrium.value
            );
        }
    }
}

/// One level is exactly `analyse_deep_mixed` (same matrix, equilibrium and counters).
#[test]
fn one_level_is_deep_mixed() {
    let original = state("eject-button-uturn");
    let evaluator = Heuristic;
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.outcome_cap = Some(2);
    let mut a = Solver::new(config, &evaluator);
    let mut b = Solver::new(config, &evaluator);
    let mut s = original.clone();
    let old = a.analyse_deep_mixed(&mut s, None, 2).unwrap();
    let new = b
        .analyse_deep_mixed_levels(
            &mut s,
            None,
            &[DeepLevel {
                beam: 2,
                outcomes: Some(2),
            }],
        )
        .unwrap();
    assert_eq!(old.matrix, new.matrix);
    assert_eq!(old.equilibrium, new.equilibrium);
    assert_eq!((old.nodes, old.turns), (new.nodes, new.turns));
}

/// Narrow beams and caps with double oracle: the same result at 1 and 4 threads and with or
/// without the tables, within the solvers' tolerance of the exact children.
#[test]
fn depth3_threads_tables_and_double_oracle() {
    let original = state("ee-recoil");
    let levels = [
        DeepLevel {
            beam: 2,
            outcomes: Some(2),
        },
        DeepLevel {
            beam: 2,
            outcomes: Some(1),
        },
    ];
    let evaluator = Heuristic;
    let run = |config: Config| {
        let mut solver = Solver::new(config, &evaluator);
        let mut s = original.clone();
        let deep = solver
            .analyse_deep_mixed_levels(&mut s, None, &levels)
            .unwrap();
        assert_eq!(s, original);
        deep
    };
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 1;
    let one = run(config);
    config.threads = 4;
    let four = run(config);
    assert_eq!(one.matrix, four.matrix);
    assert_eq!(one.equilibrium, four.equilibrium);
    assert_eq!((one.nodes, one.turns), (four.nodes, four.turns));
    config.transposition = false;
    let untabled = run(config);
    assert_eq!(one.matrix, untabled.matrix);
    let mut exact = exact_config(4);
    exact.transposition = true;
    let full = run(exact);
    assert_eq!(one.ours, full.ours);
    for (x, y) in one.matrix.values.iter().zip(&full.matrix.values) {
        assert!((x - y).abs() < 0.5, "double oracle {x} vs full {y}");
    }
}
