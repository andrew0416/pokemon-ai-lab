//! Opus SS (wave 15 L1, board R10a): once-per-battle ability flags in the state
//! (`SideHistory::{sword_boost, shield_boost, syrup_triggered}`, Showdown `pokemon.swordBoost` /
//! `.shieldBoost` / `.syrupTriggered`).

mod common;

use common::assert_exact_parity;

/// Hydrapple's Supersweet Syrup acted at the battle start; switched out and back in, its
/// `onStart` returns at once: the foes' evasion stays at -1.
#[test]
fn supersweet_syrup_acts_once() {
    assert_exact_parity("ss-supersweet-syrup-once");
}
