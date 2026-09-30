//! Exact records for separate P12 OFF/ON binaries. No internal runtime selector.
mod common;

use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::turn::{
    sample_turn, verify_position_hashes, EnumerateOptions, FactoredScope, RollMode,
};
use lab_engine::Doubles;
use lab_scenario::{
    load_scenario_file, run_decision_mid_turn_with, scenario_choices, scenario_decision,
    scenario_positions,
};
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::Write;

fn record_file(name: &str) -> Option<File> {
    std::env::var_os("LAB_P12_RECORDS").map(|directory| {
        let path = std::path::PathBuf::from(directory).join(format!("{name}.jsonl"));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
    })
}

fn emit(file: &mut Option<File>, value: Value) {
    if let Some(file) = file {
        writeln!(file, "{value}").unwrap();
    }
}

fn results(before: &Doubles, outcomes: &[Outcome]) -> Vec<Value> {
    outcomes
        .iter()
        .map(|outcome| {
            let mut applied = before.clone();
            let mut reference = before.clone();
            let mut hash = before.position_hash();
            for instruction in &outcome.instructions {
                let old_cell = reference.instruction_hash(instruction);
                reference.apply_one(instruction);
                let expected = reference
                    .instruction_hash(instruction)
                    .wrapping_sub(old_cell);
                let actual = applied.apply_hashed(instruction);
                assert_eq!(actual, expected);
                hash = hash.wrapping_add(actual);
                assert_eq!(hash, applied.position_hash());
                assert_eq!(applied, reference);
            }
            let record = json!({
                "probability_bits": outcome.probability.to_bits(),
                "instructions": format!("{:?}", outcome.instructions),
                "suspension": format!("{:?}", outcome.suspension),
                "state": format!("{applied:?}"), "hash": hash,
            });
            applied.reverse(&outcome.instructions);
            assert_eq!(applied, *before);
            assert_eq!(format!("{applied:?}"), format!("{before:?}"));
            record
        })
        .collect()
}

#[test]
fn bounded_turn_records_preserve_exact_outputs_and_rollback() {
    verify_position_hashes(true);
    let mut file = record_file("turns");
    let mut positions_seen = 0;
    let mut outcomes_seen = 0;
    #[cfg(feature = "experiment-volatile-hash-update-observer")]
    lab_engine::volatile::hash_update_observer::reset();
    for (name, rolls, factored) in [
        ("single-hit", RollMode::Full, false),
        ("single-hit", RollMode::Full, true),
        ("ff-encore-protect-stall", RollMode::Median, false),
        ("lum-persim-confusion", RollMode::Median, false),
        ("stockpile", RollMode::Median, false),
        ("double-hit", RollMode::Median, false),
        ("eject-button-uturn", RollMode::Median, false),
        ("uturn-pause", RollMode::Median, false),
    ] {
        let _scope = FactoredScope::new(factored);
        let loaded =
            load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json")))
                .unwrap();
        for (index, position) in scenario_positions(&loaded).unwrap().into_iter().enumerate() {
            positions_seen += 1;
            let decision = scenario_decision(&loaded, &position).unwrap();
            let mut state = position.state.clone();
            let outcomes = run_decision_mid_turn_with(
                &mut state,
                &position.order,
                &decision,
                &loaded.mid_turn,
                EnumerateOptions { rolls },
            )
            .unwrap();
            assert_eq!(state, position.state);
            assert_eq!(format!("{state:?}"), format!("{:?}", position.state));
            outcomes_seen += outcomes.len();
            emit(
                &mut file,
                json!({"case":name,"position":index,"rolls":format!("{rolls:?}"),
                "factored":factored,"before":format!("{:?}",position.state),
                "outcomes":results(&position.state,&outcomes)}),
            );
        }
    }
    assert!(positions_seen >= 8 && outcomes_seen > 8);
    emit(
        &mut file,
        json!({"kind":"coverage","positions":positions_seen,"outcomes":outcomes_seen}),
    );
    #[cfg(feature = "experiment-volatile-hash-update-observer")]
    assert!(lab_engine::volatile::hash_update_observer::counts().location_queries > 0);
}

#[test]
fn errors_and_seeded_samples_have_exact_records_and_restore_inputs() {
    verify_position_hashes(true);
    let mut file = record_file("errors-samples");
    let loaded = load_scenario_file(
        common::engine_dir().join("oracle/scenarios/aa-power-construct-faint.json"),
    )
    .unwrap();
    let mut errors = 0;
    let mut successes = 0;
    for (index, position) in scenario_positions(&loaded).unwrap().into_iter().enumerate() {
        let decision = scenario_decision(&loaded, &position).unwrap();
        let mut state = position.state.clone();
        let result = run_decision_mid_turn_with(
            &mut state,
            &position.order,
            &decision,
            &loaded.mid_turn,
            EnumerateOptions {
                rolls: RollMode::Median,
            },
        );
        assert_eq!(state, position.state);
        assert_eq!(format!("{state:?}"), format!("{:?}", position.state));
        let value = match result {
            Ok(outcomes) => {
                successes += 1;
                json!({"outcomes":results(&position.state,&outcomes)})
            }
            Err(error) => {
                errors += 1;
                json!({"error":error.to_string()})
            }
        };
        emit(
            &mut file,
            json!({"case":"aa-power-construct-faint","position":index,
            "before":format!("{:?}",position.state),"result":value}),
        );
    }
    assert!(errors > 0 && successes > 0);
    let loaded =
        load_scenario_file(common::engine_dir().join("oracle/scenarios/single-hit.json")).unwrap();
    let mut samples = 0;
    for (index, position) in scenario_positions(&loaded).unwrap().into_iter().enumerate() {
        let choices = scenario_choices(&loaded, &position.state).unwrap();
        for seed in [0, 7, 42] {
            let mut state = position.state.clone();
            let outcomes =
                sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, 4, seed).unwrap();
            assert_eq!(state, position.state);
            samples += 1;
            emit(
                &mut file,
                json!({"case":"sample","position":index,"seed":seed,
                "outcomes":results(&position.state,&outcomes)}),
            );
        }
    }
    emit(
        &mut file,
        json!({"kind":"coverage","errors":errors,"successes":successes,"samples":samples}),
    );
}
