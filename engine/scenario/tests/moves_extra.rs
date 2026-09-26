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

/// Crafty Shield blocks status moves (the side's own ally's too) before Magic Bounce acts;
/// damaging moves pass.
#[test]
fn crafty_shield_blocks_status_moves_before_magic_bounce() {
    assert_exact_parity("crafty-shield");
}

/// Mat Block on the first turn out blocks damaging moves (a spread move before its accuracy
/// roll), not status moves.
#[test]
fn mat_block_blocks_damaging_moves_on_the_first_turn() {
    assert_exact_parity("mat-block");
}

#[test]
fn mat_block_fails_after_the_first_turn_out() {
    assert_exact_parity("mat-block-late");
}

#[test]
fn feint_breaks_crafty_shield() {
    assert_exact_parity("feint-crafty-shield");
}

/// Protect's TryHit (priority 3) comes before Magic Bounce's (priority 1): nothing bounces.
#[test]
fn protect_stops_a_move_before_magic_bounce() {
    assert_exact_parity("magic-bounce-protect");
}

/// Heavy Slam at 80 base power (3x to 4x the target's weight).
#[test]
fn heavy_slam_power_from_the_weight_ratio() {
    assert_exact_parity("heavy-slam");
}

/// Heat Crash just over 2x the target's weight: 60 base power.
#[test]
fn heat_crash_power_at_a_bracket_edge() {
    assert_exact_parity("heat-crash");
}
