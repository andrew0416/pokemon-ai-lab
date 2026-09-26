//! Parity of Emergency Exit on a user its own recoil knocked out (Opus DD unit B27, lifting
//! Opus X's B3 refusal). `faint()` clears the user's `switchFlag`, then
//! `runEvent('EmergencyExit')` after the recoil (no HP guard) flags it again: until its faint is
//! processed it is an active Pokémon at 0 HP with `switchFlag === true`, which the
//! `getAllActive()` checks of Eject Button (AfterMoveSecondary, after the recoil in Champions)
//! and Eject Pack (AfterMove) see, so neither acts. The fainted Pokémon keeps the flag
//! (`clearVolatile(false)`) and Showdown asks for its replacement mid-turn, with actions still
//! queued.

mod common;

use common::assert_exact_parity;
use lab_engine::state::SideId;
use lab_scenario::{
    load_scenario_file, run_decision_mid_turn, scenario_decision, scenario_positions,
    side_must_switch,
};

/// Weak Armor Chansey's Eject Pack stays at AfterMove; p1 replaces the fainted Golisopod
/// mid-turn, whose switch-in uses the pack, and p2 switches Chansey out, all before Snorlax
/// moves.
#[test]
fn emergency_exit_recoil_eject_pack_matches_showdown() {
    assert_exact_parity("dd-emergency-exit-recoil-eject-pack");
}

/// Chansey's Eject Button stays unused (the recoil and Emergency Exit come before
/// AfterMoveSecondary); only p1 is asked, for the fainted Golisopod's replacement.
#[test]
fn emergency_exit_recoil_eject_button_matches_showdown() {
    assert_exact_parity("dd-emergency-exit-recoil-eject-button");
}

/// Double-Edge knocks Blissey and, by its recoil, the Emergency Exit user out (Opus N's
/// `ee-recoil-ko`, refused until now): p2 replaces the fainted Golisopod mid-turn and the
/// newcomer takes the Seismic Toss still queued against that position.
#[test]
fn emergency_exit_recoil_ko_matches_showdown() {
    assert_exact_parity("dd-emergency-exit-recoil-ko");
}

/// Without mid-turn choices the turn stays paused at the request for the fainted Golisopod:
/// p1's request is `switch`, p2's empty.
#[test]
fn emergency_exit_recoil_paused_matches_showdown() {
    assert_exact_parity("dd-emergency-exit-recoil-paused");
}

/// The paused outcomes ask p1 (the fainted Golisopod's position) and not p2 for a mid-turn
/// switch; the miss ends the turn with nobody asked.
#[test]
fn fainted_flagged_user_asks_for_a_mid_turn_switch() {
    let loaded = load_scenario_file(
        common::engine_dir().join("oracle/scenarios/dd-emergency-exit-recoil-paused.json"),
    )
    .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();
    let mut suspended = 0.0;
    for outcome in &outcomes {
        let mut end = state.clone();
        end.apply(&outcome.instructions);
        let paused = outcome.suspension.is_some();
        assert_eq!(side_must_switch(&end, SideId::One), paused);
        assert!(!side_must_switch(&end, SideId::Two));
        if paused {
            suspended += outcome.probability;
            let golisopod = &end.side(SideId::One).slots[0];
            assert_eq!(golisopod.party_index, None, "the user fainted");
            assert!(golisopod.fainted_occupant.is_some());
            assert!(golisopod.must_switch_out());
        }
    }
    assert!(
        (suspended - 0.8).abs() < 1e-9,
        "every hit knocks the user out: {suspended}"
    );
}
