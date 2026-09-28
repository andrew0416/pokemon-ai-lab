//! Board II-singles-loader: singles scenarios (`State<1>`) through the same loader, turn engine
//! and canonical form as doubles, against oracle fixtures: Battle Stadium Singles
//! (`gen9championsbssregmc`, team preview keeps 3, Adjust Level = 50) and the singles custom game
//! (`gen9championscustomgame`, every member brought).

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use lab_engine::state::SideId;
use lab_scenario::parity::{compare, engine_distribution, report_distribution};
use lab_scenario::{
    canonical_json, canonical_json_hidden, load_scenario_file, load_scenario_file_as,
    run_decision_mid_turn, scenario_decision, scenario_positions, state_from_canonical, LoadError,
    LoadedScenario, Position,
};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture(name: &str) -> Value {
    let path = engine_dir().join(format!("oracle/expected/{name}.turn.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn load(name: &str) -> LoadedScenario<1> {
    load_scenario_file_as::<1>(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap()
}

fn start(name: &str, fixture: &Value) -> (LoadedScenario<1>, Position<1>) {
    let loaded = load(name);
    let before = serde_json::to_string(&fixture["before"]).unwrap();
    let position = scenario_positions(&loaded)
        .unwrap()
        .into_iter()
        .find(|p| {
            let v: Value =
                serde_json::from_str(&canonical_json(&p.state, &loaded.meta).unwrap()).unwrap();
            serde_json::to_string(&v).unwrap() == before
        })
        .expect("a position matches the oracle's before");
    (loaded, position)
}

fn assert_parity(name: &str) -> usize {
    let fx = fixture(name);
    let (loaded, position) = start(name, &fx);
    let decision = scenario_decision(&loaded, &position).unwrap();
    let mut state = position.state.clone();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();
    let engine: HashMap<String, f64> =
        engine_distribution(&loaded.meta, &mut state, &outcomes).unwrap();
    let oracle = report_distribution(&fx).unwrap();
    let c = compare(&engine, &oracle);
    assert!(c.exact(1e-9), "{name}: {c:?}");
    engine.len()
}

#[test]
fn battle_stadium_singles_hit_matches_the_oracle() {
    assert_eq!(assert_parity("ae-singles-hit"), 406);
    let loaded = load("ae-singles-hit");
    // Team preview "213" keeps Garchomp, Machamp, Chansey (3 of 4) at level 50.
    let names: Vec<&str> = loaded.meta.sides[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["Garchomp", "Machamp", "Chansey"]);
    assert_eq!(loaded.meta.sides[1].members.len(), 3);
    assert!(loaded.state.sides[0].party[..3]
        .iter()
        .all(|m| m.level == 50));
    assert!(loaded.state.sides[0].party[3].species.is_none());
    assert_eq!(loaded.state.sides[0].slots[0].party_index, Some(0));
}

#[test]
fn singles_switch_matches_the_oracle() {
    assert_eq!(assert_parity("ae-singles-switch"), 14);
}

/// A singles scenario is not a doubles one and vice versa: each loader refuses the other's
/// format by name.
#[test]
fn formats_and_slot_counts_must_agree() {
    let singles = engine_dir().join("oracle/scenarios/ae-singles-hit.json");
    match load_scenario_file(&singles) {
        Err(LoadError::Unsupported {
            field: "format", ..
        }) => {}
        other => panic!("{other:?}"),
    }
    let doubles = engine_dir().join("oracle/scenarios/single-hit.json");
    match load_scenario_file_as::<1>(&doubles) {
        Err(LoadError::Unsupported {
            field: "format", ..
        }) => {}
        other => panic!("{other:?}"),
    }
}

/// The hand-built position path (PY2) works in singles too.
#[test]
fn singles_positions_rebuild_from_canonical() {
    let fx = fixture("ae-singles-hit");
    let (loaded, position) = start("ae-singles-hit", &fx);
    let rebuilt = state_from_canonical(&loaded.state, &loaded.meta, &fx["before"]).unwrap();
    assert_eq!(
        canonical_json(&rebuilt.state, &loaded.meta).unwrap(),
        canonical_json(&position.state, &loaded.meta).unwrap()
    );
    for o in fx["outcomes"].as_array().unwrap().iter().take(50) {
        state_from_canonical(&loaded.state, &loaded.meta, &o["state"]).unwrap();
    }
    let hidden =
        canonical_json_hidden(&position.state, &loaded.meta, Some(&position.order)).unwrap();
    let back = state_from_canonical(
        &position.state,
        &loaded.meta,
        &serde_json::from_str(&hidden).unwrap(),
    )
    .unwrap();
    assert_eq!(back.state, position.state);
    assert_eq!(back.order[SideId::One.index()], position.order[0]);
}
