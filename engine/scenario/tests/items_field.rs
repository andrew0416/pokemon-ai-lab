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

// ---- O80 / O81 remaining berries --------------------------------------------------------------

/// Lansat Berry adds `focusenergy` (crit ratio +2), Starf Berry raises a random stat by 2; both
/// eaten on the Update after a hit.
#[test]
fn lansat_and_starf_berries_match_showdown() {
    assert_exact_parity("o80-lansat-starf");
}

/// Micle Berry: eaten in the residual (setup turn), then its volatile's Accuracy handler.
#[test]
fn micle_berry_matches_showdown() {
    assert_exact_parity("o80-micle");
}

/// The Accuracy event is skipped for Toxic used by a Poison type (it cannot miss).
#[test]
fn toxic_from_a_poison_type_matches_showdown() {
    assert_exact_parity("o80-toxic-poison-user");
}

/// Custap Berry is eaten when the actions are queued and gives +0.1 priority.
#[test]
fn custap_berry_matches_showdown() {
    assert_exact_parity("o81-custap");
}

/// Enigma Berry (runEvent('Hit'), super-effective) and Jaboca Berry (DamagingHit, physical).
#[test]
fn enigma_and_jaboca_berries_match_showdown() {
    assert_exact_parity("o81-enigma-jaboca");
}

/// Rowap Berry is eaten by a holder the hit fainted; Jaboca Berry is not eaten against Magic
/// Guard.
#[test]
fn rowap_berry_at_zero_hp_and_jaboca_against_magic_guard_match_showdown() {
    assert_exact_parity("o81-rowap-fainted");
}

// ---- O92 / O102 Seeds and the TerrainChange event ----------------------------------------------

/// A Seed is used by TerrainChange at battle start (Psychic Surge) and after a terrain move,
/// and by its own switch-in handler (priority -1) under a terrain that is already up.
#[test]
fn seeds_on_terrain_change_and_switch_in_match_showdown() {
    assert_exact_parity("o92-seeds-psychic-electric");
}

/// Grassy Seed at battle start, Misty Seed after Misty Terrain replaces Grassy Terrain, and
/// Ice Spinner's `clearTerrain` (TerrainChange without a terrain).
#[test]
fn seeds_with_a_replaced_and_cleared_terrain_match_showdown() {
    assert_exact_parity("o92-seeds-grassy-misty");
}
