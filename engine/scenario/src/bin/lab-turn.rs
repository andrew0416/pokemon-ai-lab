//! Runs lab-engine's turn enumeration on an oracle scenario and writes a report in the
//! oracle's format, so `engine/oracle/compare.cjs` can compare the two.
//!
//! Usage: lab-turn <scenario.json> [--before <oracle-report.json>] [--out <file>]
//!                 [--mc <samples> [--seed <n>]]
//!
//! `--mc` samples the turn instead of enumerating it (`mode: "mc"`), for turns whose exact
//! distribution is too large; compare such reports with `oracle/marginals.cjs`.
//!
//! The scenario's leads may start in several states (switch-in randomness such as Trace);
//! Showdown's report starts from one of them (its `before`, fixed by the seed). With
//! `--before`, the matching state is used; without it the scenario must have exactly one.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use serde_json::{json, Value};

use lab_engine::rules::Ruleset;
use lab_engine::turn::sample_turn;
use lab_scenario::{
    canonical_json, load_scenario_file, run_decision_mid_turn, scenario_decision,
    scenario_positions, Decision,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-turn: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scenario = None;
    let mut before = None;
    let mut out = None;
    let mut samples: Option<usize> = None;
    let mut seed: u64 = 1;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--before" => {
                i += 1;
                before = args.get(i).cloned();
            }
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "--mc" => {
                i += 1;
                samples = Some(
                    args.get(i)
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|&n| n > 0)
                        .ok_or("--mc needs a positive sample count")?,
                );
            }
            "--seed" => {
                i += 1;
                seed = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--seed needs a number")?;
            }
            other if scenario.is_none() => scenario = Some(other.to_owned()),
            other => return Err(format!("unexpected argument {other}")),
        }
        i += 1;
    }
    let scenario =
        scenario.ok_or("usage: lab-turn <scenario.json> [--before report.json] [--out file]")?;

    let loaded = load_scenario_file(&scenario).map_err(|e| e.to_string())?;
    let states = scenario_positions(&loaded)?;
    let wanted = match &before {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let report: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            Some(report["before"].clone())
        }
        None => None,
    };
    let mut start = None;
    for outcome in &states {
        let key = canonical_json(&outcome.state, &loaded.meta).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&key).expect("valid JSON");
        match &wanted {
            Some(w) if *w == value => start = Some(outcome.clone()),
            None if states.len() == 1 => start = Some(outcome.clone()),
            _ => {}
        }
    }
    let position = start.ok_or_else(|| match &wanted {
        Some(_) => "no initial state matches the report's `before`".to_owned(),
        None => format!(
            "{} initial states; pass --before <oracle report> to pick one",
            states.len()
        ),
    })?;
    let decision = scenario_decision(&loaded, &position)?;
    let mut state = position.state;
    let before_json = canonical_json(&state, &loaded.meta).map_err(|e| e.to_string())?;

    let started = Instant::now();
    // Sampling leaves a turn suspended by a mid-turn switch as it is (no `midTurn` replay).
    let outcomes = match (samples, &decision) {
        (Some(n), Decision::Turn(choices)) => {
            sample_turn(&mut state, Ruleset::CHAMPIONS_MC, *choices, n, seed)
                .map_err(|e| e.to_string())?
        }
        (Some(_), Decision::Replacement(_)) => {
            return Err("--mc is not implemented for a replacement decision".into())
        }
        (None, decision) => {
            run_decision_mid_turn(&mut state, &position.order, decision, &loaded.mid_turn)?
        }
    };
    let elapsed = started.elapsed();

    // Engine states that differ only in data the canonical form leaves out merge here.
    let mut merged: Vec<(f64, String)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        let key = canonical_json(&state, &loaded.meta);
        state.reverse(&outcome.instructions);
        let key = key.map_err(|e| e.to_string())?;
        match index.get(&key) {
            Some(&i) => merged[i].0 += outcome.probability,
            None => {
                index.insert(key.clone(), merged.len());
                merged.push((outcome.probability, key));
            }
        }
    }
    merged.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = merged.iter().map(|(p, _)| p).sum();
    // serde_json's `Value` sorts object keys, so the canonical states are spliced in as the
    // exact strings `canonical_json` writes (the key order `canonicalKey` compares).
    let header = json!({
        "scenario": scenario,
        "format": loaded.meta.format,
        "turn": loaded.meta.turn.as_ref().map(|t| json!({"p1": t.p1, "p2": t.p2})),
        "mode": if samples.is_some() { "mc" } else { "engine" },
        "exact": samples.is_none(),
        "engine": "lab-engine",
        "branches": samples.unwrap_or(outcomes.len()),
        "distinctOutcomes": merged.len(),
        "totalProbability": total,
        "elapsedMs": elapsed.as_secs_f64() * 1000.0,
    });
    let header = serde_json::to_string(&header).expect("serializable");
    let mut text = header[..header.len() - 1].to_owned();
    text.push_str(r#","before":"#);
    text.push_str(&before_json);
    text.push_str(r#","outcomes":["#);
    for (i, (p, key)) in merged.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        let p = serde_json::to_string(p).expect("finite");
        text.push_str(&format!(r#"{{"p":{p},"state":{key},"log":[]}}"#));
    }
    text.push_str("]}");
    eprintln!(
        "engine: {} engine outcomes -> {} canonical outcomes, total p={total:.12}, {:.3} ms",
        outcomes.len(),
        merged.len(),
        elapsed.as_secs_f64() * 1000.0
    );
    match out {
        Some(path) => std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?,
        None => println!("{text}"),
    }
    Ok(())
}
