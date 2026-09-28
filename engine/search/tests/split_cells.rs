//! Replacement and mid-turn child cells valued row by row on the pool (board S24-t2): the
//! split changes no value (bit for bit against the unsplit cells) and its counters do not
//! depend on the thread count.

use std::path::PathBuf;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::{legal_choices, Chance, Config, Decision, DeepLevel, Pruning, Solver};

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

/// Values (bit patterns), and separately the counters, of the analyses with child games.
fn run(name: &str, split: bool, threads: usize, chance: Chance) -> (Vec<String>, String, u64) {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = threads;
    config.chance = chance;
    config.split_heavy_cells = split;
    config.child_nash = true;
    config.reply_beam = Some(4);
    config.outcome_cap = None;
    let evaluator = Heuristic;
    let mut solver = Solver::new(config, &evaluator);
    let original = state(name);
    let mut state = original.clone();
    let mut values = Vec::new();
    let mut counters = String::new();
    let mut split_cells = 0;

    let deep = solver.analyse_deep_mixed(&mut state, None, 4).unwrap();
    values.push(format!(
        "deep-mixed {:?} {:?}",
        deep.matrix.values, deep.equilibrium
    ));
    counters += &format!("{} {} {:?}; ", deep.nodes, deep.turns, deep.unsupported);
    split_cells += solver.stats().split_cells;
    let levels = [
        DeepLevel {
            beam: 2,
            outcomes: Some(2),
        },
        DeepLevel {
            beam: 2,
            outcomes: Some(2),
        },
    ];
    let deep3 = solver
        .analyse_deep_mixed_levels(&mut state, None, &levels)
        .unwrap();
    values.push(format!(
        "deep3 {:?} {:?}",
        deep3.matrix.values, deep3.equilibrium
    ));
    counters += &format!("{} {}; ", deep3.nodes, deep3.turns);
    split_cells += solver.stats().split_cells;
    let ours = legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        Decision::Turn,
        SideId::One,
        Pruning::Sensible,
    );
    for choice in ours.iter().take(3) {
        let plan = solver.evaluate_plan(&mut state, None, &[*choice]).unwrap();
        values.push(format!("plan {:?} {:?}", plan.value, plan.child));
        counters += &format!("{} {}; ", plan.nodes, plan.turns);
        split_cells += solver.stats().split_cells;
    }
    assert_eq!(state, original);
    (values, counters, split_cells)
}

#[test]
fn split_cells_keep_values_and_are_thread_independent() {
    for name in [
        "ee-recoil",
        "dd-emergency-exit-recoil-ko",
        "eject-button-uturn",
    ] {
        for chance in [Chance::Expect, Chance::Worst] {
            let (whole, _, none) = run(name, false, 4, chance);
            assert_eq!(none, 0);
            let (one, one_counters, split) = run(name, true, 1, chance);
            let (four, four_counters, _) = run(name, true, 4, chance);
            assert!(split > 0, "{name}: no replacement cell was split");
            assert_eq!(whole, one, "{name} {chance:?}: split vs whole");
            assert_eq!(one, four, "{name} {chance:?}: 1 vs 4 threads");
            assert_eq!(one_counters, four_counters, "{name} {chance:?}: counters");
        }
    }
}
