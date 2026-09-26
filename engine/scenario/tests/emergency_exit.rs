//! Emergency Exit checks on a move's user (`applyRecoilDamage`, Champions `spreadMoveHit` after
//! DamagingHit, `useMoveInner` after MoveFail and after AfterMoveSecondarySelf) and on spread
//! targets the move did no damage to (`hitStepMoveHitLoop` checks every target, a non-numeric
//! result counting 0). Fixtures from Showdown's exact enumeration with a `midTurn` switch.

mod common;

use common::assert_exact_parity;
use lab_scenario::{run_decision_mid_turn, scenario_decision};

/// Life Orb's recoil after the user's own attack takes it to half.
#[test]
fn life_orb_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("ee-life-orb");
}

/// Rocky Helmet (DamagingHit) takes the attacker to half.
#[test]
fn rocky_helmet_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("ee-rocky-helmet");
}

/// Double-Edge's recoil takes the user to half.
#[test]
fn recoil_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("ee-recoil");
}

/// High Jump Kick into an immune target: the crash (MoveFail) takes the user to half.
#[test]
fn crash_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("ee-crash");
}

/// A spread status move (no damage) on a target already below half after Pain Split.
#[test]
fn spread_status_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("ee-spread-done");
}

/// The same with the spread move failing on that target (Growl at -6 Attack).
#[test]
fn spread_failure_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("ee-spread-failed");
}

/// Recoil that knocks the user out from above half: Showdown's `applyRecoilDamage` has no HP
/// guard, flags the fainting Emergency Exit holder and asks for a mid-turn switch of a fainted
/// Pokémon; the engine refuses the turn.
#[test]
fn recoil_knock_out_emergency_exit_is_unsupported() {
    let loaded = lab_scenario::load_scenario_file(
        common::engine_dir().join("oracle/scenarios/ee-recoil-ko.json"),
    )
    .unwrap();
    let positions = lab_scenario::scenario_positions(&loaded).unwrap();
    assert_eq!(positions.len(), 1);
    let position = &positions[0];
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, position).unwrap();
    let result = run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn);
    match result {
        Err(why) => assert!(
            why.contains("Emergency Exit on a user its recoil knocked out"),
            "{why}"
        ),
        Ok(outcomes) => panic!(
            "expected an unsupported turn, got {} outcomes",
            outcomes.len()
        ),
    }
}
