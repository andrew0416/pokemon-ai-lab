//! Burn Up (legal in Champions): Double Shock's shape with Fire (`onTryMove` stops a user
//! without the Fire type with `null`; `self.onHit` turns Fire into `???`), and its `defrost`
//! flag does not thaw a frozen user without the Fire type. Fixtures from Showdown's exact
//! enumeration.

mod common;

use common::assert_exact_parity;

/// Mono-Fire Arcanine becomes `???`, Charizard `???/Flying`; Blissey's Burn Up fails.
#[test]
fn burn_up_matches_showdown_exactly() {
    assert_exact_parity("burn-up");
}

/// A frozen Blissey picking Burn Up goes through the freeze check (1/4 thaw) instead of
/// thawing.
#[test]
fn burn_up_frozen_matches_showdown_exactly() {
    assert_exact_parity("burn-up-frozen");
}
