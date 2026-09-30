//! P16 activation wrapper; analysis body comes from the frozen timing harness.
//! This instrumented example is never used for timing.
use lab_engine::{
    eval::{Evaluator, Heuristic},
    rules::Ruleset,
    state::SideId,
    turn::{EnumerateOptions, RollMode},
};
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::model::{pick_position, PositionPick};
use lab_search::nash::{Equilibrium, Matrix};
use lab_search::{Chance, Config, DeepLevel, MixedAnalysis, Pruning, Solver};
use serde_json::{json, Value};

fn bit(value: f32) -> u32 {
    assert!(value.is_finite(), "non-finite analysis value");
    value.to_bits()
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|&value| bit(value)).collect()
}

fn matrix(matrix: &Matrix) -> Value {
    json!({"rows": matrix.rows, "cols": matrix.cols, "values": bits(&matrix.values)})
}

fn equilibrium(value: &Equilibrium) -> Value {
    json!({
        "rows": bits(&value.rows), "cols": bits(&value.cols),
        "value": bit(value.value), "exploitability": bit(value.exploitability),
        "iterations": value.iterations,
    })
}

fn shallow(value: &MixedAnalysis<2>) -> Value {
    json!({
        "decision": format!("{:?}", value.decision),
        "ours": format!("{:?}", value.ours), "theirs": format!("{:?}", value.theirs),
        "matrix": matrix(&value.matrix), "equilibrium": equilibrium(&value.equilibrium),
        "maximin": [value.maximin.0 as u64, bit(value.maximin.1) as u64],
        "nodes": value.nodes, "turns": value.turns, "depth": value.depth,
        "unsupported": value.unsupported,
        "omitted": [value.omitted_ours, value.omitted_theirs],
    })
}

fn analyse<E: Evaluator<2> + Sync>(
    state: &mut lab_engine::Doubles,
    config: Config,
    evaluator: &E,
) -> Value {
    let levels = [DeepLevel {
        beam: 2,
        outcomes: Some(2),
    }];
    let mut solver = Solver::new(config, evaluator);
    let deep = solver
        .analyse_deep_mixed_levels(state, None, &levels)
        .expect("deep mixed analysis failed");
    let stats = solver.stats();
    json!({
        "decision": format!("{:?}", deep.decision),
        "ours": format!("{:?}", deep.ours), "theirs": format!("{:?}", deep.theirs),
        "matrix": matrix(&deep.matrix), "equilibrium": equilibrium(&deep.equilibrium),
        "maximin": [deep.maximin.0 as u64, bit(deep.maximin.1) as u64],
        "nodes": deep.nodes, "turns": deep.turns, "beam": deep.beam,
        "outcome_cap": deep.outcome_cap, "depth": deep.levels.len() + 1,
        "levels": deep.levels.iter().map(|level| {
            json!({"beam": level.beam, "outcomes": level.outcomes})
        }).collect::<Vec<_>>(),
        "unsupported": deep.unsupported,
        "omitted": [deep.omitted_ours, deep.omitted_theirs],
        "shallow": shallow(&deep.shallow),
        "stats": {
            "tt_hits": stats.tt_hits, "tt_misses": stats.tt_misses,
            "nash_solves": stats.nash_solves, "nash_iterations": stats.nash_iterations,
            "deep_tt_hits": stats.deep_tt_hits, "deep_tt_misses": stats.deep_tt_misses,
            "split_cells": stats.split_cells,
        },
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "usage: ci_nash_scratch_observer SCENARIO THREADS POSITION");
    let threads: usize = args[2].parse().expect("threads must be an integer");
    assert!(matches!(threads, 1 | 2 | 4), "threads must be 1, 2, or 4");
    assert_eq!(std::env::var("LAB_ENGINE_FACTORED").as_deref(), Ok("0"));
    let loaded = load_scenario_file(&args[1]).expect("scenario load failed");
    assert!(
        loaded.setup_rolls.is_none(),
        "fixture may not override setup Median rolls"
    );
    let positions = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .expect("scenario setup failed");
    let pick = if args[3] == "max" {
        PositionPick {
            most_probable: true,
            ..Default::default()
        }
    } else {
        PositionPick {
            index: Some(
                args[3]
                    .parse()
                    .expect("position must be max or a zero-based index"),
            ),
            ..Default::default()
        }
    };
    let position =
        pick_position(&loaded, positions, &pick, SideId::One).expect("position selection failed");
    let original = position.state.clone();
    let mut state = original.clone();
    // Depth 1 is the shallow matrix; the single DeepLevel above makes total depth 2.
    // A new Config field must be reviewed here; no changed default silently alters CI.
    let config = Config {
        #[cfg(feature = "experiment-prepared-turn")]
        prepared_turn: true,
        ruleset: Ruleset::CHAMPIONS_MC,
        us: SideId::One,
        depth: 1,
        chance: Chance::Expect,
        pruning: Pruning::Sensible,
        rolls: RollMode::Median,
        exact_lines: false,
        max_turns: None,
        threads,
        child_nash: false,
        reply_beam: Some(6),
        outcome_cap: Some(4),
        transposition: true,
        dominance: true,
        double_oracle: true,
        split_heavy_cells: false,
    };
    #[cfg(feature = "experiment-nash-scratch-observer")]
    lab_search::nash::scratch_observer::reset();
    #[cfg(feature = "experiment-borrowed-child-keys-observer")]
    lab_search::solve::child_keys_observer::reset();
    let analysis = analyse(&mut state, config, &Heuristic);
    #[cfg(all(
        feature = "experiment-nash-scratch-observer",
        feature = "experiment-borrowed-child-keys-observer"
    ))]
    {
    let c = lab_search::nash::scratch_observer::counts();
    let b = lab_search::solve::child_keys_observer::counts();
    eprintln!("P16 activation: {}", json!({
        "nash": {"solve_calls":c.solve_calls,"checkpoints":c.checkpoints,
            "iterations":c.iterations,"normalization_allocations":c.normalization_allocations,
            "evaluation_scratch_allocations":c.evaluation_scratch_allocations,
            "output_materializations":c.output_materializations},
        "borrowed": {"key_captures":b.key_captures,"job_captures":b.job_captures,
            "borrowed_queries":b.borrowed_queries,"seen_hits":b.seen_hits,
            "seen_collisions":b.seen_collisions,"seen_links":b.seen_links}
    }));
    }

    assert_eq!(state, original, "search must restore State");
    let output = json!({"schema": 1, "state_restored": true, "analysis": analysis});
    serde_json::to_writer(std::io::stdout().lock(), &output).expect("JSON output failed");
}
