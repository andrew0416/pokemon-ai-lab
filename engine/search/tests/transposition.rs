//! The transposition table of child equilibria (board S24a) changes nothing but the time: the
//! depth-2 analyses (`deep`, `deep-nash`, a plan with `--child-nash`, the child dump) give the
//! same values with the table on and off, on oracle scenarios where children recur.

use std::path::PathBuf;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::{legal_choices, Config, Decision, Pruning, Solver};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn state(name: &str) -> lab_engine::Doubles {
    let loaded =
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    let options = EnumerateOptions {
        rolls: RollMode::Median,
    };
    scenario_positions_with(&loaded, options)
        .unwrap()
        .remove(0)
        .state
}

fn config(transposition: bool) -> Config {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 2;
    config.reply_beam = Some(3);
    config.outcome_cap = Some(4);
    config.child_nash = true;
    config.transposition = transposition;
    config
}

/// Everything a depth-2 analysis reports that the table could change, as numbers.
fn run(name: &str, transposition: bool) -> (Vec<f32>, u64) {
    let evaluator = Heuristic;
    let mut solver = Solver::new(config(transposition), &evaluator);
    let original = state(name);
    let mut state = original.clone();
    let mut values = Vec::new();
    let mut hits = 0;

    let mixed = solver.analyse_deep_mixed(&mut state, None, 3).unwrap();
    values.extend(&mixed.matrix.values);
    values.push(mixed.equilibrium.value);
    hits += solver.stats().tt_hits;

    let deep = solver.analyse_deep(&mut state, None, 3).unwrap();
    for line in &deep.lines {
        values.push(line.deep);
        values.extend(line.replies.iter().map(|(_, v)| *v));
    }
    hits += solver.stats().tt_hits;

    let ours = legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        Decision::Turn,
        SideId::One,
        Pruning::Sensible,
    );
    let plan = solver.evaluate_plan(&mut state, None, &ours[..1]).unwrap();
    let child = plan
        .child
        .expect("a one-turn plan at a turn has child values");
    values.push(child.value);
    values.extend(child.replies.iter().map(|(_, v)| *v));
    hits += solver.stats().tt_hits;

    assert_eq!(state, original, "{name}: the solver changed the position");
    (values, hits)
}

#[test]
fn table_on_and_off_give_the_same_values() {
    let mut total_hits = 0;
    for name in ["aa-power-construct", "ability-change-fails"] {
        let (on, hits) = run(name, true);
        let (off, no_hits) = run(name, false);
        assert_eq!(no_hits, 0, "{name}: a disabled table answered");
        assert_eq!(on.len(), off.len(), "{name}");
        for (i, (a, b)) in on.iter().zip(&off).enumerate() {
            assert!(
                a == b || (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-4 * a.abs().max(1.0),
                "{name}: value {i}: {a} with the table, {b} without"
            );
        }
        total_hits += hits;
    }
    assert!(total_hits > 0, "the scenarios exercise the table");
}
