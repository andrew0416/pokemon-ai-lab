//! Moves that change abilities (Opus T): Skill Swap, Role Play, Entrainment, Simple Beam and
//! Worry Seed (the old ability's End, the new one's Start; Ability Shield blocks). Fixtures from
//! Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Skill Swap starts the target's new ability (Intimidate), then the user's (Drought).
#[test]
fn skill_swap_trades_and_starts_both_abilities() {
    assert_exact_parity("skill-swap");
}

/// Role Play copies Intimidate, which starts at once; Entrainment passes on the user's ability.
#[test]
fn role_play_and_entrainment_set_abilities() {
    assert_exact_parity("role-play-entrainment");
}

/// Simple Beam's Simple doubles the following Swords Dance; Worry Seed's Insomnia wakes the
/// target.
#[test]
fn simple_beam_and_worry_seed_replace_the_ability() {
    assert_exact_parity("simple-beam-worry-seed");
}

/// Ability Shield blocks Skill Swap; Worry Seed fails on an Insomnia target.
#[test]
fn ability_changes_fail_on_ability_shield_and_insomnia() {
    assert_exact_parity("ability-change-fails");
}
