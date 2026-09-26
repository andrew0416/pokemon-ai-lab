//! Weather storms, type changers, berry eaters and Stockpile (Opus V unit 8): Bleakwind /
//! Sandsear / Wildbolt Storm (never miss in the nominal target's rain), Freezy Frost, Magic
//! Powder, Camouflage, Conversion, Teatime, Stuff Cheeks, Stockpile / Spit Up / Swallow. Fixtures
//! from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`; `storm-rain`
//! with `--keep-nominal-draws`: the nominal target matters there).

mod common;

use common::assert_exact_parity;

/// A storm never misses in its nominal target's rain; Utility Umbrella hides the rain.
#[test]
fn storms_never_miss_in_rain() {
    assert_exact_parity("storm-rain");
}

/// Freezy Frost clears every active Pokémon's stat stages.
#[test]
fn freezy_frost_clears_all_boosts() {
    assert_exact_parity("freezy-frost");
}

/// Magic Powder makes its target Psychic; Grass types are immune to it.
#[test]
fn magic_powder_changes_the_type() {
    assert_exact_parity("magic-powder");
}

/// Camouflage takes the terrain's type; Conversion the first move's.
#[test]
fn camouflage_and_conversion_change_the_type() {
    assert_exact_parity("camouflage-conversion");
}

/// Teatime makes every berry holder eat its berry, Unnerve or not.
#[test]
fn teatime_feeds_every_berry_holder() {
    assert_exact_parity("teatime");
}

/// Stuff Cheeks raises Defense and eats the berry; without a berry it fails.
#[test]
fn stuff_cheeks_eats_the_berry() {
    assert_exact_parity("stuff-cheeks");
}

/// A second Stockpile adds a layer and raises again.
#[test]
fn stockpile_stacks() {
    assert_exact_parity("stockpile");
}

/// Spit Up hits by the layers and ends the stockpile, taking the raises back.
#[test]
fn spit_up_releases_the_stockpile() {
    assert_exact_parity("spit-up");
}

/// Swallow heals by the layers and takes back only the raises that took.
#[test]
fn swallow_heals_and_releases() {
    assert_exact_parity("swallow");
}
