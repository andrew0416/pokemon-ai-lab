//! Parity of move-blocking, weather-suppressing, secondary-effect, status-passing and
//! ability-ignoring abilities (work plan units O48, O49, O60, O66, O67) with Showdown's exact
//! outcome distribution (`engine/oracle/expected/*.turn.json`).

mod common;

use common::assert_exact_parity;

// ---- O48 absorbing and immunity abilities ------------------------------------------------------

#[test]
fn healing_absorbers_match_showdown() {
    assert_exact_parity("o48-absorb-heal");
}

#[test]
fn boosting_absorbers_and_flash_fire_absorb_match_showdown() {
    assert_exact_parity("o48-boost-absorb");
}

#[test]
fn flash_fire_boost_matches_showdown() {
    assert_exact_parity("o48-flash-fire-boost");
}

/// Flash Fire's `move.accuracy = true` makes the rest of a spread move never miss.
#[test]
fn flash_fire_spread_accuracy_matches_showdown() {
    assert_exact_parity("o48-flash-fire-spread");
}

#[test]
fn bulletproof_soundproof_overcoat_telepathy_match_showdown() {
    assert_exact_parity("o48-immunity");
}

#[test]
fn wonder_guard_and_good_as_gold_match_showdown() {
    assert_exact_parity("o48-wonder-guard-gold");
}

#[test]
fn dazzling_and_armor_tail_match_showdown() {
    assert_exact_parity("o48-dazzling");
}

// ---- O49 Air Lock / Cloud Nine -----------------------------------------------------------------

/// Cloud Nine and Drought both lead (start expansion); the sun counts down but does nothing.
#[test]
fn cloud_nine_suppresses_sun_match_showdown() {
    assert_exact_parity("o49-cloud-nine-sun");
}

/// The suppression ends as soon as the Air Lock holder faints, within the turn.
#[test]
fn air_lock_holder_fainting_restores_sand_match_showdown() {
    assert_exact_parity("o49-air-lock-faint");
}
