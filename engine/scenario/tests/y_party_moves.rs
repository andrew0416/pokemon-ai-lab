//! Uproar, Fling and Beat Up (Opus Y unit 3). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;
use lab_scenario::{load_scenario_file, run_decision, scenario_decision, scenario_positions};

/// Uproar wakes sleeping Pokémon and starts its three-turn lock.
#[test]
fn uproar_wakes_and_locks() {
    assert_exact_parity("y-uproar-wake");
}

/// During an uproar nobody falls asleep (Spore, Rest), and the locked Uproar costs no PP.
#[test]
fn uproar_blocks_sleep() {
    assert_exact_parity("y-uproar-sleep-blocked");
}

/// Uproar ends at the residual of its third turn.
#[test]
fn uproar_ends_after_three_turns() {
    assert_exact_parity("y-uproar-end");
}

/// Throat Chop stops the locked Uproar and ends it at the residual.
#[test]
fn uproar_ends_under_throat_chop() {
    assert_exact_parity("y-uproar-throat-chop");
}

/// A flung berry is eaten by the target; the thrower loses it at the Update (Unburden).
#[test]
fn fling_feeds_the_target_a_berry() {
    assert_exact_parity("y-fling-berry");
}

/// A flung King's Rock flinches the target.
#[test]
fn fling_kings_rock_flinches() {
    assert_exact_parity("y-fling-kings-rock");
}

/// A flung Light Ball paralyses the target.
#[test]
fn fling_light_ball_paralyses() {
    assert_exact_parity("y-fling-light-ball");
}

/// Shield Dust removes the appended secondary of a flung Poison Barb.
#[test]
fn fling_status_blocked_by_shield_dust() {
    assert_exact_parity("y-fling-shield-dust");
}

/// A flung White Herb resets the target's lowered stages.
#[test]
fn fling_white_herb_resets_drops() {
    assert_exact_parity("y-fling-white-herb");
}

/// Klutz, the user's own Mega Stone and no item make Fling fail without losing anything.
#[test]
fn fling_fails_in_prepare_hit() {
    assert_exact_parity("y-fling-fails");
}

/// Fling into Protect still throws the item at the Update after the action.
#[test]
fn fling_into_protect_loses_the_item() {
    assert_exact_parity("y-fling-protect");
}

/// A flung Life Orb boosts the damage while held and causes no recoil.
#[test]
fn fling_life_orb_boosts_without_recoil() {
    assert_exact_parity("y-fling-life-orb");
}

/// Beat Up hits once per eligible ally with the ally's own power.
#[test]
fn beat_up_hits_per_ally() {
    assert_exact_parity("y-beat-up");
}

/// Two benched allies of different power: they hit in `Side::party_order` (board R9, SS), here
/// the team order since nothing switched; the move used to be refused.
#[test]
fn beat_up_with_benched_allies_of_different_power() {
    let loaded = load_scenario_file(
        common::engine_dir().join("oracle/scenarios/y-beat-up-bench-order.json"),
    )
    .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes = run_decision(&mut state, &decision).unwrap();
    assert!(!outcomes.is_empty());
}
