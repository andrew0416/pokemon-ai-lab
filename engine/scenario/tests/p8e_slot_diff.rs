//! Baseline reconstruction is a full-state oracle in the same binary. Only the instruction
//! stream may change; ordered outcomes, exact probability bits, suspension and all state
//! fields (including hidden payloads) must match. Existing parity tests remain unchanged.

use lab_engine::instruction::Outcome;
use lab_engine::state::State;
use lab_engine::turn::slot_diff_observer::{self as observer, BaselineScope};
use lab_engine::turn::{EnumerateOptions, FactoredScope, RollMode};
use lab_scenario::{
    advance_order, load_scenario_file_as, run_decision_mid_turn_with, scenario_decision,
    scenario_positions_with,
};

fn compare<const N: usize>(name: &str, factored: bool) -> usize {
    let _scope = FactoredScope::new(factored);
    let options = EnumerateOptions {
        rolls: RollMode::Median,
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../oracle/scenarios/{name}.json"));
    let loaded = load_scenario_file_as::<N>(&path).unwrap();
    let positions = scenario_positions_with(&loaded, options).unwrap();
    let mut shortcuts = 0;
    for position in positions {
        let decision = scenario_decision(&loaded, &position).unwrap();
        let original = position.state;
        let mut state = original.clone();
        let reference = {
            let _baseline = BaselineScope::new();
            run_decision_mid_turn_with(
                &mut state,
                &position.order,
                &decision,
                &loaded.mid_turn,
                options,
            )
        };
        assert_eq!(state, original, "{name}: baseline input restored");
        observer::reset();
        let actual = run_decision_mid_turn_with(
            &mut state,
            &position.order,
            &decision,
            &loaded.mid_turn,
            options,
        );
        shortcuts += observer::counts().shortcut_slots;
        assert_eq!(state, original, "{name}: experiment input restored");
        match (reference, actual) {
            (Ok(reference), Ok(actual)) => {
                assert_eq!(
                    actual.len(),
                    reference.len(),
                    "{name}: ordered outcome count"
                );
                for (index, (reference, actual)) in reference.iter().zip(&actual).enumerate() {
                    assert_eq!(
                        actual.probability.to_bits(),
                        reference.probability.to_bits(),
                        "{name}/{index}: probability"
                    );
                    assert_eq!(
                        actual.suspension, reference.suspension,
                        "{name}/{index}: remaining queue"
                    );
                    let expected = end_state(&original, reference);
                    let end = end_state(&original, actual);
                    assert_eq!(end, expected, "{name}/{index}: every state field");
                    assert_eq!(end.position_hash(), expected.position_hash());
                    let mut expected_order = position.order.clone();
                    let mut actual_order = position.order.clone();
                    advance_order(&mut expected_order, &reference.instructions);
                    advance_order(&mut actual_order, &actual.instructions);
                    assert_eq!(
                        actual_order, expected_order,
                        "{name}/{index}: scenario party order"
                    );
                }
            }
            (Err(reference), Err(actual)) => {
                assert_eq!(
                    actual.to_string(),
                    reference.to_string(),
                    "{name}: error text"
                );
                assert_eq!(
                    format!("{actual:?}"),
                    format!("{reference:?}"),
                    "{name}: error variant"
                );
            }
            (reference, actual) => panic!("{name}: result mismatch {reference:?} / {actual:?}"),
        }
    }
    shortcuts
}

fn end_state<const N: usize>(start: &State<N>, outcome: &Outcome) -> State<N> {
    let mut state = start.clone();
    let mut hash = state.position_hash();
    for instruction in &outcome.instructions {
        hash = hash.wrapping_add(state.apply_hashed(instruction));
        assert_eq!(hash, state.position_hash());
    }
    let end = state.clone();
    state.reverse(&outcome.instructions);
    assert_eq!(&state, start, "all outcome instructions reverse");
    end
}

#[test]
fn real_turns_match_baseline_full_state_order_probability_and_suspension() {
    let names = [
        "single-hit",
        "spread-damage",
        "hypnosis-gravity",
        "ally-switch",
        "ally-switch-again",
        "ee-transform-switch-back",
        "ee-transform-faint",
        "ee-transform-mega",
        "ee-imposter-lead",
        "mega-tyranitar",
        "ko-replace",
        "o38-instruct-target",
        "y-baton-pass-copy",
    ];
    let mut shortcuts = 0;
    for factored in [false, true] {
        for name in names {
            shortcuts += compare::<2>(name, factored);
        }
        for name in [
            "ae-singles-hit",
            "ae-singles-switch",
            "vd-singles-ally-switch-aromatic-mist",
        ] {
            shortcuts += compare::<1>(name, factored);
        }
    }
    assert!(shortcuts > 0, "real enumeration must exercise the shortcut");
}
