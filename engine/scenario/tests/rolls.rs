//! Damage-roll modes (WORKPLAN F18): under `RollMode::Extremes` the engine branches only on
//! the minimum and maximum roll, exactly as the oracle's `--mode extremes` does, so the two
//! approximate distributions must agree exactly. Fixtures: `oracle/expected/<name>.extremes.json`.

mod common;

use std::collections::HashSet;

use common::{assert_extremes_parity, fixture, start};
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{canonical_json, run_decision_mid_turn_with, scenario_decision};

/// The canonical end states of the scenario's turn under `rolls`.
fn end_states(name: &str, rolls: RollMode) -> HashSet<String> {
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes = run_decision_mid_turn_with(
        &mut state,
        &position.order,
        &decision,
        &loaded.mid_turn,
        EnumerateOptions { rolls },
    )
    .unwrap();
    let mut out = HashSet::new();
    for o in &outcomes {
        state.apply(&o.instructions);
        out.insert(canonical_json(&state, &loaded.meta).unwrap());
        state.reverse(&o.instructions);
    }
    out
}

/// Every reduced mode's end states are a subset of the exact ones; Pessimistic and Median
/// leave one damage value per hit; Pessimistic for the attacker's side is its minimum roll
/// (Tyranitar keeps more HP than under the defender's pessimism).
#[test]
fn reduced_modes_are_subsets_of_the_exact_distribution() {
    let full = end_states("single-hit", RollMode::Full);
    for mode in [
        RollMode::Extremes,
        RollMode::Quartiles,
        RollMode::Median,
        RollMode::Pessimistic(SideId::One),
        RollMode::Pessimistic(SideId::Two),
    ] {
        let reduced = end_states("single-hit", mode);
        assert!(reduced.is_subset(&full), "{mode:?}");
        assert!(reduced.len() < full.len(), "{mode:?}");
    }
    let tyranitar_hp = |states: &HashSet<String>| -> Vec<i64> {
        let mut hps: Vec<i64> = states
            .iter()
            .map(|s| {
                let v: serde_json::Value = serde_json::from_str(s).unwrap();
                v["sides"][1]["pokemon"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["name"] == "Tyranitar")
                    .unwrap()["hp"]
                    .as_i64()
                    .unwrap()
            })
            .collect();
        hps.sort_unstable();
        hps.dedup();
        hps
    };
    // Rillaboom (side one) attacks Tyranitar (side two): side one's pessimism is the
    // minimum roll (more HP left), side two's the maximum.
    let ours = tyranitar_hp(&end_states(
        "single-hit",
        RollMode::Pessimistic(SideId::One),
    ));
    let theirs = tyranitar_hp(&end_states(
        "single-hit",
        RollMode::Pessimistic(SideId::Two),
    ));
    let all = tyranitar_hp(&full);
    assert_eq!(ours.iter().max(), all.iter().max(), "{ours:?} vs {all:?}");
    assert!(
        theirs.iter().max() < ours.iter().max(),
        "{theirs:?} vs {ours:?}"
    );
}

/// One damaging move (Grassy Glide) plus a miss/protect branch: the min and max roll map to
/// the same two damage values on both sides.
#[test]
fn single_hit_extremes_match_showdown() {
    assert_extremes_parity("single-hit");
}

/// A turn without damage rolls is unchanged by the mode.
#[test]
fn hypnosis_gravity_extremes_match_showdown() {
    assert_extremes_parity("hypnosis-gravity");
}
