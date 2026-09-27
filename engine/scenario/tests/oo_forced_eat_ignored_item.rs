//! Opus OO R22-forced-eat-ignored-item: Showdown `eatItem(true)` (Teatime, Stuff Cheeks) for a
//! holder that ignores its item (Klutz, Magic Room): `singleEvent('Eat')` is skipped
//! (`singleEvent` suppresses an item's handlers but Start, TakeItem and SetAbility), the berry is
//! still consumed (`lastItem`, AfterUseItem). `rr-teatime-klutz` (`refusals_rr.rs`) is the Klutz
//! case.

mod common;

use common::assert_exact_parity;

/// Magic Room (setup turn): Teatime makes Snorlax eat its Sitrus Berry at 100 HP without the heal.
#[test]
fn teatime_under_magic_room_consumes_the_berry_without_its_effect() {
    assert_exact_parity("oo-teatime-magic-room");
}
