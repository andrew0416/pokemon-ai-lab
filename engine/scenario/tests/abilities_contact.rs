//! Parity of hit-reaction, contact, boost-blocking, switch-in and switch-out abilities (work
//! plan units O55, O56, O58, O68, O63, O64, O54) with Showdown's outcome distribution
//! (`engine/oracle/expected/*.turn.json`).

mod common;

use common::assert_exact_parity;

// ---- O55 onDamagingHit -------------------------------------------------------------------------

/// Static, Flame Body and Poison Point: a 30% roll per contact hit, the holder as the source.
#[test]
fn contact_status_abilities_match_showdown() {
    assert_exact_parity("o55-contact-status");
}

/// Effect Spore's sleep / paralysis / poison draw, and a Grass attacker's powder immunity.
#[test]
fn effect_spore_matches_showdown() {
    assert_exact_parity("o55-effect-spore");
}

/// Cotton Down against Mirror Armor and Defiant, Gooey from an ally, Protective Pads.
#[test]
fn gooey_and_cotton_down_match_showdown() {
    assert_exact_parity("o55-gooey-cotton-down");
}

#[test]
fn stamina_weak_armor_tangling_hair_match_showdown() {
    assert_exact_parity("o55-stamina-weak-armor");
}

#[test]
fn steam_engine_and_water_compaction_match_showdown() {
    assert_exact_parity("o55-water-boosts");
}

/// Electromorphosis and Wind Power (a wind move, Tailwind) add `charge`, which doubles the
/// next Electric move and ends after it.
#[test]
fn charge_abilities_match_showdown() {
    assert_exact_parity("o55-charge");
}

#[test]
fn sand_spit_and_seed_sower_match_showdown() {
    assert_exact_parity("o55-sand-seed");
}

/// Aftermath and Innards Out on holders the hit fainted, fainting an attacker.
#[test]
fn aftermath_and_innards_out_match_showdown() {
    assert_exact_parity("o55-aftermath-innards");
}

/// Anger Shell's boosts and berry timing; Berserk's check left pending by a Sheer Force move
/// keeps its Sitrus Berry (Champions `onDamage`).
#[test]
fn anger_shell_and_berserk_match_showdown() {
    assert_exact_parity("o55-anger-shell-berserk");
}

// ---- O56 onSourceDamagingHit -------------------------------------------------------------------

#[test]
fn poison_touch_and_toxic_chain_match_showdown() {
    assert_exact_parity("o56-poison-touch");
}

/// Shield Dust, and the damaged target's Protective Pads (Showdown passes the target as the
/// attacker to `checkMoveMakesContact`).
#[test]
fn blocked_poison_touch_and_toxic_chain_match_showdown() {
    assert_exact_parity("o56-poison-touch-blocked");
}

// ---- O58 boost blockers, Own Tempo, Scrappy, Keen Eye; the onUpdate cures ------------------

/// The ability `onUpdate` cures: skipped at the hit's Update while a Mold Breaker move is in
/// progress (the Cheri Berry cures Limber's holder instead), run at the Update after the action
/// (Own Tempo's confusion cure, before its holder moves).
#[test]
fn update_cures_after_ability_ignoring_moves_match_showdown() {
    assert_exact_parity("o58-update-cures");
}

/// A switch-in Intimidate against Own Tempo and Oblivious; Own Tempo against Confuse Ray.
#[test]
fn own_tempo_and_oblivious_block_intimidate_match_showdown() {
    assert_exact_parity("o58-own-tempo-intimidate");
}

/// Scrappy against Intimidate (battle start) and a Ghost type; Keen Eye's ignored evasion and
/// blocked accuracy drop.
#[test]
fn scrappy_and_keen_eye_match_showdown() {
    assert_exact_parity("o58-scrappy-keen-eye");
}
