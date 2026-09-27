//! Opus SS (wave 15 L1, board B45a): the hidden-state limit of pinned positions. OO's report
//! said a pinned scenario leaves `Slot::ability_order` at its default; it does not (the engine
//! replays a pinned scenario's setup turns and only filters their outcomes by the pinned
//! canonical states, so every hidden field is rebuilt). The limit is a different one: two
//! outcomes of a setup turn can differ only in hidden state, and neither the replay nor a pin can
//! tell which one the game (or the oracle's fixed seed) took. `ss-redirect-tie-hidden-order`
//! makes such a pair: two Snorlax of the same Speed switch out for two Lightning Rod Manectric,
//! the tied switch actions decide whose ability state started first (Showdown
//! `abilityState.effectOrder`), and that decides who draws the next turn's Thunderbolt. The
//! oracle's seed took the branch where Rod B came in first; the engine keeps both at 1/2.

mod common;

use common::{engine_dir, fixture, key};
use lab_engine::state::{SideId, SlotRef};
use lab_scenario::{
    canonical_value, load_scenario_file, run_decision_mid_turn, scenario_decision,
    scenario_positions, LoadedScenario, Position,
};
use serde_json::Value;

const NAME: &str = "ss-redirect-tie-hidden-order";

fn loaded() -> LoadedScenario {
    load_scenario_file(engine_dir().join(format!("oracle/scenarios/{NAME}.json"))).unwrap()
}

fn outcome_keys(loaded: &LoadedScenario, position: &Position) -> Vec<String> {
    let mut state = position.state.clone();
    let decision = scenario_decision(loaded, position).unwrap();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();
    let mut keys: Vec<String> = common::distribution(loaded, &mut state, &outcomes)
        .into_keys()
        .collect();
    keys.sort();
    keys
}

/// Both positions have the oracle's `before`; they differ only in `ability_order`, and each
/// gives one deterministic outcome: the oracle's is one of them.
#[test]
fn a_tied_switch_leaves_two_positions_the_canonical_state_cannot_tell_apart() {
    let loaded = loaded();
    let fixture = fixture(NAME);
    let before = key(&fixture["before"]);
    let positions: Vec<Position> = scenario_positions(&loaded)
        .unwrap()
        .into_iter()
        .filter(|p| key(&canonical_value(&p.state, &loaded.meta).unwrap()) == before)
        .collect();
    assert_eq!(positions.len(), 2);
    for p in &positions {
        assert!((p.probability - 0.5).abs() < 1e-12);
    }
    let order = |p: &Position| {
        [0, 1].map(|slot| {
            p.state
                .slot(SlotRef {
                    side: SideId::One,
                    slot,
                })
                .ability_order
        })
    };
    assert_ne!(order(&positions[0]), order(&positions[1]));
    let a = outcome_keys(&loaded, &positions[0]);
    let b = outcome_keys(&loaded, &positions[1]);
    assert_eq!((a.len(), b.len()), (1, 1));
    assert_ne!(a, b, "the hidden order decides who draws the Thunderbolt");
    let oracle: Vec<String> = common::oracle_distribution(&fixture).into_keys().collect();
    assert!(oracle == a || oracle == b);
}

/// Pinning the setup turn's canonical state keeps both positions (renormalized to 1/2 each):
/// a pin restores what the replay rebuilds, not what the canonical state leaves out.
#[test]
fn a_pin_keeps_both_positions() {
    let mut loaded = loaded();
    let fixture = fixture(NAME);
    let pin: Value = fixture["before"].clone();
    loaded.setup_states = vec![Some(pin)];
    let positions = scenario_positions(&loaded).unwrap();
    assert_eq!(positions.len(), 2);
    for p in &positions {
        assert!((p.probability - 0.5).abs() < 1e-12);
    }
}
