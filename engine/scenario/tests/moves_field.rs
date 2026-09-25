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

#[test]
fn o20_yawn_puts_to_sleep_at_the_end_of_the_next_turn() {
    assert_exact_parity("o20-yawn");
}

#[test]
fn o19_perish_song_faints_everyone_and_the_last_faint_wins() {
    assert_exact_parity("o19-perish-song");
}

#[test]
fn o27_endure_leaves_1_hp_before_focus_sash() {
    assert_exact_parity("o27-endure");
}

#[test]
fn o27_endure_shares_the_stall_counter_with_protect() {
    assert_exact_parity("o27-endure-stall");
}

#[test]
fn o21_wide_guard_blocks_spread_moves_on_its_side() {
    assert_exact_parity("o21-wide-guard");
}

#[test]
fn o21_quick_guard_blocks_priority_moves_on_its_side() {
    assert_exact_parity("o21-quick-guard");
}

#[test]
fn o23_safeguard_stops_glare_and_mist_stops_intimidate() {
    assert_exact_parity("o23-safeguard-mist");
}

#[test]
fn o23_lucky_chant_stops_critical_hits() {
    assert_exact_parity("o23-lucky-chant");
}
