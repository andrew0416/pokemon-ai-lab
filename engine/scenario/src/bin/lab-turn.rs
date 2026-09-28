//! Runs lab-engine's turn enumeration on an oracle scenario and writes a report in the
//! oracle's format, so `engine/oracle/compare.cjs` can compare the two.
//!
//! Usage: lab-turn <scenario.json> [--before <oracle-report.json>] [--position <i>] [--out <file>]
//!                 [--mc <samples> [--seed <n>]]
//!                 [--rolls full|extremes|quartiles|median|pessimistic-p1|pessimistic-p2|fixed-<k>]
//!
//! `--rolls extremes` branches only on the minimum and maximum damage roll (the oracle's
//! `--mode extremes`; compare against such a report), `quartiles` on four rolls, `median` on
//! one (92%), `pessimistic-pN` on one: the minimum for side N's attacks, the maximum against it,
//! `fixed-<k>` on one for every attack: roll index k (0 = 85%, 15 = 100%; the oracle's
//! `--mode fixed --roll k`, compared exactly).
//!
//! `--factored` enumerates the turn factored (`enumerate_turn_factored`, WORKPLAN P1b: members
//! whose HP takes several values independently are listed instead of one outcome per
//! combination). When the flat outcome count is at most `--expand-limit` (default 2,000,000) the
//! outcomes are expanded and the report is the usual one; otherwise the report lists the factored
//! outcomes (`factored`: `p`, the canonical state at each listed member's smallest HP, and `hp`:
//! `"p1: Name" → [[hp, p], ...]`). A turn suspended by a mid-turn switch is not resumed.
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
use lab_engine::state::SideId;
use lab_engine::turn::{enumerate_turn_factored, sample_turn, EnumerateOptions, RollMode};
use lab_scenario::{
    canonical_json, load_scenario_file, run_decision_mid_turn_with, scenario_decision,
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
    let mut position_index: Option<usize> = None;
    let mut samples: Option<usize> = None;
    let mut seed: u64 = 1;
    let mut options = EnumerateOptions::default();
    let mut factored = false;
    let mut expand_limit: f64 = 2_000_000.0;
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
            "--position" => {
                i += 1;
                position_index = Some(
                    args.get(i)
                        .and_then(|s| s.parse::<usize>().ok())
                        .ok_or("--position needs an index")?,
                );
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
            "--factored" => factored = true,
            "--expand-limit" => {
                i += 1;
                expand_limit = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--expand-limit needs a number")?;
            }
            "--seed" => {
                i += 1;
                seed = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--seed needs a number")?;
            }
            "--rolls" => {
                i += 1;
                options.rolls = match args.get(i).map(String::as_str) {
                    Some("full") => RollMode::Full,
                    Some("extremes") => RollMode::Extremes,
                    Some("quartiles") => RollMode::Quartiles,
                    Some("median") => RollMode::Median,
                    Some("pessimistic-p1") => RollMode::Pessimistic(SideId::One),
                    Some("pessimistic-p2") => RollMode::Pessimistic(SideId::Two),
                    Some(fixed) if fixed.starts_with("fixed-") => fixed["fixed-".len()..]
                        .parse::<u8>()
                        .ok()
                        .and_then(RollMode::fixed)
                        .ok_or("--rolls fixed-<k> needs a roll index 0..15")?,
                    _ => {
                        return Err("--rolls needs full, extremes, quartiles, median, \
                                    pessimistic-p1, pessimistic-p2 or fixed-<k>"
                            .into())
                    }
                };
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
    for (i, outcome) in states.iter().enumerate() {
        let key = canonical_json(&outcome.state, &loaded.meta).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&key).expect("valid JSON");
        match &wanted {
            Some(w) if *w == value => start = Some(outcome.clone()),
            None if states.len() == 1 || position_index == Some(i) => start = Some(outcome.clone()),
            _ => {}
        }
    }
    let position = start.ok_or_else(|| match &wanted {
        Some(w) => {
            let mut text = "no initial state matches the report's `before`:
"
            .to_owned();
            text.push_str(&format!(
                "oracle: {w}
"
            ));
            for (i, outcome) in states.iter().enumerate() {
                let key =
                    canonical_json(&outcome.state, &loaded.meta).unwrap_or_else(|e| e.to_string());
                text.push_str(&format!(
                    "engine {i}: {key}
"
                ));
            }
            text
        }
        None => format!(
            "{} initial states; pass --before <oracle report> or --position <index> to pick one",
            states.len()
        ),
    })?;
    let decision = scenario_decision(&loaded, &position)?;
    let mut state = position.state;
    let before_json = canonical_json(&state, &loaded.meta).map_err(|e| e.to_string())?;

    let started = Instant::now();
    let mut factored_summary = None;
    // Sampling leaves a turn suspended by a mid-turn switch as it is (no `midTurn` replay).
    let outcomes = match (samples, &decision) {
        (None, Decision::Turn(choices)) if factored => {
            let factored =
                enumerate_turn_factored(&mut state, Ruleset::CHAMPIONS_MC, *choices, options)
                    .map_err(|e| e.to_string())?;
            let flat: f64 = factored.iter().map(|o| o.flat_count()).sum();
            let elapsed = started.elapsed();
            let largest = factored
                .iter()
                .flat_map(|o| o.hp.iter().map(|(_, v)| v.len()))
                .max()
                .unwrap_or(1);
            let suspended = factored.iter().filter(|o| o.suspension.is_some()).count();
            eprintln!(
                "engine: {} factored outcomes standing for {flat} flat ones (largest HP distribution {largest} values, {suspended} suspended), {:.3} ms",
                factored.len(),
                elapsed.as_secs_f64() * 1000.0
            );
            factored_summary = Some(json!({
                "factoredOutcomes": factored.len(),
                "flatCount": flat,
                "largestHpDistribution": largest,
                "suspended": suspended,
                "enumerateMs": elapsed.as_secs_f64() * 1000.0,
            }));
            if flat > expand_limit {
                return write_factored(
                    &scenario,
                    &loaded,
                    &mut state,
                    &before_json,
                    &factored,
                    factored_summary.expect("set"),
                    out.as_deref(),
                );
            }
            factored.iter().flat_map(|o| o.expand()).collect()
        }
        (None, Decision::Replacement(_)) if factored => {
            return Err("--factored is not implemented for a replacement decision".into())
        }
        (Some(n), Decision::Turn(choices)) => {
            sample_turn(&mut state, Ruleset::CHAMPIONS_MC, *choices, n, seed)
                .map_err(|e| e.to_string())?
        }
        (Some(_), Decision::Replacement(_)) => {
            return Err("--mc is not implemented for a replacement decision".into())
        }
        (None, decision) => run_decision_mid_turn_with(
            &mut state,
            &position.order,
            decision,
            &loaded.mid_turn,
            options,
        )?,
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
        "mode": match (samples, options.rolls) {
            (Some(_), _) => "mc",
            (None, RollMode::Full) => "engine",
            (None, RollMode::Extremes) => "extremes",
            (None, RollMode::Quartiles) => "quartiles",
            (None, RollMode::Median) => "median",
            (None, RollMode::Pessimistic(_)) => "pessimistic",
            (None, RollMode::Fixed(_)) => "fixed",
        },
        "exact": samples.is_none() && options.rolls.is_exact(),
        "engine": "lab-engine",
        "branches": samples.unwrap_or(outcomes.len()),
        "distinctOutcomes": merged.len(),
        "totalProbability": total,
        "elapsedMs": elapsed.as_secs_f64() * 1000.0,
    });
    let mut header = header;
    if let (None, RollMode::Fixed(k)) = (samples, options.rolls) {
        header["roll"] = json!(k);
    }
    if let Some(summary) = factored_summary {
        header["factored"] = summary;
    }
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

/// The report of a factored enumeration too large to expand: the header, `before` and the
/// factored outcomes (see the module documentation).
fn write_factored(
    scenario: &str,
    loaded: &lab_scenario::LoadedScenario,
    state: &mut lab_engine::Doubles,
    before_json: &str,
    factored: &[lab_engine::turn::FactoredOutcome],
    summary: Value,
    out: Option<&str>,
) -> Result<(), String> {
    let total: f64 = factored.iter().map(|o| o.probability).sum();
    let header = json!({
        "scenario": scenario,
        "format": loaded.meta.format,
        "turn": loaded.meta.turn.as_ref().map(|t| json!({"p1": t.p1, "p2": t.p2})),
        "mode": "factored",
        "exact": true,
        "engine": "lab-engine",
        "totalProbability": total,
        "factored": summary,
    });
    let header = serde_json::to_string(&header).expect("serializable");
    let mut text = header[..header.len() - 1].to_owned();
    text.push_str(r#","before":"#);
    text.push_str(before_json);
    text.push_str(r#","outcomes":[],"factoredOutcomes":["#);
    for (i, outcome) in factored.iter().enumerate() {
        if i > 0 {
            text.push(',');
        }
        state.apply(&outcome.instructions);
        let key = canonical_json(state, &loaded.meta);
        state.reverse(&outcome.instructions);
        let key = key.map_err(|e| e.to_string())?;
        let mut hp = serde_json::Map::new();
        for (pokemon, values) in &outcome.hp {
            let side = &loaded.meta.sides[pokemon.side.index()];
            let name = format!(
                "p{}: {}",
                pokemon.side.index() + 1,
                side.members[usize::from(pokemon.party)].name
            );
            hp.insert(name, json!(values));
        }
        let p = serde_json::to_string(&outcome.probability).expect("finite");
        let hp = serde_json::to_string(&hp).expect("serializable");
        text.push_str(&format!(r#"{{"p":{p},"state":{key},"hp":{hp}}}"#));
    }
    text.push_str("]}");
    match out {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?,
        None => println!("{text}"),
    }
    Ok(())
}
