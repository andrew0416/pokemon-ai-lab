//! Parity of moves Champions doubles players use that no library team carries (Opus P): Heal
//! Pulse, Ally Switch, ... Each scenario's exact outcome distribution must equal its oracle
//! fixture (`engine/oracle/expected/<name>.turn.json`).

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

#[test]
fn heal_pulse_heals_half_or_three_quarters_with_mega_launcher() {
    assert_exact_parity("heal-pulse");
}

#[test]
fn heal_pulse_bounces_and_is_blocked_by_protect() {
    assert_exact_parity("heal-pulse-bounce");
}

/// The foes' moves aimed at a position hit whoever stands there after the swap; the partner's
/// queued move follows it.
#[test]
fn ally_switch_swaps_positions_and_queued_moves_follow() {
    assert_exact_parity("ally-switch");
}

/// The second use in a row succeeds with 1/3 (counter 3, then 9) or deletes the volatile.
#[test]
fn ally_switch_again_succeeds_one_time_in_three() {
    assert_exact_parity("ally-switch-again");
}

/// A Healing Wish waiting at the new position heals the ally moved into it (`onSwap`).
#[test]
fn ally_switch_into_a_waiting_healing_wish() {
    assert_exact_parity("ally-switch-healing-wish");
}

#[test]
fn ally_switch_fails_without_a_standing_partner() {
    assert_exact_parity("ally-switch-fail");
}

#[test]
fn ally_switch_under_a_snipe_shot_is_unsupported() {
    assert_unsupported("ally-switch-snipe-shot", "tracks its original target");
}
