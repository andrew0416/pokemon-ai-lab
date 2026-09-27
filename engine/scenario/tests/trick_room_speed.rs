//! Trick Room's action Speed in Champions (board V6). The Champions mod overrides
//! `getActionSpeed` ("Remove Trick Room underflow"): the Speed is negated under Trick Room,
//! with no `10000 - speed` and no 13-bit truncation, so nothing wraps around at 1809 and a raw
//! stored Speed (`setSpecies`) sorts above every Trick Room Speed.

mod common;

use common::assert_exact_parity;

/// Dragapult at action Speed 2556 under Trick Room still moves last: Snorlax's Taunt lands
/// before its Substitute (the base game's `trunc(10000 - speed, 13)` would wrap it to the top).
#[test]
fn trick_room_speed_does_not_wrap_around() {
    assert_exact_parity("f-trick-room-speed-wrap");
}

/// Aegislash's raw Speed 80 after its forme change against Clefable's and Metagross's negated
/// Speeds: the ModifyDamage chain runs in that order.
#[test]
fn raw_speed_sorts_above_trick_room_speeds() {
    assert_exact_parity("f-trick-room-raw-speed");
}
