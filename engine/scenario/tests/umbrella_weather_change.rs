//! Parity of Utility Umbrella's WeatherChange events (Opus BB unit B24): in sun or rain the
//! umbrella's `onEnd` (its holder lost it: Trick, Knock Off, Fling) runs
//! `runEvent('WeatherChange')` on the old holder, its `onStart` (`setItem`) does so for a new
//! holder that ignores it (Magic Room, Klutz), and its `onUpdate` for an umbrella given back
//! silently after its End (a failed Trick). Forecast and Flower Gift react; Flower Gift is
//! breakable (a Mold Breaker user's Trick skips it).

mod common;

use common::assert_exact_parity;

/// Trick takes Castform's umbrella in rain: Castform-Rainy.
#[test]
fn trick_takes_umbrella_from_forecast_matches_showdown() {
    assert_exact_parity("bb-umbrella-trick-forecast");
}

/// Trick gives the umbrella back to Castform-Rainy: no WeatherChange for a holder that does
/// not ignore it, so it stays Rainy.
#[test]
fn trick_gives_umbrella_to_forecast_matches_showdown() {
    assert_exact_parity("bb-umbrella-trick-back-forecast");
}

/// A failed Trick (the user's Mega Stone stays) returns the umbrella silently; its next
/// `onUpdate` runs WeatherChange and Castform goes back to its base forme.
#[test]
fn failed_trick_umbrella_update_matches_showdown() {
    assert_exact_parity("bb-umbrella-trick-fail-forecast");
}

/// Flower Gift in sun: Cherrim loses its umbrella and blooms; a Mold Breaker Trick skips the
/// breakable Flower Gift.
#[test]
fn trick_umbrella_flower_gift_matches_showdown() {
    assert_exact_parity("bb-umbrella-trick-flower-gift");
}

/// Under Magic Room the Ends are suppressed and the new holder's Start runs WeatherChange.
#[test]
fn umbrella_start_under_magic_room_matches_showdown() {
    assert_exact_parity("bb-umbrella-magic-room");
}

/// Fling throws the umbrella (`setItem('')`): its End runs WeatherChange.
#[test]
fn fling_umbrella_forecast_matches_showdown() {
    assert_exact_parity("bb-umbrella-fling-forecast");
}

/// Knock Off takes the umbrella: its End runs WeatherChange.
#[test]
fn knock_off_umbrella_forecast_matches_showdown() {
    assert_exact_parity("bb-umbrella-knock-off-forecast");
}

/// Knock Off knocking Castform out still takes the umbrella, whose End changes the fainting
/// Castform's forme (reverted by the faint).
#[test]
fn knock_off_ko_umbrella_forecast_matches_showdown() {
    assert_exact_parity("bb-umbrella-knock-off-ko-forecast");
}
