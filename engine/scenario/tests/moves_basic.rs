//! Parity of move-specific callbacks (`core/src/turn/moves/handlers.rs`, WORKPLAN §2.1) with
//! Showdown: each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

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
