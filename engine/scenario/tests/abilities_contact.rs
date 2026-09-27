//! Parity of hit-reaction, contact, boost-blocking, switch-in and switch-out abilities (work
//! plan units O55, O56, O58, O68, O63, O64, O54) with Showdown's outcome distribution
//! (`engine/oracle/expected/*.turn.json`).

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::dex::abilities;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_scenario::scenario_choices;

// ---- O55 onDamagingHit -------------------------------------------------------------------------

/// Static, Flame Body and Poison Point: a 30% roll per contact hit, the holder as the source.
#[test]
fn contact_status_abilities_match_showdown() {
    assert_exact_parity("o55-contact-status");
}

/// Effect Spore's sleep / paralysis / poison draw, and a Grass attacker's powder immunity.
#[test]
fn effect_spore_matches_showdown() {
    assert_exact_parity("o55-effect-spore");
}

/// Cotton Down against Mirror Armor and Defiant, Gooey from an ally, Protective Pads.
#[test]
fn gooey_and_cotton_down_match_showdown() {
    assert_exact_parity("o55-gooey-cotton-down");
}

#[test]
fn stamina_weak_armor_tangling_hair_match_showdown() {
    assert_exact_parity("o55-stamina-weak-armor");
}

#[test]
fn steam_engine_and_water_compaction_match_showdown() {
    assert_exact_parity("o55-water-boosts");
}

#[test]
fn justified_and_thermal_exchange_match_showdown() {
    assert_exact_parity("o55-justified-thermal");
}

/// Thermal Exchange is breakable: a Mold Breaker attacker's Fire move does not trigger it.
#[test]
fn thermal_exchange_against_mold_breaker_matches_showdown() {
    assert_exact_parity("o55-thermal-mold-breaker");
}

/// Electromorphosis and Wind Power (a wind move, Tailwind) add `charge`, which doubles the
/// next Electric move and ends after it.
#[test]
fn charge_abilities_match_showdown() {
    assert_exact_parity("o55-charge");
}

#[test]
fn sand_spit_and_seed_sower_match_showdown() {
    assert_exact_parity("o55-sand-seed");
}

/// Aftermath and Innards Out on holders the hit fainted, fainting an attacker.
#[test]
fn aftermath_and_innards_out_match_showdown() {
    assert_exact_parity("o55-aftermath-innards");
}

/// Anger Shell's boosts and berry timing; Berserk's check left pending by a Sheer Force move
/// keeps its Sitrus Berry (Champions `onDamage`).
#[test]
fn anger_shell_and_berserk_match_showdown() {
    assert_exact_parity("o55-anger-shell-berserk");
}

// ---- O56 onSourceDamagingHit -------------------------------------------------------------------

#[test]
fn poison_touch_and_toxic_chain_match_showdown() {
    assert_exact_parity("o56-poison-touch");
}

/// Shield Dust, and the damaged target's Protective Pads (Showdown passes the target as the
/// attacker to `checkMoveMakesContact`).
#[test]
fn blocked_poison_touch_and_toxic_chain_match_showdown() {
    assert_exact_parity("o56-poison-touch-blocked");
}

// ---- O58 boost blockers, Own Tempo, Scrappy, Keen Eye; the onUpdate cures ------------------

/// The ability `onUpdate` cures: skipped at the hit's Update while a Mold Breaker move is in
/// progress (the Cheri Berry cures Limber's holder instead), run at the Update after the action
/// (Own Tempo's confusion cure, before its holder moves).
#[test]
fn update_cures_after_ability_ignoring_moves_match_showdown() {
    assert_exact_parity("o58-update-cures");
}

/// A switch-in Intimidate against Own Tempo and Oblivious; Own Tempo against Confuse Ray.
#[test]
fn own_tempo_and_oblivious_block_intimidate_match_showdown() {
    assert_exact_parity("o58-own-tempo-intimidate");
}

/// Scrappy against Intimidate (battle start) and a Ghost type; Keen Eye's ignored evasion and
/// blocked accuracy drop.
#[test]
fn scrappy_and_keen_eye_match_showdown() {
    assert_exact_parity("o58-scrappy-keen-eye");
}

// ---- O68 switch-in abilities, O63 Unnerve --------------------------------------------------------

/// The battle start's handler order: `onSwitchInPriority` before Speed (Costar after Intimidate
/// and Supersweet Syrup although it is the fastest).
#[test]
fn switch_in_priority_and_supersweet_syrup_match_showdown() {
    assert_exact_parity("o68-start-order");
}

/// Download at the start and on a switch-in, Intrepid Sword, Dauntless Shield.
#[test]
fn download_intrepid_sword_dauntless_shield_match_showdown() {
    assert_exact_parity("o68-download-sword-shield");
}

#[test]
fn hospitality_and_curious_medicine_match_showdown() {
    assert_exact_parity("o68-support-switch-ins");
}

/// Screen Cleaner on both sides; Unnerve keeps a foe's Sitrus Berry uneaten.
#[test]
fn screen_cleaner_and_unnerve_match_showdown() {
    assert_exact_parity("o68-screen-cleaner-unnerve");
}

/// Pastel Veil's `onAnySwitchIn` cures an ally poisoned through Mold Breaker; its `onUpdate`
/// cures the holder after the action.
#[test]
fn pastel_veil_any_switch_in_and_update_match_showdown() {
    assert_exact_parity("o68-pastel-veil");
}

/// Pastel Veil's `onStart` cure and `onAllySetStatus` block.
#[test]
fn pastel_veil_start_matches_showdown() {
    assert_exact_parity("o68-pastel-veil-start");
}

// ---- O64 Unburden --------------------------------------------------------------------------------

/// A berry eaten (AfterUseItem) doubles the holder's Speed at once: it now moves first.
#[test]
fn unburden_after_a_berry_matches_showdown() {
    assert_exact_parity("o64-unburden-berry");
}

/// Knock Off against a Mega Stone (TakeItem runs Unburden before the stone refuses) and an
/// Air Balloon's pop.
#[test]
fn unburden_take_item_and_air_balloon_match_showdown() {
    assert_exact_parity("o64-unburden-take");
}

#[test]
fn unburden_after_trick_matches_showdown() {
    assert_exact_parity("o64-unburden-trick");
}

// ---- O54 onSwitchOut -----------------------------------------------------------------------------

/// Regenerator and Natural Cure (the Champions `onSwitchOut`) on a switch action.
#[test]
fn regenerator_and_natural_cure_match_showdown() {
    assert_exact_parity("o54-regenerator-natural-cure");
}

/// Intrepid Sword acts once per battle (`pokemon.swordBoost`, `SideHistory::sword_boost`, board
/// R10a): a switch-in after the battle start boosts Attack unless the flag is already set.
#[test]
fn intrepid_sword_after_the_start_acts_once() {
    let name = "o68-download-sword-shield";
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let porygon2 = state
        .side(SideId::One)
        .party
        .iter()
        .position(|p| p.species.data().name == "Porygon2")
        .unwrap();
    let mon = &mut state.side_mut(SideId::One).party[porygon2];
    mon.ability = abilities::INTREPID_SWORD;
    mon.base_ability = abilities::INTREPID_SWORD;
    // Porygon2's Attack stage in every outcome (it switches in during the turn).
    let atk_stages = |state: &mut lab_engine::Doubles| -> Vec<i8> {
        let outcomes = enumerate_turn(state, Ruleset::CHAMPIONS_MC, choices).unwrap();
        outcomes
            .iter()
            .map(|o| {
                let mut end = state.clone();
                end.apply(&o.instructions);
                let side = end.side(SideId::One);
                side.slots
                    .iter()
                    .find(|s| s.party_index == Some(porygon2 as u8))
                    .map_or(0, |s| s.boosts[0])
            })
            .collect()
    };
    assert!(atk_stages(&mut state).iter().all(|&a| a == 1));
    state.side_mut(SideId::One).history.sword_boost |= 1 << porygon2;
    assert!(atk_stages(&mut state).iter().all(|&a| a == 0));
}
