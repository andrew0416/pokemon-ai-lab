//! Heavy turns (JJ-heavy-turn-parity): turns whose Showdown enumeration the parity sweeps could not
//! finish even in extremes mode (spread moves plus Speed ties: every tie in an Update after every
//! hit is a Showdown PRNG decision), and the engine bugs their comparison found. The heavy turns'
//! oracle fixtures come from `enumerate.cjs --staged`, which merges identical states between
//! actions (the same distribution as the plain enumeration, checked against every plain fixture by
//! `oracle/check-staged.cjs`); the reduced bug scenarios' from the plain enumeration:
//! - `<name>.fixed<k>.json` (`--mode fixed --roll k`: every damage roll at index k, everything else
//!   enumerated) against `RollMode::Fixed(k)`;
//! - `<name>.extremes.json` against `RollMode::Extremes`.

mod common;

use common::{assert_exact_parity, assert_extremes_parity, assert_fixed_parity};

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

/// A confused Garchomp's self-hit (the only damage of the turn): its roll is a damage roll like any
/// other (`getConfusionDamage` -> `battle.randomizer`), so the reduced modes must reduce it too.
/// Found by the V2 parity positions (a confused Floette's Protect): the engine drew all 16 rolls
/// (`uniform(16)`) in every mode, which only the exact mode agreed with. Plain-enumeration fixtures.
#[test]
fn confusion_self_hit_roll_follows_the_roll_mode() {
    let name = "jj-confusion-self-hit-roll";
    assert_exact_parity(name);
    assert_extremes_parity(name);
    for roll in [0, 7, 15] {
        assert_fixed_parity(name, roll);
    }
}
