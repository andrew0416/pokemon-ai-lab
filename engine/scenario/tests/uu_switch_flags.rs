//! Switch requests raised where a decision meets `switchFlag` (Opus UU, wave 15 lane L3): a
//! replacement's Emergency Exit (board R3). Fixtures from Showdown's exact enumeration.

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
