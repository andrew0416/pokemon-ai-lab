//! Board A4-t4-from-canonical-audit (VD): the `from_canonical.rs` refusals classified as
//! `unsupported` in `scripts/refusals.classification.json` (`loader`) that no other test
//! reaches. (A lossy volatile without `x-hidden` and a mid-turn request are
//! `ae_hand_positions.rs::missing_hidden_state_is_refused`.)

mod common;

use lab_scenario::{canonical_json, load_scenario_file, scenario_positions, state_from_canonical};
use serde_json::Value;

/// Trick-or-Treat and Forest's Curse (standard in Champions) give a third type, which
/// `getTypes()` prints and the engine's two type slots cannot hold: refused, not truncated.
#[test]
fn a_third_type_is_refused() {
    let loaded =
        load_scenario_file(common::engine_dir().join("oracle/scenarios/hypnosis-gravity.json"))
            .unwrap();
    let position = &scenario_positions(&loaded).unwrap()[0];
    let text = canonical_json(&position.state, &loaded.meta).unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    let mon = value["sides"][0]["pokemon"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|m| !m["slot"].is_null())
        .unwrap();
    mon["types"] = "Psychic/Fairy/Ghost".into();
    let e = state_from_canonical(&loaded.state, &loaded.meta, &value).unwrap_err();
    assert!(e.is_unsupported(), "{}", e.message());
    assert!(e.message().contains("at most two"), "{}", e.message());
}
