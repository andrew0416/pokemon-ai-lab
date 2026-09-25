//! Shared helpers for oracle-parity integration tests. Each test file does `mod common;`.
//!
//! An oracle fixture (`engine/oracle/expected/<name>.turn.json`, made by `enumerate.cjs` +
//! `strip-report.cjs`) holds the canonical `before` state and the exact outcome distribution of
//! one turn. `assert_exact_parity` checks that lab-engine enumerates the same canonical end
//! states with the same probabilities.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use lab_engine::Doubles;
use lab_scenario::{
    canonical_json, load_scenario_file, run_decision, scenario_decision, scenario_positions,
    LoadedScenario, Position,
};

pub fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Canonical state as an order-independent key (serde_json sorts object keys).
pub fn key(state: &Value) -> String {
    serde_json::to_string(state).expect("serializable")
}

pub fn fixture(name: &str) -> Value {
    let path = engine_dir().join(format!("oracle/expected/{name}.turn.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

/// The scenario and the position (after switch-ins, setup turns and patch) matching the
/// fixture's `before`.
pub fn start(name: &str, fixture: &Value) -> (LoadedScenario, Position) {
    let loaded =
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    let before = key(&fixture["before"]);
    let position = scenario_positions(&loaded)
        .unwrap()
        .into_iter()
        .find(|p| {
            let json: Value =
                serde_json::from_str(&canonical_json(&p.state, &loaded.meta).unwrap())
                    .expect("valid JSON");
            key(&json) == before
        })
        .expect("one position matches the oracle's `before`");
    (loaded, position)
}

/// Engine outcomes as canonical key → probability.
pub fn distribution(
    loaded: &LoadedScenario,
    state: &mut Doubles,
    outcomes: &[lab_engine::instruction::Outcome],
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    let original = state.clone();
    for outcome in outcomes {
        state.apply(&outcome.instructions);
        let json: Value =
            serde_json::from_str(&canonical_json(state, &loaded.meta).unwrap()).unwrap();
        state.reverse(&outcome.instructions);
        assert_eq!(*state, original, "outcome instructions must reverse");
        *out.entry(key(&json)).or_insert(0.0) += outcome.probability;
    }
    out
}

/// The oracle's outcome distribution as canonical key → probability.
pub fn oracle_distribution(fixture: &Value) -> HashMap<String, f64> {
    let mut oracle: HashMap<String, f64> = HashMap::new();
    for o in fixture["outcomes"].as_array().unwrap() {
        *oracle.entry(key(&o["state"])).or_insert(0.0) += o["p"].as_f64().unwrap();
    }
    oracle
}

/// Exact parity of one scenario's turn with its oracle fixture of the same name.
pub fn assert_exact_parity(name: &str) {
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes = run_decision(&mut state, &decision).unwrap();
    let engine = distribution(&loaded, &mut state, &outcomes);
    let oracle = oracle_distribution(&fixture);
    assert_eq!(engine.len(), oracle.len(), "{name}: number of outcomes");
    for (state, p) in &oracle {
        let q = engine.get(state).unwrap_or_else(|| {
            panic!(
                "{name}: engine lacks an oracle outcome:
{state}"
            )
        });
        assert!(
            (p - q).abs() < 1e-12,
            "{name}: p {p} vs engine {q} for
{state}"
        );
    }
}
