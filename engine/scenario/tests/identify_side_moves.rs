//! Identifying moves and side / field boosts (Opus V unit 9b): Foresight / Odor Sleuth /
//! Miracle Eye (type immunity negated, positive evasion ignored), Gear Up / Magnetic Flux (Plus
//! and Minus holders), Flower Shield / Rototiller (Grass types). Fixtures from Showdown's exact
//! enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Foresight lets Normal moves hit a Ghost and ignores its evasion.
#[test]
fn foresight_negates_ghost_immunity_and_evasion() {
    assert_exact_parity("foresight");
}

/// Miracle Eye lets Psychic moves hit a Dark type; Odor Sleuth then fails on it.
#[test]
fn miracle_eye_negates_dark_immunity() {
    assert_exact_parity("miracle-eye");
}

/// Gear Up and Magnetic Flux boost the Plus / Minus holders of the user's side.
#[test]
fn gear_up_and_magnetic_flux_boost_plus_minus() {
    assert_exact_parity("gear-up");
}

/// Flower Shield boosts every Grass type; Rototiller only the grounded ones.
#[test]
fn flower_shield_and_rototiller_boost_grass_types() {
    assert_exact_parity("flower-shield-rototiller");
}
