//! Parity of support and control moves (WORKPLAN §2.1: O7, O13–O15, O22, O28, O32, O33, O35,
//! O38) with Showdown: each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::action::{Gimmick, SlotAction};
use lab_engine::rules::Ruleset;
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_engine::Doubles;
use lab_scenario::scenario_choices;

/// Runs the scenario's turn with one slot's choice replaced and expects an `InvalidChoice`
/// whose reason contains `expected`.
fn assert_invalid_choice(name: &str, side: usize, slot: usize, choice: SlotAction, expected: &str) {
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state: Doubles = position.state;
    let mut choices = scenario_choices(&loaded, &state).unwrap();
    choices[side][slot] = choice;
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::InvalidChoice { reason, .. }) => {
            assert!(reason.contains(expected), "{reason}")
        }
        other => panic!("expected InvalidChoice, got {other:?}"),
    }
}

fn move_choice(index: u8, target: i8) -> SlotAction {
    SlotAction::Move {
        index,
        target,
        gimmick: Gimmick::None,
    }
}

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

#[test]
fn o13_taunt_blocks_a_status_move_this_turn_and_lasts_longer_after_moving() {
    assert_exact_parity("o13-taunt");
}

#[test]
fn o13_taunt_ends_in_the_residual() {
    assert_exact_parity("o13-taunt-ends");
}

/// Taunt's `onDisableMove`: the taunted Clefable cannot choose Calm Mind.
#[test]
fn o13_a_taunted_pokemon_cannot_choose_a_status_move() {
    assert_invalid_choice("o13-taunt-ends", 1, 0, move_choice(0, 0), "Taunt");
}

#[test]
fn o14_disable_before_and_after_the_target_moves() {
    assert_exact_parity("o14-disable");
}

#[test]
fn o14_disable_fails_without_a_last_move() {
    assert_exact_parity("o14-disable-fail");
}

#[test]
fn o14_disable_counts_down_on_the_next_turn() {
    assert_exact_parity("o14-disable-next");
}

/// Disable's `onDisableMove`: Clefable cannot choose its disabled Calm Mind.
#[test]
fn o14_a_disabled_move_cannot_be_chosen() {
    assert_invalid_choice("o14-disable-next", 1, 0, move_choice(0, 0), "Disable");
}

#[test]
fn o15_torment_and_imprison_block_a_shared_move() {
    assert_exact_parity("o15-torment-imprison");
}

#[test]
fn o15_torment_and_imprison_two_turns_later() {
    assert_exact_parity("o15-torment-imprison-next");
}

/// Torment's `onDisableMove`: Clefable cannot repeat its last move, Draining Kiss.
#[test]
fn o15_a_tormented_pokemon_cannot_repeat_its_last_move() {
    assert_invalid_choice(
        "o15-torment-imprison-next",
        1,
        0,
        move_choice(1, 1),
        "Torment",
    );
}

/// Imprison's `onFoeDisableMove`: Hitmontop knows Calm Mind and Protect, so neither foe can
/// choose them.
#[test]
fn o15_imprison_disables_the_moves_its_holder_knows() {
    assert_invalid_choice(
        "o15-torment-imprison-next",
        1,
        0,
        move_choice(0, 0),
        "Imprison",
    );
    assert_invalid_choice(
        "o15-torment-imprison-next",
        1,
        1,
        move_choice(2, 0),
        "Imprison",
    );
}
