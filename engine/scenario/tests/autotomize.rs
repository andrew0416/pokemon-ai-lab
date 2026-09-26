//! Parity of Autotomize (Opus W unit 5) with Showdown: Speed +2 and 100 kg less weight (at
//! least 0.1 kg) until the next `setSpecies` (switching out, fainting, a forme change). The
//! weight is hidden state on both sides, so each scenario reads it through Low Kick's power.

mod common;

use common::assert_exact_parity;

#[test]
fn autotomize_lowers_the_weight() {
    assert_exact_parity("w-autotomize-once");
}

#[test]
fn autotomize_twice_reaches_the_minimum_weight() {
    assert_exact_parity("w-autotomize-twice");
}

#[test]
fn autotomize_ends_on_switching_out() {
    assert_exact_parity("w-autotomize-switch");
}

#[test]
fn autotomize_ends_on_mega_evolution() {
    assert_exact_parity("w-autotomize-mega");
}
