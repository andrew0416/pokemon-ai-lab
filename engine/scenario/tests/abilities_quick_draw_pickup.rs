//! Quick Draw, Mycelium Might, Run Away, Moody, Pickup and Power Construct (Opus AA unit 2).
//! Fixtures from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`) and
//! its trapping flags (`oracle/trapped.cjs` → `<name>.trapped.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::trapped;

/// Quick Draw's 3/10 moves the slower holder's damaging move first.
#[test]
fn quick_draw_moves_first_three_times_in_ten() {
    assert_exact_parity("aa-quick-draw");
}

/// Mycelium Might: a status move goes last in its priority bracket and ignores Sweet Veil.
#[test]
fn mycelium_might_status_moves_go_last_and_ignore_abilities() {
    assert_exact_parity("aa-mycelium-might");
}

/// Run Away frees its holder from Shadow Tag and Mean Look; the holder switches out.
#[test]
fn run_away_escapes_trapping() {
    assert_trapped_parity("aa-run-away");
    assert_exact_parity("aa-run-away");
}

/// Moody: +2 to one random stat, -1 to another.
#[test]
fn moody_raises_one_stat_and_lowers_another() {
    assert_exact_parity("aa-moody");
}

/// Pickup takes a used item of a random adjacent Pokémon; a picked-up White Herb starts at once.
#[test]
fn pickup_takes_an_item_used_this_turn() {
    assert_exact_parity("aa-pickup");
}

/// Power Construct: Zygarde and Zygarde-10% at half HP become Zygarde-Complete at the residual.
#[test]
fn power_construct_completes_zygarde_at_half_hp() {
    assert_exact_parity("aa-power-construct");
}

/// `turn::trapped` for every active Pokémon of the scenario's position against Showdown's
/// `pokemon.trapped`, and a switch choice of each trapped one is rejected
/// (`ActionError::Trapped`) and never generated (as `abilities_library.rs`).
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
            let bench: Vec<u8> = (0..state.side(side).party.len() as u8)
                .filter(|&i| {
                    state.side(side).party[i as usize].hp > 0
                        && !state
                            .side(side)
                            .slots
                            .iter()
                            .any(|s| s.party_index == Some(i))
                })
                .collect();
            for party_index in bench {
                let switch = SlotAction::Switch { party_index };
                let result = Ruleset::CHAMPIONS_MC.validate_slot_action(&state, r, switch);
                if want {
                    assert_eq!(
                        result,
                        Err(ActionError::Trapped { slot }),
                        "{name}: {mon_name}"
                    );
                } else {
                    assert_eq!(result, Ok(()), "{name}: {mon_name}");
                }
            }
        }
    }
    let count: usize = ["p1", "p2"]
        .iter()
        .map(|k| fixture["trapped"][k].as_object().unwrap().len())
        .sum();
    assert_eq!(checked, count, "{name}");
}
