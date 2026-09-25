//! Two-turn moves (WORKPLAN F9, rest): the charge turn (`twoturnmove` + the move's own
//! volatile, semi-invulnerability), the locked second turn aimed at the chosen target, the
//! charge skips (sun, rain, Power Herb), the charge-turn boosts, and an aborted second turn.
//! Fixtures from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

#[test]
fn solar_beam_charge_matches_showdown_exactly() {
    assert_exact_parity("solar-beam-charge");
}

#[test]
fn solar_beam_second_turn_matches_showdown_exactly() {
    assert_exact_parity("solar-beam-hit");
}

#[test]
fn solar_beam_in_sun_matches_showdown_exactly() {
    assert_exact_parity("solar-beam-sun");
}

#[test]
fn power_herb_matches_showdown_exactly() {
    assert_exact_parity("power-herb");
}

/// Semi-invulnerable: Seismic Toss misses, Gust reaches for double damage.
#[test]
fn fly_charge_matches_showdown_exactly() {
    assert_exact_parity("fly-charge");
}

#[test]
fn fly_second_turn_matches_showdown_exactly() {
    assert_exact_parity("fly-hit");
}

/// Earthquake reaches the underground Pokémon for double damage.
#[test]
fn dig_against_earthquake_matches_showdown_exactly() {
    assert_exact_parity("dig-earthquake");
}

/// Meteor Beam raises SpA while charging; Electro Shot in rain raises it and fires at once.
#[test]
fn meteor_beam_and_electro_shot_match_showdown_exactly() {
    assert_exact_parity("meteor-beam");
}

/// Full paralysis on the second turn ends the lock and the move's volatile.
#[test]
fn aborted_second_turn_matches_showdown_exactly() {
    assert_exact_parity("charge-abort");
}
