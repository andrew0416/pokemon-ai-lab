#![allow(dead_code)]
use lab_engine::{
    state::State,
    turn::{EnumerateOptions, RollMode, Suspension},
};
use lab_scenario::{load_scenario_str_as, scenario_positions_with};
use std::path::PathBuf;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn toy<const N: usize>() -> State<N> {
    let mon = |species| {
        serde_json::json!({"species": species, "ability":"Honey Gather",
        "nature":"Serious", "evs":{}, "moves":["Harden"], "level":50})
    };
    let order = if N == 1 { "1" } else { "12" };
    let format = if N == 1 {
        lab_scenario::SINGLES_FORMAT
    } else {
        lab_scenario::DOUBLES_FORMAT
    };
    let json = serde_json::json!({"format":format,
        "p1":{"team":[mon("Talonflame"),mon("Snorlax"),mon("Ditto")],"order":order},
        "p2":{"team":[mon("Swampert"),mon("Excadrill"),mon("Pikachu")],"order":order}});
    let loaded = load_scenario_str_as::<N>(&json.to_string(), &root()).unwrap();
    let mut state = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap()
    .remove(0)
    .state;
    // One legal Harden per live active: no huge reserve-switch matrix in logic checks.
    for side in &mut state.sides {
        for mon in side.party.iter_mut().skip(N) {
            mon.hp = 0;
        }
    }
    state
}

pub fn suspended() -> (State<2>, Suspension) {
    suspended_named("eject-button-uturn")
}

pub fn suspended_named(name: &str) -> (State<2>, Suspension) {
    let loaded =
        lab_scenario::load_scenario_file(root().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
    let position = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap()
    .remove(0);
    let lab_scenario::Decision::Turn(pair) =
        lab_scenario::scenario_decision(&loaded, &position).unwrap()
    else {
        panic!("expected ordinary fixture turn")
    };
    let mut state = position.state;
    let outcomes = lab_engine::turn::enumerate_turn_with(
        &mut state,
        lab_engine::rules::Ruleset::CHAMPIONS_MC,
        pair,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap();
    let outcome = outcomes
        .into_iter()
        .find(|outcome| outcome.suspension.is_some())
        .expect("suspended fixture");
    state.apply(&outcome.instructions);
    (state, outcome.suspension.unwrap())
}
