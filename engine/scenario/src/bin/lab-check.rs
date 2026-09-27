//! Checks lab-engine against one `enumerate.cjs` report of a scenario: the engine replays the
//! scenario to the report's `before` state, enumerates the decision in the report's roll mode
//! (`full` → exact, `extremes` → min/max rolls, `fixed` with the report's `roll` k → every
//! damage roll at index k, `RollMode::Fixed(k)`) and compares the canonical outcome
//! distributions exactly (the comparator of the fixture tests, `lab_scenario::parity`).
//!
//! Usage: lab-check <scenario.json> <oracle-report.json> [--out verdict.json] [--tolerance p]
//!
//! Prints one JSON verdict (also written to `--out`). `status` is one of
//! - `match`: same canonical outcomes, probabilities within the tolerance (default 1e-9);
//! - `mismatch`: `differences` names the first canonical fields in which the most probable
//!   outcome only one side produced differs from the other side's closest outcome;
//! - `unsupported`: the engine refused the decision or a setup turn (`TurnError::Unsupported`);
//! - `engine-error`: any other engine failure (loading, parsing a choice, canonical output);
//! - `no-position`: no engine position after the setup turns has the report's `before` state;
//! - `ambiguous`: several engine positions share the `before` state (they differ in state the
//!   canonical form leaves out) and their distributions differ, so the oracle's single
//!   position cannot be matched to one of them. The comparison is against the first.
//!
//! The exit code is 0 for `match`, 1 for anything else, 2 for a usage error.

use std::process::ExitCode;
use std::time::Instant;

use serde_json::{json, Value};

use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::parity::{
    compare, engine_distribution, first_differences, json_diff, report_distribution, Distribution,
};
use lab_scenario::{
    canonical_value, load_scenario_file, run_decision_mid_turn_with, scenario_decision,
    scenario_positions, ScenarioError,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files = Vec::new();
    let mut out: Option<String> = None;
    let mut tolerance = 1e-9;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "--tolerance" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(t) => tolerance = t,
                    None => {
                        eprintln!("lab-check: --tolerance needs a number");
                        return ExitCode::from(2);
                    }
                }
            }
            other => files.push(other.to_owned()),
        }
        i += 1;
    }
    let [scenario, report] = files.as_slice() else {
        eprintln!("usage: lab-check <scenario.json> <oracle-report.json> [--out verdict.json]");
        return ExitCode::from(2);
    };
    let started = Instant::now();
    let mut verdict = check(scenario, report, tolerance);
    verdict["scenario"] = json!(scenario);
    verdict["report"] = json!(report);
    verdict["engineMs"] = json!((started.elapsed().as_secs_f64() * 1000.0).round());
    let text = serde_json::to_string_pretty(&verdict).expect("serializable");
    println!("{text}");
    if let Some(path) = out {
        if let Err(e) = std::fs::write(&path, &text) {
            eprintln!("lab-check: {path}: {e}");
            return ExitCode::from(2);
        }
    }
    if verdict["status"] == "match" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn failure(status: &str, error: impl Into<String>) -> Value {
    json!({"status": status, "error": error.into()})
}

/// `unsupported` for what the engine does not implement ([`ScenarioError::Unsupported`]),
/// else `engine-error`; `prefix` goes before the message.
fn engine_failure(prefix: &str, error: ScenarioError) -> Value {
    let status = if error.is_unsupported() {
        "unsupported"
    } else {
        "engine-error"
    };
    failure(status, format!("{prefix}{error}"))
}

fn check(scenario: &str, report_path: &str, tolerance: f64) -> Value {
    let report: Value = match std::fs::read_to_string(report_path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(r) => r,
        Err(e) => return failure("engine-error", format!("{report_path}: {e}")),
    };
    let mode = report["mode"].as_str().unwrap_or("");
    let roll = report["roll"].as_u64();
    let options = EnumerateOptions {
        rolls: match mode {
            "full" => RollMode::Full,
            "extremes" => RollMode::Extremes,
            "fixed" => match roll
                .and_then(|k| u8::try_from(k).ok())
                .and_then(RollMode::fixed)
            {
                Some(rolls) => rolls,
                None => {
                    return failure(
                        "engine-error",
                        format!("report mode \"fixed\" with roll {}", report["roll"]),
                    )
                }
            },
            other => return failure("engine-error", format!("report mode {other:?}")),
        },
    };
    let oracle = match report_distribution(&report) {
        Ok(d) => d,
        Err(e) => return failure("engine-error", e),
    };
    let loaded = match load_scenario_file(scenario) {
        Ok(l) => l,
        Err(e) => return failure("engine-error", e.to_string()),
    };
    let positions = match scenario_positions(&loaded) {
        Ok(p) => p,
        Err(e) => return engine_failure("setup: ", e),
    };
    let before = &report["before"];
    let mut matching = Vec::new();
    let mut closest: Option<Vec<String>> = None;
    for position in &positions {
        let value = match canonical_value(&position.state, &loaded.meta) {
            Ok(v) => v,
            Err(e) => return failure("engine-error", format!("canonical: {e}")),
        };
        if value == *before {
            matching.push(position.clone());
        } else {
            let d = json_diff(before, &value, 5);
            if closest.as_ref().is_none_or(|c| d.len() < c.len()) {
                closest = Some(d);
            }
        }
    }
    if matching.is_empty() {
        return json!({
            "status": "no-position",
            "error": format!("none of {} engine position(s) has the oracle's `before`", positions.len()),
            "differences": closest.unwrap_or_default(),
        });
    }
    let mut distributions: Vec<Distribution> = Vec::new();
    for position in &matching {
        let decision = match scenario_decision(&loaded, position) {
            Ok(d) => d,
            Err(e) => return failure("engine-error", format!("decision: {e}")),
        };
        let mut state = position.state.clone();
        let outcomes = match run_decision_mid_turn_with(
            &mut state,
            &position.order,
            &decision,
            &loaded.mid_turn,
            options,
        ) {
            Ok(o) => o,
            Err(e) => return engine_failure("", e),
        };
        match engine_distribution(&loaded.meta, &mut state, &outcomes) {
            Ok(d) => distributions.push(d),
            Err(e) => return failure("engine-error", format!("canonical: {e}")),
        }
    }
    let engine = &distributions[0];
    let variants_agree = distributions[1..]
        .iter()
        .all(|d| compare(d, engine).exact(tolerance));
    let comparison = compare(engine, &oracle);
    let status = if !variants_agree {
        "ambiguous"
    } else if comparison.exact(tolerance) {
        "match"
    } else {
        "mismatch"
    };
    let mut differences = first_differences(&comparison, engine, &oracle, 8);
    if differences.is_empty() && !comparison.exact(tolerance) {
        differences.push(format!(
            "probability: largest difference {:e} over shared outcomes",
            comparison.max_shared_diff
        ));
    }
    json!({
        "status": status,
        "mode": mode,
        "roll": roll,
        "engineOutcomes": comparison.engine_outcomes,
        "oracleOutcomes": comparison.oracle_outcomes,
        "onlyEngine": comparison.only_engine.len(),
        "onlyOracle": comparison.only_oracle.len(),
        "maxSharedDiff": comparison.max_shared_diff,
        "tv": comparison.tv,
        "variants": matching.len(),
        "differences": differences,
    })
}
