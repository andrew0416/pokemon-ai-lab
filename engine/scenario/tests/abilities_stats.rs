//! Parity of the stat-modifier abilities (Opus Q unit 1: `onModifyAtk` / `onModifySpA` /
//! `onModifyDef`) with Showdown: each scenario's exact outcome distribution must equal its
//! oracle fixture (`engine/oracle/expected/*.turn.json`).

mod common;

use common::assert_exact_parity;

#[test]
fn huge_power_pure_power_fur_coat_grass_pelt_match_showdown() {
    assert_exact_parity("q-huge-power-fur-coat");
}

#[test]
fn steelworker_and_transistor_match_showdown() {
    assert_exact_parity("q-steelworker-transistor");
}

#[test]
fn dragons_maw_and_rocky_payload_match_showdown() {
    assert_exact_parity("q-dragons-maw-rocky-payload");
}

#[test]
fn fire_mane_and_defeatist_match_showdown() {
    assert_exact_parity("q-fire-mane-defeatist");
}

/// Stakeout doubles against a Pokémon that switched in this turn; on turn 1 the leads are no
/// longer `newlySwitched` (Payback on a lead that already moved is doubled).
#[test]
fn stakeout_and_payback_on_turn_one_match_showdown() {
    assert_exact_parity("q-stakeout-payback");
}

#[test]
fn plus_and_minus_match_showdown() {
    assert_exact_parity("q-plus-minus");
}
