//! Mega Sol's sun view after the move inside the same action (Opus BB unit B25, a probe):
//! Showdown keeps `battle.activePokemon` until `runAction`'s `clearActiveMove()`, after the
//! phazing step, where the engine's active move already ended. Only a Move's or a Weather's
//! handler reads `effectiveWeather()` through Mega Sol, and none runs in that window: the
//! dragged-in Pokémon's switch-in handlers belong to its ability, item and conditions.

mod common;

use common::assert_exact_parity;

/// Mega Meganium's Roar drags Castform in during Pelipper's rain: Forecast's `onStart` reads
/// the weather with the ability as the effect, so Castform-Rainy (not Sunny).
#[test]
fn mega_sol_roar_drag_in_matches_showdown() {
    assert_exact_parity("bb-mega-sol-roar-forecast");
}
