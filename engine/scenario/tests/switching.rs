//! Mid-turn switching (WORKPLAN F6): U-turn, Volt Switch and Parting Shot suspend the turn for
//! a switch decision (`Outcome::suspension`, `resume_turn`), which the scenario's `midTurn`
//! choices resolve. Fixtures from Showdown's exact enumeration with the same choices.

mod common;

use common::assert_exact_parity;

/// U-turn, the switch, then the foe's move hits the newcomer.
#[test]
fn uturn_switch_matches_showdown_exactly() {
    assert_exact_parity("uturn-switch");
}

/// Without a bench the flag is dropped and nothing is asked.
#[test]
fn uturn_without_a_bench_matches_showdown_exactly() {
    assert_exact_parity("uturn-no-bench");
}

/// Without a `midTurn` choice the paused position is the outcome (`request: switch`).
#[test]
fn uturn_pause_matches_showdown_exactly() {
    assert_exact_parity("uturn-pause");
}

/// Every outcome of a U-turn turn without a choice carries a suspension.
#[test]
fn uturn_without_a_choice_stays_suspended() {
    let fixture = common::fixture("uturn-pause");
    let (loaded, position) = common::start("uturn-pause", &fixture);
    let mut state = position.state.clone();
    let decision = lab_scenario::scenario_decision(&loaded, &position).unwrap();
    let outcomes = lab_scenario::run_decision(&mut state, &decision).unwrap();
    assert!(!outcomes.is_empty());
    assert!(outcomes.iter().all(|o| o.suspension.is_some()));
}

/// Two U-turns: the turn suspends twice, once per side.
#[test]
fn two_uturns_match_showdown_exactly() {
    assert_exact_parity("uturn-both");
}

/// Parting Shot switches after its drops; blocked drops withdraw the switch.
#[test]
fn parting_shot_matches_showdown_exactly() {
    assert_exact_parity("parting-shot");
}

/// Volt Switch into an immune target neither hits nor switches.
#[test]
fn volt_switch_into_an_immune_target_matches_showdown_exactly() {
    assert_exact_parity("volt-switch-immune");
}

/// A knock-out by U-turn: the user still switches, and the foe replaces at the end of the turn.
#[test]
fn uturn_knock_out_matches_showdown_exactly() {
    assert_exact_parity("uturn-ko");
}

/// Roar drags a random bench member in; Suction Cups stops it without failing the move.
#[test]
fn roar_matches_showdown_exactly() {
    assert_exact_parity("roar-drag");
}

/// Roar fails against a side without a bench.
#[test]
fn roar_without_a_bench_matches_showdown_exactly() {
    assert_exact_parity("roar-no-bench");
}

/// Dragon Tail: damage, then the drag right after the move.
#[test]
fn dragon_tail_matches_showdown_exactly() {
    assert_exact_parity("dragon-tail");
}

/// Eject Button asks its holder to switch; its queued move is cancelled.
#[test]
fn eject_button_matches_showdown_exactly() {
    assert_exact_parity("eject-button");
}

/// U-turn into Eject Button: both sides switch (Champions keeps the attacker's flag).
#[test]
fn eject_button_against_uturn_matches_showdown_exactly() {
    assert_exact_parity("eject-button-uturn");
}

/// Red Card drags the attacker out for a random bench member.
#[test]
fn red_card_matches_showdown_exactly() {
    assert_exact_parity("red-card");
}

/// Emergency Exit after a hit that crosses half.
#[test]
fn emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("emergency-exit");
}

/// No Emergency Exit when the holder was already at half or below.
#[test]
fn emergency_exit_below_half_matches_showdown_exactly() {
    assert_exact_parity("emergency-exit-below");
}

/// Emergency Exit from residual damage suspends the turn after the residual phase.
#[test]
fn emergency_exit_from_residual_matches_showdown_exactly() {
    assert_exact_parity("emergency-exit-residual");
}

/// Emergency Exit from entry hazards in the newcomer's own runSwitch.
#[test]
fn emergency_exit_from_hazards_matches_showdown_exactly() {
    assert_exact_parity("emergency-exit-hazard");
}

/// A chosen switch's `runAction` tail runs the Update before the newcomer's `runSwitch`: a
/// Sitrus Berry holder at 20 HP eats the berry, then takes Stealth Rock, and lives.
#[test]
fn a_switch_runs_the_update_before_the_entry_hazards() {
    assert_exact_parity("switch-update-sitrus");
}

/// U-turn into an Emergency Exit target: the user's switch flag is set in `runMoveEffects`
/// and the target's Emergency Exit clears it, so only the target switches.
#[test]
fn uturn_into_emergency_exit_matches_showdown_exactly() {
    assert_exact_parity("uturn-emergency-exit");
}

/// A Life Orb user dragged out by Red Card pays no recoil (`!source.forceSwitchFlag`).
#[test]
fn red_card_skips_the_life_orb_recoil() {
    assert_exact_parity("red-card-life-orb");
}

/// Unnerve blocks berries only once it has started: while one Unnerve holder replaces another,
/// the switch action's Update lets a foe eat its pending Sitrus Berry.
#[test]
fn unnerve_does_not_block_before_it_starts() {
    assert_exact_parity("unnerve-switch-update");
}
