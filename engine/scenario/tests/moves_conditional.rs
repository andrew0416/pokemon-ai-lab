//! Moves that only work after something else happened (Opus T): Belch (`ateBerry`, kept in
//! `SideHistory::ate_berry`), Last Resort (`moveSlot.used`, `SlotHistory::moves_used`) and
//! Dark Void (Darkrai only). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Belch works after its user ate its Sitrus Berry this turn, and fails for one that never ate
/// a berry.
#[test]
fn belch_needs_an_eaten_berry() {
    assert_exact_parity("belch");
}

/// Last Resort works once every other known move was used since switching in.
#[test]
fn last_resort_needs_every_other_move_used() {
    assert_exact_parity("last-resort");
}

/// Dark Void works for Darkrai and fails for anyone else.
#[test]
fn dark_void_is_darkrai_only() {
    assert_exact_parity("dark-void");
}
