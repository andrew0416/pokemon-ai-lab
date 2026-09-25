//! Parity of held items (work plan units O82–O93) with Showdown's exact outcome distribution
//! (`engine/oracle/expected/*.turn.json`), plus the combinations the engine refuses on purpose.

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::rules::Ruleset;
use lab_engine::state::{Pokemon, SideId};
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_engine::Doubles;
use lab_scenario::scenario_choices;

/// The fixture's start position with `change` applied to the named Pokémon, run as the
/// scenario's turn; the error the engine gives.
fn refused_with(
    name: &str,
    side: SideId,
    species: &str,
    change: impl FnOnce(&mut Pokemon),
) -> String {
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state: Doubles = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let mon = state
        .side_mut(side)
        .party
        .iter_mut()
        .find(|p| p.species.data().name == species)
        .unwrap_or_else(|| panic!("{species} is in {name}"));
    change(mon);
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::Unsupported(why)) => why,
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

// ---- O82 type-resist berries -------------------------------------------------------------------

#[test]
fn resist_berries_on_a_spread_move_match_showdown() {
    assert_exact_parity("o82-resist-berries");
}

#[test]
fn chilan_and_roseli_berries_match_showdown() {
    assert_exact_parity("o82-chilan-roseli");
}

/// Klutz makes Showdown ignore the holder's item (`ignoringItem`); no item handler checks it
/// yet, so the engine refuses the combination.
#[test]
fn klutz_holding_an_item_is_refused() {
    let why = refused_with("o82-resist-berries", SideId::Two, "Heatran", |mon| {
        mon.ability = lab_engine::dex::abilities::KLUTZ;
    });
    assert!(why.contains("Klutz"), "{why}");
}
