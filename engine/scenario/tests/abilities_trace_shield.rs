//! Parity of Trace with its holder's Ability Shield (Opus X unit B9): `trace.onStart` checks
//! `pokemon.hasItem('Ability Shield')`, the effective item.

mod common;

use common::assert_exact_parity;

/// An effective shield sets `effectState.seek = false`: Trace never copies and stays Trace.
#[test]
fn trace_with_ability_shield_keeps_trace_matches_showdown() {
    assert_exact_parity("x-trace-shield");
}

/// Under Magic Room the shield is ignored: Trace copies a random adjacent foe's ability (the
/// shield's `onSetAbility` is skipped too).
#[test]
fn trace_ignores_ability_shield_under_magic_room_matches_showdown() {
    assert_exact_parity("x-trace-shield-magic-room");
}
