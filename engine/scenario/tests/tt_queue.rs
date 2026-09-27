//! Opus TT (wave 15, lane L2): the order of hazards and queued actions (Showdown `effectOrder`
//! and `battle-queue.ts`) and two small item/ability windows, each against the oracle's exact
//! distribution.

mod common;

use common::assert_exact_parity;

/// R2a: Toxic Spikes set before Stealth Rock. The newcomer (Arbok, Poison, 5 HP) meets them in
/// that order (`effectOrder`): it absorbs the spikes, then the rocks knock it out. The other
/// order is `rr-hazard-order` (`refusals_rr.rs`).
#[test]
fn hazards_run_in_the_order_they_were_set() {
    assert_exact_parity("tt-hazard-order-toxic-first");
}

/// R2a: Court Change moves the effect states whole, `effectOrder` included. Stealth Rock then
/// Toxic Spikes on p1's side move to p2's; Arbok (5 HP) meets the rocks first and faints, the
/// spikes stay (the swap's own order, Toxic Spikes before Stealth Rock, would let it absorb
/// them).
#[test]
fn court_change_keeps_the_hazard_order() {
    assert_exact_parity("tt-hazard-order-court-change");
}
