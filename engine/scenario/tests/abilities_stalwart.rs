//! Parity of Stalwart and Propeller Tail (Opus S unit 6: `move.tracksTarget`) with Showdown:
//! each scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Follow Me and Storm Drain do not draw a Stalwart or Propeller Tail holder's move.
#[test]
fn stalwart_and_propeller_tail_ignore_redirection() {
    assert_exact_parity("s-stalwart-propeller-tail");
}

/// After Ally Switch, Showdown keeps aiming at the original target (`getTarget`'s
/// `originalTarget`): Duraludon's Dragon Pulse follows Mr. Mime, a Fairy (Opus OO R7; was
/// unsupported).
#[test]
fn stalwart_keeps_its_target_after_ally_switch() {
    assert_exact_parity("s-stalwart-ally-switch");
}
