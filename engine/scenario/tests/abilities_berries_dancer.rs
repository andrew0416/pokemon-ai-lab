//! Cheek Pouch, Cud Chew, Ripen, Poison Puppeteer and Dancer (Opus U unit 3). Fixtures from
//! Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::{assert_exact_parity, assert_extremes_parity};

/// Cheek Pouch heals 1/3 after the berry's own heal (EatItem).
#[test]
fn cheek_pouch_heals_after_eating() {
    assert_exact_parity("u-cheek-pouch");
}

/// Ripen doubles a berry's heal and halves a resist berry's hit once more.
#[test]
fn ripen_doubles_heals_and_resist_berries() {
    assert_exact_parity("u-ripen");
}

/// Ripen doubles a berry's boosts.
#[test]
fn ripen_doubles_berry_boosts() {
    assert_exact_parity("u-ripen-liechi");
}

/// Cud Chew eats a berry eaten during a turn again at the next turn's residual.
#[test]
fn cud_chew_eats_again_next_turn() {
    assert_exact_parity("u-cud-chew");
}

/// A berry eaten during the residual (nothing left in the queue) is eaten again at once.
#[test]
fn cud_chew_during_the_residual_eats_again_at_once() {
    assert_exact_parity("u-cud-chew-sand");
}

/// Poison Puppeteer confuses the target Pecharunt's move poisons.
#[test]
fn poison_puppeteer_confuses_on_poison() {
    assert_exact_parity("u-poison-puppeteer");
}

/// Dancers copy dance moves, slowest first, at the targets Showdown picks, without PP or
/// `lastMove`.
#[test]
fn dancers_copy_dance_moves() {
    assert_exact_parity("u-dancer");
}

/// A sleeping Dancer's copy only spends a sleep turn (BeforeMove).
#[test]
fn a_sleeping_dancer_spends_a_sleep_turn() {
    assert_exact_parity("u-dancer-asleep");
}

/// A Dancer copying Petal Dance picks its own random target and is not locked (the exact
/// distribution, 2992 outcomes, matched too; the fixture keeps the extreme rolls).
#[test]
fn dancer_petal_dance_is_not_locked() {
    assert_extremes_parity("u-dancer-petal-dance");
}
