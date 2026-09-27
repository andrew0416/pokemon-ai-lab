//! Heavy turns (JJ-heavy-turn-parity): turns whose Showdown enumeration the parity sweeps could not
//! finish even in extremes mode (spread moves plus Speed ties: every tie in an Update after every
//! hit is a Showdown PRNG decision). The oracle fixtures come from `enumerate.cjs --staged`, which
//! merges identical states between actions (the same distribution as the plain enumeration, checked
//! against every plain fixture by `oracle/check-staged.cjs`):
//! - `<name>.fixed<k>.json` (`--mode fixed --roll k`: every damage roll at index k, everything else
//!   enumerated) against `RollMode::Fixed(k)`;
//! - `<name>.extremes.json` against `RollMode::Extremes`.

mod common;

use common::{assert_extremes_parity, assert_fixed_parity};

/// Floette-Eternal and Rillaboom on both sides (Speed 122 = 122, 105 = 105): both Dazzling Gleams,
/// High Horsepower and Wood Hammer (recoil) into the Floettes. About 10^12 plain branches in fixed
/// mode, 18,496 staged runs.
#[test]
fn floette_mirror_spread_fixed_rolls() {
    for roll in [0, 7, 15] {
        assert_fixed_parity("jj-floette-mirror-spread", roll);
    }
}

/// The same turn with min/max rolls: 896 outcomes (388,352 staged runs).
#[test]
fn floette_mirror_spread_extremes() {
    assert_extremes_parity("jj-floette-mirror-spread");
}
