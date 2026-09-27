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

use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_engine::Doubles;
use lab_scenario::{
    canonical_json, load_scenario_file, run_decision_mid_turn, run_decision_mid_turn_with,
    scenario_decision, scenario_positions, LoadedScenario, Position,
};

pub fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Canonical state as an order-independent key (serde_json sorts object keys).
pub fn key(state: &Value) -> String {
    lab_scenario::parity::value_key(state)
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
    let original = state.clone();
    let out = lab_scenario::parity::engine_distribution(&loaded.meta, state, outcomes).unwrap();
    assert_eq!(*state, original, "outcome instructions must reverse");
    out
}

/// The oracle's outcome distribution as canonical key → probability.
pub fn oracle_distribution(fixture: &Value) -> HashMap<String, f64> {
    lab_scenario::parity::report_distribution(fixture).unwrap()
}

/// Parity with a Monte Carlo oracle fixture (`oracle/expected/<name>.mc.json`, `enumerate.cjs
/// --mode mc`): the engine's exact distribution against Showdown's sampled one, within
/// sampling noise (`sqrt(k / 2πn)` for `k` outcomes and `n` samples), and no sampled outcome
/// the engine does not produce. For turns whose exact enumeration is out of Showdown's reach
/// (multi-hit moves).
pub fn assert_mc_parity(name: &str) {
    let path = engine_dir().join(format!("oracle/expected/{name}.mc.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let fixture: Value = serde_json::from_str(&text).unwrap();
    let samples = fixture["branches"]
        .as_u64()
        .expect("mc reports count samples") as f64;
    let (loaded, position) = start(name, &fixture);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();
    let engine = distribution(&loaded, &mut state, &outcomes);
    let oracle = oracle_distribution(&fixture);
    let mut tv = 0.0;
    for (k, p) in &engine {
        tv += (p - oracle.get(k).copied().unwrap_or(0.0)).abs() / 2.0;
    }
    for k in oracle.keys() {
        assert!(
            engine.contains_key(k),
            "{name}: Showdown sampled an outcome the engine lacks:
{k}"
        );
        if !engine.contains_key(k) {
            tv += oracle[k] / 2.0;
        }
    }
    let noise = (engine.len() as f64 / (2.0 * std::f64::consts::PI * samples)).sqrt();
    assert!(
        tv < 3.0 * noise,
        "{name}: TV {tv} vs noise {noise} ({} outcomes)",
        engine.len()
    );
}

/// Exact parity of one scenario's turn with its oracle fixture of the same name.
pub fn assert_exact_parity(name: &str) {
    assert_parity_with(name, &fixture(name), EnumerateOptions::default());
}

/// Parity with the oracle's `--mode extremes` fixture (`oracle/expected/<name>.extremes.json`,
/// damage rolls only min and max at 1/2 each) under `RollMode::Extremes`: the same
/// approximation on both sides gives an exact match (WORKPLAN F18).
pub fn assert_extremes_parity(name: &str) {
    let path = engine_dir().join(format!("oracle/expected/{name}.extremes.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let fixture: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        fixture["mode"], "extremes",
        "{name}: not an extremes fixture"
    );
    assert_parity_with(
        name,
        &fixture,
        EnumerateOptions {
            rolls: RollMode::Extremes,
        },
    );
}

fn assert_parity_with(name: &str, fixture: &Value, options: EnumerateOptions) {
    let (loaded, position) = start(name, fixture);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes = run_decision_mid_turn_with(
        &mut state,
        &position.order,
        &decision,
        &loaded.mid_turn,
        options,
    )
    .unwrap();
    let engine = distribution(&loaded, &mut state, &outcomes);
    let oracle = oracle_distribution(fixture);
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
