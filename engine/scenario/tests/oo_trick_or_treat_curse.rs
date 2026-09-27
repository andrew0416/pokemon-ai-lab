//! Opus OO R17-trick-or-treat-curse: Trick-or-Treat's Curse Glitch. A target in the second
//! position with Curse queued gets `action.targetLoc = -1` (its ally's position); the Curse, now
//! a Ghost's, turns an ally target into `randomNormal` in its ModifyMove, and useMove draws a
//! random foe. `rr-trick-or-treat-curse-glitch` (`refusals_rr.rs`) is the glitch.

mod common;

use common::assert_exact_parity;

/// Snorlax in the first position: no glitch; the Ghost Curse goes to a random foe as well.
#[test]
fn trick_or_treat_on_the_first_position_leaves_the_curse_alone() {
    assert_exact_parity("oo-trick-or-treat-curse-first-slot");
}
