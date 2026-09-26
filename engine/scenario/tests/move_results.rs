//! `moveThisTurnResult` of a two-turn move's charging turn. Showdown's charge `onTryMove`
//! returns `null` and `useMove` stores it, so the next turn's `moveLastTurnResult` is `null`,
//! not `false` (Stomping Tantrum and Temper Flare double only on `=== false`). The history is
//! hidden state (not in the canonical output), and a Pokémon that charged is locked into its
//! move the next turn (its own result then replaces the charge's), so the difference never
//! reaches an end-of-turn state; this test pins the recording itself.

mod common;

use lab_engine::state::{MoveResult, SideId, SlotRef};
use lab_scenario::{run_decision_mid_turn, scenario_decision};

#[test]
fn a_charging_turn_leaves_a_null_move_result() {
    let fixture = common::fixture("solar-beam-charge");
    let (loaded, position) = common::start("solar-beam-charge", &fixture);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let outcomes =
        run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn).unwrap();
    assert!(!outcomes.is_empty());
    // Roserade (p1a) charged Solar Beam; the turn ended, so the result moved to `last`.
    let roserade = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        let history = state.slot(roserade).history;
        state.reverse(&outcome.instructions);
        assert_eq!(history.move_last_turn_result, MoveResult::Null);
    }
}
