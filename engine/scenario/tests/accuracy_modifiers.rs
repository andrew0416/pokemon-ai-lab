//! Parity of the accuracy abilities and items (Opus Q unit 5: `onModifyAccuracy`,
//! `onSourceModifyAccuracy`, `onAnyModifyAccuracy`) with Showdown: each scenario's exact outcome
//! distribution must equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Sand Veil (and its sandstorm immunity), Victory Star, Compound Eyes, Bright Powder.
#[test]
fn sand_veil_victory_star_compound_eyes_bright_powder_match_showdown() {
    assert_exact_parity("q-accuracy-sand");
}

/// Wonder Skin, Tangled Feet, Snow Cloak, Lax Incense.
#[test]
fn wonder_skin_tangled_feet_snow_cloak_lax_incense_match_showdown() {
    assert_exact_parity("q-accuracy-snow");
}
