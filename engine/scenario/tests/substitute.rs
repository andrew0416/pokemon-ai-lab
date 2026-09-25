//! Substitute (WORKPLAN F11): the move, the volatile with its HP (`Slot::substitute_hp`,
//! canonical `volatiles.substitute.hp`) and the substitute's `onTryPrimaryHit` routing; and
//! Double Shock with Showdown's `???` type (`Type::Unknown`), from the same session. Each
//! scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// A spread move hits a fresh substitute and the ally; a later fixed-damage move breaks what is
/// left of the substitute, or hits the Pokémon once it is gone.
#[test]
fn substitute_takes_spread_and_fixed_damage() {
    assert_exact_parity("substitute-spread");
}

/// Recoil and drain from a hit on a substitute: the damage is capped at its HP, the recoil is
/// applied inside `onTryPrimaryHit` (not again for `totalDamage`), the drain rounds up.
#[test]
fn substitute_recoil_and_drain() {
    assert_exact_parity("substitute-recoil-drain");
}

/// A status move is stopped by a standing substitute (and hits once it broke); an attack on
/// the substitute still gives its user a secondary's self boost.
#[test]
fn substitute_stops_status_moves() {
    assert_exact_parity("substitute-status");
}

/// Air Balloon pops when its holder's substitute is hit; a resist berry is neither eaten nor
/// applied, and Knock Off takes nothing, when the hit goes to the substitute.
#[test]
fn substitute_items() {
    assert_exact_parity("substitute-items");
}

/// Intimidate skips a Pokémon behind a substitute; a two-hit move's second hit reaches the
/// Pokémon once the first broke the substitute, and only that hit triggers Rocky Helmet.
#[test]
fn substitute_multihit_and_intimidate() {
    assert_exact_parity("substitute-multihit");
}

/// Substitute fails with one already up and at a quarter of the max HP or less; a new one ends
/// partial trapping; a move's self drops apply when it hits a substitute.
#[test]
fn substitute_failures_and_trap_removal() {
    assert_exact_parity("substitute-fail");
}

/// Rapid Spin's `onAfterSubDamage`: Leech Seed and the hazards go even when the hit went to a
/// substitute.
#[test]
fn substitute_rapid_spin() {
    assert_exact_parity("substitute-rapid-spin");
}

/// Ice Spinner's `onAfterSubDamage` clears the terrain.
#[test]
fn substitute_ice_spinner() {
    assert_exact_parity("substitute-ice-spinner");
}

/// Pollen Puff on an ally goes through its substitute; Aromatherapy skips an ally behind one;
/// Defog reaches a substitute's holder but lowers no evasion.
#[test]
fn substitute_bypass() {
    assert_exact_parity("substitute-bypass");
}

/// A status move the substitute stopped did not fail (`null`, not `false`): Stomping Tantrum
/// next turn is not doubled.
#[test]
fn substitute_stopped_move_is_no_failure() {
    assert_exact_parity("substitute-blocked-result");
}

/// Double Shock turns the user's Electric type into `???` (neutral both ways, no paralysis
/// immunity; canonical `???` / `???/Flying`) and fails with `null` without an Electric type
/// (Stomping Tantrum next turn is not doubled).
#[test]
fn double_shock_leaves_unknown_type() {
    assert_exact_parity("double-shock");
}
