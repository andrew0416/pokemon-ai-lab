//! The other two-turn moves on the F9 machinery (Opus V unit 7): Skull Bash (Defense +1 on the
//! charge, before Power Herb), Razor Wind (spread, crit ratio 2), Freeze Shock, Ice Burn
//! (secondaries) and Geomancy (status; boosts on the second turn). Fixtures from Showdown's exact
//! enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Skull Bash raises Defense and charges; Razor Wind charges.
#[test]
fn skull_bash_and_razor_wind_charge() {
    assert_exact_parity("skull-bash-charge");
}

/// The locked Skull Bash hits on the second turn.
#[test]
fn skull_bash_hits_on_the_second_turn() {
    assert_exact_parity("skull-bash-hit");
}

/// The locked Razor Wind hits both foes on the second turn.
#[test]
fn razor_wind_hits_both_foes() {
    assert_exact_parity("razor-wind-hit");
}

/// Power Herb skips Skull Bash's charge after its Defense boost.
#[test]
fn skull_bash_with_power_herb() {
    assert_exact_parity("skull-bash-power-herb");
}

/// Geomancy boosts on its second turn, or at once with Power Herb.
#[test]
fn geomancy_boosts_after_charging() {
    assert_exact_parity("geomancy");
}

/// Freeze Shock hits on its second turn with its paralysis chance.
#[test]
fn freeze_shock_hits_on_the_second_turn() {
    assert_exact_parity("freeze-shock-hit");
}

/// Ice Burn with Power Herb hits at once with its burn chance.
#[test]
fn ice_burn_with_power_herb() {
    assert_exact_parity("ice-burn-power-herb");
}
