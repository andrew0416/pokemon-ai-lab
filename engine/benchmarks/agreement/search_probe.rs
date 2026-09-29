//! Bounded-by-caller search differential probe, not a timing benchmark.
//! Copy unchanged into search/examples/search_probe.rs for every source variant.
//! Usage: search_probe SCENARIO maximin|mixed|deep 1|2 expect|worst median|extremes|full 1|2
//!        [POSITION_INDEX] [--position N] [--before JSON] [--prepared on|off]
//! --before accepts a complete oracle report or its before object. Position index is
//! an ordinal among canonical matches (otherwise among all scenario positions).
//! Setup uses scenario_positions' existing default/pinned replay policy unchanged.
//! Search errors are structured stdout results; CLI/invariant failures exit nonzero.
//! Observer metadata is stderr-only. Timeouts are external process limits, not Budget.

use std::io::{self, Write};
use std::path::PathBuf;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{FactoredScope, RollMode, TurnError};
use lab_scenario::{canonical_value, load_scenario_file, scenario_positions};
use lab_search::nash::{Equilibrium, Matrix};
use lab_search::solve::SearchStats;
use lab_search::{
    Analysis, Chance, Config, DeepLevel, DeepMixedAnalysis, MixedAnalysis, Pruning, SearchError,
    Solver,
};
use serde_json::{json, Value};

struct Args {
    scenario: PathBuf,
    mode: String,
    us: SideId,
    chance: Chance,
    rolls: RollMode,
    threads: usize,
    position: usize,
    before: Option<PathBuf>,
    prepared: bool,
}

fn arguments() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.len() < 6 {
        return Err("usage: search_probe SCENARIO MODE US CHANCE ROLLS THREADS [POSITION_INDEX] [--before JSON] [--prepared on|off]".into());
    }
    if !matches!(raw[1].as_str(), "maximin" | "mixed" | "deep") {
        return Err("mode must be maximin, mixed or deep".into());
    }
    let mut args = Args {
        scenario: PathBuf::from(&raw[0]),
        mode: raw[1].clone(),
        us: match raw[2].as_str() {
            "1" => SideId::One,
            "2" => SideId::Two,
            _ => return Err("us must be 1 or 2".into()),
        },
        chance: match raw[3].as_str() {
            "expect" => Chance::Expect,
            "worst" => Chance::Worst,
            _ => return Err("chance must be expect or worst".into()),
        },
        rolls: match raw[4].as_str() {
            "median" => RollMode::Median,
            "extremes" => RollMode::Extremes,
            "full" => RollMode::Full,
            _ => return Err("rolls must be median, extremes or full".into()),
        },
        threads: match raw[5].as_str() {
            "1" => 1,
            "2" => 2,
            _ => return Err("threads must be 1 or 2".into()),
        },
        position: 0,
        before: None,
        prepared: true,
    };
    let mut seen_position = false;
    let mut seen_prepared = false;
    let mut index = 6;
    while index < raw.len() {
        match raw[index].as_str() {
            "--before" if args.before.is_none() => {
                index += 1;
                args.before = Some(PathBuf::from(
                    raw.get(index).ok_or("--before needs a path")?,
                ));
            }
            "--prepared" if !seen_prepared => {
                index += 1;
                args.prepared = match raw.get(index).map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--prepared needs on or off".into()),
                };
                seen_prepared = true;
            }
            "--position" if !seen_position => {
                index += 1;
                args.position = raw
                    .get(index)
                    .ok_or("--position needs an index")?
                    .parse()
                    .map_err(|_| "position must be a nonnegative integer")?;
                seen_position = true;
            }
            value if !seen_position && !value.starts_with('-') => {
                args.position = value
                    .parse()
                    .map_err(|_| "position must be a nonnegative integer")?;
                seen_position = true;
            }
            value => return Err(format!("unknown or repeated option: {value}")),
        }
        index += 1;
    }
    #[cfg(not(feature = "experiment-prepared-turn"))]
    if !args.prepared {
        return Err("--prepared off requires a build with experiment-prepared-turn".into());
    }
    Ok(args)
}

fn bit(value: f32) -> u32 {
    assert!(value.is_finite(), "non-finite public search value");
    value.to_bits()
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|&value| bit(value)).collect()
}

fn matrix(value: &Matrix) -> Value {
    json!({"rows": value.rows, "cols": value.cols, "value_bits": bits(&value.values)})
}

fn equilibrium(value: &Equilibrium) -> Value {
    json!({"row_bits": bits(&value.rows), "col_bits": bits(&value.cols),
        "value_bits": bit(value.value), "exploitability_bits": bit(value.exploitability),
        "iterations": value.iterations})
}

fn stats(value: SearchStats) -> Value {
    // Only elapsed measurements are omitted. Every public integer work counter remains.
    json!({"tt_hits": value.tt_hits, "tt_misses": value.tt_misses,
        "nash_solves": value.nash_solves, "nash_iterations": value.nash_iterations,
        "deep_tt_hits": value.deep_tt_hits, "deep_tt_misses": value.deep_tt_misses,
        "split_cells": value.split_cells})
}

fn maximin(value: &Analysis<2>) -> Value {
    json!({"decision": format!("{:?}", value.decision), "value_bits": bit(value.value),
        "depth": value.depth, "nodes": value.nodes, "turns": value.turns,
        "unsupported": value.unsupported, "omitted_pairs": value.omitted_pairs,
        "lines": value.lines.iter().map(|line| json!({
            "ours": format!("{:?}", line.ours), "reply": format!("{:?}", line.reply),
            "value_bits": bit(line.value), "exact": line.exact})).collect::<Vec<_>>()})
}

fn mixed(value: &MixedAnalysis<2>) -> Value {
    json!({"decision": format!("{:?}", value.decision),
        "ours": format!("{:?}", value.ours), "theirs": format!("{:?}", value.theirs),
        "matrix": matrix(&value.matrix), "equilibrium": equilibrium(&value.equilibrium),
        "maximin": {"row": value.maximin.0, "value_bits": bit(value.maximin.1)},
        "depth": value.depth, "nodes": value.nodes, "turns": value.turns,
        "unsupported": value.unsupported, "omitted_ours": value.omitted_ours,
        "omitted_theirs": value.omitted_theirs})
}

fn deep(value: &DeepMixedAnalysis<2>) -> Value {
    json!({"decision": format!("{:?}", value.decision),
        "ours": format!("{:?}", value.ours), "theirs": format!("{:?}", value.theirs),
        "matrix": matrix(&value.matrix), "equilibrium": equilibrium(&value.equilibrium),
        "maximin": {"row": value.maximin.0, "value_bits": bit(value.maximin.1)},
        "shallow": mixed(&value.shallow), "beam": value.beam, "outcome_cap": value.outcome_cap,
        "levels": value.levels.iter().map(|level| json!({"beam": level.beam,
            "outcomes": level.outcomes})).collect::<Vec<_>>(),
        "nodes": value.nodes, "turns": value.turns, "unsupported": value.unsupported,
        "omitted_ours": value.omitted_ours, "omitted_theirs": value.omitted_theirs})
}

fn turn_error(error: &TurnError) -> Value {
    match error {
        TurnError::BattleOver => json!({"kind": "BattleOver"}),
        TurnError::ReplacementPending(side) => {
            json!({"kind": "ReplacementPending", "side": format!("{side:?}")})
        }
        TurnError::Action { side, error } => {
            json!({"kind": "Action", "side": format!("{side:?}"), "error": format!("{error:?}")})
        }
        TurnError::InvalidChoice { side, slot, reason } => {
            json!({"kind": "InvalidChoice", "side": format!("{side:?}"), "slot": slot, "reason": reason})
        }
        TurnError::Unsupported(reason) => json!({"kind": "Unsupported", "reason": reason}),
    }
}

fn search_error(error: &SearchError) -> Value {
    match error {
        SearchError::Turn(error) => json!({"kind": "Turn", "error": turn_error(error)}),
        SearchError::NoChoice(side) => json!({"kind": "NoChoice", "side": format!("{side:?}")}),
        SearchError::Budget => json!({"kind": "Budget"}),
        SearchError::Unsupported(reasons) => json!({"kind": "Unsupported", "reasons": reasons}),
    }
}

fn reset_observers() {
    #[cfg(feature = "experiment-leaf-ending-observer")]
    lab_engine::turn::final_state_observer::reset();
    #[cfg(feature = "experiment-prepared-turn-observe")]
    lab_engine::turn::reset_validation_counts();
}

fn observer_metadata(args: &Args, phase: &str) {
    #[allow(unused_mut)]
    let mut metadata = json!({"kind": "search_observers", "phase": phase,
        "requested_threads": args.threads, "scope": "current-thread TLS only; setup excluded",
        "factored": false, "prepared_requested": args.prepared,
        "prepared_compiled": cfg!(feature = "experiment-prepared-turn"),
        "leaf_observer_compiled": cfg!(feature = "experiment-leaf-ending-observer"),
        "prepared_observer_compiled": cfg!(feature = "experiment-prepared-turn-observe")});
    #[cfg(feature = "experiment-leaf-ending-observer")]
    {
        let counts = lab_engine::turn::final_state_observer::counts();
        metadata["leaf"] = json!({"batches": counts.batches, "visits": counts.visits,
            "materialized_outcomes": counts.materialized_outcomes,
            "emitted_instructions": counts.emitted_instructions});
    }
    #[cfg(feature = "experiment-prepared-turn-observe")]
    {
        let counts = lab_engine::turn::validation_counts();
        metadata["prepared"] = json!({"parent_checks": counts[0],
            "side_checks": counts[1], "support_checks": counts[2]});
    }
    serde_json::to_writer(io::stderr().lock(), &metadata).expect("observer metadata write failed");
    eprintln!();
}

fn run(args: &Args) -> Value {
    let _flat = FactoredScope::new(false);
    let loaded = match load_scenario_file(&args.scenario) {
        Ok(value) => value,
        Err(error) => {
            return json!({"schema_version": 1, "stage": "load", "status": "error", "error": format!("{error:?}")})
        }
    };
    let positions = match scenario_positions(&loaded) {
        Ok(value) => value,
        Err(error) => {
            return json!({"schema_version": 1, "stage": "setup", "status": "error",
            "error": {"kind": if error.is_unsupported() { "Unsupported" } else { "Invalid" }, "reason": error.message()}})
        }
    };
    let before = args.before.as_ref().map(|path| {
        let bytes = std::fs::read(path).expect("before input unreadable");
        let value: Value = serde_json::from_slice(&bytes).expect("before input must be JSON");
        value.get("before").cloned().unwrap_or(value)
    });
    let mut matching = Vec::new();
    for (index, position) in positions.iter().enumerate() {
        let canonical = match canonical_value(&position.state, &loaded.meta) {
            Ok(value) => value,
            Err(error) => {
                return json!({"schema_version": 1, "stage": "parent-canonical", "status": "error", "source_position_index": index, "error": format!("{error:?}")})
            }
        };
        if before
            .as_ref()
            .is_none_or(|expected| *expected == canonical)
        {
            matching.push((index, canonical));
        }
    }
    let Some((source_index, canonical)) = matching.get(args.position) else {
        return json!({"schema_version": 1, "stage": "parent-selection", "status": "error",
            "error": if matching.is_empty() { "no canonical before match" } else { "position index outside matching parents" },
            "requested_position_index": args.position, "matching_positions": matching.len(), "all_positions": positions.len()});
    };
    let position = &positions[*source_index];
    assert!(
        position.probability.is_finite(),
        "non-finite setup probability"
    );
    let original = position.state.clone();
    let original_debug = format!("{original:?}");
    let input = json!({"requested_position_index": args.position, "source_position_index": source_index,
        "before_filter": before.is_some(), "matching_positions": matching.len(), "all_positions": positions.len(),
        "state": original_debug, "canonical": canonical, "party_order": format!("{:?}", position.order),
        "setup_probability_bits": position.probability.to_bits(), "suspension": null});
    let config = Config {
        #[cfg(feature = "experiment-prepared-turn")]
        prepared_turn: args.prepared,
        ruleset: Ruleset::CHAMPIONS_MC,
        us: args.us,
        depth: 1,
        chance: args.chance,
        pruning: Pruning::Sensible,
        rolls: args.rolls,
        exact_lines: false,
        max_turns: None,
        child_nash: false,
        reply_beam: Some(6),
        outcome_cap: Some(4),
        threads: args.threads,
        transposition: true,
        dominance: true,
        double_oracle: true,
        split_heavy_cells: false,
    };
    let mut work = original.clone();
    let mut solver = Solver::new(config, &Heuristic);
    reset_observers();
    observer_metadata(args, "search-start");
    let result = match args.mode.as_str() {
        "maximin" => solver.analyse(&mut work, None).map(|value| maximin(&value)),
        "mixed" => solver
            .analyse_mixed(&mut work, None)
            .map(|value| mixed(&value)),
        "deep" => solver
            .analyse_deep_mixed_levels(
                &mut work,
                None,
                &[DeepLevel {
                    beam: 2,
                    outcomes: Some(2),
                }],
            )
            .map(|value| deep(&value)),
        _ => unreachable!(),
    };
    observer_metadata(args, "search-complete");
    let restored = work == original && format!("{work:?}") == original_debug;
    let (status, result) = match result {
        Ok(value) => ("ok", value),
        Err(error) => ("error", search_error(&error)),
    };
    json!({"schema_version": 1, "stage": "search", "status": status,
        "input": input, "config": {"mode": args.mode, "us": format!("{:?}", args.us),
            "chance": format!("{:?}", args.chance), "rolls": format!("{:?}", args.rolls),
            "threads": args.threads, "depth": if args.mode == "deep" { 2 } else { 1 },
            "pruning": "Sensible", "evaluator": "Heuristic", "max_turns": null, "factored": false},
        "result": result, "stats": stats(solver.stats()), "state_restored": restored,
        "state_after": format!("{work:?}")})
}

fn main() {
    let args = arguments().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let output = run(&args);
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &output).expect("search result write failed");
    stdout
        .write_all(b"\n")
        .expect("search result newline failed");
    stdout.flush().expect("search result flush failed");
    if output.get("state_restored") == Some(&Value::Bool(false)) {
        std::process::exit(3);
    }
}
