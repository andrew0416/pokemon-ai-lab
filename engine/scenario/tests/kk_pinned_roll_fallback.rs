//! V8-parity-regression-corpus (Opus KK): a pinned setup turn whose recorded outcome needs a
//! damage roll the scenario's reduced `setupRolls` mode does not branch on.
//!
//! The corpus re-check found 11 positions of one V2 game (psy-lello vs psy-nihat, random game 0,
//! decisions 7-17) whose pinned turn 7 has Arcanine at 78 HP after a confusion self-hit: a middle
//! roll, which the engine that played the game (b04d9fb) still drew in `Extremes` mode (the
//! self-hit went through `uniform(16)` until JJ's fix b9258ca). Showdown reaches the pin (the
//! oracle replays its trace), the current engine's `Extremes` replay cannot, so the positions
//! failed with "none of 12 position(s) has the pinned canonical state". Replaying the turn in
//! `Full` when the reduced mode misses the pin finds it; the pin keeps only its own positions, so
//! the result is what a `Full` replay gives.

mod common;

use lab_scenario::{canonical_value, load_scenario_str, scenario_positions, LoadedScenario};
use serde_json::Value;

/// `single-hit` (Grassy Glide into Tyranitar, Low Kick into Rillaboom) as a setup turn, then the
/// same choices again as the decision.
fn scenario(rolls: &str, pin: Option<&Value>) -> LoadedScenario {
    let dir = common::engine_dir().join("oracle/scenarios");
    let mut s: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("single-hit.json")).unwrap())
            .unwrap();
    let turn = s["turn"].clone();
    s["setupTurns"] = serde_json::json!([[turn["p1"], turn["p2"]]]);
    s["setupRolls"] = Value::from(rolls);
    if let Some(pin) = pin {
        s["setupStates"] = serde_json::json!([pin]);
    }
    load_scenario_str(&s.to_string(), &dir).unwrap()
}

fn canonical_states(loaded: &LoadedScenario) -> Vec<(Value, f64)> {
    scenario_positions(loaded)
        .unwrap()
        .iter()
        .map(|p| {
            (
                canonical_value(&p.state, &loaded.meta).unwrap(),
                p.probability,
            )
        })
        .collect()
}

#[test]
fn a_pin_from_a_middle_roll_is_found_in_the_exact_distribution() {
    let full = canonical_states(&scenario("full", None));
    let extremes: Vec<Value> = canonical_states(&scenario("extremes", None))
        .into_iter()
        .map(|(v, _)| v)
        .collect();
    // An outcome only a middle damage roll makes.
    let (pin, _) = full
        .iter()
        .find(|(v, _)| !extremes.contains(v))
        .expect("a full-mode outcome outside the extremes outcomes")
        .clone();

    let found = canonical_states(&scenario("extremes", Some(&pin)));
    assert!(!found.is_empty());
    assert!(found.iter().all(|(v, _)| *v == pin));
    let total: f64 = found.iter().map(|(_, p)| p).sum();
    assert!((total - 1.0).abs() < 1e-12, "{total}");
    // The same positions as a replay in full mode.
    assert_eq!(found, canonical_states(&scenario("full", Some(&pin))));
}

#[test]
fn a_pin_the_reduced_mode_reaches_does_not_need_the_exact_replay() {
    let extremes = canonical_states(&scenario("extremes", None));
    let (pin, _) = extremes[0].clone();
    let found = canonical_states(&scenario("extremes", Some(&pin)));
    assert!(found.iter().all(|(v, _)| *v == pin));
    let total: f64 = found.iter().map(|(_, p)| p).sum();
    assert!((total - 1.0).abs() < 1e-12, "{total}");
}

#[test]
fn a_pin_no_roll_reaches_is_still_an_error() {
    let full = canonical_states(&scenario("full", None));
    let mut pin = full[0].0.clone();
    let tyranitar = pin["sides"][1]["pokemon"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == "Tyranitar")
        .unwrap();
    tyranitar["hp"] = Value::from(1);
    let error = scenario_positions(&scenario("extremes", Some(&pin))).unwrap_err();
    assert!(error.starts_with("setup turn 1: none of"), "{error}");
    assert!(
        error.ends_with("(nor in the turn's exact distribution)"),
        "{error}"
    );
}
