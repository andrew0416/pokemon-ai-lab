//! The worker pool (board S24p) changes nothing but the time: every analysis gives the same
//! values, counters and refusal lists at 1, 2 and 4 threads, for both chance modes.

use std::path::PathBuf;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::{legal_choices, Chance, Config, Decision, Pruning, Solver};

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

/// Values and counters of every analysis mode, bit for bit.
fn run(name: &str, threads: usize, chance: Chance) -> Vec<String> {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = threads;
    config.chance = chance;
    config.reply_beam = Some(3);
    config.outcome_cap = Some(3);
    config.child_nash = true;
    let evaluator = Heuristic;
    let mut solver = Solver::new(config, &evaluator);
    let original = state(name);
    let mut state = original.clone();
    let mut out = Vec::new();

    let mixed = solver.analyse_mixed(&mut state, None).unwrap();
    out.push(format!(
        "mixed {:?} {:?} {} {} {:?}",
        mixed.matrix.values, mixed.equilibrium, mixed.nodes, mixed.turns, mixed.unsupported
    ));
    let deep_mixed = solver.analyse_deep_mixed(&mut state, None, 3).unwrap();
    out.push(format!(
        "deep-mixed {:?} {:?} {} {} {:?}",
        deep_mixed.matrix.values,
        deep_mixed.equilibrium,
        deep_mixed.nodes,
        deep_mixed.turns,
        deep_mixed.unsupported
    ));
    let deep = solver.analyse_deep(&mut state, None, 3).unwrap();
    out.push(format!(
        "deep {:?} {} {} {:?}",
        deep.lines, deep.nodes, deep.turns, deep.unsupported
    ));
    let ours = legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        Decision::Turn,
        SideId::One,
        Pruning::Sensible,
    );
    let plan = solver.evaluate_plan(&mut state, None, &ours[..1]).unwrap();
    out.push(format!(
        "plan {} {:?} {:?} {} {}",
        plan.value, plan.replies, plan.child, plan.nodes, plan.turns
    ));
    let stats = solver.stats();
    out.push(format!(
        "stats {} {} {} {}",
        stats.tt_hits, stats.tt_misses, stats.nash_solves, stats.nash_iterations
    ));
    let mut exact = Config { ..solver.config };
    exact.exact_lines = true;
    exact.depth = 2;
    let mut solver = Solver::new(exact, &evaluator);
    // With one thread the exact lines run the alpha-beta loop (a reply's value above the
    // row's current minimum is only a bound, so the node counts and, among equal replies, the
    // one reported differ); with more the full matrix. The line values agree.
    let analysis = solver.analyse(&mut state, None).unwrap();
    let lines: Vec<_> = analysis
        .lines
        .iter()
        .map(|l| (l.ours, l.value, l.exact))
        .collect();
    out.push(format!("maximin {lines:?}"));
    assert_eq!(state, original, "{name}: the solver changed the position");
    out
}

#[test]
fn thread_count_changes_nothing() {
    for name in ["aa-power-construct", "eject-button-uturn"] {
        for chance in [Chance::Expect, Chance::Worst] {
            let one = run(name, 1, chance);
            for threads in [2, 4] {
                let many = run(name, threads, chance);
                for (a, b) in one.iter().zip(&many) {
                    assert_eq!(a, b, "{name} {chance:?}: 1 thread vs {threads}");
                }
            }
        }
    }
}
