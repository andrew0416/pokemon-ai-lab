//! Child matrix games solved on the dominance-reduced game (board S24d) keep their values:
//! the depth-2 analyses agree with dominance on and off within the equilibrium solver's
//! tolerance (a child's value is an RM+ approximation either way, within its exploitability,
//! about 0.01–0.1 on the HP-hundredths scale).

use std::path::PathBuf;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::{legal_choices, Config, Decision, Pruning, Solver};

/// Two RM+ approximations of the same child game differ by at most the sum of their
/// exploitabilities; the cells average a few children.
const TOLERANCE: f32 = 0.5;

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

fn run(name: &str, dominance: bool) -> Vec<f32> {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 2;
    config.reply_beam = Some(3);
    config.outcome_cap = Some(4);
    config.child_nash = true;
    config.dominance = dominance;
    let evaluator = Heuristic;
    let mut solver = Solver::new(config, &evaluator);
    let mut state = state(name);
    let mut values = Vec::new();
    let mixed = solver.analyse_deep_mixed(&mut state, None, 3).unwrap();
    values.extend(&mixed.matrix.values);
    values.push(mixed.equilibrium.value);
    let deep = solver.analyse_deep(&mut state, None, 3).unwrap();
    for line in &deep.lines {
        values.push(line.deep);
        values.extend(line.replies.iter().map(|(_, v)| *v));
    }
    let ours = legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        Decision::Turn,
        SideId::One,
        Pruning::Sensible,
    );
    let plan = solver.evaluate_plan(&mut state, None, &ours[..1]).unwrap();
    let child = plan.child.expect("child values");
    values.push(child.value);
    values.extend(child.replies.iter().map(|(_, v)| *v));
    values
}

#[test]
fn dominance_keeps_child_values() {
    for name in [
        "aa-power-construct",
        "ability-change-fails",
        "eject-button-uturn",
    ] {
        let on = run(name, true);
        let off = run(name, false);
        assert_eq!(on.len(), off.len(), "{name}");
        for (i, (a, b)) in on.iter().zip(&off).enumerate() {
            assert!(
                (a.is_nan() && b.is_nan()) || (a - b).abs() <= TOLERANCE,
                "{name}: value {i}: {a} with dominance, {b} without"
            );
        }
    }
}
