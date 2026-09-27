//! Parity of a Trace that keeps seeking (board R1-trace-seeking, Opus MM). Trace's `onStart`
//! sets `effectState.seek` and runs its `onUpdate` at once; with no traceable foe it copies
//! nothing and every later `Update` in which its ability acts tries again
//! (`data/abilities.ts` `trace`; the Champions mod does not override it). The engine keeps the
//! flag as the hidden volatile `traceseek` (`switching::trace_update`).
//!
//! The repro scenarios of the refusal audit (`rr-trace-*`, `rr-mega-alakazam-trace`) are in
//! `refusals_rr.rs`.

mod common;

use common::assert_exact_parity;

/// A foe switching in: the switch action's Update comes before the newcomer's runSwitch, so the
/// seeking Trace copies Intimidate before the newcomer's own Intimidate starts.
#[test]
fn seeking_trace_copies_a_foe_switching_in() {
    assert_exact_parity("mm-trace-seek-switch-in");
}

/// Two seeking Traces (tied Speed) and two traceable replacements sent in at once: each Trace
/// picks one of the two uniformly at random at the Update after the last instaswitch.
#[test]
fn two_seeking_traces_pick_among_simultaneous_replacements() {
    assert_exact_parity("mm-trace-seek-replacements");
}

/// Gastro Acid on a seeking Trace: its `onUpdate` is suppressed, so it copies nothing.
#[test]
fn suppressed_seeking_trace_copies_nothing() {
    assert_exact_parity("mm-trace-seek-gastro-acid");
}

/// Skill Swap moves Trace (with a fresh `abilityState`) to the ally, which starts it and seeks in
/// turn; the old holder stops seeking.
#[test]
fn skill_swapped_trace_seeks_on_its_new_holder() {
    assert_exact_parity("mm-trace-seek-skill-swap");
}

/// A foe's Mega Evolution makes its ability traceable (Trace → Pixilate): the seeking Trace copies
/// it at the Update after the megaEvo action.
#[test]
fn seeking_trace_copies_a_foe_mega_ability() {
    assert_exact_parity("mm-trace-seek-foe-mega");
}

/// A Trace Mega (Meowstic-F-Mega) facing only `notrace` foes keeps seeking after its Mega
/// Evolution and copies the next traceable foe (the setup turn's Mega Evolution is replayed).
#[test]
fn trace_mega_keeps_seeking_and_copies_a_later_foe() {
    assert_exact_parity("mm-trace-mega-seek");
}

/// The seeker Mega Evolves in the turn a traceable foe switches in: the switch and its Update come
/// first (Trace copies Intimidate), then Mega Evolution replaces the copy with Pixilate.
#[test]
fn seeking_trace_copies_before_its_own_mega_evolution() {
    assert_exact_parity("mm-trace-seek-mega-own");
}

/// Imposter transforms Ditto into the seeking Gardevoir: Trace (copied by Transform) starts on
/// Ditto and copies Thick Fat, which Gardevoir's Trace then copies from Ditto. The oracle fixture
/// is `full` with `--staged` (the transformed Ditto ties Gardevoir's Speed at every Update).
#[test]
fn imposter_copies_a_seeking_trace_which_then_copies_from_it() {
    assert_exact_parity("mm-trace-seek-imposter");
}

/// The turn after a start in which the fastest Trace met only Trace holders and copied at the
/// start's Update (its initial distribution is `initial.rs`); `full` with `--staged` (the two p2
/// Gardevoir tie).
#[test]
fn turn_after_a_start_with_only_trace_foes() {
    assert_exact_parity("mm-trace-start-only-traces");
}

/// Neutralizing Gas (outside the Champions standard range) keeps Trace from starting; its End as
/// its holder switches out restarts Trace, which seeks and copies the replacement's ability at the
/// switch's Update.
#[test]
fn trace_restarted_by_neutralizing_gas_end_seeks() {
    assert_exact_parity("mm-trace-neutralizing-gas");
}
