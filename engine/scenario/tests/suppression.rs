//! Item suppression (WORKPLAN F17): Magic Room and Klutz make a holder ignore its item
//! (Showdown `ignoringItem`) without touching the item itself. Fixtures from Showdown's exact
//! enumeration.

mod common;

use common::assert_exact_parity;

/// Life Orb, Rocky Helmet and Leftovers do nothing under Magic Room.
#[test]
fn magic_room_matches_showdown_exactly() {
    assert_exact_parity("magic-room");
}

/// A second Magic Room ends it; a berry held back by it is eaten at the next Update.
#[test]
fn magic_room_ending_matches_showdown_exactly() {
    assert_exact_parity("magic-room-end");
}

/// Mega Evolution reads the Mega Stone itself, which Magic Room does not suppress.
#[test]
fn magic_room_does_not_stop_mega_evolution() {
    assert_exact_parity("magic-room-mega");
}

/// Klutz: no Life Orb recoil, no berry; the foe's Rocky Helmet still applies.
#[test]
fn klutz_matches_showdown_exactly() {
    assert_exact_parity("klutz");
}
