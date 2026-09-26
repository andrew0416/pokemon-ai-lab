//! Parity of a recharge turn's action order (board B22): the `recharge` pseudo-move is a move
//! action, so `FractionalPriority` applies to it (Stall's -0.1, Mycelium Might's -0.1 for a status
//! move); an engine that queues the recharge at priority 0 lets a Stall holder act before a
//! slower Payback user and doubles Payback's power.

mod common;

use common::assert_exact_parity;

/// Sableye (Stall) recharging after Hyper Beam acts after the slower Snorlax's Payback, which
/// therefore keeps its 50 base power.
#[test]
fn a_stall_holders_recharge_keeps_its_fractional_priority() {
    assert_exact_parity("f-recharge-stall");
}
