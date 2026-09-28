//! Board P3a-incremental-hash: the position hash the turn engine keeps incrementally
//! (`State::position_hash`, updated by every instruction a run applies) equals the full
//! recomputation at every merged position, and tells positions apart: over every oracle
//! scenario's positions (Median and Extremes enumerations, a 16-sample `sample_turn`), and over
//! all the end states reached, two states share a hash exactly when they are equal.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use lab_engine::rules::Ruleset;
use lab_engine::state::State;
use lab_engine::turn::{sample_turn, verify_position_hashes, EnumerateOptions, RollMode};
use lab_scenario::{
    load_scenario_file, run_decision_with, scenario_decision, scenario_positions, Decision,
};

fn fingerprint(state: &State<2>) -> u64 {
    let mut hasher = DefaultHasher::new();
    state.hash(&mut hasher);
    hasher.finish()
}

fn scenario_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            // Scenarios are `<name>.json`; team files are `<name>.p1.json` and the like.
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            name.ends_with(".json") && name.matches('.').count() == 1
        })
        .collect();
    files.sort();
    files
}

#[test]
fn incremental_position_hashes_match_and_separate_positions() {
    verify_position_hashes(true);
    let ruleset = Ruleset::CHAMPIONS_MC;
    // Every end state reached: its position hash -> an independent fingerprint (SipHash of the
    // derived `Hash`) of the first state seen with it. Equal states have equal fingerprints;
    // two different states with one position hash would (but for a 2^-64 chance) differ in it.
    let mut seen: HashMap<u64, u64> = HashMap::new();
    let mut ends = 0usize;
    let mut positions_checked = 0usize;
    for path in scenario_files() {
        let Ok(loaded) = load_scenario_file(&path) else {
            continue;
        };
        // The setup turns' replay enumerates and merges too (checked by the same switch).
        let Ok(positions) = scenario_positions(&loaded) else {
            continue;
        };
        for position in &positions {
            let Ok(decision) = scenario_decision(&loaded, position) else {
                continue;
            };
            positions_checked += 1;
            for rolls in [RollMode::Median, RollMode::Extremes] {
                let mut state = position.state.clone();
                let Ok(outcomes) =
                    run_decision_with(&mut state, &decision, EnumerateOptions { rolls })
                else {
                    continue;
                };
                assert_eq!(state, position.state);
                for outcome in &outcomes {
                    let mut end = position.state.clone();
                    end.apply(&outcome.instructions);
                    let hash = end.position_hash();
                    let print = fingerprint(&end);
                    ends += 1;
                    let first = *seen.entry(hash).or_insert(print);
                    assert_eq!(
                        first,
                        print,
                        "{}: two different states share the position hash {hash:#x}",
                        path.display()
                    );
                }
            }
            if let Decision::Turn(choices) = decision {
                let mut state = position.state.clone();
                let _ = sample_turn(&mut state, ruleset, choices, 16, 7);
                assert_eq!(state, position.state);
            }
        }
    }
    eprintln!(
        "{positions_checked} positions, {ends} end states, {} distinct",
        seen.len()
    );
    assert!(positions_checked > 1000, "{positions_checked} positions");
    assert!(
        seen.len() > 5000,
        "{} distinct end states of {ends}",
        seen.len()
    );
}
