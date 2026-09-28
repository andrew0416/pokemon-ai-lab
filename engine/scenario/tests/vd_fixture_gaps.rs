//! Board A1-t2-fixture-gaps (VD): exact parity with Showdown for the standard entries that have
//! their own handlers but had neither an oracle fixture nor a library use
//! (`engine/reports/events-audit-2026-09-27.json`, `reactors_without_oracle_fixture`).
//! Each scenario's description says what it exercises.

mod common;

use common::{assert_exact_parity, assert_extremes_parity};

/// Upper Hand's `onTry`: hits a target that chose a priority move, fails into a status move.
#[test]
fn upper_hand() {
    assert_exact_parity("vd-upper-hand");
}

/// Illuminate: `onModifyMove` ignores the target's evasion, `onTryBoost` blocks accuracy drops.
#[test]
fn illuminate() {
    assert_exact_parity("vd-illuminate");
}

/// Rattled: `onAfterBoost` on Intimidate and `onDamagingHit` on a Bug move.
#[test]
fn rattled() {
    assert_exact_parity("vd-rattled");
}

/// Gluttony: a pinch berry eaten at 1/2 HP.
#[test]
fn gluttony() {
    assert_exact_parity("vd-gluttony");
}

/// Leppa Berry: `onUpdate` on a move at 0 PP, `onEat` +10 PP capped at the max.
#[test]
fn leppa_berry() {
    assert_exact_parity("vd-leppa-berry");
}

/// Misty Explosion: x1.5 on Misty Terrain for a grounded user.
#[test]
fn misty_explosion() {
    assert_exact_parity("vd-misty-explosion");
}

/// Skill Link: a [2, 5] multi-hit move always hits 5 times (extremes: 5 rolls and crits).
#[test]
fn skill_link() {
    assert_extremes_parity("vd-skill-link");
}

/// Axe Kick: miss and Protect both crash for 1/2 max HP; 30% confusion.
#[test]
fn axe_kick() {
    assert_exact_parity("vd-axe-kick");
}

/// Supercell Slam: an immune target and a miss crash for 1/2 max HP.
#[test]
fn supercell_slam() {
    assert_exact_parity("vd-supercell-slam");
}

/// Temper Flare: 150 BP after the user's move failed (Flash Fire) in the setup turn.
#[test]
fn temper_flare() {
    assert_exact_parity("vd-temper-flare");
}

/// Dragonize: Normal moves become boosted Dragon moves; Weather Ball is excluded.
#[test]
fn dragonize() {
    assert_exact_parity("vd-dragonize");
}

/// Phantom Force: the second turn after the charge breaks Protect.
#[test]
fn phantom_force() {
    assert_exact_parity("vd-phantom-force");
}

/// Solar Blade: no charge turn in sun.
#[test]
fn solar_blade_sun() {
    assert_exact_parity("vd-solar-blade-sun");
}

/// Solar Blade: the charged hit at half power in sand.
#[test]
fn solar_blade_sand() {
    assert_exact_parity("vd-solar-blade-sand");
}

/// Sky Attack: the hit after the charge turn (crit ratio 2, 30% flinch).
#[test]
fn sky_attack() {
    assert_exact_parity("vd-sky-attack");
}

/// Flail and Water Spout: base power by the user's HP.
#[test]
fn flail_water_spout() {
    assert_exact_parity("vd-flail-water-spout");
}

/// Power Trip (positive boosts) and Infernal Parade (Champions 65 BP, doubled on a statused
/// target).
#[test]
fn power_trip_infernal_parade() {
    assert_exact_parity("vd-power-trip-infernal-parade");
}

/// Big Pecks and White Smoke: foes' drops blocked, the holder's own drops applied.
#[test]
fn big_pecks_white_smoke() {
    assert_exact_parity("vd-big-pecks-white-smoke");
}

/// Queenly Majesty: priority moves at the holder's side fail.
#[test]
fn queenly_majesty() {
    assert_exact_parity("vd-queenly-majesty");
}
