//! Gulp Missile and Bad Dreams (Opus U unit 4). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Surf at half HP makes Cramorant-Gorging, whose spit paralyzes the next attacker.
#[test]
fn gulp_missile_surf_gorging_paralyzes() {
    assert_exact_parity("u-gulp-missile-surf");
}

/// Dive's charging turn makes Cramorant-Gulping, and nothing hits it underground.
#[test]
fn gulp_missile_dive_catches_on_the_charging_turn() {
    assert_exact_parity("u-gulp-missile-dive");
}

/// Cramorant-Gulping spits at its next attacker: 1/4 of its max HP and Defense -1.
#[test]
fn gulp_missile_gulping_spits_after_dive() {
    assert_exact_parity("u-gulp-missile-dive-spit");
}

/// Bad Dreams hurts sleeping foes and Comatose foes, not a sleeping ally.
#[test]
fn bad_dreams_hurts_sleeping_foes() {
    assert_exact_parity("u-bad-dreams");
}
