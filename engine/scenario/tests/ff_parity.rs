//! FF-parity-harness: positions from played games, replayed as pinned setup turns
//! (`startState`/`setupStates`/`setupRolls`), against the Showdown oracle.

mod common;

use lab_scenario::{load_scenario_file, scenario_positions};
use serde_json::Value;

/// Turn 8 of a random-policy game (gardevoir-braverilla vs library sand-owen): choice lock and
/// Encore on Rillaboom, a -1 Atk Sableye, sand and Grassy Terrain timers, a fainted lead; the
/// eight earlier decisions (turns and replacements) are pinned setup turns.
#[test]
fn pinned_mid_game_position_sand_owen_turn_8() {
    common::assert_exact_parity("ff-pinned-sand-owen-t8");
}

/// The pins leave exactly one position (the recorded game's), with probability 1.
#[test]
fn pinned_setup_turns_leave_one_position() {
    let loaded = load_scenario_file(
        common::engine_dir().join("oracle/scenarios/ff-pinned-sand-owen-t8.json"),
    )
    .unwrap();
    assert_eq!(loaded.setup_states.len(), loaded.setup_turns.len());
    let positions = scenario_positions(&loaded).unwrap();
    assert_eq!(positions.len(), 1);
    assert!((positions[0].probability - 1.0).abs() < 1e-12);
}

/// A pin no setup outcome reaches is an error naming where the closest outcome differs.
#[test]
fn unreachable_pin_is_an_error() {
    let path = common::engine_dir().join("oracle/scenarios/ff-pinned-sand-owen-t8.json");
    let mut scenario: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    // Tyranitar at 1 HP after the third decision: no outcome of that turn leaves it there.
    let pin = &mut scenario["setupStates"][2]["sides"][1]["pokemon"];
    let tyranitar = pin
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == "Tyranitar")
        .unwrap();
    tyranitar["hp"] = Value::from(1);
    let loaded =
        lab_scenario::load_scenario_str(&scenario.to_string(), path.parent().unwrap()).unwrap();
    let error = scenario_positions(&loaded).unwrap_err();
    assert!(error.starts_with("setup turn 3: none of"), "{error}");
    assert!(error.contains("Tyranitar].hp: 1 vs"), "{error}");
}
