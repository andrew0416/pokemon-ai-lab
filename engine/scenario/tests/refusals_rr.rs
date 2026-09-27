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

use common::{assert_exact_parity, assert_extremes_parity, engine_dir, start};
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
const REFUSED: &[(&str, &str, &str)] = &[
    (
        "rr-trace-no-traceable",
        "Trace has no traceable foe and would keep seeking on later Updates",
        "R1-trace-seeking",
    ),
    (
        "rr-mega-alakazam-trace",
        "Alakazam-Mega: ability Trace",
        "R1-trace-seeking",
    ),
    (
        "rr-trace-gastro-acid-baton-pass",
        "Gardevoir: ability Trace",
        "R1-trace-seeking",
    ),
    (
        "rr-hazard-order",
        "switching into hazards whose order (Showdown effectOrder) decides the outcome",
        "R2-hazard-effect-order",
    ),
    (
        "rr-emergency-exit-replacement",
        "Emergency Exit of a replacement hit by entry hazards",
        "R3-emergency-exit-replacement",
    ),
    (
        "rr-redirect-tie",
        "redirection tie between Clefable and Ariados",
        "R4-redirection-tie",
    ),
    (
        "rr-future-sight-user-left",
        "hitting after its user left the field",
        "R5-future-move-edges",
    ),
    (
        "rr-future-sight-red-card",
        "Future Sight hitting a holder of Red Card",
        "R5-future-move-edges",
    ),
    (
        "rr-instruct-target",
        "Instruct repeating Dragon Claw (its lastMoveTargetLoc is not kept)",
        "R6-instruct",
    ),
    (
        "rr-instruct-quick-claw",
        "Instruct on a Quick Claw holder",
        "R6-instruct",
    ),
    (
        "rr-instruct-quick-draw",
        "Instruct on a Quick Draw holder",
        "R6-instruct",
    ),
    (
        "rr-ally-switch-snipe-shot",
        "Snipe Shot aimed at a side whose Pokémon Ally Switch swapped",
        "R7-ally-switch-target",
    ),
    (
        "rr-encore-counter",
        "Encore replacing a queued action with Counter",
        "R8-encore-edges",
    ),
    (
        "rr-beat-up-bench",
        "Beat Up with benched allies of different power",
        "R9-beat-up-order",
    ),
    (
        "rr-supersweet-syrup-switch",
        "Supersweet Syrup after the battle start",
        "R10-once-per-battle-flags",
    ),
    (
        "rr-recycle-seed",
        "Recycle restoring Grassy Seed (its onStart)",
        "R11-item-restart",
    ),
    (
        "rr-cute-charm-undecided-gender",
        "with an undecided gender (give the sets a gender)",
        "R13-attract-gender",
    ),
    (
        "rr-attract-undecided-gender",
        "Attract with Snorlax of undecided gender",
        "R13-attract-gender",
    ),
    (
        "rr-rivalry-undecided-gender",
        "Rivalry next to Luxray of undecided gender",
        "R13-attract-gender",
    ),
    (
        "rr-rivalry-switch-in-undecided-gender",
        "Luxray: Rivalry next to a Pokémon of undecided gender",
        "R13-attract-gender",
    ),
    (
        "rr-copycat-multihit",
        "Double Hit called by Copycat: a multi-hit called move",
        "R14-called-multi-hit",
    ),
    (
        "rr-sleep-talk-multihit",
        "Sleep Talk calling Double Hit (a multi-hit move)",
        "R14-called-multi-hit",
    ),
    (
        "rr-fling-innards-out",
        "Fling's user fainted before its item was thrown",
        "R16-fling-user-fainted",
    ),
    (
        "rr-trick-or-treat-curse-glitch",
        "Trick-or-Treat's Curse Glitch",
        "R17-trick-or-treat-curse",
    ),
    (
        "rr-copycat-two-turn",
        "Copycat calling Solar Beam (a two-turn move)",
        "R21-copycat-called-moves",
    ),
    (
        "rr-copycat-outrage",
        "Copycat calling Outrage (a lock on the called move)",
        "R21-copycat-called-moves",
    ),
    (
        "rr-copycat-mirror-coat",
        "Copycat calling Mirror Coat (queue actions of its own)",
        "R21-copycat-called-moves",
    ),
    (
        "rr-teatime-klutz",
        "Sitrus Berry eaten by force while its holder ignores its item",
        "R22-forced-eat-ignored-item",
    ),
];

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

#[test]
#[ignore = "refused: Trace has no traceable foe and would keep seeking on later Updates (board R1-trace-seeking)"]
fn trace_with_no_traceable_foe() {
    assert_exact_parity("rr-trace-no-traceable");
}

#[test]
#[ignore = "refused: {}: ability {} ({}) — Trace as a Mega's ability (board R1-trace-seeking)"]
fn mega_alakazam_trace() {
    assert_exact_parity("rr-mega-alakazam-trace");
}

#[test]
#[ignore = "refused: {}: ability {} ({}) — a Trace that never started (board R1-trace-seeking)"]
fn trace_under_passed_gastro_acid() {
    assert_exact_parity("rr-trace-gastro-acid-baton-pass");
}

#[test]
#[ignore = "refused: {} switching into hazards whose order (Showdown effectOrder) decides the outcome (board R2-hazard-effect-order)"]
fn hazard_order() {
    assert_exact_parity("rr-hazard-order");
}

#[test]
#[ignore = "refused: Emergency Exit of a replacement hit by entry hazards (board R3-emergency-exit-replacement)"]
fn emergency_exit_of_a_replacement() {
    assert_exact_parity("rr-emergency-exit-replacement");
}

#[test]
#[ignore = "refused: redirection tie between {} and {} (Showdown breaks it by effectOrder) (board R4-redirection-tie)"]
fn redirection_tie() {
    assert_exact_parity("rr-redirect-tie");
}

#[test]
#[ignore = "refused: {} of {} hitting after its user left the field (board R5-future-move-edges)"]
fn future_sight_after_its_user_left() {
    assert_exact_parity("rr-future-sight-user-left");
}

#[test]
#[ignore = "refused: {} hitting a holder of {} (board R5-future-move-edges)"]
fn future_sight_on_a_red_card_holder() {
    assert_exact_parity("rr-future-sight-red-card");
}

#[test]
#[ignore = "refused: Instruct repeating {} (its lastMoveTargetLoc is not kept) (board R6-instruct)"]
fn instruct_a_targeted_move() {
    assert_exact_parity("rr-instruct-target");
}

#[test]
#[ignore = "refused: Instruct on a Quick Claw holder (board R6-instruct)"]
fn instruct_a_quick_claw_holder() {
    assert_exact_parity("rr-instruct-quick-claw");
}

#[test]
#[ignore = "refused: Instruct on a Quick Draw holder (board R6-instruct)"]
fn instruct_a_quick_draw_holder() {
    assert_exact_parity("rr-instruct-quick-draw");
}

#[test]
#[ignore = "refused: {} aimed at a side whose Pokémon Ally Switch swapped (it tracks its original target) (board R7-ally-switch-target)"]
fn snipe_shot_after_ally_switch() {
    assert_exact_parity("rr-ally-switch-snipe-shot");
}

#[test]
#[ignore = "refused: Encore replacing a queued action with {} (a callback action it would queue) (board R8-encore-edges)"]
fn encore_into_counter() {
    assert_exact_parity("rr-encore-counter");
}

/// An `--mode extremes` fixture: three hits with their rolls and critical hits are about
/// 6.6 million branches in full mode (`fullBranchEstimate`).
#[test]
#[ignore = "refused: Beat Up with benched allies of different power {} (board R9-beat-up-order)"]
fn beat_up_bench_order() {
    assert_extremes_parity("rr-beat-up-bench");
}

#[test]
#[ignore = "refused: {} after the battle start (its once-per-battle flag is not in the state) (board R10-once-per-battle-flags)"]
fn supersweet_syrup_after_the_start() {
    assert_exact_parity("rr-supersweet-syrup-switch");
}

#[test]
#[ignore = "refused: Recycle restoring {} (its onStart) (board R11-item-restart)"]
fn recycle_a_seed() {
    assert_exact_parity("rr-recycle-seed");
}

#[test]
#[ignore = "refused: Attract between {} and {} with an undecided gender (board R13-attract-gender)"]
fn cute_charm_with_undecided_genders() {
    assert_exact_parity("rr-cute-charm-undecided-gender");
}

#[test]
#[ignore = "refused: {} with {} of undecided gender (board R13-attract-gender)"]
fn attract_with_undecided_genders() {
    assert_exact_parity("rr-attract-undecided-gender");
}

#[test]
#[ignore = "refused: Rivalry next to {} of undecided gender (board R13-attract-gender)"]
fn rivalry_with_undecided_genders() {
    assert_exact_parity("rr-rivalry-undecided-gender");
}

#[test]
#[ignore = "refused: {}: Rivalry next to a Pokémon of undecided gender (board R13-attract-gender)"]
fn rivalry_switching_in_with_undecided_genders() {
    assert_exact_parity("rr-rivalry-switch-in-undecided-gender");
}

#[test]
#[ignore = "refused: {} called by {}: a multi-hit called move (board R14-called-multi-hit)"]
fn copycat_a_multi_hit_move() {
    assert_exact_parity("rr-copycat-multihit");
}

#[test]
#[ignore = "refused: Sleep Talk calling {} (a multi-hit move) (board R14-called-multi-hit)"]
fn sleep_talk_with_a_multi_hit_move() {
    assert_exact_parity("rr-sleep-talk-multihit");
}

/// The `run_move_inner` guard never fired here (the hit loop's faint processing clears the
/// Fling volatile first) and the engine answered without the thrown item's `lastItem`; the
/// Update now refuses it.
#[test]
#[ignore = "refused: Fling's user fainted before its item was thrown (board R16-fling-user-fainted)"]
fn fling_user_fainted_by_innards_out() {
    assert_exact_parity("rr-fling-innards-out");
}

#[test]
#[ignore = "refused: Trick-or-Treat's Curse Glitch (board R17-trick-or-treat-curse)"]
fn trick_or_treat_curse_glitch() {
    assert_exact_parity("rr-trick-or-treat-curse-glitch");
}

#[test]
#[ignore = "refused: Copycat calling {} — a two-turn move (board R21-copycat-called-moves)"]
fn copycat_a_two_turn_move() {
    assert_exact_parity("rr-copycat-two-turn");
}

/// An `--mode extremes` fixture: the exact distribution (two damage rolls, two lock durations
/// and two random targets) is about 540,800 branches (`fullBranchEstimate`), over the oracle's
/// 500,000 limit.
#[test]
#[ignore = "refused: Copycat calling {} — a lock on the called move (board R21-copycat-called-moves)"]
fn copycat_outrage() {
    assert_extremes_parity("rr-copycat-outrage");
}

#[test]
#[ignore = "refused: Copycat calling {} — queue actions of its own (board R21-copycat-called-moves)"]
fn copycat_mirror_coat() {
    assert_exact_parity("rr-copycat-mirror-coat");
}

#[test]
#[ignore = "refused: {} eaten by force while its holder ignores its item (board R22-forced-eat-ignored-item)"]
fn teatime_with_klutz() {
    assert_exact_parity("rr-teatime-klutz");
}

// ---- found by the audit, outside its scope (a mismatch, not a refusal) ------------------------

/// Instruct on a Pokémon whose last move is Counter: Showdown's `resolveAction(...)[0]` is the
/// `beforeTurnMove` action, so Counter never runs (no PP); the engine runs it.
#[test]
#[ignore = "mismatch: Instruct repeating a move with a beforeTurnCallback runs only the callback in Showdown (not on the board yet)"]
fn instruct_counter_runs_only_its_callback() {
    assert_exact_parity("rr-x-instruct-counter");
}
