//! Parity of Unseen Fist and Piercing Drill (Opus S unit 2: Champions `onHitProtect`) with
//! Showdown: each scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// A contact move goes through Spiky Shield at a quarter (no contact punishment); a non-contact
/// move is stopped by Protect.
#[test]
fn unseen_fist_through_protect_matches_showdown() {
    assert_exact_parity("s-unseen-fist-protect");
}

/// Every hit of a multi-hit contact move through Protect is quartered.
#[test]
fn unseen_fist_multi_hit_through_protect_matches_showdown() {
    assert_exact_parity("s-unseen-fist-multihit");
}

/// Wide Guard (Unseen Fist) and Quick Guard (Piercing Drill) let contact moves through too.
#[test]
fn unseen_fist_and_piercing_drill_through_the_guards_match_showdown() {
    assert_exact_parity("s-unseen-fist-guards");
}

/// Mat Block is bypassed by contact; Punching Glove removes contact, so its punch is stopped.
#[test]
fn unseen_fist_mat_block_and_punching_glove_match_showdown() {
    assert_exact_parity("s-unseen-fist-glove-mat-block");
}
