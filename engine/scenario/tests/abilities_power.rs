//! Parity of the base power, damage and crit-ratio abilities (Opus Q unit 2: `onBasePower`,
//! `onAllyBasePower`, `onModifyDamage`, `onModifyCritRatio`) with Showdown: each scenario's
//! exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

#[test]
fn sniper_and_tinted_lens_match_showdown() {
    assert_exact_parity("q-sniper-tinted-lens");
}

#[test]
fn neuroforce_and_analytic_match_showdown() {
    assert_exact_parity("q-neuroforce-analytic");
}

#[test]
fn toxic_boost_and_flare_boost_match_showdown() {
    assert_exact_parity("q-toxic-flare-boost");
}

/// Sand Force (and its sandstorm immunity), Battery and Power Spot.
#[test]
fn sand_force_battery_and_power_spot_match_showdown() {
    assert_exact_parity("q-sand-force-battery-power-spot");
}

#[test]
fn super_luck_and_merciless_match_showdown() {
    assert_exact_parity("q-super-luck-merciless");
}
