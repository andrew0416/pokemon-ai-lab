//! Parity of move-specific callbacks (`core/src/turn/moves/handlers.rs`, WORKPLAN §2.1) with
//! Showdown: each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_scenario::scenario_choices;

#[test]
fn o2_grav_apple_under_gravity() {
    assert_exact_parity("o2-grav-apple");
}

#[test]
fn o3_rising_voltage_and_psyblade_in_electric_terrain() {
    assert_exact_parity("o3-rising-voltage");
}

#[test]
fn o3_rising_voltage_by_and_into_ungrounded() {
    assert_exact_parity("o3-rising-voltage-ungrounded");
}

#[test]
fn o5_thunder_never_misses_in_rain() {
    assert_exact_parity("o5-thunder-rain");
}

#[test]
fn o5_thunder_accuracy_50_in_sun_then_gravity() {
    assert_exact_parity("o5-thunder-sun-gravity");
}

#[test]
fn o5_blizzard_never_misses_in_snow() {
    assert_exact_parity("o5-blizzard-snow");
}

#[test]
fn o34_freeze_dry_and_flying_press_effectiveness() {
    assert_exact_parity("o34-freeze-dry-flying-press");
}

#[test]
fn o34_freeze_dry_4x_and_flying_press_still_immune() {
    assert_exact_parity("o34-freeze-dry-flying-press-immune");
}

#[test]
fn o17_poltergeist_hits_and_knock_off_keeps_a_mega_stone() {
    assert_exact_parity("o17-poltergeist-knock-off");
}

#[test]
fn o17_poltergeist_fails_and_knock_off_doubles_acrobatics() {
    assert_exact_parity("o17-acrobatics-knock-off");
}

#[test]
fn o17_acrobatics_with_item_then_knock_off() {
    assert_exact_parity("o17-acrobatics-item");
}

#[test]
fn o11_first_impression_on_the_first_turn_out() {
    assert_exact_parity("o11-first-impression");
}

#[test]
fn o11_first_impression_blocked_by_psychic_terrain_on_grounded_targets() {
    assert_exact_parity("o11-first-impression-psychic-terrain");
}

/// Champions `onDisableMove`: once the user has acted since switching in, First Impression
/// cannot be chosen.
#[test]
fn o11_first_impression_is_disabled_after_the_first_action() {
    let name = "o11-first-impression";
    let fixture = fixture(name);
    let (loaded, mut state) = start(name, &fixture);
    let choices = scenario_choices(&loaded, &state).unwrap();
    let user = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    state.slot_mut(user).move_actions = 1;
    let error = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap_err();
    assert!(
        matches!(
            error,
            TurnError::InvalidChoice {
                side: SideId::One,
                slot: 0,
                ..
            }
        ),
        "{error}"
    );
}
