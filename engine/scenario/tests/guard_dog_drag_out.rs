//! Parity of Guard Dog's `onDragOut` (board B28): like Suction Cups it returns `null`, so Roar,
//! Whirlwind, Dragon Tail, Circle Throw and Red Card neither drag the holder out nor fail; the
//! ability is breakable, so a Mold Breaker phazer still drags it.

mod common;

use common::assert_exact_parity;

#[test]
fn roar_does_not_drag_a_guard_dog_out() {
    assert_exact_parity("f-guard-dog-roar");
}

#[test]
fn roar_drags_the_control_out() {
    assert_exact_parity("f-guard-dog-roar-control");
}

#[test]
fn dragon_tail_damages_but_does_not_drag_a_guard_dog_out() {
    assert_exact_parity("f-guard-dog-dragon-tail");
}

#[test]
fn red_card_does_not_drag_a_guard_dog_attacker_out() {
    assert_exact_parity("f-guard-dog-red-card");
}

#[test]
fn a_mold_breaker_roar_drags_a_guard_dog_out() {
    assert_exact_parity("f-mold-breaker-roar-guard-dog");
}
