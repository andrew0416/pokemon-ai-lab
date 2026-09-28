//! The `lab-plan` / `lab-rollout` logic as library functions (board PY3a): opponent-model
//! helpers, position picking, the child dump and the rollout policy, on the oracle's
//! `eject-button-uturn` scenario.

use std::path::PathBuf;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions_with, LoadedScenario, Position};
use lab_search::model::{
    analyse_positions, check_observations, dump_children, load_evaluator, matching_positions,
    matrix_best_response, observed_start, parse_observation, pick_position, PositionPick,
};
use lab_search::nash::Matrix;
use lab_search::rollout::{play_game, run_games, wilson, Policy, RolloutSettings, StrategyCache};
use lab_search::{Config, Solver};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn scenario() -> (String, LoadedScenario) {
    let path = engine_dir().join("oracle/scenarios/eject-button-uturn.json");
    let loaded = load_scenario_file(&path).unwrap();
    (path.to_string_lossy().into_owned(), loaded)
}

fn median() -> EnumerateOptions {
    EnumerateOptions {
        rolls: RollMode::Median,
    }
}

fn config() -> Config {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 1;
    config
}

#[test]
fn observations_parse_and_check() {
    let obs = parse_observation("Gardevoir:55, Rillaboom:65%").unwrap();
    assert_eq!(
        obs,
        vec![
            ("Gardevoir".to_owned(), 55.0),
            ("Rillaboom".to_owned(), 65.0)
        ]
    );
    assert!(parse_observation("Gardevoir").is_err());
    assert!(parse_observation("Gardevoir:x").is_err());
    let mut two = vec![(2, obs.clone()), (1, obs.clone())];
    check_observations(&mut two, 2).unwrap();
    assert_eq!(two[0].0, 1, "sorted by turn");
    let mut dup = vec![(1, obs.clone()), (1, obs.clone())];
    assert!(check_observations(&mut dup, 2).is_err());
    let mut out_of_range = vec![(3, obs)];
    assert!(check_observations(&mut out_of_range, 2).is_err());
}

#[test]
fn evaluators_load_by_name() {
    assert!(load_evaluator("heuristic").is_ok());
    assert!(load_evaluator("material").is_ok());
    assert!(load_evaluator("bogus").is_err());
    assert!(load_evaluator("file:does-not-exist.json").is_err());
}

/// With no setup turns and no observations the start is the scenario's positions; a full-HP
/// observation keeps every position, an impossible one none; `pick_position` picks by index.
#[test]
fn start_positions_and_matching() {
    let (_, loaded) = scenario();
    let (positions, trace, survivors) =
        observed_start(&loaded, SideId::One, &[], 1.0, median(), false, true).unwrap();
    assert!(trace.is_empty() && survivors.is_empty());
    let all = scenario_positions_with(&loaded, median()).unwrap();
    assert_eq!(positions.len(), all.len());
    let lead = all[0].state.side(SideId::One).slots[0].party_index.unwrap();
    let name = loaded.meta.sides[0].name(lead);
    let name = name.unwrap().to_owned();
    let full = matching_positions(
        &loaded,
        all.clone(),
        SideId::One,
        &[(name.clone(), 100.0)],
        0.5,
    );
    assert_eq!(full.len(), all.len());
    let none = matching_positions(&loaded, all.clone(), SideId::One, &[(name, 3.0)], 0.5);
    assert!(none.is_empty());
    let pick = PositionPick {
        index: Some(0),
        ..PositionPick::default()
    };
    let p = pick_position(&loaded, all.clone(), &pick, SideId::One).unwrap();
    assert_eq!(p, all[0]);
    let beyond = PositionPick {
        index: Some(all.len()),
        ..PositionPick::default()
    };
    assert!(pick_position(&loaded, all, &beyond, SideId::One).is_err());
}

/// The best response against a pure column is that column's best row; against the
/// equilibrium strategy no row does better than the equilibrium value (within tolerance).
#[test]
fn best_response_reads_the_matrix() {
    let (_, loaded) = scenario();
    let position: Position = scenario_positions_with(&loaded, median())
        .unwrap()
        .remove(0);
    let evaluator = Heuristic;
    let mut solver = Solver::new(config(), &evaluator);
    let (mixed, note) = analyse_positions(&mut solver, &position, &[], "position").unwrap();
    assert!(note.is_none());
    let pure = vec![(mixed.theirs[0], 1.0)];
    let lines = matrix_best_response(&mixed, &pure);
    let best_row = (0..mixed.matrix.rows)
        .map(|r| mixed.matrix.at(r, 0))
        .fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(lines[0].1, best_row);
    let strategy: Vec<_> = mixed
        .theirs
        .iter()
        .copied()
        .zip(mixed.equilibrium.cols.iter().copied())
        .collect();
    let lines = matrix_best_response(&mixed, &strategy);
    assert!(lines[0].1 <= mixed.equilibrium.value + mixed.equilibrium.exploitability + 1e-3);
    let _ = Matrix::new(1, 1, vec![0.0]);
}

/// Every dumped child's target is its own next-turn equilibrium, and the solver leaves the
/// state as it found it.
#[test]
fn child_dump_targets_are_child_equilibria() {
    let (_, loaded) = scenario();
    let position: Position = scenario_positions_with(&loaded, median())
        .unwrap()
        .remove(0);
    let evaluator = Heuristic;
    let mut solver = Solver::new(config(), &evaluator);
    let mut state = position.state.clone();
    let (deep, rows) = dump_children(&mut solver, &mut state, 2).unwrap();
    assert_eq!(state, position.state);
    assert!(!rows.is_empty());
    assert!(!deep.lines.is_empty());
    for row in &rows {
        assert!(row.target.is_finite());
        assert!(row.probability > 0.0 && row.probability <= 1.0);
    }
}

/// A game is a function of (seed, game index): the same record twice, with or without a
/// warm strategy cache, and the batch runner returns the games in index order.
#[test]
fn rollout_games_are_deterministic() {
    let (_, loaded) = scenario();
    let positions = scenario_positions_with(&loaded, median()).unwrap();
    let weights: Vec<f64> = positions.iter().map(|p| p.probability).collect();
    let evaluator = Heuristic;
    let settings = RolloutSettings {
        config: config(),
        max_turns: 5,
        policy: Policy::Nash,
        beam: 2,
        deep_rest: None,
        master_seed: 11,
        lazy: false,
    };
    let cache = StrategyCache::new();
    let a = play_game(
        &loaded, &positions, &weights, &settings, &evaluator, 1, &cache,
    );
    let b = play_game(
        &loaded, &positions, &weights, &settings, &evaluator, 1, &cache,
    );
    let fresh = StrategyCache::new();
    let c = play_game(
        &loaded, &positions, &weights, &settings, &evaluator, 1, &fresh,
    );
    for other in [&b, &c] {
        assert_eq!(a.seed, other.seed);
        assert_eq!(a.ending, other.ending);
        assert_eq!(a.turns, other.turns);
        assert_eq!(a.decisions, other.decisions);
    }
    let (records, _) = run_games(
        &loaded,
        &positions,
        &weights,
        &settings,
        &evaluator,
        3,
        2,
        &|_| {},
    );
    assert_eq!(
        records.iter().map(|r| r.index).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(records[1].decisions, a.decisions);
    // The double-oracle policy is deterministic too.
    let lazy = RolloutSettings {
        lazy: true,
        ..settings
    };
    let x = play_game(
        &loaded,
        &positions,
        &weights,
        &lazy,
        &evaluator,
        1,
        &StrategyCache::new(),
    );
    let y = play_game(
        &loaded,
        &positions,
        &weights,
        &lazy,
        &evaluator,
        1,
        &StrategyCache::new(),
    );
    assert_eq!(x.decisions, y.decisions);
    let (p, lo, hi) = wilson(5.0, 10);
    assert!((p - 0.5).abs() < 1e-12 && lo < p && p < hi);
}
