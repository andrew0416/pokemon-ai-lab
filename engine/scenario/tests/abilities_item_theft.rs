//! Parity of Pickpocket and Magician (Opus S unit 9b: item theft) with Showdown: each
//! scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Contact steals the attacker's item; the attacker's Unburden starts.
#[test]
fn pickpocket_matches_showdown() {
    assert_exact_parity("s-pickpocket");
}

/// The user steals from the fastest of its hit targets.
#[test]
fn magician_matches_showdown() {
    assert_exact_parity("s-magician");
}
