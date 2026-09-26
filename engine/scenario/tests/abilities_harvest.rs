//! Parity of Harvest (Opus S unit 9a: `onResidual`) with Showdown: each scenario's exact
//! outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Half the time the eaten berry comes back and is eaten again.
#[test]
fn harvest_matches_showdown() {
    assert_exact_parity("s-harvest");
}

/// In harsh sunlight it always comes back.
#[test]
fn harvest_in_sun_matches_showdown() {
    assert_exact_parity("s-harvest-sun");
}
