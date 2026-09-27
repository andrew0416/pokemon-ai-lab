//! Refusal audit (board R0-refusal-audit, `engine/REFUSALS.md`): one scenario per refusal a
//! Champions-legal battle can reach (`engine/oracle/scenarios/rr-*.json`, standard species,
//! abilities, items and learnable moves only), with Showdown's answer for it
//! (`engine/oracle/expected/rr-*.turn.json`, `enumerate.cjs --mode full`; two turns too large
//! for that have `rr-*.extremes.json`).
//!
//! `every_reachable_refusal_is_refused` runs each scenario and checks that the engine still
//! refuses it with the listed message; the parity test of each is ignored until its board task
//! implements the mechanic (then the refusal row goes and the parity test is un-ignored). The
//! first tests are refusals this audit fixed (a few lines each, checked against the oracle).

mod common;

use common::{assert_exact_parity, engine_dir, start};
use lab_scenario::{run_decision_mid_turn, scenario_decision};
use serde_json::Value;

/// The scenario's oracle fixture: the exact one, else the `--mode extremes` one (turns whose
/// exact distribution is too large to enumerate).
fn any_fixture(name: &str) -> Value {
    let expected = engine_dir().join("oracle/expected");
    let path = [".turn.json", ".extremes.json"]
        .iter()
        .map(|suffix| expected.join(format!("{name}{suffix}")))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("{name}: no oracle fixture"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// (scenario, text the refusal must contain, board task).
const REFUSED: &[(&str, &str, &str)] = &[(
    "rr-fling-innards-out",
    "Fling's user fainted before its item was thrown",
    "R16-fling-user-fainted",
)];

/// Each repro still reaches its refusal from the oracle fixture's position (the scenario is
/// the one the fixture was made from).
#[test]
fn every_reachable_refusal_is_refused() {
    for &(name, expected, board) in REFUSED {
        let fixture = any_fixture(name);
        let (loaded, position) = start(name, &fixture);
        let mut state = position.state.clone();
        let decision = scenario_decision(&loaded, &position).unwrap();
        match run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn) {
            Err(why) => assert!(
                why.contains(expected),
                "{name} ({board}): refused with `{why}`, expected `{expected}`"
            ),
            Ok(outcomes) => panic!(
                "{name} ({board}): no longer refused ({} outcomes): un-ignore its parity test and \
                 drop the row",
                outcomes.len()
            ),
        }
    }
}

// ---- fixed by the audit ---------------------------------------------------------------------

/// Struggle (and Transform) are `failinstruct`: Instruct fails before looking for the move in the
/// target's slots (it was refused as a move the target does not know).
#[test]
fn instruct_fails_on_a_last_move_that_is_failinstruct() {
    assert_exact_parity("rr-instruct-struggle");
}

/// Spite takes the last PP after the choice: `cant ... nopp`, the move fails without `lastMove`
/// (was refused as "no PP left when used").
#[test]
fn a_move_whose_pp_ran_out_after_the_choice_fails() {
    assert_exact_parity("rr-spite-no-pp");
}

/// Toxic Spikes poison a Synchronize holder; Synchronize ignores Toxic Spikes, so the foe lead is
/// not poisoned (was refused).
#[test]
fn synchronize_ignores_toxic_spikes() {
    assert_exact_parity("rr-toxic-spikes-synchronize");
}

/// Future Sight hits an Eject Button holder; Eject Button ignores future moves (was refused).
#[test]
fn eject_button_ignores_a_future_move() {
    assert_exact_parity("rr-future-sight-eject-button");
}

// ---- still refused: Showdown's answer, for the board task that implements it --------------

/// The `run_move_inner` guard never fired here (the hit loop's faint processing clears the
/// Fling volatile first) and the engine answered without the thrown item's `lastItem`; the
/// Update now refuses it.
#[test]
#[ignore = "refused: Fling's user fainted before its item was thrown (board R16-fling-user-fainted)"]
fn fling_user_fainted_by_innards_out() {
    assert_exact_parity("rr-fling-innards-out");
}
