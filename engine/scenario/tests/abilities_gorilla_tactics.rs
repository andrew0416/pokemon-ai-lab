//! Parity of Gorilla Tactics (Opus S unit 4) with Showdown: each scenario's exact outcome
//! distribution must equal its oracle fixture, and the lock disables the other moves as
//! Showdown's choice validation does ("Darmanitan's Double-Edge is disabled").

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{enumerate_turn, legal_joint_actions, TurnError};
use lab_scenario::scenario_choices;

/// Attack 1.5x on a physical move.
#[test]
fn gorilla_tactics_attack_matches_showdown() {
    assert_exact_parity("s-gorilla-tactics");
}

/// Locked into Protect by the setup turn: the second Protect succeeds 1/3 of the time.
#[test]
fn gorilla_tactics_status_lock_matches_showdown() {
    assert_exact_parity("s-gorilla-tactics-locked");
}

/// The locked Pokémon can only choose its locked move; another is refused.
#[test]
fn gorilla_tactics_disables_the_other_moves() {
    let name = "s-gorilla-tactics-locked";
    let fixture = common::fixture(name);
    let (loaded, position) = common::start(name, &fixture);
    let mut state = position.state;
    let legal = legal_joint_actions(&state, Ruleset::CHAMPIONS_MC, SideId::One);
    assert!(!legal.is_empty());
    for action in &legal {
        match action[0] {
            // Protect is the second move.
            SlotAction::Move { index: 1, .. } => {}
            other => panic!("Darmanitan may only Protect, got {other:?}"),
        }
    }
    let mut choices = scenario_choices(&loaded, &state).unwrap();
    choices[0][0] = SlotAction::Move {
        index: 0,
        target: 1,
        gimmick: Gimmick::None,
    };
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::InvalidChoice {
            side: SideId::One,
            slot: 0,
            ..
        }) => {}
        other => panic!("expected Double-Edge to be disabled, got {other:?}"),
    }
}
