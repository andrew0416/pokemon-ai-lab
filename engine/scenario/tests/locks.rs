//! Forced choices (WORKPLAN F9): a locked move (Outrage), the recharge turn (Hyper Beam),
//! Encore's override of a chosen move, and confusion when a locked move ends by fatigue.
//! Fixtures from Showdown's exact enumeration; every scenario reaches its position through a
//! setup turn replayed by the engine.

mod common;

use common::assert_exact_parity;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{locked_move, Locked};
use lab_engine::volatile::Volatile;
use lab_scenario::{scenario_decision, scenario_positions, Decision};

/// The forced second Outrage turn: no PP charged, a random foe, and a 2-turn lock ending
/// in confusion.
#[test]
fn locked_outrage_matches_showdown_exactly() {
    assert_exact_parity("outrage-lock");
}

/// The recharge turn after Hyper Beam: the user does nothing while it is hit.
#[test]
fn hyper_beam_recharge_matches_showdown_exactly() {
    assert_exact_parity("hyper-beam-recharge");
}

/// Encore overrides the target's chosen move by its last one, at the chosen move's priority.
#[test]
fn encore_override_matches_showdown_exactly() {
    assert_exact_parity("encore");
}

/// After the setup turn Dragonite is locked into Outrage (both real lock lengths are possible
/// positions), and any move choice is normalised to it.
#[test]
fn outrage_positions_are_locked() {
    let loaded = lab_scenario::load_scenario_file(
        common::engine_dir().join("oracle/scenarios/outrage-lock.json"),
    )
    .unwrap();
    let positions = scenario_positions(&loaded).unwrap();
    let dragonite = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    let mut lengths = std::collections::BTreeSet::new();
    for position in &positions {
        let locked = position
            .state
            .slot(dragonite)
            .volatiles
            .get(Volatile::LockedMove);
        assert!(locked.active, "Outrage did not lock");
        assert_eq!(locked.duration, 1, "one turn of the lock was spent");
        lengths.insert(locked.hidden);
        assert!(matches!(
            locked_move(&position.state, dragonite),
            Some(Locked::Move(id)) if id == lab_engine::dex::moves::OUTRAGE
        ));
        let Decision::Turn(choices) = scenario_decision(&loaded, position).unwrap() else {
            panic!("a turn decision");
        };
        assert!(matches!(
            choices[0][0],
            lab_engine::action::SlotAction::Move { index: 0, .. }
        ));
    }
    assert_eq!(lengths.into_iter().collect::<Vec<_>>(), vec![1, 2]);
}
