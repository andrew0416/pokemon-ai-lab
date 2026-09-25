//! Parity of support and control moves (WORKPLAN §2.1: O7, O13–O15, O22, O28, O32, O33, O35,
//! O38) with Showdown: each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

#[test]
fn o7_helping_hand_boosts_both_allies_attacks() {
    assert_exact_parity("o7-helping-hand");
}

#[test]
fn o7_helping_hand_on_a_newcomer_succeeds_and_on_a_moved_ally_fails() {
    assert_exact_parity("o7-helping-hand-fail");
}

#[test]
fn o7_helping_hand_volatile_shows_without_its_multiplier() {
    assert_exact_parity("o7-helping-hand-ko");
}
