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

// ---- Flower Veil -------------------------------------------------------------------------------

/// A Grass ally is spared a foe's status and stat drops; the non-Grass holder is not.
#[test]
fn flower_veil_status_and_drops_match_showdown() {
    assert_exact_parity("flower-veil-status-boost");
}

/// Yawn is blocked, a self-inflicted drop is not, a Mold Breaker move ignores Flower Veil.
#[test]
fn flower_veil_yawn_self_drop_mold_breaker_match_showdown() {
    assert_exact_parity("flower-veil-yawn-mold-breaker");
}

/// Flower Veil and the target's Mirror Armor run in their holders' Speed order.
#[test]
fn flower_veil_against_mirror_armor_matches_showdown() {
    assert_exact_parity("flower-veil-mirror-armor");
    assert_exact_parity("flower-veil-mirror-armor-faster");
}

// ---- No Guard and the Accuracy event -----------------------------------------------------------

/// Moves by and against a No Guard holder never miss.
#[test]
fn no_guard_matches_showdown() {
    assert_exact_parity("no-guard");
}

/// Micle Berry's Accuracy handler ends its volatile whatever the accuracy: for a move that
/// never misses, and after a Glaive Rush drawback (a faster holder) already answered `true`.
#[test]
fn micle_berry_ends_on_every_accuracy_event() {
    assert_exact_parity("micle-accuracy-true");
    assert_exact_parity("micle-glaive-rush");
}
