//! Board II-t1-singles-parity (VD): the doubles-minded paths of the turn engine at N = 1
//! (`State<1>`, singles custom game) against oracle fixtures: moves that fail in singles
//! (Follow Me, Quash, After You: `activePerHalf === 1`), moves with an ally target and no ally
//! (Helping Hand, Aromatic Mist, Coaching, Ally Switch), spread moves with one target (no spread
//! modifier), Reflect at 1/2 in singles, Dragon Darts' smart target and Beat Up's party count.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::parity::{compare, engine_distribution, report_distribution};
use lab_scenario::{
    canonical_json, load_scenario_file_as, run_decision_mid_turn_with, scenario_decision,
    scenario_positions,
};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Exact parity of a singles scenario with its fixture (`<name>.turn.json`, or
/// `<name>.extremes.json` under `RollMode::Extremes`).
fn assert_singles_parity(name: &str, extremes: bool) -> usize {
    let file = if extremes {
        format!("oracle/expected/{name}.extremes.json")
    } else {
        format!("oracle/expected/{name}.turn.json")
    };
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(engine_dir().join(file)).unwrap()).unwrap();
    let loaded =
        load_scenario_file_as::<1>(engine_dir().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
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
    let decision = scenario_decision(&loaded, &position).unwrap();
    let mut state = position.state.clone();
    let options = EnumerateOptions {
        rolls: if extremes {
            RollMode::Extremes
        } else {
            RollMode::Full
        },
    };
    let outcomes = run_decision_mid_turn_with(
        &mut state,
        &position.order,
        &decision,
        &loaded.mid_turn,
        options,
    )
    .unwrap();
    let engine: HashMap<String, f64> =
        engine_distribution(&loaded.meta, &mut state, &outcomes).unwrap();
    let oracle = report_distribution(&fixture).unwrap();
    let c = compare(&engine, &oracle);
    assert!(c.exact(1e-9), "{name}: {c:?}");
    engine.len()
}

#[test]
fn follow_me_and_quash_fail_in_singles() {
    assert_eq!(
        assert_singles_parity("vd-singles-follow-me-quash", false),
        1
    );
}

#[test]
fn helping_hand_and_after_you_in_singles() {
    assert_singles_parity("vd-singles-helping-hand-after-you", false);
}

#[test]
fn ally_switch_and_aromatic_mist_in_singles() {
    assert_eq!(
        assert_singles_parity("vd-singles-ally-switch-aromatic-mist", false),
        1
    );
}

#[test]
fn spread_move_and_reflect_in_singles() {
    assert_singles_parity("vd-singles-spread-reflect", false);
}

#[test]
fn coaching_and_rock_slide_in_singles() {
    assert_singles_parity("vd-singles-coaching-rock-slide", false);
}

#[test]
fn dragon_darts_and_beat_up_in_singles() {
    assert_singles_parity("vd-singles-dragon-darts-beat-up", true);
}
