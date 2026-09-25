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

#[test]
fn psych_up_copies_stages_and_focus_energy_after_a_speed_swap() {
    assert_exact_parity("psych-up-speed-swap");
}

/// Leaving the field recalculates the stored Speed a Speed Swap exchanged.
#[test]
fn speed_swap_ends_when_the_pokemon_switches_out() {
    assert_exact_parity("speed-swap-switch");
}

#[test]
fn strength_sap_heals_by_the_boosted_attack_and_pain_split_averages() {
    assert_exact_parity("strength-sap-pain-split");
}

#[test]
fn spite_reflect_type_and_soak_on_a_roosting_target() {
    assert_exact_parity("spite-soak-reflect-type");
}

#[test]
fn endeavor_equalizes_hp_and_fails_against_a_lower_target() {
    assert_exact_parity("endeavor");
}

#[test]
fn bug_bite_eats_the_targets_berry_and_corrosive_gas_removes_items() {
    assert_exact_parity("bug-bite-corrosive-gas");
}

/// Pluck's user eats a Figy Berry whose flavor its nature dislikes: it is confused.
#[test]
fn pluck_eats_a_confusing_berry_and_incinerate_burns_one() {
    assert_exact_parity("pluck-incinerate");
}

#[test]
fn recycle_restores_the_last_consumed_item() {
    assert_exact_parity("recycle");
}

#[test]
fn hex_doubles_against_a_status_and_reversal_grows_at_low_hp() {
    assert_exact_parity("hex-reversal");
}

#[test]
fn eruption_power_follows_the_users_hp() {
    assert_exact_parity("eruption");
}

#[test]
fn stored_power_and_punishment_count_positive_stages() {
    assert_exact_parity("stored-power-punishment");
}

#[test]
fn electro_ball_and_gyro_ball_compare_modified_speeds() {
    assert_exact_parity("electro-ball-gyro-ball");
}

#[test]
fn crush_grip_and_hard_press_follow_the_targets_hp() {
    assert_exact_parity("crush-grip-hard-press");
}

#[test]
fn trump_card_follows_its_pp_and_return_frustration_use_default_happiness() {
    assert_exact_parity("trump-card-return");
}

#[test]
fn bolt_beak_doubles_before_the_target_moves() {
    assert_exact_parity("bolt-beak");
}

#[test]
fn fishious_rend_doubles_against_a_newcomer() {
    assert_exact_parity("bolt-beak-newcomer");
}

/// Triple Axel's power by hit number (Showdown sampled).
#[test]
fn triple_axel_and_water_shuriken_hit_by_hit() {
    common::assert_mc_parity("triple-axel");
}

#[test]
fn leech_seed_drains_into_the_seeder_and_misses_grass_types() {
    assert_exact_parity("leech-seed");
}

#[test]
fn leech_seed_heals_whoever_stands_in_the_seeders_slot() {
    assert_exact_parity("leech-seed-slot");
}

#[test]
fn bind_and_fire_spin_with_grip_claw_and_binding_band() {
    assert_exact_parity("bind-fire-spin");
}

#[test]
fn partial_trapping_ends_when_the_trapper_leaves() {
    assert_exact_parity("bind-source-leaves");
}

#[test]
fn rapid_spin_removes_leech_seed_and_partial_trapping() {
    assert_exact_parity("rapid-spin-trap");
}

#[test]
fn gigaton_hammer_and_blood_moon_rest_a_turn() {
    assert_exact_parity("gigaton-hammer");
}

/// Partial trapping's `onTrapPokemon` while the trapper is in: the bound Snorlax cannot switch.
#[test]
fn a_partially_trapped_pokemon_cannot_switch() {
    assert_invalid_choice(
        "bind-source-leaves",
        1,
        0,
        SlotAction::Switch { party_index: 2 },
        "partially trapped",
    );
}

/// `cantusetwice`: Tinkaton cannot choose Gigaton Hammer right after using it.
#[test]
fn gigaton_hammer_cannot_be_chosen_twice_in_a_row() {
    assert_invalid_choice(
        "gigaton-hammer",
        0,
        0,
        move_choice(0, 1),
        "cannot be used twice in a row",
    );
}

#[test]
fn psychic_fangs_breaks_reflect_and_light_screen_before_hitting() {
    assert_exact_parity("psychic-fangs-screens");
}

#[test]
fn raging_bull_changes_type_and_brick_break_breaks_nothing_on_immunity() {
    assert_exact_parity("raging-bull-veil");
}

#[test]
fn ceaseless_edge_sets_spikes() {
    assert_exact_parity("ceaseless-edge");
}

#[test]
fn mortal_spin_poisons_and_clears_its_sides_hazards() {
    assert_exact_parity("mortal-spin");
}

#[test]
fn stone_axe_sets_stealth_rock() {
    assert_exact_parity("stone-axe");
}

#[test]
fn stone_axe_with_sheer_force_sets_nothing() {
    assert_exact_parity("stone-axe-sheer-force");
}

#[test]
fn high_jump_kick_and_jump_kick_crash_on_protect_and_immunity() {
    assert_exact_parity("high-jump-kick-crash");
}

#[test]
fn explosion_faints_its_user_before_hitting() {
    assert_exact_parity("explosion");
}

#[test]
fn memento_and_final_gambit_faint_their_users() {
    assert_exact_parity("memento-final-gambit");
}

#[test]
fn sheer_cold_against_focus_sash_and_horn_drill_against_sturdy() {
    assert_exact_parity("ohko");
}

#[test]
fn destiny_bond_takes_the_attacker_down() {
    assert_exact_parity("destiny-bond");
}

#[test]
fn destiny_bond_fails_when_used_again() {
    assert_exact_parity("destiny-bond-again");
}

#[test]
fn destiny_bond_ends_at_the_next_move() {
    assert_exact_parity("destiny-bond-next-move");
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
