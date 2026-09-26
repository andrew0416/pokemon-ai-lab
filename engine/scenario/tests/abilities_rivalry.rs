//! Parity of Rivalry (Opus S unit 8: `onBasePower` with `Pokemon::gender`) with Showdown, and
//! the loader's gender (the set's, else the species' fixed one).

mod common;

use common::assert_exact_parity;
use lab_engine::dex::Gender;
use lab_engine::state::SideId;
use lab_engine::turn::TurnError;
use lab_scenario::{load_scenario_file, run_decision, scenario_decision, scenario_positions};

/// 0.75x against the other gender, 1.25x against the same.
#[test]
fn rivalry_matches_showdown() {
    assert_exact_parity("s-rivalry");
}

/// The set's gender, else the species' fixed one; genderless species.
#[test]
fn the_loader_resolves_gender() {
    let loaded =
        load_scenario_file(common::engine_dir().join("oracle/scenarios/s-rivalry.json")).unwrap();
    let genders = |side: SideId| -> Vec<Gender> {
        loaded.state.side(side).party[..2]
            .iter()
            .map(|m| m.gender)
            .collect()
    };
    // Haxorus (set M), Nidoqueen (female-only species).
    assert_eq!(genders(SideId::One), [Gender::Male, Gender::Female]);
    // Magnezone (genderless), Blissey (female-only).
    assert_eq!(genders(SideId::Two), [Gender::Genderless, Gender::Female]);
}

/// A gender Showdown would draw at random is refused next to Rivalry.
#[test]
fn rivalry_with_an_undecided_gender_is_unsupported() {
    let loaded =
        load_scenario_file(common::engine_dir().join("oracle/scenarios/s-rivalry-undecided.json"))
            .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    match run_decision(&mut state, &decision) {
        Err(TurnError::Unsupported(what)) => assert!(what.contains("undecided gender"), "{what}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
