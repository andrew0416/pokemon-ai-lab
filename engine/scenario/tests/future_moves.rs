//! Future Sight and Doom Desire (Opus T): the `futuremove` slot condition (F12 machinery), set
//! by the moves' `onTry` and hitting at the residual of the turn after next (order 3). Fixtures
//! from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::turn::TurnError;
use lab_scenario::{load_scenario_file, run_decision, scenario_decision, scenario_positions};

/// Runs the scenario's turn (first position) and expects `Unsupported` naming `expected`.
fn assert_unsupported(name: &str, expected: &str) {
    let loaded =
        load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    match run_decision(&mut state, &decision) {
        Err(TurnError::Unsupported(what)) => assert!(what.contains(expected), "{what}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

/// The use adds the slot condition and succeeds without a hit or Life Orb recoil; a second
/// future move at the same position fails.
#[test]
fn future_sight_use_sets_the_slot_condition() {
    assert_exact_parity("future-sight-use");
}

/// The hits two turns later: Future Sight's hit respects type immunity (Dark) yet costs Life Orb
/// recoil; Doom Desire uses its user's current boosts.
#[test]
fn future_moves_hit_two_turns_later() {
    assert_exact_parity("future-sight-hit");
}

/// The hit lands on whoever holds the position; a fainted holder is not hit.
#[test]
fn future_sight_hits_the_position() {
    assert_exact_parity("future-sight-switched-target");
}

/// A slot condition's residual runs for a fainted occupant: the Wish ends without a heal.
#[test]
fn wish_ends_on_a_fainted_holder() {
    assert_exact_parity("wish-fainted-holder");
}

/// A future move whose user has left the field is refused (Showdown computes it with the
/// benched user's stored stats).
#[test]
fn future_sight_after_its_user_left_is_unsupported() {
    assert_unsupported("future-sight-user-left", "after its user left the field");
}
