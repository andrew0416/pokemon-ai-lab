//! Mega Evolution parity (WORKPLAN F3): the `megaEvo` action runs before moves, changes
//! species/stats/ability (so Speed order and damage change), starts the new ability, and
//! spends the side's Mega budget. Fixtures made by `enumerate.cjs` on the fixed Showdown
//! commit; see `oracle/scenarios/mega-tyranitar*.json`.

mod common;

use common::assert_exact_parity;
use lab_engine::dex::{abilities, species};
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::enumerate_turn;
use lab_scenario::scenario_choices;

/// Sand Stream starts again on Mega Evolution and replaces Abomasnow's snow; Mega Tyranitar
/// (Speed 123) now moves before Gyarados (118).
#[test]
fn mega_tyranitar_matches_showdown_exactly() {
    assert_exact_parity("mega-tyranitar");
}

/// The same weather cannot be set again by an ability, so the patched 3-turn sand keeps its
/// duration through the Mega Evolution.
#[test]
fn mega_under_own_sand_matches_showdown_exactly() {
    assert_exact_parity("mega-tyranitar-sand");
}

/// Every outcome ends with Tyranitar as Tyranitar-Mega (stats recalculated from its set),
/// the Mega budget spent, and the other side's still open.
#[test]
fn mega_evolution_is_permanent_and_spends_the_side_budget() {
    let fixture = common::fixture("mega-tyranitar");
    let (loaded, position) = common::start("mega-tyranitar", &fixture);
    let mut state = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let outcomes = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    assert!(!outcomes.is_empty());
    let before = state.side(SideId::One).party[0].clone();
    assert_eq!(before.species, species::TYRANITAR);
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        let mon = &state.side(SideId::One).party[0];
        assert_eq!(mon.species, species::TYRANITAR_MEGA);
        assert_eq!(mon.ability, abilities::SAND_STREAM);
        assert_eq!(mon.base_ability, abilities::SAND_STREAM);
        assert_eq!(mon.types, species::TYRANITAR_MEGA.data().types);
        // Same HP formula (base 100), so max HP is unchanged; Atk/Def/SpD/Spe rise.
        assert_eq!(mon.max_hp, before.max_hp);
        assert_eq!(mon.stats, before.forme_as(species::TYRANITAR_MEGA).stats);
        assert!(mon.stats[0] > before.stats[0] && mon.stats[4] > before.stats[4]);
        assert!(state
            .side(SideId::One)
            .gimmicks_used
            .contains(Gimmick::Mega));
        assert!(state.side(SideId::Two).gimmicks_used.is_empty());
        state.reverse(&outcome.instructions);
    }
}
