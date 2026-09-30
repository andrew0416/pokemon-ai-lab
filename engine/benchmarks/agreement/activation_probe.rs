//! Separate fixed P8c/P9 interaction controls; not part of the frozen 6,184-case corpus.
//! Run the identical all-on binary twice per control, changing only prepared_turn.
use serde_json::{json, Value};
use std::io::{self, Write};

#[cfg(all(
    feature = "experiment-prepared-turn-observe",
    feature = "experiment-leaf-ending-observer"
))]
mod enabled {
    use super::*;
    use lab_engine::eval::Heuristic;
    use lab_engine::rules::Ruleset;
    use lab_engine::state::{SideId, State};
    use lab_engine::turn::{self, EnumerateOptions, FactoredScope, RollMode};
    use lab_scenario::{load_scenario_str, scenario_positions_with};
    use lab_search::solve::SearchStats;
    use lab_search::{Analysis, Chance, Config, MixedAnalysis, Solver};
    use std::path::PathBuf;

    fn toy() -> State<2> {
        let mon = |species: &str| {
            json!({"species": species, "ability": "Honey Gather",
            "nature": "Serious", "evs": {}, "moves": ["Harden", "Protect"], "level": 50})
        };
        let scenario = json!({"format": "gen9championsdoublescustomgame",
            "p1": {"team": [mon("Talonflame"), mon("Snorlax")], "order": "12"},
            "p2": {"team": [mon("Swampert"), mon("Excadrill")], "order": "12"}});
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let loaded = load_scenario_str(&scenario.to_string(), &root).expect("fixed toy must load");
        scenario_positions_with(
            &loaded,
            EnumerateOptions {
                rolls: RollMode::Median,
            },
        )
        .expect("fixed toy must initialize")
        .remove(0)
        .state
    }

    fn bits(values: &[f32]) -> Vec<u32> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    fn stats(value: SearchStats) -> Value {
        // Every public integer work counter remains; only elapsed times are excluded.
        json!({"tt_hits": value.tt_hits, "tt_misses": value.tt_misses,
            "nash_solves": value.nash_solves, "nash_iterations": value.nash_iterations,
            "deep_tt_hits": value.deep_tt_hits, "deep_tt_misses": value.deep_tt_misses,
            "split_cells": value.split_cells})
    }

    fn maximin(value: &Analysis<2>) -> Value {
        json!({"decision": format!("{:?}", value.decision), "value_bits": value.value.to_bits(),
            "depth": value.depth, "nodes": value.nodes, "turns": value.turns,
            "unsupported": value.unsupported, "omitted_pairs": value.omitted_pairs,
            "lines": value.lines.iter().map(|line| json!({
                "ours": format!("{:?}", line.ours), "reply": format!("{:?}", line.reply),
                "value_bits": line.value.to_bits(), "exact": line.exact})).collect::<Vec<_>>()})
    }

    fn mixed(value: &MixedAnalysis<2>) -> Value {
        json!({"decision": format!("{:?}", value.decision),
            "ours": format!("{:?}", value.ours), "theirs": format!("{:?}", value.theirs),
            "matrix": {"rows": value.matrix.rows, "cols": value.matrix.cols,
                "value_bits": bits(&value.matrix.values)},
            "equilibrium": {"row_bits": bits(&value.equilibrium.rows),
                "col_bits": bits(&value.equilibrium.cols),
                "value_bits": value.equilibrium.value.to_bits(),
                "exploitability_bits": value.equilibrium.exploitability.to_bits(),
                "iterations": value.equilibrium.iterations},
            "maximin": {"row": value.maximin.0, "value_bits": value.maximin.1.to_bits()},
            "depth": value.depth, "nodes": value.nodes, "turns": value.turns,
            "unsupported": value.unsupported, "omitted_ours": value.omitted_ours,
            "omitted_theirs": value.omitted_theirs})
    }

    fn run(state: &State<2>, mode: &str, side: SideId, chance: Chance, prepared: bool) -> Value {
        let mut config = Config::new(Ruleset::CHAMPIONS_MC, side);
        config.threads = 1;
        config.rolls = RollMode::Median;
        config.depth = 2;
        config.exact_lines = true;
        config.chance = chance;
        config.prepared_turn = prepared;
        let mut solver = Solver::new(config, &Heuristic);
        let mut work = state.clone();
        let before = format!("{state:?}");
        turn::reset_validation_counts();
        turn::final_state_observer::reset();
        let result = if mode == "exact" {
            solver.analyse(&mut work, None).map(|value| maximin(&value))
        } else {
            solver
                .analyse_mixed(&mut work, None)
                .map(|value| mixed(&value))
        };
        let counts = turn::validation_counts();
        let leaf = turn::final_state_observer::counts();
        let restored = work == *state && format!("{work:?}") == before;
        let successful = result.is_ok();
        let signature = json!({"result": result.unwrap_or_else(|error|
            json!({"error": format!("{error:?}")})), "stats": stats(solver.stats())})
        .to_string();
        json!({"prepared_requested": prepared, "validation_counts": counts,
            "leaf": {"batches": leaf.batches, "visits": leaf.visits,
                "materialized_outcomes": leaf.materialized_outcomes,
                "emitted_instructions": leaf.emitted_instructions},
            "restored": restored, "successful": successful, "signature": signature})
    }

    pub(super) fn controls() -> Vec<Value> {
        let _flat = FactoredScope::new(false);
        let state = toy();
        let mut controls = Vec::new();
        for mode in ["exact", "mixed"] {
            for (side, name) in [(SideId::One, "side1"), (SideId::Two, "side2")] {
                for (chance, chance_name) in [(Chance::Expect, "expect"), (Chance::Worst, "worst")]
                {
                    let off = run(&state, mode, side, chance, false);
                    let on = run(&state, mode, side, chance, true);
                    let equal = on["signature"] == off["signature"];
                    let reduction = (0..3).all(|i| {
                        let a = on["validation_counts"][i].as_u64().unwrap();
                        let b = off["validation_counts"][i].as_u64().unwrap();
                        0 < a && a < b
                    });
                    let active = [&on, &off].into_iter().all(|value| {
                        value["restored"] == true
                            && value["successful"] == true
                            && value["leaf"]["batches"].as_u64().unwrap() > 0
                            && value["leaf"]["visits"].as_u64().unwrap() > 0
                            && value["leaf"]["materialized_outcomes"].as_u64().unwrap() > 0
                    });
                    let passed = equal && reduction && active && on["leaf"] == off["leaf"];
                    controls.push(json!({"id": format!("{mode}-{name}-{chance_name}"),
                        "on": on, "off": off, "equal": equal, "passed": passed}));
                }
            }
        }
        controls
    }
}

fn main() {
    let features = json!({
        "prepared_compiled": cfg!(feature = "experiment-prepared-turn"),
        "prepared_observer_compiled": cfg!(feature = "experiment-prepared-turn-observe"),
        "leaf_compiled": cfg!(feature = "experiment-leaf-ending-states"),
        "leaf_observer_compiled": cfg!(feature = "experiment-leaf-ending-observer")});
    #[cfg(all(
        feature = "experiment-prepared-turn-observe",
        feature = "experiment-leaf-ending-observer"
    ))]
    let controls = enabled::controls();
    #[cfg(not(all(
        feature = "experiment-prepared-turn-observe",
        feature = "experiment-leaf-ending-observer"
    )))]
    let controls: Vec<Value> = Vec::new();
    let passed = features
        .as_object()
        .unwrap()
        .values()
        .all(|value| value == true)
        && controls.len() == 8
        && controls.iter().all(|value| value["passed"] == true);
    let output = json!({"schema": 1, "suite": "prepared-leaf-nonleaf-v1",
        "config": {"depth": 2, "threads": 1, "rolls": "Median", "factored": false,
            "fixture": "harden-protect-toy-v1"},
        "features": features, "controls": controls, "passed": passed});
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &output).expect("activation JSON write failed");
    stdout
        .write_all(b"\n")
        .expect("activation newline write failed");
    stdout.flush().expect("activation flush failed");
    if !passed {
        std::process::exit(1);
    }
}
