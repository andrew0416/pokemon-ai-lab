//! P12 observer-only proof using SetVolatile instructions from actual scenarios.
//! It distinguishes enumeration counters from the measured instruction replay.
#[cfg(not(feature = "experiment-volatile-hash-update-observer"))]
fn main() {
    panic!("P12 probe requires the separate scenario observer feature");
}

#[cfg(feature = "experiment-volatile-hash-update-observer")]
fn main() {
    proof::run();
}

#[cfg(feature = "experiment-volatile-hash-update-observer")]
mod proof {
    use lab_engine::instruction::Instruction;
    use lab_engine::turn::{verify_position_hashes, EnumerateOptions, FactoredScope, RollMode};
    use lab_engine::volatile::hash_update_observer as observe;
    use lab_scenario::{load_scenario_file, run_decision_mid_turn_with, scenario_decision, scenario_positions};
    use serde_json::json;

    pub fn run() {
        let engine = std::path::PathBuf::from(std::env::args().nth(1).expect("engine directory"));
        let _scope = FactoredScope::new(false);
        verify_position_hashes(true);
        let mut positions = 0u64;
        let mut outcomes_seen = 0u64;
        let mut updates = 0u64;
        let mut original_locations = 0u64;
        let mut candidate_locations = 0u64;
        let mut original_ranks = 0u64;
        let mut candidate_ranks = 0u64;
        let mut enumeration_locations = 0u64;
        let mut enumeration_ranks = 0u64;
        let cases = ["ff-encore-protect-stall", "lum-persim-confusion", "stockpile", "double-hit",
                     "uturn-pause", "eject-button-uturn"];
        for name in cases {
            let loaded = load_scenario_file(engine.join(format!("oracle/scenarios/{name}.json"))).unwrap();
            for position in scenario_positions(&loaded).unwrap() {
                positions += 1;
                let decision = scenario_decision(&loaded, &position).unwrap();
                let mut state = position.state.clone();
                observe::reset();
                let outcomes = run_decision_mid_turn_with(&mut state, &position.order, &decision,
                    &loaded.mid_turn, EnumerateOptions { rolls: RollMode::Median }).unwrap();
                let counts = observe::counts();
                enumeration_locations += counts.location_queries as u64;
                enumeration_ranks += counts.rank_queries as u64;
                assert_eq!(state, position.state);
                for outcome in outcomes {
                    outcomes_seen += 1;
                    let mut actual = position.state.clone();
                    let mut original = position.state.clone();
                    let mut hash = actual.position_hash();
                    for instruction in &outcome.instructions {
                        observe::reset();
                        let before = original.instruction_hash(instruction);
                        original.apply_one(instruction);
                        let expected_delta = original.instruction_hash(instruction).wrapping_sub(before);
                        let reference_counts = observe::counts();
                        observe::reset();
                        let delta = actual.apply_hashed(instruction);
                        let candidate_counts = observe::counts();
                        if matches!(instruction, Instruction::SetVolatile { .. }) {
                            updates += 1;
                            assert_eq!(reference_counts.location_queries, 3);
                            assert_eq!(candidate_counts.location_queries, 1);
                            assert!(candidate_counts.rank_queries <= 1);
                            assert!(reference_counts.rank_queries >= candidate_counts.rank_queries);
                            original_locations += reference_counts.location_queries as u64;
                            candidate_locations += candidate_counts.location_queries as u64;
                            original_ranks += reference_counts.rank_queries as u64;
                            candidate_ranks += candidate_counts.rank_queries as u64;
                        }
                        assert_eq!(delta, expected_delta);
                        hash = hash.wrapping_add(delta);
                        assert_eq!(hash, actual.position_hash());
                        assert_eq!(actual, original);
                    }
                    actual.reverse(&outcome.instructions);
                    assert_eq!(actual, position.state);
                }
            }
        }
        assert!(positions >= 6 && outcomes_seen > 0 && updates > 0);
        assert!(enumeration_locations > 0 && original_ranks > candidate_ranks);
        println!("{}", json!({"schema":1,"kind":"p12-real-instruction-proof","cases":cases,
            "positions":positions,"outcomes":outcomes_seen,"volatile_updates":updates,
            "original_locations":original_locations,"candidate_locations":candidate_locations,
            "original_ranks":original_ranks,"candidate_ranks":candidate_ranks,
            "enumeration_locations":enumeration_locations,"enumeration_ranks":enumeration_ranks,
            "full_state_and_hash_equal":true,"rollback_equal":true}));
    }
}
