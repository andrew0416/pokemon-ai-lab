//! Parity of support and control moves (WORKPLAN §2.1: O7, O13–O15, O22, O28, O32, O33, O35,
//! O38) with Showdown: each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::action::{Gimmick, SlotAction};
use lab_engine::rules::Ruleset;
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_engine::Doubles;
use lab_scenario::{
    load_scenario_file, run_decision, scenario_choices, scenario_decision, scenario_positions,
};

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

/// Runs a scenario without an oracle fixture (its first position) and expects the engine to
/// refuse it with an `Unsupported` naming `expected`.
fn assert_unsupported(name: &str, expected: &str) {
    let loaded =
        load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    match run_decision(&mut state, &decision) {
        Err(TurnError::Unsupported(what)) => assert!(what.contains(expected), "{what}"),
        other => panic!("expected Unsupported, got {other:?}"),
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

#[test]
fn o22_hazards_are_set_without_duration() {
    assert_exact_parity("o22-hazards-set");
}

#[test]
fn o22_spikes_and_toxic_spikes_stack_layers() {
    assert_exact_parity("o22-hazards-layers");
}

#[test]
fn o22_hazards_fail_at_their_layer_limits() {
    assert_exact_parity("o22-hazards-full");
}

#[test]
fn o22_entry_hazards_on_mid_turn_switches() {
    assert_exact_parity("o22-entry");
}

#[test]
fn o22_stealth_rock_effectiveness_and_ungrounded_spikes() {
    assert_exact_parity("o22-entry-sr");
}

#[test]
fn o22_entry_hazard_knock_out_mid_turn() {
    assert_exact_parity("o22-entry-ko");
}

#[test]
fn o22_entry_hazard_knock_out_on_a_replacement() {
    assert_exact_parity("o22-replace-ko");
}

#[test]
fn o22_defog_and_rapid_spin_remove_hazards() {
    assert_exact_parity("o22-defog-spin");
}

#[test]
fn o22_toxic_spikes_absorbed_first_on_a_double_replacement() {
    assert_exact_parity("o22-replace-toxic-spikes");
}

#[test]
fn o22_toxic_spikes_badly_poison_then_get_absorbed_on_a_double_replacement() {
    assert_exact_parity("o22-replace-toxic");
}

#[test]
fn o22_three_spikes_layers_mirror_armor_and_magic_guard() {
    assert_exact_parity("o22-entry-web");
}

#[test]
fn o28_struggle_is_typeless_with_direct_recoil() {
    assert_exact_parity("o28-struggle");
}

/// Without a usable move, `move 1` is Struggle (Showdown's request lists only Struggle); a
/// status move is still disabled by Taunt and Struggle is no choice while a move is usable.
#[test]
fn o28_struggle_is_the_only_choice_without_a_usable_move() {
    let name = "o28-struggle";
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let choices = scenario_choices(&loaded, &position.state).unwrap();
    let mut state: Doubles = position.state.clone();
    let expected = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    let mut by_index = choices;
    by_index[1][0] = move_choice(0, 0);
    let mut state: Doubles = position.state.clone();
    let got = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, by_index).unwrap();
    assert_eq!(got, expected);
    assert_invalid_choice(name, 1, 0, move_choice(1, 0), "Taunt");
    assert_invalid_choice(
        name,
        1,
        1,
        move_choice(lab_engine::turn::STRUGGLE_INDEX, 0),
        "Struggle while a move is usable",
    );
}

/// Stealth Rock could knock out a newcomer that Toxic Spikes also poisons: Showdown's result
/// depends on the order the hazards were set, which the state does not keep.
#[test]
fn o22_hazard_order_that_shows_is_unsupported() {
    assert_unsupported("o22-hazard-order", "effectOrder");
}

#[test]
fn o22_court_change_swaps_side_conditions() {
    assert_exact_parity("o22-court-change");
}
