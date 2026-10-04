//! Forced ally-target actions remain legal under Sensible pruning. A selectable
//! ally attack is still pruned, including one in the other slot of the same pair.
//! The two corpus fixtures are unchanged copies; provenance.json records hashes.

use std::path::{Path, PathBuf};

use lab_engine::action::SlotAction;
use lab_engine::dex::{items, moves, MoveId};
use lab_engine::eval::Material;
use lab_engine::rules::Ruleset;
use lab_engine::state::{MoveSlot, SideId, SlotRef};
use lab_engine::turn::lock::encode_target_loc;
use lab_engine::turn::{locked_move, EnumerateOptions, FactoredScope, Locked, RollMode};
use lab_engine::volatile::{Volatile, VolatileState};
use lab_engine::Doubles;
use lab_scenario::{canonical_value, load_scenario_file, load_scenario_str, scenario_positions};
use lab_search::{decision, legal_choices, transitions, Choice, Config, Decision, Pruning, Solver};
use serde_json::{json, Value};

const OPTIONS: EnumerateOptions = EnumerateOptions {
    rolls: RollMode::Median,
};

fn choices(state: &Doubles, side: SideId, pruning: Pruning) -> Vec<Choice<2>> {
    legal_choices(state, Ruleset::CHAMPIONS_MC, Decision::Turn, side, pruning)
}

fn action_at(choice: &Choice<2>, slot: u8) -> SlotAction {
    let Choice::Turn(actions) = choice else {
        panic!("expected turn choice")
    };
    actions[usize::from(slot)]
}

fn is_move(choice: &Choice<2>, slot: u8, index: u8, target: i8) -> bool {
    matches!(action_at(choice, slot), SlotAction::Move { index: i, target: t, .. }
        if i == index && t == target)
}

fn ally_target(slot: u8) -> i8 {
    // Slot 0's ally is -2; slot 1's ally is -1, on either side.
    -((1 - slot) as i8 + 1)
}

fn assert_forced_choices(state: &Doubles, slot: SlotRef, id: MoveId) {
    let target = ally_target(slot.slot);
    assert_eq!(decision(state, None).unwrap(), Decision::Turn);
    assert_eq!(
        locked_move(state, slot),
        Some(Locked::TwoTurn { id, target })
    );
    let all = choices(state, slot.side, Pruning::All);
    let sensible = choices(state, slot.side, Pruning::Sensible);
    assert!(!all.is_empty(), "turn engine offers the forced action");
    assert!(
        !sensible.is_empty(),
        "Sensible removed a forced ally-target action"
    );
    let index = state
        .active(slot)
        .unwrap()
        .moves
        .iter()
        .position(|m| m.id == id)
        .unwrap_or(0) as u8;
    for choice in &sensible {
        assert!(all.contains(choice), "pruning must not invent an action");
        assert!(is_move(choice, slot.slot, index, target), "{choice:?}");
    }
}

fn corpus_state(name: &str) -> Doubles {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forced_ally_charge");
    let loaded = load_scenario_file(dir.join(format!("{name}.scenario.json"))).unwrap();
    let before: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("{name}.before.json"))).unwrap(),
    )
    .unwrap();
    let matches: Vec<_> = scenario_positions(&loaded)
        .unwrap()
        .into_iter()
        .filter(|position| canonical_value(&position.state, &loaded.meta).unwrap() == before)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "the recorded canonical parent must select one state"
    );
    matches.into_iter().next().unwrap().state
}

fn assert_one_transition_restores(state: &Doubles) {
    let original = state.clone();
    let pair = [SideId::One, SideId::Two].map(|side| {
        *choices(state, side, Pruning::Sensible)
            .first()
            .expect("a legal choice")
    });
    let mut work = original.clone();
    let outcomes = transitions(
        &mut work,
        Ruleset::CHAMPIONS_MC,
        OPTIONS,
        Decision::Turn,
        None,
        pair,
    )
    .unwrap();
    assert!(!outcomes.is_empty());
    assert_eq!(work, original, "enumeration changed its parent");
    for outcome in outcomes {
        work.apply(&outcome.instructions);
        work.reverse(&outcome.instructions);
        assert_eq!(
            work, original,
            "instructions did not restore the full State"
        );
    }
}

#[test]
fn corpus_electro_shot_at_an_empty_ally_slot_remains_selectable() {
    let _flat = FactoredScope::new(false);
    let state = corpus_state("electro-shot");
    let slot = SlotRef {
        side: SideId::One,
        slot: 1,
    };
    assert!(state
        .active(SlotRef {
            side: SideId::One,
            slot: 0
        })
        .is_none());
    assert_forced_choices(&state, slot, moves::ELECTRO_SHOT);
    assert_one_transition_restores(&state);
}

#[test]
fn corpus_solar_beam_at_an_ally_remains_selectable() {
    let _flat = FactoredScope::new(false);
    let state = corpus_state("solar-beam");
    let slot = SlotRef {
        side: SideId::Two,
        slot: 1,
    };
    assert!(state
        .active(SlotRef {
            side: SideId::Two,
            slot: 0
        })
        .is_some());
    assert_forced_choices(&state, slot, moves::SOLAR_BEAM);
    assert_one_transition_restores(&state);
}

/// Deliberately synthetic sets: this is a state/choice regression, not a claim
/// about species learnsets. Distinct Speeds avoid irrelevant setup ties.
fn fresh(id: MoveId) -> Doubles {
    let mon = |side: usize, slot: usize| {
        json!({
            "name": format!("s{side}p{slot}"), "species": "Venusaur",
            "ability": "Overgrow", "item": "", "nature": "Serious", "level": 50,
            "evs": {"hp": 32, "spe": (side * 2 + slot) * 4},
            "moves": [id.data().name, "Tackle", "Growth"]
        })
    };
    let input = json!({"format": "gen9championsdoublescustomgame",
        "p1": {"team": [mon(0, 0), mon(0, 1)], "order": "12"},
        "p2": {"team": [mon(1, 0), mon(1, 1)], "order": "12"}});
    let loaded = load_scenario_str(&input.to_string(), Path::new(".")).unwrap();
    let positions = scenario_positions(&loaded).unwrap();
    assert_eq!(positions.len(), 1);
    positions.into_iter().next().unwrap().state
}

fn forced_state(id: MoveId, side: SideId, slot: u8) -> Doubles {
    let mut state = fresh(id);
    let at = SlotRef { side, slot };
    let volatile = match id {
        id if id == moves::ROLLOUT => Volatile::Rollout,
        id if id == moves::ICE_BALL => Volatile::IceBall,
        _ => Volatile::TwoTurnMove,
    };
    state.slot_mut(at).volatiles.set(
        volatile,
        VolatileState {
            active: true,
            duration: 1,
            counter: encode_target_loc(ally_target(slot)),
            mv: id,
            hidden: if volatile == Volatile::TwoTurnMove {
                0
            } else {
                1
            },
            ..VolatileState::NONE
        },
    );
    if volatile == Volatile::TwoTurnMove {
        let own = if id == moves::SOLAR_BEAM {
            Volatile::SolarBeam
        } else {
            assert_eq!(id, moves::ELECTRO_SHOT);
            Volatile::ElectroShot
        };
        state.slot_mut(at).volatiles.set(
            own,
            VolatileState {
                active: true,
                ..VolatileState::NONE
            },
        );
    }
    state
}

#[test]
fn charge_locks_keep_both_negative_targets_on_both_sides() {
    for id in [moves::SOLAR_BEAM, moves::ELECTRO_SHOT] {
        for side in [SideId::One, SideId::Two] {
            for slot in 0..2 {
                let state = forced_state(id, side, slot);
                assert_forced_choices(&state, SlotRef { side, slot }, id);
            }
        }
    }
}

#[test]
fn a_forced_charge_does_not_keep_an_optional_ally_attack_in_the_other_slot() {
    for side in [SideId::One, SideId::Two] {
        for slot in 0..2 {
            let state = forced_state(moves::SOLAR_BEAM, side, slot);
            let other = 1 - slot;
            let all = choices(&state, side, Pruning::All);
            let sensible = choices(&state, side, Pruning::Sensible);
            // Tackle is move slot 1. Its ally/foe targets remain a real choice.
            let ally: Vec<_> = all
                .iter()
                .filter(|c| is_move(c, other, 1, ally_target(other)))
                .collect();
            assert!(!ally.is_empty());
            assert!(ally.iter().all(|c| !sensible.contains(c)));
            assert!(sensible.iter().any(|c| is_move(c, other, 1, 1)));
        }
    }
}

#[test]
fn optional_ally_attacks_stay_pruned_without_a_lock_and_under_encore_or_choice_lock() {
    for condition in [None, Some(Volatile::Encore), Some(Volatile::ChoiceLock)] {
        for side in [SideId::One, SideId::Two] {
            for slot in 0..2 {
                let mut state = fresh(moves::SOLAR_BEAM);
                let at = SlotRef { side, slot };
                if let Some(volatile) = condition {
                    // These conditions deliberately use different payload fields:
                    // Encore stores mv, while ChoiceLock stores MoveId in counter.
                    // Match items::on_modify_move rather than fabricating a lock
                    // on MoveId::NONE (which would make the user Struggle).
                    let payload = if volatile == Volatile::ChoiceLock {
                        VolatileState {
                            active: true,
                            counter: moves::TACKLE.0,
                            ..VolatileState::NONE
                        }
                    } else {
                        VolatileState {
                            active: true,
                            duration: 2,
                            mv: moves::TACKLE,
                            ..VolatileState::NONE
                        }
                    };
                    state.slot_mut(at).volatiles.set(volatile, payload);
                    if volatile == Volatile::ChoiceLock {
                        state.active_mut(at).unwrap().item = items::CHOICE_BAND;
                    }
                }
                assert_eq!(
                    locked_move(&state, at),
                    None,
                    "Encore/ChoiceLock still allow target selection"
                );
                let all = choices(&state, side, Pruning::All);
                let sensible = choices(&state, side, Pruning::Sensible);
                assert!(
                    all.iter().any(|c| is_move(c, slot, 1, ally_target(slot))),
                    "{condition:?} {at:?}: optional ally Tackle missing before pruning: {all:?}"
                );
                if condition.is_some() {
                    assert!(
                        all.iter().all(|c| matches!(action_at(c, slot), SlotAction::Move { index: 1, .. })),
                        "{condition:?} {at:?}: fixture must restrict the move to Tackle, not its target"
                    );
                }
                assert!(
                    !sensible
                        .iter()
                        .any(|c| is_move(c, slot, 1, ally_target(slot))),
                    "{condition:?} {at:?}: optional ally Tackle survived pruning"
                );
                assert!(
                    sensible.iter().any(|c| is_move(c, slot, 1, 1)),
                    "{condition:?} {at:?}: foe-target Tackle must remain selectable"
                );
                if condition.is_none() {
                    assert!(all.iter().any(|c| is_move(c, slot, 0, ally_target(slot))));
                    assert!(!sensible
                        .iter()
                        .any(|c| is_move(c, slot, 0, ally_target(slot))));
                }
            }
        }
    }
}

#[test]
fn rollout_and_ice_ball_keep_their_original_ally_target() {
    for id in [moves::ROLLOUT, moves::ICE_BALL] {
        for side in [SideId::One, SideId::Two] {
            for slot in 0..2 {
                let state = forced_state(id, side, slot);
                assert_forced_choices(&state, SlotRef { side, slot }, id);
            }
        }
    }
}

#[test]
fn called_charge_uses_the_locked_move_even_when_index_zero_names_another_move() {
    for side in [SideId::One, SideId::Two] {
        for slot in 0..2 {
            let mut state = forced_state(moves::SOLAR_BEAM, side, slot);
            let at = SlotRef { side, slot };
            // Copycat can leave a lock on an unknown move. Index 0 is a placeholder,
            // here a damaging move; its metadata must not veto the forced action.
            state.active_mut(at).unwrap().moves[0] = MoveSlot::full(moves::TACKLE);
            assert!(state
                .active(at)
                .unwrap()
                .moves
                .iter()
                .all(|m| m.id != moves::SOLAR_BEAM));
            assert_forced_choices(&state, at, moves::SOLAR_BEAM);
        }
    }
}

#[test]
fn one_cell_maximin_and_mixed_search_match_all_pruning_and_restore_state() {
    let _flat = FactoredScope::new(false);
    for side in [SideId::One, SideId::Two] {
        let at = SlotRef { side, slot: 1 };
        let mut original = forced_state(moves::SOLAR_BEAM, side, 1);
        // One forced action and one Growth per remaining slot: a 1x1 shallow
        // game, so this checks the solver entry points without a large search.
        for actor in [SideId::One, SideId::Two] {
            for slot in 0..2 {
                let here = SlotRef { side: actor, slot };
                let id = if here == at {
                    moves::SOLAR_BEAM
                } else {
                    moves::GROWTH
                };
                let mon = original.active_mut(here).unwrap();
                mon.moves = [MoveSlot::default(); 4];
                mon.moves[0] = MoveSlot::full(id);
            }
        }
        let mut answers = Vec::new();
        for pruning in [Pruning::All, Pruning::Sensible] {
            for actor in [SideId::One, SideId::Two] {
                assert_eq!(choices(&original, actor, pruning).len(), 1);
            }
            let mut config = Config::new(Ruleset::CHAMPIONS_MC, side);
            config.pruning = pruning;
            config.rolls = RollMode::Median;
            config.depth = 1;
            config.threads = 1;
            let mut solver = Solver::new(config, &Material);
            let mut work = original.clone();
            let maximin = solver.analyse(&mut work, None).unwrap();
            assert_eq!(work, original);
            let mixed = solver.analyse_mixed(&mut work, None).unwrap();
            assert_eq!(work, original);
            assert_eq!((mixed.matrix.rows, mixed.matrix.cols), (1, 1));
            answers.push((
                maximin.value.to_bits(),
                mixed.matrix.values[0].to_bits(),
                mixed.equilibrium.value.to_bits(),
                maximin.decision,
                mixed.decision,
            ));
        }
        assert_eq!(answers[0], answers[1], "forced game changed under Sensible");
    }
}
