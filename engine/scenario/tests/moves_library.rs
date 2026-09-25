//! Parity of moves the team library uses (COVERAGE.md "라이브러리에서 쓰이는데 미지원"):
//! Throat Chop, the protect variants and Feint, ... Each scenario's exact outcome distribution
//! must equal its oracle fixture (`engine/oracle/expected/<name>.turn.json`).

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
fn throat_chop_stops_a_sound_move_before_it_is_used() {
    assert_exact_parity("throat-chop");
}

#[test]
fn throat_chop_lasts_one_more_turn_and_does_not_restart() {
    assert_exact_parity("throat-chop-next");
}

#[test]
fn throat_chop_fails_a_sound_move_called_by_sleep_talk() {
    assert_exact_parity("throat-chop-sleep-talk");
}

/// Throat Chop's `onDisableMove`: the throat-chopped Primarina cannot choose Hyper Voice.
#[test]
fn a_throat_chopped_pokemon_cannot_choose_a_sound_move() {
    assert_invalid_choice("throat-chop-next", 1, 0, move_choice(0, 0), "Throat Chop");
}
