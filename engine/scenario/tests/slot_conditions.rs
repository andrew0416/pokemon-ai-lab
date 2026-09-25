//! Slot conditions (WORKPLAN F12): Wish, Healing Wish and Revival Blessing live on the side's
//! position (`Side::slot_conditions`) and outlast the Pokémon that set them. Fixtures from
//! Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

#[test]
fn wish_matches_showdown_exactly() {
    assert_exact_parity("wish");
}

#[test]
fn wish_heal_matches_showdown_exactly() {
    assert_exact_parity("wish-heal");
}

/// The Wish heals whoever stands in the slot when it resolves.
#[test]
fn wish_after_a_switch_matches_showdown_exactly() {
    assert_exact_parity("wish-switch");
}

#[test]
fn healing_wish_matches_showdown_exactly() {
    assert_exact_parity("healing-wish");
}

/// The replacement is healed and cured by the condition at its switch-in.
#[test]
fn healing_wish_replacement_matches_showdown_exactly() {
    assert_exact_parity("healing-wish-replace");
}

#[test]
fn healing_wish_without_a_bench_matches_showdown_exactly() {
    assert_exact_parity("healing-wish-no-bench");
}

/// Revival Blessing suspends the turn for the choice of a fainted party member.
#[test]
fn revival_blessing_matches_showdown_exactly() {
    assert_exact_parity("revival-blessing");
}

/// A Pokémon revived while still holding its active position switches straight back in.
#[test]
fn revival_blessing_of_an_active_position_matches_showdown_exactly() {
    assert_exact_parity("revival-blessing-active");
}
