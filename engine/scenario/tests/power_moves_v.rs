//! Power, damage and immunity callbacks (Opus V unit 9a): Present, Secret Power, Nature Power,
//! Psywave, Barb Barrage, Fickle Beam, Synchronoise, Captivate. Fixtures from Showdown's exact
//! enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Present draws its power, or heals its target a quarter.
#[test]
fn present_draws_power_or_heals() {
    assert_exact_parity("present");
}

/// Secret Power's secondary follows the terrain.
#[test]
fn secret_power_follows_the_terrain() {
    assert_exact_parity("secret-power");
}

/// Nature Power calls the terrain's move at its target.
#[test]
fn nature_power_calls_the_terrain_move() {
    assert_exact_parity("nature-power");
}

/// Psywave deals 50–150% of the user's level.
#[test]
fn psywave_damage_draw() {
    assert_exact_parity("psywave");
}

/// Barb Barrage doubles against a poisoned target.
#[test]
fn barb_barrage_doubles_on_poison() {
    assert_exact_parity("barb-barrage");
}

/// Fickle Beam doubles its power 30% of the time.
#[test]
fn fickle_beam_sometimes_doubles() {
    assert_exact_parity("fickle-beam");
}

/// Synchronoise only hits Pokémon sharing a type with the user.
#[test]
fn synchronoise_needs_a_shared_type() {
    assert_exact_parity("synchronoise");
}

/// Captivate only affects the opposite gender.
#[test]
fn captivate_needs_the_opposite_gender() {
    assert_exact_parity("captivate");
}
