//! Multi-hit moves (WORKPLAN F10): each hit is its own stage. A fixed two-hit move against
//! Showdown's exact enumeration; the 2-5 hit draw, Loaded Dice and Population Bomb's per-hit
//! accuracy against Monte Carlo fixtures (Showdown cannot enumerate those exactly).

mod common;

use common::{assert_exact_parity, assert_mc_parity};

#[test]
fn fixed_two_hit_move_matches_showdown_exactly() {
    assert_exact_parity("double-hit");
}

#[test]
fn two_to_five_hits_match_showdown_sampling() {
    assert_mc_parity("bullet-seed");
}

#[test]
fn loaded_dice_matches_showdown_sampling() {
    assert_mc_parity("loaded-dice");
}

#[test]
fn population_bomb_matches_showdown_sampling() {
    assert_mc_parity("population-bomb");
}
