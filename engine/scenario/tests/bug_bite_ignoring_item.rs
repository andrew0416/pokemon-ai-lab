//! Parity of Bug Bite / Pluck with item suppression (Opus X unit B8): the move reads the
//! target's raw item (`target.getItem()`) and takes it, then runs the berry's `onEat` on the
//! user through `singleEvent('Eat', item, ..., source)`, which Showdown skips while the user
//! ignores its item (Klutz, even with no item held; Magic Room). `ateBerry` is set either way.

mod common;

use common::assert_exact_parity;

/// A Klutz user with no item takes the berry but does not eat it.
#[test]
fn bug_bite_by_a_klutz_user_does_not_eat_matches_showdown() {
    assert_exact_parity("x-bug-bite-klutz");
}

/// Under Magic Room the berry is taken (raw item) but not eaten.
#[test]
fn bug_bite_under_magic_room_does_not_eat_matches_showdown() {
    assert_exact_parity("x-bug-bite-magic-room");
}

/// The skipped `Eat` still returns `true`: EatItem runs, so Cheek Pouch heals under Magic Room.
#[test]
fn bug_bite_under_magic_room_still_runs_eat_item_matches_showdown() {
    assert_exact_parity("x-bug-bite-cheek-pouch-magic-room");
}

/// A Klutz target's berry is still taken (raw item) and the user eats it.
#[test]
fn bug_bite_takes_a_klutz_targets_berry_matches_showdown() {
    assert_exact_parity("x-bug-bite-klutz-target");
}
