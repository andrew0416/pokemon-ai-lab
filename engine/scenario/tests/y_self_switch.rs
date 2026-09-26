//! Volatile-passing and field self-switches (Opus Y unit 1): Baton Pass (`copyVolatileFrom`:
//! stat stages and every volatile that is not `noCopy`, then the `Copy` handlers), Shed Tail (its
//! substitute only), Chilly Reception (`priorityChargeCallback` condition, snow, `selfSwitch` on
//! a field move). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Baton Pass passes the stages, the substitute with its HP and Magnet Rise, not Stockpile.
#[test]
fn baton_pass_copies_boosts_and_volatiles() {
    assert_exact_parity("y-baton-pass");
}

/// Power Trick's `onCopy` swaps the newcomer's own Attack and Defense; Ingrain passes too.
#[test]
fn baton_pass_runs_power_trick_on_copy() {
    assert_exact_parity("y-baton-pass-copy");
}

/// Gastro Acid's `onCopy` ends it on a `cantsuppress` ability (Comatose).
#[test]
fn baton_pass_gastro_acid_ends_on_comatose() {
    assert_exact_parity("y-baton-pass-gastro");
}

/// Shed Tail costs half the max HP (rounded up) and passes only its substitute.
#[test]
fn shed_tail_passes_only_the_substitute() {
    assert_exact_parity("y-shed-tail");
}

/// Chilly Reception sets snow (Icy Rock: 8 turns) and switches its user out.
#[test]
fn chilly_reception_sets_snow_and_switches() {
    assert_exact_parity("y-chilly-reception");
}

/// Stopped for its switch, the user still has the `chillyreception` condition.
#[test]
fn chilly_reception_pause_keeps_its_condition() {
    assert_exact_parity("y-chilly-reception-pause");
}

/// In snow the weather fails but the switch still counts.
#[test]
fn chilly_reception_in_snow_still_switches() {
    assert_exact_parity("y-chilly-reception-snow");
}

/// Without a bench Baton Pass and Shed Tail fail and Chilly Reception only sets snow.
#[test]
fn self_switches_without_a_bench() {
    assert_exact_parity("y-self-switch-no-bench");
}
