//! Parity of Berry Juice (Opus W unit 3) with Showdown.

mod common;

use common::assert_exact_parity;

/// Used on the Update after the hit that takes its holder to half HP: +20.
#[test]
fn berry_juice_heals_20_at_half() {
    assert_exact_parity("w-berry-juice");
}

/// Heal Block's TryHeal stops it: the juice stays.
#[test]
fn berry_juice_waits_under_heal_block() {
    assert_exact_parity("w-berry-juice-heal-block");
}
