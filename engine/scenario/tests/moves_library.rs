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

#[test]
fn spiky_shield_hurts_and_baneful_bunker_poisons_a_contact_attacker() {
    assert_exact_parity("spiky-shield-baneful-bunker");
}

#[test]
fn kings_shield_lowers_attack_and_obstruct_lets_a_status_move_through() {
    assert_exact_parity("kings-shield-status");
}

#[test]
fn obstruct_lowers_defense_and_silk_trap_lowers_speed() {
    assert_exact_parity("obstruct-silk-trap");
}

#[test]
fn burning_bulwark_burns_and_protective_pads_avoid_spiky_shield() {
    assert_exact_parity("burning-bulwark-pads");
}

/// The attacker faints to Spiky Shield in the TryHit step and its spread move still hits the
/// other foe.
#[test]
fn spiky_shield_knocks_out_a_spread_attacker_that_still_hits_the_other_foe() {
    assert_exact_parity("spiky-shield-spread-ko");
}

#[test]
fn feint_breaks_protect_and_wide_guard() {
    assert_exact_parity("feint-wide-guard");
}

#[test]
fn feint_breaks_spiky_shield_before_a_contact_move() {
    assert_exact_parity("feint-spiky-shield");
}

#[test]
fn belly_drum_contrary_and_fillet_away_pay_half_their_hp() {
    assert_exact_parity("belly-drum");
}

#[test]
fn clangorous_soul_fillet_away_and_no_retreat_raise_stats() {
    assert_exact_parity("clangorous-soul");
}

#[test]
fn no_retreat_fails_again_and_a_ghost_can_still_switch() {
    assert_exact_parity("no-retreat");
}

#[test]
fn heal_bell_and_aromatherapy_cure_the_party_but_not_soundproof_or_sap_sipper() {
    assert_exact_parity("heal-bell-aromatherapy");
}

#[test]
fn rest_refresh_and_purify_cure_and_heal() {
    assert_exact_parity("rest-refresh-purify");
}

#[test]
fn purify_after_take_heart_and_rest_failures() {
    assert_exact_parity("purify-take-heart");
}

#[test]
fn jungle_healing_lunar_blessing_and_floral_healing_in_grassy_terrain() {
    assert_exact_parity("jungle-healing-floral");
}

/// No Retreat's `onTrapPokemon`: Snorlax cannot switch to the benched Kommo-o.
#[test]
fn no_retreat_traps_its_user() {
    assert_invalid_choice(
        "no-retreat",
        0,
        0,
        SlotAction::Switch { party_index: 2 },
        "trapped by No Retreat",
    );
}
