//! Parity of Wandering Spirit (Opus S unit 9c: `skillSwap` on contact) with Showdown: the
//! scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// The abilities trade places and the new one starts (Intimidate); Ability Shield blocks it.
#[test]
fn wandering_spirit_matches_showdown() {
    assert_exact_parity("s-wandering-spirit");
}
