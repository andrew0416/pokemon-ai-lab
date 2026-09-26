//! Parity of the Choice lock with Struggle (Opus BB unit B15): `choicelock`'s `onBeforeMove`
//! only stops a move other than the locked one when it is not Struggle (`move.id !==
//! 'struggle'`). A Choice-locked Pokémon whose locked move cannot be used either (here Gigaton
//! Hammer twice in a row; also Taunt or Disable on it) has only Struggle, which hits.

mod common;

use common::assert_exact_parity;

/// Choice Band Tinkaton after Gigaton Hammer: Struggle hits a random foe (the engine failed it).
#[test]
fn choice_locked_struggle_matches_showdown() {
    assert_exact_parity("bb-choicelock-struggle");
}
