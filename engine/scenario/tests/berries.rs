//! The `Update` event (WORKPLAN F14) and the berries it eats, against Showdown's exact
//! enumeration.

mod common;

use common::assert_exact_parity;

/// Sitrus Berry eaten right after the action that brings its holder to half HP (before the
/// residual heals), Lum Berry eaten the moment Hypnosis lands.
#[test]
fn sitrus_and_lum_berries_match_showdown_exactly() {
    assert_exact_parity("sitrus-lum");
}

/// Lum Berry and Persim Berry are eaten at the Update after a hit for confusion alone, and
/// their `onEat` removes the confusion.
#[test]
fn lum_and_persim_cure_confusion_matches_showdown_exactly() {
    assert_exact_parity("lum-persim-confusion");
}
