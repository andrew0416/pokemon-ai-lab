//! A locked Pokémon's choices in the search (boards B38, B39): a recharging Porygon-Z
//! (`hyper-beam-recharge`, Hyper Beam in the setup turn) has the one legal choice
//! `move recharge`, which the ruleset accepts and the turn engine runs; the scenario's own
//! choice string for it (`move recharge 1`) is that legal choice once normalized.

use std::path::PathBuf;

use lab_engine::action::SlotAction;
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{
    legal_joint_actions, locked_move, EnumerateOptions, Locked, RollMode, RECHARGE_INDEX,
};
use lab_scenario::{load_scenario_file, parse_choice, scenario_positions};
use lab_search::game::normalize_turn_choice;
use lab_search::{format_choice, legal_choices, transitions, Choice, Decision, Pruning};

const EXTREMES: EnumerateOptions = EnumerateOptions {
    rolls: RollMode::Extremes,
};

fn positions() -> (lab_scenario::LoadedScenario, Vec<lab_scenario::Position>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios/hyper-beam-recharge.json");
    let loaded = load_scenario_file(path).unwrap();
    // The positions after the setup turn in which Porygon-Z must recharge (in the others its
    // Hyper Beam missed, so nothing is locked).
    let positions: Vec<_> = scenario_positions(&loaded)
        .unwrap()
        .into_iter()
        .filter(|p| {
            locked_move(
                &p.state,
                SlotRef {
                    side: SideId::One,
                    slot: 0,
                },
            ) == Some(Locked::Recharge)
        })
        .collect();
    assert!(!positions.is_empty());
    (loaded, positions)
}

/// Board B38: `legal_joint_actions` offers `RECHARGE_INDEX` for the recharging slot, and the
/// ruleset used to refuse it (`MoveIndexOutOfRange`), so the search had no legal choice.
#[test]
fn a_recharging_pokemon_has_a_legal_choice() {
    let (_, positions) = positions();
    let ruleset = Ruleset::CHAMPIONS_MC;
    for position in &positions {
        let mut state = position.state.clone();
        let ours = legal_joint_actions(&state, ruleset, SideId::One);
        assert!(!ours.is_empty());
        for action in &ours {
            assert!(
                matches!(action[0], SlotAction::Move { index, target: 0, .. } if index == RECHARGE_INDEX),
                "{action:?}"
            );
            ruleset
                .validate_joint_action(&state, SideId::One, action)
                .unwrap();
        }
        let us = legal_choices(&state, ruleset, Decision::Turn, SideId::One, Pruning::All);
        let them = legal_choices(&state, ruleset, Decision::Turn, SideId::Two, Pruning::All);
        assert_eq!(us.len(), ours.len());
        let outcomes = transitions(
            &mut state,
            ruleset,
            EXTREMES,
            Decision::Turn,
            None,
            [us[0], them[0]],
        )
        .unwrap();
        assert!(!outcomes.is_empty());
    }
    // A recharge index for a Pokémon that is not recharging is still refused.
    let fresh = &positions[0].state;
    let not_recharging = [SlotAction::Move {
        index: RECHARGE_INDEX,
        target: 0,
        gimmick: Default::default(),
    }; 2];
    assert!(ruleset
        .validate_joint_action(fresh, SideId::Two, &not_recharging)
        .is_err());
}

/// Board B39: the scenario's own choice (`move recharge 1, move protect`) parses with a target
/// on the locked slot; normalized it is one of the legal choices `lab-plan` ranks.
#[test]
fn the_scenarios_own_choice_is_among_the_legal_choices() {
    let (loaded, positions) = positions();
    let own = &loaded.meta.turn.as_ref().unwrap().p1;
    let ruleset = Ruleset::CHAMPIONS_MC;
    for position in &positions {
        let state = &position.state;
        let parsed = parse_choice(state, SideId::One, &position.order[0], own).unwrap();
        let legal = legal_choices(state, ruleset, Decision::Turn, SideId::One, Pruning::All);
        let normalized = normalize_turn_choice(state, ruleset, SideId::One, parsed);
        assert!(
            legal.contains(&Choice::Turn(normalized)),
            "{} not in the legal choices",
            format_choice(state, SideId::One, &position.order[0], &normalized)
        );
        // Showdown wants a target on a locked move in doubles (see `format_choice`).
        assert_eq!(
            format_choice(state, SideId::One, &position.order[0], &normalized),
            "move recharge 1, move protect"
        );
        // An unlocked side is left as parsed.
        let theirs = parse_choice(
            state,
            SideId::Two,
            &position.order[1],
            &loaded.meta.turn.as_ref().unwrap().p2,
        )
        .unwrap();
        assert_eq!(
            normalize_turn_choice(state, ruleset, SideId::Two, theirs),
            theirs
        );
    }
}

/// A Pokémon locked into Outrage (`outrage-lock`, Outrage in the setup turn): its choice is
/// written with target 1 (`move outrage 1`), since Showdown's choice parser rejects a named
/// locked move without a target in doubles (`Can't move: Outrage needs a target`; V11a found it
/// as lab-parity positions after a Raging Fury lock that the oracle refused). The text parses
/// back to the same normalized choice.
#[test]
fn a_locked_move_is_written_with_a_target() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios/outrage-lock.json");
    let loaded = load_scenario_file(path).unwrap();
    let slot = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    let ruleset = Ruleset::CHAMPIONS_MC;
    let mut locked = 0;
    for position in scenario_positions(&loaded).unwrap() {
        let state = &position.state;
        if !matches!(locked_move(state, slot), Some(Locked::Move(_))) {
            continue;
        }
        locked += 1;
        for choice in legal_choices(state, ruleset, Decision::Turn, SideId::One, Pruning::All) {
            let Choice::Turn(action) = choice else {
                panic!("{choice:?}")
            };
            let text = format_choice(state, SideId::One, &position.order[0], &action);
            assert!(text.starts_with("move outrage 1, "), "{text}");
            let parsed = parse_choice(state, SideId::One, &position.order[0], &text).unwrap();
            assert_eq!(
                normalize_turn_choice(state, ruleset, SideId::One, parsed),
                action
            );
        }
    }
    assert!(locked > 0);
}
