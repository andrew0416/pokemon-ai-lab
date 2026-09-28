//! Board V15-frozen-turns-canonical (VD): Champions' freeze keeps a turn counter
//! (`data/mods/champions/conditions.ts` `frz`: `startTime = 3`, `time--` before each move,
//! thawed at 0 or on a 1/4 draw), but the canonical form (`canonical.cjs`, `canonical.rs`)
//! writes `statusTime` only for sleep, so states that differ only in a freeze's remaining turns
//! merge and a parity check cannot tell them apart.
//!
//! `vd-frozen-time-merge`: the plain oracle report has 20 outcomes; with `statusTime` written for
//! `frz` as well (`vd-frozen-time-merge.frz-time.json`, made by a copy of `canonical.cjs` with
//! that one change) it has 30. The engine keeps the counter (`Pokemon::status_turns`), so the
//! second test compares it too.

mod common;

use std::collections::HashMap;

use common::{assert_exact_parity, engine_dir, key, start};
use lab_engine::state::{SideId, Status};
use lab_scenario::{canonical_value, run_decision_mid_turn, scenario_decision};
use serde_json::Value;

/// The plain canonical comparison (freeze turns merged) matches.
#[test]
fn frozen_time_merge_plain() {
    assert_exact_parity("vd-frozen-time-merge");
}

/// With the freeze's remaining turns in the key, the engine's distribution is Showdown's too
/// (30 states where the plain form has 20).
#[test]
fn frozen_time_with_the_counter() {
    let name = "vd-frozen-time-merge";
    let plain = common::fixture(name);
    let text =
        std::fs::read_to_string(engine_dir().join(format!("oracle/expected/{name}.frz-time.json")))
            .unwrap();
    let timed: Value = serde_json::from_str(&text).unwrap();
    let (loaded, position) = start(name, &plain);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();

    let mut engine: HashMap<String, f64> = HashMap::new();
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        let mut value = canonical_value(&state, &loaded.meta).unwrap();
        for side in [SideId::One, SideId::Two] {
            let meta = &loaded.meta.sides[side.index()];
            for mon in value["sides"][side.index()]["pokemon"]
                .as_array_mut()
                .unwrap()
            {
                let index = meta.party_index(mon["name"].as_str().unwrap()).unwrap();
                let member = &state.side(side).party[usize::from(index)];
                if member.status == Status::Freeze {
                    mon.as_object_mut()
                        .unwrap()
                        .insert("statusTime".into(), member.status_turns.into());
                }
            }
        }
        state.reverse(&outcome.instructions);
        *engine.entry(key(&value)).or_default() += outcome.probability;
    }
    let oracle = lab_scenario::parity::report_distribution(&timed).unwrap();
    assert_eq!(oracle.len(), 30);
    assert_eq!(common::oracle_distribution(&plain).len(), 20);
    assert_eq!(engine.len(), oracle.len(), "number of outcomes");
    for (k, p) in &oracle {
        let q = engine
            .get(k)
            .unwrap_or_else(|| panic!("engine lacks an oracle outcome:\n{k}"));
        assert!((p - q).abs() < 1e-12, "p {p} vs engine {q} for\n{k}");
    }
}
