//! Parity of held items and field effects (work plan units O86, O80/O81, O92/O102, O95,
//! O101, O103) with Showdown's outcome distribution (`engine/oracle/expected/`), plus the
//! combinations the engine refuses on purpose.

mod common;

use common::assert_exact_parity;

// ---- O86 items that boost when hit ---------------------------------------------------------------

/// A super-effective hit uses Weakness Policy; a fixed-damage move computes no `typeMod`.
#[test]
fn weakness_policy_matches_showdown() {
    assert_exact_parity("o86-weakness-policy");
}

#[test]
fn absorb_bulb_and_luminous_moss_match_showdown() {
    assert_exact_parity("o86-absorb-bulb-moss");
}

#[test]
fn cell_battery_and_snowball_match_showdown() {
    assert_exact_parity("o86-cell-battery-snowball");
}

/// Kee / Maranga Berry are eaten in AfterMoveSecondary, after the hit loop.
#[test]
fn kee_and_maranga_berries_match_showdown() {
    assert_exact_parity("o86-kee-maranga");
}
