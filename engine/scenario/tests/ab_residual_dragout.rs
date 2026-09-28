//! The phazing step after a residual that ended the battle (board R5-t2, Opus AB): Showdown's
//! `fieldEvent('Residual')` returns once a faint ends the battle, but `runAction` still runs the
//! phazing step after it, so a Red Card a future move set off drags the user out after the win.
//! Fixture from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Future Sight sets off Clefable's Red Card; poison then knocks out p2's last two Pokémon on
/// the low rolls: Tyranitar is still dragged in for Slowking after the win.
#[test]
fn red_card_drags_after_a_residual_that_ended_the_battle() {
    assert_exact_parity("ab-residual-end-red-card-drag");
}
