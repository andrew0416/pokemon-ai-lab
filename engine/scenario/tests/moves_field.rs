//! Parity of move callbacks and targeting (WORKPLAN §2.1: O1, O4, O6, O8, O9, O16, O18–O21,
//! O23, O27, O69) with Showdown: each scenario's exact outcome distribution must equal its
//! oracle fixture (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

#[test]
fn o1_expanding_force_spreads_in_psychic_terrain() {
    assert_exact_parity("o1-expanding-force");
}

#[test]
fn o1_expanding_force_ungrounded_and_retargeted_from_an_ally() {
    assert_exact_parity("o1-expanding-force-ungrounded");
}

#[test]
fn o4_weather_ball_is_fire_in_sun() {
    assert_exact_parity("o4-weather-ball-sun");
}

#[test]
fn o4_terrain_pulse_electric_and_weather_ball_rock_in_sand() {
    assert_exact_parity("o4-terrain-pulse-sand");
}

#[test]
fn o4_terrain_pulse_ungrounded_and_weather_ball_water_in_rain() {
    assert_exact_parity("o4-rain-ungrounded");
}
