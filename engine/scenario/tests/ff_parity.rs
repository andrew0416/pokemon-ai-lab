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

/// Turn 6 of a random-policy game (library balance-ddee vs crown-cecil9) with a mid-turn switch:
/// Parting Shot at the user's own ally, then the user switches out (`midTurn`); a Throat Chop
/// volatile, stat stages and Mega Floette on the field.
#[test]
fn pinned_mid_game_position_parting_shot_mid_turn() {
    common::assert_exact_parity("ff-pinned-parting-shot-midturn");
}

/// Reduced from a sweep mismatch (crown-cecil9 vs perish-mrada, random game 0, decision 3):
/// Good as Gold's `onTryHit` returns `null` for another Pokémon's status move, so Perish Song
/// spares Gholdengo; the engine used to give it `perishsong`.
#[test]
fn perish_song_spares_good_as_gold() {
    common::assert_exact_parity("ff-perish-song-good-as-gold");
}

/// Reduced from a sweep mismatch (gardevoir-braverilla vs psy-cona, random game 2, decision 3):
/// the Champions mod's Encore replaces the target's queued action (`queue.changeAction`), so the
/// encored Protect moves at +4, before the faster partner, and can succeed; the engine kept the
/// chosen move's priority 0 (base game `onOverrideAction`), so the Protect came last and failed.
#[test]
fn champions_encore_changes_the_queued_action() {
    common::assert_exact_parity("ff-encore-protect-stall");
}

/// Reduced from a sweep mismatch (starmie-braverilla vs sand-owen, random game 2, decision 3): a
/// flinch on a target that already moved is still on it when a later U-turn stops the turn, so
/// the paused state shows it; the engine's F18 shortcut skipped that roll.
#[test]
fn flinch_on_a_mover_shows_in_a_paused_turn() {
    common::assert_exact_parity("ff-flinch-before-uturn-pause");
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
    assert!(!error.is_unsupported(), "{error}");
    let error = error.message();
    assert!(error.starts_with("setup turn 3: none of"), "{error}");
    assert!(error.contains("Tyranitar].hp: 1 vs"), "{error}");
}
