//! Parity of Supreme Overlord (Opus S unit 1) with Showdown: each scenario's exact outcome
//! distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Two allies fainted before Kingambit replaced one of them: 4915/4096 power.
#[test]
fn supreme_overlord_counts_the_fallen_at_its_start() {
    assert_exact_parity("s-supreme-overlord");
}

/// An ally that faints after Kingambit started does not count.
#[test]
fn supreme_overlord_ignores_later_faints() {
    assert_exact_parity("s-supreme-overlord-on-field");
}
