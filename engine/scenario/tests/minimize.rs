//! Minimize (Opus Z unit 1): the `minimize` volatile (its `onRestart` returns `null`), and against
//! a holder the `minimize` moves never miss (`onAccuracy`) and deal double damage
//! (`onSourceModifyDamage`). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// A second Minimize raises evasion again; the volatile stays (its restart returns `null`).
#[test]
fn minimize_again_raises_evasion() {
    assert_exact_parity("z-minimize");
}

/// Flying Press never misses a minimized Clefable and deals double damage; Tackle rolls
/// against its evasion.
#[test]
fn minimize_moves_always_hit_for_double_damage() {
    assert_exact_parity("z-minimize-hit");
}
