//! Moves that change their user after the hit or by the weather (Opus V unit 2): Growth
//! (`onModifyMove`: +2 / +2 in sun), Fell Stinger, Order Up and Relic Song
//! (`onAfterMoveSecondarySelf`). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Growth doubles in the user's sun; Utility Umbrella hides the sun from its holder.
#[test]
fn growth_doubles_in_sun() {
    assert_exact_parity("growth-sun");
}

/// Fell Stinger raises Attack by 3 only when it knocks its target out.
#[test]
fn fell_stinger_boosts_on_a_knockout() {
    assert_exact_parity("fell-stinger");
}

/// Order Up raises the commanded Dondozo's stat of its Tatsugiri's forme (Droopy: Defense).
#[test]
fn order_up_boosts_by_the_commander_forme() {
    assert_exact_parity("order-up");
}

/// Relic Song changes Meloetta to Pirouette forme.
#[test]
fn relic_song_changes_meloetta_forme() {
    assert_exact_parity("relic-song");
}

/// A second Relic Song changes Meloetta-Pirouette back to Meloetta.
#[test]
fn relic_song_changes_meloetta_back() {
    assert_exact_parity("relic-song-back");
}
