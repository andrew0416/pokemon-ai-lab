//! Faint replacement parity (WORKPLAN F5): after a turn with faints, the replacement decision
//! (`request: switch`) sends the newcomers in, clears the fainted Pokémon's `fnt`, runs the
//! newcomers' start handlers in Speed order (ties uniformly at random) and advances the turn
//! counter. The scenario reaches the position through a `setupTurns` entry replayed by the
//! engine (WORKPLAN F4: one switch-in path for the start, mid-turn switches and replacements).

mod common;

use common::assert_exact_parity;
use lab_engine::state::{SideId, Status};
use lab_scenario::{scenario_decision, scenario_positions, Decision};

#[test]
fn double_ko_replacement_matches_showdown_exactly() {
    assert_exact_parity("ko-replace");
}

/// The setup turn is deterministic (both KOs are guaranteed), so exactly one position waits
/// for replacements, with both fainted Pokémon marked `fnt`; the choice strings resolve
/// through Showdown's party order.
#[test]
fn setup_turn_leaves_one_position_awaiting_replacements() {
    let loaded = common::engine_dir().join("oracle/scenarios/ko-replace.json");
    let loaded = lab_scenario::load_scenario_file(loaded).unwrap();
    let positions = scenario_positions(&loaded).unwrap();
    assert_eq!(positions.len(), 1);
    let position = &positions[0];
    assert!((position.probability - 1.0).abs() < 1e-12);
    let state = &position.state;
    assert_eq!(state.turn, 1, "the turn counter waits for the replacements");
    for side in [SideId::One, SideId::Two] {
        let s = state.side(side);
        assert_eq!(s.slots[1].party_index, None);
        assert_eq!(s.slots[1].fainted_occupant, Some(1));
        assert_eq!(s.party[1].status, Status::Fainted);
        assert_eq!(s.party[1].hp, 0);
    }
    match scenario_decision(&loaded, position).unwrap() {
        Decision::Replacement(choices) => {
            assert_eq!(choices, [[None, Some(2)], [None, Some(2)]]);
        }
        other => panic!("{other:?}"),
    }
}
