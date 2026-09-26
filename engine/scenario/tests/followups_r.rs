//! Follow-up fixes reported by earlier sessions (Opus R): event-order details of mid-turn
//! switches, spread AfterMoveSecondary, trapping, guards, accuracy, ties, Speed and weight.
//! Fixtures from Showdown's exact enumeration.

mod common;

use common::{assert_exact_parity, assert_extremes_parity};
use lab_engine::action::SlotAction;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::trapped;

/// `turn::trapped` for every active Pokémon of the scenario's position against Showdown's
/// `pokemon.trapped` (`oracle/trapped.cjs` → `oracle/expected/<name>.trapped.json`), and a switch
/// of each trapped one is rejected (`ActionError::Trapped`) while a free one's is accepted.
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
            let bench = (0..state.side(side).party.len() as u8).filter(|&i| {
                state.side(side).party[i as usize].hp > 0
                    && !state
                        .side(side)
                        .slots
                        .iter()
                        .any(|s| s.party_index == Some(i))
            });
            for party_index in bench {
                let result = Ruleset::CHAMPIONS_MC.validate_slot_action(
                    &state,
                    r,
                    SlotAction::Switch { party_index },
                );
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

/// Mid-turn switch-outs run no BeforeSwitchOut Update (`skipBeforeSwitchOutEventFlag`): with
/// the Unnerve holder gone first, the U-turn user still leaves without eating its Sitrus Berry.
#[test]
fn instaswitch_skips_the_pre_switch_update() {
    assert_exact_parity("r-instaswitch-skips-update");
}

/// One spread move, two Eject Buttons: the speed-sorted AfterMoveSecondary lets the faster
/// holder switch out; the slower one's button stays unused.
#[test]
fn spread_eject_buttons_act_in_speed_order() {
    assert_exact_parity("r-spread-eject-buttons");
}

/// Two Eject Buttons at the same Speed: the tied handlers are shuffled, each holder switches out
/// half of the time (Monte Carlo fixture: the tie also shuffles every Update).
#[test]
fn tied_spread_eject_buttons_act_in_random_order() {
    common::assert_mc_parity("r-spread-eject-buttons-tie");
}

/// One spread move, two Red Cards: the faster holder's card drags the attacker; the slower
/// one's card sees the attacker's `forceSwitchFlag` and stays held.
#[test]
fn spread_red_cards_act_in_speed_order() {
    assert_exact_parity("r-spread-red-cards");
}

/// Magic Room suppresses Shed Shell: Shadow Tag traps its holder.
#[test]
fn magic_room_suppresses_shed_shell_against_shadow_tag() {
    assert_trapped_parity("r-shed-shell-magic-room");
    assert_exact_parity("r-shed-shell-magic-room");
}

/// Klutz suppresses its holder's Shed Shell: Shadow Tag traps it; a plain holder switches.
#[test]
fn klutz_suppresses_shed_shell_against_shadow_tag() {
    assert_trapped_parity("r-shed-shell-klutz");
    assert_exact_parity("r-shed-shell-klutz");
}

/// Triple Axel's later hits re-roll accuracy with float stages first: at +1 evasion the
/// fractional 67.5 skips Compound Eyes and hits 68 times in 100 (the first hit: 87).
#[test]
fn multi_accuracy_reroll_applies_float_stages_before_modifiers() {
    assert_extremes_parity("r-triple-axel-evasion");
}

/// An Explosion that takes out every Pokémon: the user is queued to faint first, so the side of
/// the last target dequeued wins (p2 when p1 explodes, p1 when p2 does), not a tie.
#[test]
fn everyone_fainting_at_once_goes_to_the_last_dequeued_side() {
    assert_extremes_parity("r-explosion-everyone-p1");
    assert_extremes_parity("r-explosion-everyone-p2");
}

/// Power Anklet and Macho Brace halve Speed, Quick Powder doubles Ditto's: the move order
/// decides which weather and terrain stay.
#[test]
fn speed_items_change_the_move_order() {
    assert_exact_parity("r-speed-items");
}

/// Klutz: Power Anklet still halves (`ignoreKlutz`), Choice Scarf does nothing.
#[test]
fn klutz_keeps_power_items_and_drops_choice_scarf_speed() {
    assert_exact_parity("r-speed-items-klutz");
}

/// Magic Room: Iron Ball no longer halves its holder's Speed.
#[test]
fn magic_room_drops_iron_ball_speed() {
    assert_exact_parity("r-speed-items-magic-room");
}

/// ModifyWeight: Heavy Metal doubles the Heavy Slam user's weight (120 power, not 80); Light
/// Metal halves the Low Kick target's (80, not 100).
#[test]
fn heavy_and_light_metal_change_weight_based_power() {
    assert_exact_parity("r-weight-metal");
}

/// ModifyWeight: Float Stone halves the Low Kick target's weight (40 power, not 60); Mold
/// Breaker ignores the target's Light Metal (100 power).
#[test]
fn float_stone_halves_weight_and_mold_breaker_ignores_light_metal() {
    assert_exact_parity("r-weight-float-stone");
}
