//! Steel Beam and the move Attract (Opus Y unit 7; Mind Blown is Past in Champions). Fixtures from
//! Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Steel Beam costs half the user's max HP (rounded) after a hit, and on a miss (onMoveFail).
#[test]
fn steel_beam_costs_half_the_hp() {
    assert_exact_parity("y-steel-beam");
}

/// Steel Beam into Protect: onMoveFail's cost, which Magic Guard stops.
#[test]
fn steel_beam_into_protect_and_magic_guard() {
    assert_exact_parity("y-steel-beam-protect");
}

/// Steel Beam into a substitute: the substitute's hit applies the recoil once.
#[test]
fn steel_beam_into_a_substitute() {
    assert_exact_parity("y-steel-beam-substitute");
}

/// Attract between opposite genders; the attracted Pokémon fails to move half the time.
#[test]
fn attract_infatuates() {
    assert_exact_parity("y-attract");
}

/// Attract fails between equal genders and on a genderless target.
#[test]
fn attract_needs_opposite_genders() {
    assert_exact_parity("y-attract-immune");
}

/// Destiny Knot on the target attracts the user back.
#[test]
fn attract_destiny_knot_attracts_back() {
    assert_exact_parity("y-attract-destiny-knot");
}
