//! Child matrix games solved on the dominance-reduced game or by double oracle over lazily
//! valued cells (board S24d) keep their values: the depth-2 analyses agree with both off
//! within the equilibrium solver's tolerance (a child's value is an RM+ approximation either
//! way, within its exploitability, about 0.01–0.1 on the HP-hundredths scale).

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

fn run(name: &str, dominance: bool, double_oracle: bool) -> Vec<f32> {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 2;
    config.reply_beam = Some(3);
    config.outcome_cap = Some(4);
    config.child_nash = true;
    config.dominance = dominance;
    config.double_oracle = double_oracle;
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
fn reductions_keep_child_values() {
    for name in [
        "aa-power-construct",
        "ability-change-fails",
        "eject-button-uturn",
    ] {
        let off = run(name, false, false);
        for (dominance, double_oracle) in [(true, false), (false, true), (true, true)] {
            let on = run(name, dominance, double_oracle);
            assert_eq!(on.len(), off.len(), "{name}");
            for (i, (a, b)) in on.iter().zip(&off).enumerate() {
                assert!(
                    (a.is_nan() && b.is_nan()) || (a - b).abs() <= TOLERANCE,
                    "{name}: value {i}: {a} with dominance {dominance} and double oracle {double_oracle}, {b} with neither"
                );
            }
        }
    }
}

/// The root by double oracle (`lab-plan --solve nash --lazy`): its value agrees with the full
/// analysis within both exploitabilities, the exploitability it reports is its strategies'
/// own in the full matrix, and every pair it valued has the full analysis' value.
#[test]
fn lazy_root_matches_the_full_matrix() {
    let mut lazy_roots = 0;
    for name in [
        "aa-power-construct",
        "ability-change-fails",
        "eject-button-uturn",
        "spread-damage",
    ] {
        let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
        config.rolls = RollMode::Median;
        config.threads = 2;
        let evaluator = Heuristic;
        let mut solver = Solver::new(config, &evaluator);
        let mut state = state(name);
        let full = solver.analyse_mixed(&mut state, None).unwrap();
        let lazy = solver.analyse_mixed_lazy(&mut state, None).unwrap();
        assert_eq!(full.ours, lazy.ours, "{name}");
        assert_eq!(full.theirs, lazy.theirs, "{name}");
        let (x, y) = (&lazy.equilibrium.rows, &lazy.equilibrium.cols);
        let m = &full.matrix;
        let row_best = (0..m.rows)
            .map(|r| (0..m.cols).map(|c| y[c] * m.at(r, c)).sum::<f32>())
            .fold(f32::NEG_INFINITY, f32::max);
        let col_best = (0..m.cols)
            .map(|c| (0..m.rows).map(|r| x[r] * m.at(r, c)).sum::<f32>())
            .fold(f32::INFINITY, f32::min);
        let exploitability = row_best - col_best;
        assert!(
            (exploitability - lazy.equilibrium.exploitability).abs() <= 1e-2,
            "{name}: reported {} vs {exploitability} in the full matrix",
            lazy.equilibrium.exploitability
        );
        assert!(
            (full.equilibrium.value - lazy.equilibrium.value).abs()
                <= full.equilibrium.exploitability + lazy.equilibrium.exploitability + 1e-2,
            "{name}: {} full vs {} lazy",
            full.equilibrium.value,
            lazy.equilibrium.value
        );
        for (a, b) in lazy.matrix.values.iter().zip(&full.matrix.values) {
            assert!(a.is_nan() || a == b, "{name}: {a} vs {b}");
        }
        lazy_roots += usize::from(lazy.matrix.values.iter().any(|v| v.is_nan()));
    }
    assert!(
        lazy_roots > 0,
        "some root is solved without valuing every pair"
    );
}
