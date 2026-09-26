//! Opus X unit B3: Showdown's `switchFlag === true` checks (Eject Button's `onAfterMoveSecondary`,
//! Eject Pack's `onUseItem`) go over `getAllActive()`, which includes a Pokémon at 0 HP whose
//! faint is not processed yet, while the engine's go over `all_alive()`. `faint()` clears the
//! flag, so the only way to such a Pokémon with the flag is Emergency Exit's `runEvent` after
//! the user's own recoil, crash or Life Orb, which has no HP guard. The oracle scenario
//! `x-switchflag-unprocessed-faint` shows it: Golisopod's Head Smash recoil knocks it out, its
//! Emergency Exit flags it anyway, and Weak Armor Chansey's Eject Pack then stays at AfterMove.
//! The engine refuses that Emergency Exit (`moves::user_emergency_exit`), so the difference
//! cannot be reached; this test keeps the refusal in place until the checks include such a
//! Pokémon.

mod common;

use lab_scenario::{
    load_scenario_file, run_decision_mid_turn, scenario_decision, scenario_positions,
};

#[test]
fn emergency_exit_after_a_recoil_knockout_is_refused() {
    let loaded = load_scenario_file(
        common::engine_dir().join("oracle/scenarios/x-switchflag-unprocessed-faint.json"),
    )
    .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    match run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn) {
        Err(what) => {
            assert!(
                what.contains("Emergency Exit on a user its recoil knocked out"),
                "{what}"
            )
        }
        Ok(outcomes) => panic!("expected a refusal, got {} outcomes", outcomes.len()),
    }
}
