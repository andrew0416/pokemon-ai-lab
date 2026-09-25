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

#[test]
fn o6_ice_spinner_clears_grassy_terrain_after_the_hit() {
    assert_exact_parity("o6-ice-spinner");
}

#[test]
fn o6_steel_roller_clears_the_terrain_then_fails_without_one() {
    assert_exact_parity("o6-steel-roller");
}

#[test]
fn o8_coaching_life_dew_decorate_and_pollen_puff_on_allies() {
    assert_exact_parity("o8-ally-support");
}

#[test]
fn o8_pollen_puff_damages_foes_and_milk_drink_heals_an_ally() {
    assert_exact_parity("o8-pollen-puff");
}

#[test]
fn o9_foul_play_uses_the_target_attack_and_body_press_defense() {
    assert_exact_parity("o9-foul-play-body-press");
}

#[test]
fn o16_trick_and_switcheroo_swap_items() {
    assert_exact_parity("o16-trick");
}

#[test]
fn o16_trick_fails_on_own_or_receiving_mega_stones() {
    assert_exact_parity("o16-trick-fail");
}

#[test]
fn o18_roost_removes_flying_until_the_end_of_the_turn() {
    assert_exact_parity("o18-roost");
}

#[test]
fn o69_protean_and_libero_change_type_before_the_hit() {
    assert_exact_parity("o69-protean");
}

#[test]
fn o69_protean_and_libero_act_once_per_switch_in() {
    assert_exact_parity("o69-protean-once");
}
