//! Fairy Lock, Electrify, Aura Wheel and Eerie Spell (Opus Z unit 5): the `fairylock`
//! pseudo-weather traps every active Pokémon for the next choices (Ghosts and Shed Shell holders
//! excepted); Electrify makes its target's moves Electric for the turn and fails on a target that
//! has already moved; Aura Wheel is Electric or Dark by Morpeko's forme and fails for anyone else;
//! Eerie Spell takes 3 PP from its target's last move. Fixtures from Showdown's exact enumeration
//! (`<name>.turn.json`) and trapping flags (`oracle/trapped.cjs` → `<name>.trapped.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::trapped;

/// `turn::trapped` for every active Pokémon against Showdown's `pokemon.trapped`, and the ruleset
/// rejects exactly the trapped ones' switches to a bench member.
fn assert_trapped_parity(name: &str) {
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
            let bench = (0..state.side(side).party.len() as u8).find(|&i| {
                state.side(side).party[i as usize].hp > 0
                    && !state
                        .side(side)
                        .slots
                        .iter()
                        .any(|s| s.party_index == Some(i))
            });
            let party_index = bench.expect("each side has a bench member");
            let result = Ruleset::CHAMPIONS_MC.validate_slot_action(
                &state,
                r,
                SlotAction::Switch { party_index },
            );
            let expected = if want {
                Err(ActionError::Trapped { slot })
            } else {
                Ok(())
            };
            assert_eq!(result, expected, "{name}: {mon_name}");
        }
    }
}

/// Fairy Lock adds its field condition for two turns (one left after the residual).
#[test]
fn fairy_lock_starts() {
    assert_exact_parity("z-fairy-lock");
}

/// Under Fairy Lock everyone but the Ghost and the Shed Shell holder is trapped; a second Fairy
/// Lock fails, and the condition ends at the residual.
#[test]
fn fairy_lock_traps_the_next_choice() {
    assert_trapped_parity("z-fairy-lock-held");
    assert_exact_parity("z-fairy-lock-held");
}

/// An electrified Tackle is Electric: the Ground type is immune.
#[test]
fn electrify_makes_moves_electric() {
    assert_exact_parity("z-electrify");
}

/// Electrify fails on a target that has already moved (Stomping Tantrum then doubles).
#[test]
fn electrify_fails_on_a_target_that_moved() {
    assert_exact_parity("z-electrify-fail");
}

/// Aura Wheel is Dark for Morpeko-Hangry and fails for Pikachu.
#[test]
fn aura_wheel_follows_morpeko() {
    assert_exact_parity("z-aura-wheel");
}

/// Eerie Spell takes 3 PP from the target's last move.
#[test]
fn eerie_spell_drains_pp() {
    assert_exact_parity("z-eerie-spell");
}
