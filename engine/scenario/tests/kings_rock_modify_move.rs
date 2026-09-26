//! Parity of King's Rock / Razor Fang's appended flinch with the ModifyMove order (Opus BB unit
//! B21): Sheer Force (priority 0) deletes the move's secondaries, then King's Rock (-1) appends
//! its 10% flinch when none is left, then Serene Grace (-2) doubles every secondary chance,
//! the appended flinch's included.

mod common;

use common::assert_exact_parity;

/// Serene Grace + King's Rock: 20% flinch (the engine gave 10%); Razor Fang without Serene
/// Grace stays 10%.
#[test]
fn kings_rock_serene_grace_matches_showdown() {
    assert_exact_parity("bb-kings-rock-serene-grace");
}

/// Sheer Force + King's Rock on Headbutt: the move's own 30% flinch is gone, King's Rock's 10%
/// is added (the engine gave none).
#[test]
fn kings_rock_sheer_force_matches_showdown() {
    assert_exact_parity("bb-kings-rock-sheer-force");
}
