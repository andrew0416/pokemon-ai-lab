//! Parity of Illusion (Opus EE unit EE2). In a complete-information engine the disguise changes
//! no damage or turn order; its state (`Pokemon::illusion`, Showdown `pokemon.illusion` as a
//! flag) matters only to `transformInto`, which fails while either Pokémon is under Illusion.
//! The canonical state never shows it, so each fixture reads it through Transform or Imposter.

mod common;

use common::assert_exact_parity;

/// Zoroark leading with a live Pokémon after it in party order: Imposter opposite it fails, and
/// so does a Transform at it.
#[test]
fn illusion_with_a_pokemon_behind_stops_imposter_and_transform() {
    assert_exact_parity("ee-illusion-imposter");
}

/// Zoroark last in party order has no disguise: Imposter opposite it transforms.
#[test]
fn illusion_with_nobody_behind_lets_imposter_transform() {
    assert_exact_parity("ee-illusion-last");
}

/// A damaging hit ends Illusion (`onDamagingHit` → `End`); a Transform after it works.
#[test]
fn a_damaging_hit_ends_illusion() {
    assert_exact_parity("ee-illusion-broken");
}

/// Switching out keeps the (ended) state; switching back in sets the disguise again.
#[test]
fn illusion_returns_when_zoroark_switches_back_in() {
    assert_exact_parity("ee-illusion-switch-back");
}

/// Neutralizing Gas coming in ends Illusion; a Transform after it works.
#[test]
fn neutralizing_gas_ends_illusion() {
    assert_exact_parity("ee-illusion-neutralizing-gas");
}

/// The illusion outlives a switch-out (`beingCalledBack`), and a Zoroark switching back in
/// under Neutralizing Gas skips `onBeforeSwitchIn`: the old illusion still stops Transform.
#[test]
fn a_stale_illusion_survives_a_suppressed_switch_in() {
    assert_exact_parity("ee-illusion-gas-stale");
}
