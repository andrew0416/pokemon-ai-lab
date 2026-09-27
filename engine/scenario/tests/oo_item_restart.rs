//! Opus OO R11-item-restart: Recycle is `pokemon.lastItem = ''; pokemon.setItem(item, source,
//! move)`, and `setItem` runs the item's `Start` on its holder (`singleEvent('Start')`, exempt from
//! the item suppression). `rr-recycle-seed` (`refusals_rr.rs`) is the Grassy Seed used again in
//! its terrain.

mod common;

use common::assert_exact_parity;

/// Clefable's Charm lowers Snorlax's Attack after its White Herb was used in the setup turn;
/// Recycle gives the herb back, its `onStart` resets the stage and it is used up again.
#[test]
fn recycled_white_herb_resets_a_lowered_stage() {
    assert_exact_parity("oo-recycle-white-herb");
}

/// A Metronome flung into Protect comes back through Recycle: the leftover condition went at
/// Recycle's TryMove (no item), and the item's `onStart` adds a fresh one.
#[test]
fn recycled_metronome_starts_its_condition() {
    assert_exact_parity("oo-recycle-metronome");
}
