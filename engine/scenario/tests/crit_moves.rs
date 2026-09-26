//! Critical-hit volatiles (Opus V unit 4): Focus Energy (+2), Dragon Cheer (+2 on a Dragon type
//! when it starts, else +1; the two exclude each other), Laser Focus (every move crits, 2 turns,
//! restartable), and Psych Up copying them. Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Focus Energy raises the crit ratio by 2; Dragon Cheer fails on its holder.
#[test]
fn focus_energy_raises_the_crit_ratio() {
    assert_exact_parity("focus-energy");
}

/// Dragon Cheer gives +2 to a Dragon type and +1 to anyone else.
#[test]
fn dragon_cheer_depends_on_the_dragon_type() {
    assert_exact_parity("dragon-cheer");
}

/// Laser Focus makes the next moves crit for two turns; using it again restarts it.
#[test]
fn laser_focus_always_crits() {
    assert_exact_parity("laser-focus");
}

/// Psych Up copies Dragon Cheer as the target had it (`hasDragonType`).
#[test]
fn psych_up_copies_dragon_cheer() {
    assert_exact_parity("psych-up-dragon-cheer");
}

/// Psych Up copies Laser Focus with a fresh duration.
#[test]
fn psych_up_copies_laser_focus() {
    assert_exact_parity("psych-up-laser-focus");
}
