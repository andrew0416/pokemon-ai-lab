//! Trapping moves (Opus V unit 3): Anchor Shot and Spirit Shackle (a 100% secondary `onHit`
//! adding `trapped` linked to the user's `trapper`), Jaw Lock (both ends trap each other) and
//! Octolock (its own volatile: trapped while the source is active, -1 Def / -1 SpD each
//! residual). Fixtures from Showdown's exact enumeration (`<name>.turn.json`) and trapping flags
//! (`oracle/trapped.cjs` → `<name>.trapped.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::trapped;

/// `turn::trapped` for every active Pokémon of the scenario's position against Showdown's
/// `pokemon.trapped`, and the ruleset rejects exactly the trapped ones' switches (as
/// `moves_extra.rs` checks Mean Look).
fn assert_trapped_parity(name: &str) {
    let path = common::engine_dir().join(format!("oracle/expected/{name}.trapped.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let fixture: serde_json::Value = serde_json::from_str(&text).unwrap();
    let (loaded, position) = common::start(name, &fixture);
    let state = position.state;
    let mut checked = 0;
    for (key, side) in [("p1", SideId::One), ("p2", SideId::Two)] {
        let expected = fixture["trapped"][key].as_object().unwrap();
        for slot in 0..2u8 {
            let r = SlotRef { side, slot };
            let Some(party) = state.slot(r).party_index else {
                continue;
            };
            let mon_name = loaded.meta.sides[side.index()].name(party).unwrap();
            let want = expected[mon_name].as_bool().unwrap();
            assert_eq!(trapped(&state, r), want, "{name}: {mon_name}");
            checked += 1;
            let bench = (0..state.side(side).party.len() as u8).find(|&i| {
                state.side(side).party[i as usize].hp > 0
                    && !state
                        .side(side)
                        .slots
                        .iter()
                        .any(|s| s.party_index == Some(i))
            });
            if let Some(party_index) = bench {
                let switch = SlotAction::Switch { party_index };
                let result = Ruleset::CHAMPIONS_MC.validate_slot_action(&state, r, switch);
                let expected = if want {
                    Err(ActionError::Trapped { slot })
                } else {
                    Ok(())
                };
                assert_eq!(result, expected, "{name}: {mon_name}");
            }
        }
    }
    let total: usize = ["p1", "p2"]
        .iter()
        .map(|k| fixture["trapped"][k].as_object().unwrap().len())
        .sum();
    assert_eq!(checked, total, "{name}");
}

/// Anchor Shot's secondary traps its target, linked to the user.
#[test]
fn anchor_shot_traps_its_target() {
    assert_exact_parity("anchor-shot");
}

/// Spirit Shackle cannot trap a Ghost type.
#[test]
fn spirit_shackle_does_not_trap_a_ghost() {
    assert_exact_parity("spirit-shackle-ghost");
}

/// Jaw Lock traps both the user and its target, each linked to the other.
#[test]
fn jaw_lock_traps_both_ends() {
    assert_exact_parity("jaw-lock");
}

/// Both Jaw Lock Pokémon are trapped; dragging one out frees the other of both links.
#[test]
fn jaw_lock_ends_with_either_pokemon() {
    assert_trapped_parity("jaw-lock-drag");
    assert_exact_parity("jaw-lock-drag");
}

/// Octolock traps (not a Ghost) and lowers Def and SpD at the residual.
#[test]
fn octolock_traps_and_lowers_defenses() {
    assert_exact_parity("octolock");
}

/// Octolock keeps its target trapped and lowers its defenses every turn.
#[test]
fn octolock_holds() {
    assert_trapped_parity("octolock-hold");
    assert_exact_parity("octolock-hold");
}

/// Octolock ends at the residual once its source switched out.
#[test]
fn octolock_ends_with_its_source() {
    assert_exact_parity("octolock-release");
}
