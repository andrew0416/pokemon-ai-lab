//! Damage-roll modes (WORKPLAN F18): under `RollMode::Extremes` the engine branches only on
//! the minimum and maximum roll, exactly as the oracle's `--mode extremes` does, so the two
//! approximate distributions must agree exactly. Fixtures: `oracle/expected/<name>.extremes.json`.

mod common;

use common::assert_extremes_parity;

/// One damaging move (Grassy Glide) plus a miss/protect branch: the min and max roll map to
/// the same two damage values on both sides.
#[test]
fn single_hit_extremes_match_showdown() {
    assert_extremes_parity("single-hit");
}

/// A turn without damage rolls is unchanged by the mode.
#[test]
fn hypnosis_gravity_extremes_match_showdown() {
    assert_extremes_parity("hypnosis-gravity");
}
