//! V10 (staged serializer faithful): turns in which a value set by one queued action decides a
//! later one, so the staged oracle enumeration (`enumerate.cjs --staged`) carries it across a
//! `Battle.toJSON`/`fromJSON` round trip between the two actions. Each fixture is the plain
//! enumeration's (`enumerate.cjs`); `oracle/check-staged.cjs` shows the staged enumeration gives the
//! same distribution and `oracle/check-roundtrip.cjs` the same branch-by-branch state. Here the
//! engine is held to the same fixtures.

mod common;

use common::assert_exact_parity;

/// Champions `formeChange` sets `moveThisTurnResult = true` on Mega Evolution (the megaEvo action,
/// before every move); a Truant given by Entrainment later in the turn reads it (`onStart`: active
/// before the turn and "has acted"), so the Mega Gardevoir loafs instead of using Moonblast.
#[test]
fn mega_evolution_counts_as_an_action_for_truant() {
    assert_exact_parity("ll-staged-mega-truant");
}

/// Comeuppance reads the attacker record two actions later: the last Pokémon that damaged the
/// user this turn (`attackedBy`: `slot`, `damage`, `thisTurn`) is its target and 1.5x its damage
/// the move's; no damage this turn and it fails.
#[test]
fn comeuppance_reads_the_attackers_of_earlier_actions() {
    assert_exact_parity("ll-staged-comeuppance");
}

/// Quash sets the target's queued action to order 201 (after every move), which every later
/// re-sort keeps: the slower Stone Edge goes first and can knock the Quashed Garchomp out.
#[test]
fn quash_order_outlasts_the_resorts() {
    assert_exact_parity("ll-staged-quash-order");
}
