//! Forecast (Opus AA unit 4). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Forecast follows the weather: Sunny at the start under Drought, then Rainy, plain and Snowy
/// as three weather moves replace each other.
#[test]
fn forecast_follows_every_weather_change() {
    assert_exact_parity("aa-forecast");
}

/// Air Lock switching in suppresses the sun: Castform goes back to its base forme.
#[test]
fn forecast_reverts_when_air_lock_comes_in() {
    assert_exact_parity("aa-forecast-air-lock");
}
