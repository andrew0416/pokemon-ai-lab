//! Parity of Stalwart and Propeller Tail (Opus S unit 6: `move.tracksTarget`) with Showdown:
//! each scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;
use lab_engine::turn::TurnError;
use lab_scenario::{load_scenario_file, run_decision, scenario_decision, scenario_positions};

/// Follow Me and Storm Drain do not draw a Stalwart or Propeller Tail holder's move.
#[test]
fn stalwart_and_propeller_tail_ignore_redirection() {
    assert_exact_parity("s-stalwart-propeller-tail");
}

/// After Ally Switch, Showdown keeps aiming at the original target (`getTarget`), which the
/// engine does not model: refused.
#[test]
fn stalwart_after_ally_switch_is_unsupported() {
    let name = "s-stalwart-ally-switch";
    let loaded =
        load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    match run_decision(&mut state, &decision) {
        Err(TurnError::Unsupported(what)) => {
            assert!(what.contains("tracks its original target"), "{what}")
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
