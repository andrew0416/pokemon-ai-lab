//! Parity of Hydro Steam in sun (Opus BB unit B23): sun's `onWeatherModifyDamage` boosts Hydro
//! Steam 1.5x when the attacker's `effectiveWeather()` is sun, before its Water 0.5x for the
//! defender; Mega Sol's handler runs the same code with the Mega Sol user's sun view.

mod common;

use common::assert_exact_parity;

/// Hydro Steam in sun: 1.5x (the engine gave 0.5x).
#[test]
fn hydro_steam_sun_matches_showdown() {
    assert_exact_parity("bb-hydro-steam-sun");
}

/// The attacker's Utility Umbrella hides its sun (then the defender's sun gives 0.5x); the
/// defender's umbrella does not stop the 1.5x.
#[test]
fn hydro_steam_umbrella_matches_showdown() {
    assert_exact_parity("bb-hydro-steam-umbrella");
}

/// A Mega Sol user's Hydro Steam with no weather: 1.5x.
#[test]
fn hydro_steam_mega_sol_matches_showdown() {
    assert_exact_parity("bb-hydro-steam-mega-sol");
}
