//! Turn engine parity with Showdown: lab-engine's exact outcome distribution for an oracle
//! scenario must equal the oracle's `full` enumeration (`engine/oracle/expected/*.turn.json`,
//! made by `enumerate.cjs` + `strip-report.cjs`): the same canonical end states with the same
//! probabilities.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use lab_engine::rules::Ruleset;
use lab_engine::turn::{enumerate_turn, sample_turn};
use lab_engine::Doubles;
use lab_scenario::{
    canonical_json, load_scenario_file, scenario_choices, scenario_states, LoadedScenario,
};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Canonical state as an order-independent key (serde_json sorts object keys).
fn key(state: &Value) -> String {
    serde_json::to_string(state).expect("serializable")
}

fn fixture(name: &str) -> Value {
    let path = engine_dir().join(format!("oracle/expected/{name}.turn.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

/// The scenario and the initial state matching the fixture's `before`.
fn start(name: &str, fixture: &Value) -> (LoadedScenario, Doubles) {
    let loaded =
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    let before = key(&fixture["before"]);
    let state = scenario_states(&loaded)
        .unwrap()
        .into_iter()
        .map(|o| o.state)
        .find(|s| {
            let json: Value = serde_json::from_str(&canonical_json(s, &loaded.meta).unwrap())
                .expect("valid JSON");
            key(&json) == before
        })
        .expect("one initial state matches the oracle's `before`");
    (loaded, state)
}

/// Engine outcomes as canonical key → probability.
fn distribution(
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

fn assert_exact_parity(name: &str) {
    let fixture = fixture(name);
    let (loaded, mut state) = start(name, &fixture);
    let choices = scenario_choices(&loaded, &state).unwrap();
    let outcomes = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    let engine = distribution(&loaded, &mut state, &outcomes);

    let mut oracle: HashMap<String, f64> = HashMap::new();
    for o in fixture["outcomes"].as_array().unwrap() {
        *oracle.entry(key(&o["state"])).or_insert(0.0) += o["p"].as_f64().unwrap();
    }
    assert_eq!(engine.len(), oracle.len(), "{name}: number of outcomes");
    for (state, p) in &oracle {
        let q = engine
            .get(state)
            .unwrap_or_else(|| panic!("{name}: engine lacks an oracle outcome:\n{state}"));
        assert!(
            (p - q).abs() < 1e-12,
            "{name}: p {p} vs engine {q} for\n{state}"
        );
    }
}

#[test]
fn single_hit_matches_showdown_exactly() {
    assert_exact_parity("single-hit");
}

#[test]
fn hypnosis_under_gravity_matches_showdown_exactly() {
    assert_exact_parity("hypnosis-gravity");
}

#[test]
fn sampling_agrees_with_enumeration() {
    let fixture = fixture("single-hit");
    let (loaded, mut state) = start("single-hit", &fixture);
    let choices = scenario_choices(&loaded, &state).unwrap();
    let exact = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    let exact = distribution(&loaded, &mut state, &exact);
    let samples = 40_000;
    let sampled = sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, samples, 11).unwrap();
    let sampled = distribution(&loaded, &mut state, &sampled);
    let mut tv = 0.0;
    for (k, p) in &exact {
        tv += (p - sampled.get(k).copied().unwrap_or(0.0)).abs() / 2.0;
    }
    for k in sampled.keys() {
        assert!(exact.contains_key(k), "sampled an impossible outcome");
    }
    // Noise ~ sqrt(k / (2 pi n)) with k = 271 outcomes.
    let noise = (exact.len() as f64 / (2.0 * std::f64::consts::PI * samples as f64)).sqrt();
    assert!(tv < 3.0 * noise, "TV {tv} vs noise {noise}");
}
