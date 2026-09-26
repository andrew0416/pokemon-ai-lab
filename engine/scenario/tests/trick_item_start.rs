//! Parity of the `Start` of an item given by Trick to a holder that ignores its item (Opus X
//! unit B1): `setItem`'s `singleEvent('Start')` is exempt from the item suppression (Klutz,
//! Magic Room), so the item's `onStart` runs; events it raises in turn (White Herb's `Use`) are
//! suppressed. The Seeds and Room Service check `ignoringItem()` themselves.

mod common;

use common::assert_exact_parity;

/// White Herb onto a stat-lowered Klutz holder: used up (`useItem`), stages unchanged (`onUse`
/// suppressed).
#[test]
fn white_herb_start_under_klutz_matches_showdown() {
    assert_exact_parity("x-trick-white-herb-klutz");
}

/// The same under Magic Room.
#[test]
fn white_herb_start_under_magic_room_matches_showdown() {
    assert_exact_parity("x-trick-white-herb-magic-room");
}

/// Choice items traded under Magic Room: each new holder's Start removes its `choicelock`.
#[test]
fn choice_item_start_under_magic_room_matches_showdown() {
    assert_exact_parity("x-trick-choice-magic-room");
}
