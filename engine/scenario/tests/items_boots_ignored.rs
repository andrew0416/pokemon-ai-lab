//! Parity of Heavy-Duty Boots held by a Pokémon that ignores its item (Opus X unit B7): every
//! entry hazard checks `pokemon.hasItem('heavydutyboots')`, which is false while
//! `ignoringItem()` holds (Klutz, Magic Room), so all four hazards act on the holder.

mod common;

use common::assert_exact_parity;

/// Klutz: Stealth Rock, Spikes, Toxic Spikes and Sticky Web all act on the Boots holder.
#[test]
fn klutz_ignores_heavy_duty_boots_matches_showdown() {
    assert_exact_parity("x-boots-klutz");
}

/// Magic Room: the same for a holder without Klutz.
#[test]
fn magic_room_ignores_heavy_duty_boots_matches_showdown() {
    assert_exact_parity("x-boots-magic-room");
}
