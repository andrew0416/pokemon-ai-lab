//! Parity of Cud Chew's "queue empty" check in a switch batch requested after the residual (Opus
//! X unit B10): the replacement's `runSwitch` takes the last queued action, so a berry eaten
//! there sees `!this.queue.peek()` and Cud Chew's counter starts at 1: it eats the berry again
//! at the next residual.

mod common;

use common::assert_exact_parity;

/// Eject Pack (from Octolock's residual drop) switches Snorlax out after the residual; Farigiraf
/// eats its Lum Berry against Toxic Spikes in its `runSwitch`, and Cud Chew eats it again at the
/// next turn's residual, curing Toxic.
#[test]
fn cud_chew_after_a_residual_switch_matches_showdown() {
    assert_exact_parity("x-cud-chew-after-residual");
}
