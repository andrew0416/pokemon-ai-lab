//! Parity of status-, priority-, PP-, damage- and residual-related abilities (work plan units
//! O50–O53, O61, O62, O65) with Showdown's exact outcome distribution
//! (`engine/oracle/expected/*.turn.json`), plus the states the engine refuses on purpose.

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::rules::Ruleset;
use lab_engine::state::Status;
use lab_engine::turn::enumerate_turn;
use lab_scenario::scenario_choices;

// ---- O50 residual abilities ----------------------------------------------------------------

#[test]
fn speed_boost_shed_skin_and_hydration_match_showdown() {
    assert_exact_parity("o50-residual-abilities");
}

// ---- O51 weather abilities -------------------------------------------------------------------

#[test]
fn rain_dish_and_dry_skin_in_rain_match_showdown() {
    assert_exact_parity("o51-rain");
}

#[test]
fn solar_power_and_dry_skin_in_sun_match_showdown() {
    assert_exact_parity("o51-sun");
}

#[test]
fn ice_body_in_snow_matches_showdown() {
    assert_exact_parity("o51-snow");
}

// ---- O52 Rock Head / Magic Guard -------------------------------------------------------------

#[test]
fn rock_head_and_magic_guard_match_showdown() {
    assert_exact_parity("o52-rock-head-magic-guard");
}

// ---- O53 Sturdy / Battle Armor / Shell Armor --------------------------------------------------

#[test]
fn sturdy_and_ability_ignoring_moves_match_showdown() {
    assert_exact_parity("o53-sturdy");
}

#[test]
fn crit_immunity_and_ability_ignoring_moves_match_showdown() {
    assert_exact_parity("o53-shell-armor");
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

/// A Water Veil holder that starts the turn burned is cured at the first Update (after the
/// first action, `abilities::on_update`), so no outcome leaves it burned or burn-damaged.
/// (Showdown's patch cannot burn a Water Veil holder, so there is no oracle fixture; the
/// Update timing is checked by `o58-update-cures`.)
#[test]
fn status_that_an_ability_cures_on_update_is_cured() {
    let fixture = fixture("o65-status-block");
    let (loaded, position) = start("o65-status-block", &fixture);
    let mut state = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let two = lab_engine::state::SideId::Two;
    let floatzel = state
        .side(two)
        .party
        .iter()
        .position(|p| p.species.data().name == "Floatzel")
        .unwrap();
    state.side_mut(two).party[floatzel].status = Status::Burn;
    let outcomes = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        assert_eq!(state.side(two).party[floatzel].status, Status::None);
        state.reverse(&outcome.instructions);
    }
}
