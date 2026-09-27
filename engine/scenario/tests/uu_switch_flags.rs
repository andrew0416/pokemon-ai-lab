//! Switch requests raised where a decision meets `switchFlag` (Opus UU, wave 15 lane L3): a
//! replacement's Emergency Exit (board R3), a future move whose user left the field (R5).
//! Fixtures from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// A replacement Golisopod the Stealth Rock takes to half: `runAction('runSwitch')`'s tail
/// asks for another switch before the turn ends (the replacement suspends; turn stays 1).
#[test]
fn emergency_exit_of_a_replacement() {
    assert_exact_parity("rr-emergency-exit-replacement");
}

/// The same, answered with Snorlax: the rocks hit it and only `endTurn` is left (turn 2).
#[test]
fn emergency_exit_of_a_replacement_resumed() {
    assert_exact_parity("uu-emergency-exit-replacement-resume");
}

/// Both sides replace with Golisopod at equal Speed, both taken to half: only the first queued
/// `runSwitch` action's Pokémon gets the Emergency Exit check (`insertChoice` breaks the tie at
/// random), so one side (1/2 each) is asked for a switch.
#[test]
fn emergency_exit_of_two_replacements_at_equal_speed() {
    assert_exact_parity("uu-emergency-exit-two-replacements-tie");
}

/// The faster replacement's `runSwitch` action is first: only it switches out.
#[test]
fn emergency_exit_of_two_replacements_faster_first() {
    assert_exact_parity("uu-emergency-exit-two-replacements-fast");
}

/// Future Sight hits after Slowking switched out: the benched user's stored stats (board R5a).
#[test]
fn future_sight_after_its_user_left() {
    assert_exact_parity("rr-future-sight-user-left");
}

/// The same with Slowking holding Twisted Spoon (Regenerator) at +1 SpA before it left: the
/// item, the ability and the stage are ignored from the bench.
#[test]
fn future_sight_after_its_user_left_ignores_its_item_and_stages() {
    assert_exact_parity("uu-future-sight-user-left-item");
}

/// Future Sight hits after its user fainted with no bench (its position stays empty).
#[test]
fn future_sight_after_its_user_fainted() {
    assert_exact_parity("uu-future-sight-user-fainted");
}

/// Future Sight hits a Red Card holder: the card drags Slowking out after the residual (the
/// phazing step of the residual action; board R5b).
#[test]
fn future_sight_on_a_red_card_holder() {
    assert_exact_parity("rr-future-sight-red-card");
}

/// From the bench the user is not active: the Red Card stays unused.
#[test]
fn future_sight_from_the_bench_on_a_red_card_holder() {
    assert_exact_parity("uu-future-sight-user-left-red-card");
}
