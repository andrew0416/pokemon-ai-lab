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
