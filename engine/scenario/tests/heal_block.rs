//! Heal Block and Psychic Noise (Opus T): the `healblock` volatile (5 turns, 2 from Psychic
//! Noise) disables `heal` moves and stops `battle.heal` (TryHeal) and the healing berries.
//! Fixtures from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Heal moves cannot be used, Sitrus Berry is not eaten, Mental Herb cures it at once.
#[test]
fn heal_block_stops_heal_moves_and_berries() {
    assert_exact_parity("heal-block");
}

/// Heal Block again on blocked targets counts as a failure for Stomping Tantrum (`onRestart`).
#[test]
fn heal_block_restart_fails_the_move_result() {
    assert_exact_parity("heal-block-restart");
}

/// Psychic Noise's Heal Block lasts 2 turns and stops Leftovers.
#[test]
fn psychic_noise_blocks_leftovers() {
    assert_exact_parity("psychic-noise");
}

/// Regenerator (`pokemon.heal`) still heals; Pollen Puff on an ally fails under Heal Block.
#[test]
fn heal_block_spares_regenerator_and_fails_ally_pollen_puff() {
    assert_exact_parity("heal-block-regenerator");
}
