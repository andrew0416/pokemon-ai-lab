//! Parity of the phazing window (Opus DD unit B26): Showdown's `battle.activeMove` stays set
//! until `runAction`'s `clearActiveMove()`, which comes after the phazing step (`dragIn` for
//! every `forceSwitchFlag`). A Pokémon dragged in by a Mold Breaker / Teravolt user's Roar or
//! Dragon Tail therefore switches in under `suppressingAbility`: `singleEvent('SwitchIn')` skips
//! breakable abilities (Flower Gift, Pastel Veil's `onAnySwitchIn`), `dragIn`'s own `DragOut`
//! skips Suction Cups, and every other breakable handler of anyone but the user is skipped
//! (Levitate's grounding against Spikes, Hyper Cutter against Intimidate, Pastel Veil against
//! Toxic Spikes). The controls use the same Roar without Mold Breaker.

mod common;

use common::assert_exact_parity;

/// Mold Breaker Roar in sun: the dragged-in Cherrim's Flower Gift SwitchIn is skipped, so
/// Cherrim stays in its base forme.
#[test]
fn mold_breaker_roar_flower_gift_matches_showdown() {
    assert_exact_parity("dd-mold-breaker-roar-flower-gift");
}

/// The same Roar without Mold Breaker: Cherrim blooms.
#[test]
fn roar_flower_gift_matches_showdown() {
    assert_exact_parity("dd-roar-flower-gift");
}

/// Teravolt Dragon Tail (damage rolls, crits, a miss) in sun: every drag leaves Cherrim in its
/// base forme.
#[test]
fn teravolt_dragon_tail_flower_gift_matches_showdown() {
    assert_exact_parity("dd-teravolt-dragon-tail-flower-gift");
}

/// Mold Breaker Roar drags in Intimidate (not breakable: it acts), whose drop on the Roar
/// user's ally skips that ally's breakable Hyper Cutter.
#[test]
fn mold_breaker_roar_intimidate_matches_showdown() {
    assert_exact_parity("dd-mold-breaker-roar-intimidate");
}

/// The same Roar without Mold Breaker: Hyper Cutter blocks Intimidate's drop.
#[test]
fn roar_intimidate_matches_showdown() {
    assert_exact_parity("dd-roar-intimidate");
}

/// Mold Breaker Roar on Suction Cups: both DragOut events skip it, so the holder is dragged
/// out (`roar-drag` has Suction Cups hold against a plain Roar).
#[test]
fn mold_breaker_roar_suction_cups_matches_showdown() {
    assert_exact_parity("dd-mold-breaker-roar-suction-cups");
}

/// Mold Breaker Roar drags a Levitate Pokémon onto Spikes: it is grounded and takes 1/8.
#[test]
fn mold_breaker_roar_levitate_spikes_matches_showdown() {
    assert_exact_parity("dd-mold-breaker-roar-levitate-spikes");
}

/// Mold Breaker Roar drags a Pokémon onto Toxic Spikes next to Pastel Veil: the ally block and
/// Pastel Veil's `onAnySwitchIn` cure are both skipped, so it stays poisoned.
#[test]
fn mold_breaker_roar_pastel_veil_matches_showdown() {
    assert_exact_parity("dd-mold-breaker-roar-pastel-veil");
}

/// The same Roar without Mold Breaker: Pastel Veil blocks the Toxic Spikes poison.
#[test]
fn roar_pastel_veil_matches_showdown() {
    assert_exact_parity("dd-roar-pastel-veil");
}
