//! Forest's Curse and Trick-or-Treat (Opus Z unit 2): `addType` gives the target a third type
//! (Showdown `pokemon.addedType`, the engine's hidden `Volatile::AddedType`), which every
//! `getTypes()` / `hasType` reader sees (immunity, effectiveness, STAB, trapping) while the
//! canonical `types` (`getTypes(true)`) leaves it out; a new added type replaces the old one, and
//! `setType` (Soak) clears it. Fixtures from Showdown's exact enumeration (`<name>.turn.json`) and
//! trapping flags (`oracle/trapped.cjs` → `<name>.trapped.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::trapped;

/// Trick-or-Treat's Ghost type makes Snorlax immune to Tackle and gives its Shadow Ball STAB.
#[test]
fn trick_or_treat_adds_ghost() {
    assert_exact_parity("z-trick-or-treat");
}

/// Forest's Curse's Grass type makes Flame Charge super effective and gives Seed Bomb STAB.
#[test]
fn forests_curse_adds_grass() {
    assert_exact_parity("z-forests-curse");
}

/// Forest's Curse replaces Trick-or-Treat's Ghost type: Tackle hits again.
#[test]
fn a_new_added_type_replaces_the_old() {
    assert_exact_parity("z-added-type-replace");
}

/// Soak (`setType`) clears the added type: Flame Charge is resisted by the pure Water target.
#[test]
fn set_type_clears_the_added_type() {
    assert_exact_parity("z-added-type-soak");
}

/// A Pokémon trapped by Mean Look is free once Trick-or-Treat makes it a Ghost (the `trapped`
/// volatile stays; `tryTrap` fails for the added Ghost type).
#[test]
fn an_added_ghost_type_frees_a_trapped_pokemon() {
    let name = "z-trick-or-treat-trap";
    let path = common::engine_dir().join(format!("oracle/expected/{name}.trapped.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let fixture: serde_json::Value = serde_json::from_str(&text).unwrap();
    let (loaded, position) = common::start(name, &fixture);
    let state = position.state;
    for (key, side) in [("p1", SideId::One), ("p2", SideId::Two)] {
        for slot in 0..2u8 {
            let r = SlotRef { side, slot };
            let party = state.slot(r).party_index.expect("both slots are filled");
            let mon_name = loaded.meta.sides[side.index()].name(party).unwrap();
            let want = fixture["trapped"][key][mon_name].as_bool().unwrap();
            assert_eq!(trapped(&state, r), want, "{name}: {mon_name}");
        }
    }
    assert_exact_parity(name);
}
