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
