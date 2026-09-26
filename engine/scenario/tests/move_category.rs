//! Moves whose ModifyMove changes their category (Opus T): Photon Geyser and Shell Side Arm
//! (`ActiveMove.category`, read by every chain through the move's data). Fixtures from
//! Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Photon Geyser is physical when the user's staged Attack beats its staged SpA: the physical
/// one ignores Light Screen, the special one (Attack lowered) does not.
#[test]
fn photon_geyser_picks_its_category_from_staged_stats() {
    assert_exact_parity("photon-geyser");
}

/// A physical Shell Side Arm is what Ice Face (`effect.category`) blocks.
#[test]
fn physical_shell_side_arm_meets_ice_face() {
    assert_exact_parity("shell-side-arm-ice-face");
}

/// A physical Shell Side Arm makes contact (Rough Skin); Slowking's, special by the formula,
/// stays special.
#[test]
fn shell_side_arm_makes_contact_when_physical() {
    assert_exact_parity("shell-side-arm-contact");
}
