//! Surge Surfer, Orichalcum Pulse, Hadron Engine, Liquid Ooze and Sticky Hold (Opus AA unit 3).
//! Fixtures from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Surge Surfer doubles Speed on Electric Terrain.
#[test]
fn surge_surfer_doubles_speed_on_electric_terrain() {
    assert_exact_parity("aa-surge-surfer");
}

/// Orichalcum Pulse sets sun and boosts Attack in it; Hadron Engine sets Electric Terrain and
/// boosts Special Attack on it.
#[test]
fn orichalcum_pulse_and_hadron_engine_set_the_field_and_boost() {
    assert_exact_parity("aa-orichalcum-hadron");
}

/// Liquid Ooze turns drain and Leech Seed heals into damage (Big Root first).
#[test]
fn liquid_ooze_hurts_drainers() {
    assert_exact_parity("aa-liquid-ooze");
}

/// Liquid Ooze and Heal Block (both TryHeal priority 0) go by Speed: the faster decides.
#[test]
fn liquid_ooze_and_heal_block_go_by_speed() {
    assert_exact_parity("aa-liquid-ooze-heal-block");
}

/// Sticky Hold keeps the item from Corrosive Gas; Mold Breaker's Knock Off takes it.
#[test]
fn sticky_hold_keeps_items_unless_broken() {
    assert_exact_parity("aa-sticky-hold");
}

/// Sticky Hold keeps the item from Thief and the berry from Bug Bite.
#[test]
fn sticky_hold_keeps_items_from_thief_and_bug_bite() {
    assert_exact_parity("aa-sticky-hold-thief");
}

/// Sticky Hold keeps the item from Knock Off, whose power is still boosted.
#[test]
fn sticky_hold_keeps_items_from_knock_off() {
    assert_exact_parity("aa-sticky-hold-knock-off");
}
