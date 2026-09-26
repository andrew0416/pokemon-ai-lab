//! Parity of the items of Opus Q unit 6 (Clear Amulet, Ability Shield, Big Root, Blunder
//! Policy, Muscle Band, Wise Glasses, Punching Glove, Light Ball, Thick Club, Mental Herb) with
//! Showdown: each scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Clear Amulet against Intimidate (at the start) and Growl; Ability Shield against Mummy.
#[test]
fn clear_amulet_and_ability_shield_match_showdown() {
    assert_exact_parity("q-clear-amulet-ability-shield");
}

/// Big Root on a drain and on Leech Seed; Blunder Policy after a miss.
#[test]
fn big_root_and_blunder_policy_match_showdown() {
    assert_exact_parity("q-big-root-blunder-policy");
}

/// Light Ball; Punching Glove's power and its lost contact (no Rocky Helmet).
#[test]
fn light_ball_and_punching_glove_match_showdown() {
    assert_exact_parity("q-light-ball-punching-glove");
}

#[test]
fn muscle_band_wise_glasses_and_thick_club_match_showdown() {
    assert_exact_parity("q-muscle-band-thick-club");
}

/// Mental Herb cures a Taunt at the next Update.
#[test]
fn mental_herb_matches_showdown() {
    assert_exact_parity("q-mental-herb");
}
