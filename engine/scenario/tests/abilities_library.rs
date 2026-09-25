//! Parity of the abilities the library teams use (O70 type changers, the auras, Flower Veil, No
//! Guard, Cursed Body, Protosynthesis / Quark Drive, Magic Bounce, the trapping abilities, Trace
//! on a mid-battle switch-in) with Showdown's outcome distribution
//! (`engine/oracle/expected/*.turn.json`).

mod common;

use common::assert_exact_parity;

// ---- O70 onModifyType / onBasePower ------------------------------------------------------------

/// Pixilate and Aerilate turn Strength Fairy / Flying (a Ghost is no longer immune), 4915/4096.
#[test]
fn pixilate_and_aerilate_match_showdown() {
    assert_exact_parity("o70-pixilate-aerilate");
}

/// Galvanize's Electric Strength is absorbed by Volt Absorb; Refrigerate's Ice Strength.
#[test]
fn galvanize_and_refrigerate_match_showdown() {
    assert_exact_parity("o70-galvanize-refrigerate");
}

/// Normalize makes Dragon Pulse Normal (and boosts it); Liquid Voice's Water Sing is absorbed.
#[test]
fn normalize_and_liquid_voice_match_showdown() {
    assert_exact_parity("o70-normalize-liquid-voice");
}

// ---- Fairy Aura / Dark Aura / Aura Break -------------------------------------------------------

/// Two Fairy Aura holders boost a Fairy move once (`move.auraBooster`); Dark Aura its own move.
#[test]
fn fairy_and_dark_aura_match_showdown() {
    assert_exact_parity("aura-fairy-dark");
}

/// Aura Break reverses the aura (3072/4096) unless a Mold Breaker move skips it.
#[test]
fn aura_break_matches_showdown() {
    assert_exact_parity("aura-break");
}
