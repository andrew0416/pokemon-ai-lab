//! Regression bundle of real library turns (Opus CC unit verify-library-turns): six doubles
//! library teams (`teams/library/doubles/m-c`: the four `validated` teams with public SP that
//! the search runs use — sand-owen, coaching-panda, psy-cona, psy-sand-udon — and balance-ddee
//! and crown-cecil9, the first further teams in `lab-library`'s loading order that are not
//! `needs-review`; the last two keep the library file's zero SP, their SP not being public),
//! each leading against another library team's lead pair in VGC Reg M-C (team preview keeps 4).
//! The scenarios reference the library team files, so a changed library set shows up here.
//!
//! A real turn has too many damage-roll combinations for the oracle's exact mode, so each
//! fixture is `enumerate.cjs --mode extremes` (damage rolls min and max only, 1/2 each), matched
//! exactly by the engine's `RollMode::Extremes`. Every scenario keeps the four actives' Speeds
//! apart and at most one `random(100)` draw (a secondary effect, Fake Out's flinch, or a
//! self-drop such as Close Combat's, which Showdown's `selfDrops` also rolls). No turn here
//! reaches an unsupported effect.

mod common;

use common::assert_extremes_parity;

/// Sand Stream + Grassy Surge; Mega Tyranitar flinched by Fake Out, Sucker Punch before a Sand
/// Rush High Horsepower, Focus Sash, Black Glasses, sand and Grassy Terrain residuals.
#[test]
fn sand_owen_vs_coaching_panda_turn_one_matches_showdown() {
    assert_extremes_parity("cc-lib-sand-owen-vs-coaching-panda");
}

/// Psychic Surge replacing Grassy Surge; spread Expanding Force (a Dark-type immune), Follow Me
/// drawing two attacks, Colbur Berry, Black Glasses.
#[test]
fn coaching_panda_vs_psy_sand_udon_turn_one_matches_showdown() {
    assert_extremes_parity("cc-lib-coaching-panda-vs-psy-sand-udon");
}

/// Psychic Surge (Psychic Seed) + Sand Stream; Follow Me, Sand Rush, Expanding Force with Life
/// Orb, Mega Tyranitar's Knock Off.
#[test]
fn psy_cona_vs_sand_owen_turn_one_matches_showdown() {
    assert_extremes_parity("cc-lib-psy-cona-vs-sand-owen");
}

/// Helping Hand + Expanding Force into Protect; Mega Floette (Fairy Aura) Dazzling Gleam.
#[test]
fn psy_sand_udon_vs_balance_ddee_turn_one_matches_showdown() {
    assert_extremes_parity("cc-lib-psy-sand-udon-vs-balance-ddee");
}

/// Grassy Seed and Unburden; Grassy Glide's terrain priority into a Mega Evolving Floette, Close
/// Combat into Focus Sash, Fairy Aura Dazzling Gleam knocking Arcanine-Hisui out before it moves.
#[test]
fn balance_ddee_vs_crown_cecil9_turn_one_matches_showdown() {
    assert_extremes_parity("cc-lib-balance-ddee-vs-crown-cecil9");
}

/// Fake Out on Snorlax, Mega Gengar's Perish Song, Head Smash; Leftovers and Grassy Terrain.
#[test]
fn crown_cecil9_vs_perish_mrada_turn_one_matches_showdown() {
    assert_extremes_parity("cc-lib-crown-cecil9-vs-perish-mrada");
}
