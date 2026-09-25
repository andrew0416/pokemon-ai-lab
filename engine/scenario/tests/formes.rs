//! In-battle forme changes (WORKPLAN F19, `core/src/turn/forme.rs`): exact parity with
//! Showdown fixtures made by `enumerate.cjs` on the fixed commit, see
//! `oracle/scenarios/<name>.json`.

mod common;

use common::assert_exact_parity;
use lab_engine::dex::{abilities, items, species};
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::enumerate_turn;
use lab_scenario::scenario_choices;

// ---- Disguise ---------------------------------------------------------------------------------

/// Disguise absorbs a super-effective hit: no damage, no critical hit, neutral effectiveness
/// (Weakness Policy stays), then Mimikyu-Busted and 1/8 of max HP at the Update.
#[test]
fn disguise_absorbs_a_hit() {
    assert_exact_parity("disguise-hit");
}

/// Mold Breaker ignores Disguise: full damage, critical hits, Weakness Policy.
#[test]
fn disguise_is_broken_by_mold_breaker() {
    assert_exact_parity("disguise-mold-breaker");
}

/// A two-hit move: the first hit is absorbed (Rocky Helmet still activates on 0 damage), the
/// Update between the hits busts the disguise, the second hit deals damage.
#[test]
fn disguise_busts_between_hits() {
    assert_exact_parity("disguise-multihit");
}

/// The confusion self-hit is a Move effect: Disguise absorbs it.
#[test]
fn disguise_absorbs_a_confusion_self_hit() {
    assert_exact_parity("disguise-confusion");
}

// ---- Ice Face ---------------------------------------------------------------------------------

/// A physical hit is absorbed and Eiscue becomes Eiscue-Noice; the faster Noice forme then
/// moves before Gardevoir (the queue is re-sorted), its Snowscape restores the face, and
/// Gardevoir's special move hits the restored Eiscue.
#[test]
fn ice_face_absorbs_a_physical_hit_and_snow_restores_it() {
    assert_exact_parity("ice-face-snowscape");
}

/// Eiscue-Noice stays Noice on the bench (permanent forme) and gets its face back when it
/// switches in during snow; the next physical hit is absorbed again.
#[test]
fn ice_face_is_restored_on_switch_in_during_snow() {
    assert_exact_parity("ice-face-switch-in");
}

/// The confusion self-hit has no category: Ice Face lets it through.
#[test]
fn ice_face_lets_a_confusion_self_hit_through() {
    assert_exact_parity("ice-face-confusion");
}

// ---- Stance Change ----------------------------------------------------------------------------

/// The Blade forme's SpA for the attack, its Defense for the hit it takes later in the turn;
/// fainted, it is Aegislash again.
#[test]
fn stance_change_takes_the_blade_forme_for_the_turn() {
    assert_exact_parity("stance-change-blade");
}

/// King's Shield returns to the Shield forme before protecting.
#[test]
fn stance_change_takes_the_shield_forme_for_kings_shield() {
    assert_exact_parity("stance-change-kings-shield");
}

/// The Blade forme is temporary: back to Aegislash on switching out.
#[test]
fn stance_change_reverts_on_switch_out() {
    assert_exact_parity("stance-change-switch-out");
}

// ---- Zero to Hero -----------------------------------------------------------------------------

/// Palafin switching out becomes Palafin-Hero on the bench.
#[test]
fn zero_to_hero_changes_forme_on_switch_out() {
    assert_exact_parity("zero-to-hero-switch-out");
}

/// Palafin-Hero stays Hero on the bench and attacks with the Hero forme's Attack once back.
#[test]
fn zero_to_hero_forme_is_permanent() {
    assert_exact_parity("zero-to-hero");
}

// ---- Schooling, Shields Down, Hunger Switch ------------------------------------------------------

/// School forme at switch-in; at the end of the turn Solo at or below a quarter of max HP
/// (the boundary included), School above; a fainted Wishiwashi is Solo again.
#[test]
fn schooling_follows_hp_at_the_residual() {
    assert_exact_parity("schooling-residual");
}

/// The School forme is temporary: Solo on the bench.
#[test]
fn schooling_reverts_on_switch_out() {
    assert_exact_parity("schooling-switch-out");
}

/// Hangry and back at each residual; the Hangry forme is temporary (Full Belly on the bench).
#[test]
fn hunger_switch_toggles_and_reverts() {
    assert_exact_parity("hunger-switch");
}

/// Minior-Meteor is immune to every status and to Yawn.
#[test]
fn shields_down_meteor_is_status_immune() {
    assert_exact_parity("shields-down-status");
}

/// At or below half HP the shields drop at the residual (plain Minior's core).
#[test]
fn shields_down_drops_at_half_hp() {
    assert_exact_parity("shields-down-residual");
}

/// Shields Down on a core colour other than plain Minior is refused where it would take the
/// Meteor forme: the colour to come back to (the set's species) is not in the state.
#[test]
fn shields_down_refuses_a_core_colour() {
    let json = r#"{
      "format": "gen9championsdoublescustomgame",
      "p1": {"team": [
        {"species": "Minior-Orange", "item": "", "ability": "Shields Down", "nature": "Hardy",
         "evs": {"hp": 32}, "moves": ["Calm Mind"], "level": 50},
        {"species": "Blissey", "item": "", "ability": "Honey Gather", "nature": "Bold",
         "evs": {"hp": 32}, "moves": ["Calm Mind"], "level": 50}], "order": "12"},
      "p2": {"team": [
        {"species": "Snorlax", "item": "", "ability": "Honey Gather", "nature": "Impish",
         "evs": {"hp": 32}, "moves": ["Calm Mind"], "level": 50},
        {"species": "Metagross", "item": "", "ability": "Honey Gather", "nature": "Bold",
         "evs": {"hp": 32}, "moves": ["Calm Mind"], "level": 50}], "order": "12"},
      "turn": {"p1": "move calmmind, move calmmind", "p2": "move calmmind, move calmmind"}
    }"#;
    let loaded = lab_scenario::load_scenario_str(json, &common::engine_dir()).unwrap();
    let error = lab_scenario::scenario_positions(&loaded).unwrap_err();
    assert!(error.contains("Shields Down on Minior-Orange"), "{error}");
}

/// A temporary forme cannot be a set's species (Showdown would keep it as the base species).
#[test]
fn temporary_formes_are_refused_as_set_species() {
    let team = r#"[{"species": "Aegislash-Blade", "item": "", "ability": "Stance Change",
        "nature": "Modest", "evs": {"hp": 32}, "moves": ["Hex"], "level": 50}]"#;
    let sets = lab_scenario::parse_team(team).unwrap();
    assert_eq!(
        lab_scenario::build_pokemon(&sets[0]).unwrap_err(),
        lab_scenario::SetProblem::TemporaryForme("Aegislash-Blade".into())
    );
}

/// The busted forme is permanent and keeps Disguise (inert on Mimikyu-Busted); max HP is the
/// same, the Weakness Policy is still held.
#[test]
fn busted_disguise_keeps_ability_and_item() {
    let fixture = common::fixture("disguise-hit");
    let (loaded, position) = common::start("disguise-hit", &fixture);
    let mut state = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let outcomes = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    let before = state.side(SideId::Two).party[0].clone();
    assert_eq!(before.species, species::MIMIKYU);
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        let mon = &state.side(SideId::Two).party[0];
        assert_eq!(mon.species, species::MIMIKYU_BUSTED);
        assert_eq!(mon.ability, abilities::DISGUISE);
        assert_eq!(mon.base_ability, abilities::DISGUISE);
        assert_eq!(mon.max_hp, before.max_hp);
        assert_eq!(mon.hp, before.max_hp - before.max_hp / 8);
        assert_eq!(mon.item, items::WEAKNESS_POLICY);
        state.reverse(&outcome.instructions);
    }
}
