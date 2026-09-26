//! Parity of the Ruin abilities (Opus Q unit 3: `onAnyModifyAtk` / `onAnyModifySpA` /
//! `onAnyModifyDef` / `onAnyModifySpD`) with Showdown: each scenario's exact outcome
//! distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Tablets of Ruin lowers its ally's Attack too; Sword of Ruin spares its holder.
#[test]
fn tablets_and_sword_of_ruin_match_showdown() {
    assert_exact_parity("q-ruin-tablets-sword");
}

/// Vessel of Ruin lowers its ally's Special Attack; Beads of Ruin does not meet a special move
/// against Defense.
#[test]
fn vessel_and_beads_of_ruin_match_showdown() {
    assert_exact_parity("q-ruin-vessel-beads");
}

/// Tablets of Ruin (priority 0) chained after Huge Power (5) and Choice Band (1).
#[test]
fn tablets_of_ruin_chains_after_huge_power_and_choice_band_like_showdown() {
    assert_exact_parity("q-ruin-chain");
}

/// Two holders of the same Ruin ability: one 0.75 only.
#[test]
fn two_swords_of_ruin_drop_once_like_showdown() {
    assert_exact_parity("q-ruin-two-swords");
}
