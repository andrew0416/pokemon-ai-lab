//! R5d: existing Full oracle fixtures plus non-canonical state/instruction checks.
//! The fixtures are unchanged; these tests add no new oracle expectations.

mod common;

use lab_engine::state::{PokemonRef, SideId};
use lab_scenario::{run_decision_mid_turn, scenario_decision};

fn assert_overlay_roundtrip(name: &str) {
    let fixture = common::fixture(name);
    assert_eq!(fixture["mode"], "full");
    let (loaded, position) = common::start(name, &fixture);
    let before = position.state.clone();
    let source = PokemonRef {
        side: SideId::One,
        party: 0,
    };
    assert!(
        (0..2).all(|slot| before.side(SideId::One).slots[slot].party_index != Some(source.party))
    );
    let decision = scenario_decision(&loaded, &position).unwrap();
    let mut state = before.clone();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();
    assert!(!outcomes.is_empty());
    assert_eq!(state, before, "enumeration restores its input");
    assert_eq!(format!("{state:?}"), format!("{before:?}"));
    let oracle = common::oracle_distribution(&fixture);
    let engine = common::distribution(&loaded, &mut state, &outcomes);
    assert_eq!(engine.len(), oracle.len());
    for (key, probability) in oracle {
        assert!((engine.get(&key).expect("oracle state exists") - probability).abs() < 1e-12);
    }
    for outcome in outcomes {
        assert!(outcome.suspension.is_none());
        let mut hash = before.position_hash();
        for instruction in &outcome.instructions {
            hash = hash.wrapping_add(state.apply_hashed(instruction));
            assert_eq!(hash, state.position_hash(), "{instruction:?}");
        }
        assert_eq!(
            state.pokemon(source),
            before.pokemon(source),
            "the inactive source remains unchanged"
        );
        state.reverse(&outcome.instructions);
        assert_eq!(state, before);
        assert_eq!(format!("{state:?}"), format!("{before:?}"));
        assert_eq!(state.position_hash(), before.position_hash());
    }
}

#[test]
fn two_unnerve_occupants_match_full_oracle_and_reverse_every_result() {
    assert_overlay_roundtrip("ab-future-sight-absent-user-two-occupants");
}

#[test]
fn cotton_down_displaced_occupant_matches_full_oracle_and_reverses_every_result() {
    assert_overlay_roundtrip("ab-future-sight-absent-user-cotton-down-displaced");
}
