//! Parity of Color Change (Opus S unit 9d: `onAfterMoveSecondary`) with Showdown: the
//! scenario's exact outcome distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// The holder becomes the type of the move that hit it; the next move of that type is then
/// super effective.
#[test]
fn color_change_matches_showdown() {
    assert_exact_parity("s-color-change");
}
