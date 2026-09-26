//! Parity of the Choice lock's end-of-turn check (Opus BB unit B16): `choicelock`'s
//! `onDisableMove` removes the lock when the holder no longer has the locked move
//! (`!pokemon.hasMove(this.effectState.move)`). Reachable through Dancer: the copied dance is an
//! external move whose ModifyMove starts the dancer's lock on a move it does not know.

mod common;

use common::assert_exact_parity;

/// Choice Scarf Oricorio danced Dragon Dance in the setup turn; the lock on Dragon Dance goes at
/// the end of that turn, so its Calm Mind now works (the engine kept the lock).
#[test]
fn choice_lock_on_unknown_move_ends_matches_showdown() {
    assert_exact_parity("bb-choicelock-dancer");
}
