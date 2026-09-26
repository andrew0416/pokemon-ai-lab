//! Corrosion, Early Bird, Steadfast, Anger Point, Stench and Long Reach (Opus AA unit 1).
//! Fixtures from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// A Corrosion source's poison skips the status immunity: Toxic on a Steel type, and Toxic Orb
/// on its own Rock/Poison holder (the orb's source is the holder).
#[test]
fn corrosion_poisons_steel_and_poison_types() {
    assert_exact_parity("aa-corrosion");
}

/// Early Bird takes two off the sleep counter at each BeforeMove: a 2-turn sleep ends at the
/// first move attempt.
#[test]
fn early_bird_wakes_twice_as_fast() {
    assert_exact_parity("aa-early-bird");
}

/// Steadfast raises Speed when a flinch stops its holder's move.
#[test]
fn steadfast_raises_speed_on_flinch() {
    assert_exact_parity("aa-steadfast");
}

/// Anger Point maxes Attack on a critical hit (always with Storm Throw, 1/24 with Tackle).
#[test]
fn anger_point_maxes_attack_on_a_critical_hit() {
    assert_exact_parity("aa-anger-point");
}

/// Stench and King's Rock together add one 10% flinch.
#[test]
fn stench_and_kings_rock_add_one_flinch() {
    assert_exact_parity("aa-stench");
}

/// Long Reach removes contact: no Iron Barbs, Sticky Barb or Rocky Helmet.
#[test]
fn long_reach_makes_no_contact() {
    assert_exact_parity("aa-long-reach");
}
