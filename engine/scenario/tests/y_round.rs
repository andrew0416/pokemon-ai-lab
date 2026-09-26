//! Round (Opus Y unit 6; Fury Cutter, Echoed Voice and the three Pledges are Past in Champions).
//! Fixtures from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Round moves the next queued Round up (order 3), which then has double power.
#[test]
fn round_prioritizes_the_allys_round() {
    assert_exact_parity("y-round");
}

/// Round moves any Pokémon's queued Round up, a foe's included.
#[test]
fn round_prioritizes_a_foes_round() {
    assert_exact_parity("y-round-foe");
}
