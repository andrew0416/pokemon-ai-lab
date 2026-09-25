//! Parity of move callbacks and targeting (WORKPLAN §2.1: O1, O4, O6, O8, O9, O16, O18–O21,
//! O23, O27, O69) with Showdown: each scenario's exact outcome distribution must equal its
//! oracle fixture (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

#[test]
fn o1_expanding_force_spreads_in_psychic_terrain() {
    assert_exact_parity("o1-expanding-force");
}

#[test]
fn o1_expanding_force_ungrounded_and_retargeted_from_an_ally() {
    assert_exact_parity("o1-expanding-force-ungrounded");
}
