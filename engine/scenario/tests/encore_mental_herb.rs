//! Champions Encore against a Mental Herb holder (board V4). The mod's
//! `encore.condition.onStart` replaces the target's queued action unless the target
//! `hasItem('mentalherb')`; the herb then cures the Encore in the next Update, so the holder
//! uses the move it chose, at that move's own priority. With the item ignored (Klutz) the
//! action is replaced like anyone's.

mod common;

use common::assert_exact_parity;

#[test]
fn a_mental_herb_holder_keeps_its_chosen_move() {
    assert_exact_parity("f-encore-mental-herb");
}

#[test]
fn an_ignored_mental_herb_does_not_protect_the_action() {
    assert_exact_parity("f-encore-mental-herb-klutz");
}
