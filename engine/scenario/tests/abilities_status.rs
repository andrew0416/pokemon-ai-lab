//! Parity of status-, priority-, PP-, damage- and residual-related abilities (work plan units
//! O50–O53, O61, O62, O65) with Showdown's exact outcome distribution
//! (`engine/oracle/expected/*.turn.json`), plus the states the engine refuses on purpose.

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::rules::Ruleset;
use lab_engine::state::Status;
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_scenario::scenario_choices;

// ---- O52 Rock Head / Magic Guard -------------------------------------------------------------

#[test]
fn rock_head_and_magic_guard_match_showdown() {
    assert_exact_parity("o52-rock-head-magic-guard");
}

// ---- O61 priority ----------------------------------------------------------------------------

#[test]
fn gale_wings_and_triage_priority_match_showdown() {
    assert_exact_parity("o61-gale-wings-triage");
}

#[test]
fn gale_wings_below_full_hp_matches_showdown() {
    assert_exact_parity("o61-gale-wings-damaged");
}

#[test]
fn stall_fractional_priority_matches_showdown() {
    assert_exact_parity("o61-stall");
}

// ---- O62 Pressure ------------------------------------------------------------------------------

#[test]
fn pressure_extra_pp_matches_showdown() {
    assert_exact_parity("o62-pressure");
}

// ---- O65 status immunities -------------------------------------------------------------------

#[test]
fn status_blocking_abilities_match_showdown() {
    assert_exact_parity("o65-status-block");
}

#[test]
fn comatose_sweet_veil_and_magma_armor_match_showdown() {
    assert_exact_parity("o65-comatose-sweet-veil");
}

#[test]
fn leaf_guard_and_sun_freeze_immunity_match_showdown() {
    assert_exact_parity("o65-leaf-guard-sun");
}

/// Showdown's `Update` event would cure a Water Veil holder's burn after the first action; the
/// engine has no `Update` event, so it refuses the state instead of running it uncured.
#[test]
fn status_that_an_ability_would_cure_on_update_is_refused() {
    let fixture = fixture("o65-status-block");
    let (loaded, mut state) = start("o65-status-block", &fixture);
    let choices = scenario_choices(&loaded, &state).unwrap();
    let floatzel = state
        .side(lab_engine::state::SideId::Two)
        .party
        .iter()
        .position(|p| p.species.data().name == "Floatzel")
        .unwrap();
    state.side_mut(lab_engine::state::SideId::Two).party[floatzel].status = Status::Burn;
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::Unsupported(why)) => assert!(why.contains("Water Veil"), "{why}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
