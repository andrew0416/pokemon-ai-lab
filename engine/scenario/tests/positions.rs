//! `scenario_positions_filtered`: the filter after each setup turn prunes what the next turn
//! replays, and what survives keeps its joint probability (the likelihood lab-plan's opponent
//! model ② weighs believed teams by).
mod common;

use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{
    load_scenario_file, load_scenario_str, scenario_positions, scenario_positions_consistent,
    scenario_positions_filtered, LoadedScenario,
};

const FULL: EnumerateOptions = EnumerateOptions {
    rolls: RollMode::Full,
};

fn load(name: &str) -> LoadedScenario {
    load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap()
}

#[test]
fn identity_filter_replays_like_scenario_positions() {
    let loaded = load("double-shock");
    let all = scenario_positions(&loaded).unwrap();
    let mut turns = Vec::new();
    let same = scenario_positions_filtered(&loaded, FULL, &mut |turn, positions| {
        turns.push((turn, positions.len()));
        positions
    })
    .unwrap();
    assert_eq!(same, all);
    assert_eq!(turns.len(), loaded.setup_turns.len());
    assert_eq!(turns[0], (1, all.len()));
    assert!(all.len() > 1, "the fixture's setup turn must branch");
}

#[test]
fn a_filter_keeps_the_joint_probability_of_what_it_passes() {
    let loaded = load("double-shock");
    let all = scenario_positions(&loaded).unwrap();
    let mut kept_mass = 0.0;
    let some = scenario_positions_filtered(&loaded, FULL, &mut |turn, mut positions| {
        assert_eq!(turn, 1);
        positions.sort_by(|a, b| b.probability.total_cmp(&a.probability));
        positions.truncate(1);
        kept_mass = positions[0].probability;
        positions
    })
    .unwrap();
    assert_eq!(some.len(), 1);
    assert!(kept_mass > 0.0 && kept_mass < 1.0);
    assert!((some[0].probability - kept_mass).abs() < 1e-12);
    assert!(all.contains(&some[0]));
    // A filter that passes nothing ends the replay with nothing.
    let none = scenario_positions_filtered(&loaded, FULL, &mut |_, _| Vec::new()).unwrap();
    assert!(none.is_empty());
}

#[test]
fn an_illegal_setup_choice_fails_the_replay_unless_the_teams_are_only_believed() {
    // Double Shock's fixture with a setup choice no position accepts (a switch to slot 6 when
    // the side has fewer Pokémon): the strict replay reports it, the consistent replay drops
    // every position instead (the choices made contradict the believed teams there).
    let path = common::engine_dir().join("oracle/scenarios/double-shock.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["setupTurns"][0][0] = serde_json::Value::String("switch 6, move harden".into());
    let loaded = load_scenario_str(&json.to_string(), path.parent().unwrap()).unwrap();
    let error =
        scenario_positions_filtered(&loaded, FULL, &mut |_, positions| positions).unwrap_err();
    assert!(error.starts_with("setup turn 1:"), "{error}");
    let mut turns = 0;
    let none = scenario_positions_consistent(&loaded, FULL, &mut |_, positions| {
        turns += 1;
        positions
    })
    .unwrap();
    assert!(none.is_empty());
    assert_eq!(turns, 1, "the filter still runs on the (empty) turn");
}
