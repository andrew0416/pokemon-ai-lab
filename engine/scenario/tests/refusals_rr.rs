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

// ---- fixed by R1-trace-seeking (Opus MM; more in `abilities_trace_seek.rs`) -----------------

/// Trace facing only `notrace` foes copies nothing and keeps seeking (the hidden `traceseek`).
#[test]
fn trace_with_no_traceable_foe() {
    assert_exact_parity("rr-trace-no-traceable");
}

/// Alakazam-Mega's Trace starts as it Mega Evolves and copies one of two traceable foes at
/// random.
#[test]
fn mega_alakazam_trace() {
    assert_exact_parity("rr-mega-alakazam-trace");
}

/// Gastro Acid passed by Baton Pass: the newcomer's Trace never starts (its switch-in handler is
/// suppressed) and stays inert (Gastro Acid lasts as long as it stays).
#[test]
fn trace_under_passed_gastro_acid() {
    assert_exact_parity("rr-trace-gastro-acid-baton-pass");
}

// ---- still refused: Showdown's answer, for the board task that implements it --------------

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

/// Follow Me (Clefable, p2a) and Rage Powder (Ariados, p2b) at equal Speed: Showdown's
/// `compareRedirectOrder` puts the holder whose ability state started first (Clefable) first, so
/// Follow Me takes the move whoever moved first (was refused; board R4, `oo_redirection_tie.rs`).
#[test]
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
fn instruct_a_targeted_move() {
    assert_exact_parity("rr-instruct-target");
}

#[test]
fn instruct_a_quick_claw_holder() {
    assert_exact_parity("rr-instruct-quick-claw");
}

#[test]
fn instruct_a_quick_draw_holder() {
    assert_exact_parity("rr-instruct-quick-draw");
}

/// Inteleon's Snipe Shot aimed at Starmie follows it through Ally Switch (`getTarget` returns the
/// action's `originalTarget` while it is active; was refused; board R7, `oo_ally_switch_target.rs`).
#[test]
fn snipe_shot_after_ally_switch() {
    assert_exact_parity("rr-ally-switch-snipe-shot");
}

#[test]
fn encore_into_counter() {
    assert_exact_parity("rr-encore-counter");
}

/// An `--mode extremes` fixture: three hits with their rolls and critical hits are about
/// 6.6 million branches in full mode (`fullBranchEstimate`). Hits in `Side::party_order`
/// (board R9; `ss_party_order.rs` has the reordered cases).
#[test]
fn beat_up_bench_order() {
    assert_extremes_parity("rr-beat-up-bench");
}

/// Board R10: the first start after the battle start acts (`SideHistory::syrup_triggered`;
/// `ss_once_per_battle.rs` has the second start).
#[test]
fn supersweet_syrup_after_the_start() {
    assert_exact_parity("rr-supersweet-syrup-switch");
}

/// Recycle gives back a Grassy Seed used at the battle start; setItem's Start uses it again in the
/// Grassy Terrain (was refused; board R11, `oo_item_restart.rs`).
#[test]
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
fn copycat_a_multi_hit_move() {
    assert_exact_parity("rr-copycat-multihit");
}

#[test]
fn sleep_talk_with_a_multi_hit_move() {
    assert_exact_parity("rr-sleep-talk-multihit");
}

/// Fling knocks out Mega Victreebel, whose Innards Out knocks out Snorlax before the hit loop's
/// Update: Fling's condition runs on the 0-HP user (`setItem('')` fails, `lastItem` is set). Was
/// refused (and before the audit answered without `lastItem`); board R16,
/// `oo_fling_user_fainted.rs`.
#[test]
fn fling_user_fainted_by_innards_out() {
    assert_exact_parity("rr-fling-innards-out");
}

/// Trick-or-Treat's Curse Glitch: Snorlax in the second position gets its queued Curse aimed at -1
/// (its ally); the now Ghost Curse re-draws a random foe (was refused; board R17,
/// `oo_trick_or_treat_curse.rs`).
#[test]
fn trick_or_treat_curse_glitch() {
    assert_exact_parity("rr-trick-or-treat-curse-glitch");
}

#[test]
fn copycat_a_two_turn_move() {
    assert_exact_parity("rr-copycat-two-turn");
}

/// An `--mode extremes` fixture: the exact distribution (two damage rolls, two lock durations
/// and two random targets) is about 540,800 branches (`fullBranchEstimate`), over the oracle's
/// 500,000 limit.
#[test]
fn copycat_outrage() {
    assert_extremes_parity("rr-copycat-outrage");
}

#[test]
fn copycat_mirror_coat() {
    assert_exact_parity("rr-copycat-mirror-coat");
}

/// Teatime makes Lopunny (Klutz) eat its Sitrus Berry: the Eat event is skipped for a holder
/// ignoring its item, the berry is consumed (was refused; board R22, `oo_forced_eat_ignored_item.rs`).
#[test]
fn teatime_with_klutz() {
    assert_exact_parity("rr-teatime-klutz");
}

// ---- found by the audit, outside its scope (a mismatch, not a refusal) ------------------------

/// Instruct on a Pokémon whose last move is Counter: Showdown's `resolveAction(...)[0]` is the
/// `beforeTurnMove` action, so Counter never runs (no PP); the engine runs it.
#[test]
fn instruct_counter_runs_only_its_callback() {
    assert_exact_parity("rr-x-instruct-counter");
}
