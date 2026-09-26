//! Mega Sol (Opus AA unit 6). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Weather Ball of a Mega Sol user in rain: Fire, 100 power, sun's 1.5x.
#[test]
fn mega_sol_weather_ball_sees_sun_in_rain() {
    assert_exact_parity("aa-mega-sol");
}

/// Solar Beam of a Mega Sol user in rain: no charge, full power.
#[test]
fn mega_sol_solar_beam_needs_no_charge() {
    assert_exact_parity("aa-mega-sol-solar-beam");
}

/// A Mega Sol user's special move gets no sandstorm Special Defense boost on a Rock type.
#[test]
fn mega_sol_ignores_sand_special_defense() {
    assert_exact_parity("aa-mega-sol-sand");
}

/// Synthesis of a Mega Sol user in sand heals 2/3.
#[test]
fn mega_sol_synthesis_heals_as_in_sun() {
    assert_exact_parity("aa-mega-sol-synthesis");
}
