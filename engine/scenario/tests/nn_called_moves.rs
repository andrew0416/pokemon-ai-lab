//! Called moves and re-resolved actions (Opus NN: boards R21, R14, R8, R6, B33, B34): a move
//! Copycat or Sleep Talk runs from inside another move, and an action the queue resolves again
//! mid-turn (Champions Encore's `changeAction`, Instruct's `resolveAction`). Each scenario is
//! checked against Showdown (`engine/oracle/expected/nn-*.turn.json`, `enumerate.cjs --mode full`,
//! `--staged` where the plain enumeration is too long).

mod common;

use common::{assert_exact_parity, assert_extremes_parity};

/// Copycat calls Solar Beam in its charge turn (setup): the called move starts `twoturnmove` aimed
/// at the target it drew. Next turn Umbreon is locked into Solar Beam, which it does not know: it
/// fires at the stored target, spends no PP and makes Solar Beam its last move.
#[test]
fn copycat_solar_beam_second_turn() {
    assert_exact_parity("nn-copycat-solar-beam-lock");
}

/// Copycat calls Outrage (setup): Umbreon is locked into it (`lockedmove`: move outrage, a hidden
/// true duration of 2 or 3). Next turn its locked Outrage, a move it does not know, hits the
/// only foe without PP; a lock whose true duration ran out ends and confuses it. (`--staged`:
/// 452,896 branches, 2,204 outcomes.)
#[test]
fn copycat_outrage_second_turn() {
    assert_exact_parity("nn-copycat-outrage-lock");
}

/// A locked Pokémon's move (Outrage's second turn) aimed at a Pressure holder: Showdown's source
/// effect for it is the `lockedmove` condition, which has no PP, so Pressure takes none.
#[test]
fn pressure_takes_no_pp_on_a_locked_turn() {
    assert_exact_parity("nn-pressure-locked-outrage");
}

/// Sleep Talk calls Double Hit (R14): the called move's second hit is a stage of its own; after
/// it, the called move's tail (Life Orb recoil), Sleep Talk's hit-loop tail and AfterMove with
/// Double Hit active.
#[test]
fn sleep_talk_double_hit_with_life_orb() {
    assert_exact_parity("nn-sleep-talk-double-hit-life-orb");
}

/// Copycat calls Bullet Seed (R14): up to four more hits, each a stage, then Copycat's tail. An
/// `--mode extremes` fixture (up to five damage rolls in one action).
#[test]
fn copycat_bullet_seed() {
    assert_extremes_parity("nn-copycat-bullet-seed");
}

/// Champions Encore replaces Snorlax's queued Curse with Counter and queues Counter's
/// `beforeTurnMove` too (R8): it runs next, so the physical hit that follows is countered.
#[test]
fn encore_into_counter_queues_its_callback() {
    assert_exact_parity("nn-encore-counter-hit");
}

/// Encore into Focus Punch queues its `priorityChargeMove` (R8): the `focuspunch` condition
/// starts, Foul Play breaks its focus and Focus Punch fails.
#[test]
fn encore_into_focus_punch_queues_its_charge() {
    assert_exact_parity("nn-encore-focus-punch-hit");
}

/// Encore into Beak Blast queues its `priorityChargeMove` (R8): the contact attacker that hits it
/// is burned; Beak Blast fires at the target drawn when the action was replaced.
#[test]
fn encore_into_beak_blast_queues_its_charge() {
    assert_exact_parity("nn-encore-beak-blast-hit");
}

/// Instruct on Umbreon after its locked Raging Fury, a move Copycat called and it does not know
/// (R6): no slot, so the PP check passes; the repeat runs without PP while Umbreon is still
/// locked and fails (`cant nopp`) once its lock ended.
#[test]
fn instruct_a_called_raging_fury() {
    assert_exact_parity("nn-instruct-called-raging-fury");
}

/// Instruct on Slowking after its Chilly Reception (B33): `resolveAction(...)[0]` is the
/// `priorityChargeMove` action, so only the `chillyreception` condition starts; Chilly Reception
/// does not run again (no PP spent).
#[test]
fn instruct_chilly_reception_runs_only_its_charge_callback() {
    assert_exact_parity("nn-instruct-chilly-reception");
}

/// Instruct on Snorlax while it charges Focus Punch (B34): Instruct fails on a target with the
/// `focuspunch` condition, so its last move (Curse) is not repeated.
#[test]
fn instruct_fails_on_a_focus_punch_charge() {
    assert_exact_parity("nn-instruct-focus-punch-charge");
}

/// Copycat calls Uproar (setup): Umbreon keeps Uproar's `uproar` volatile, whose `onLockMove`
/// locks it into a move it does not know (the engine rejected that choice before).
#[test]
fn copycat_uproar_second_turn() {
    assert_exact_parity("nn-copycat-uproar-lock");
}
