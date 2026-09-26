//! Parity of moves Champions doubles players use that no library team carries (Opus P): Heal
//! Pulse, Ally Switch, ... Each scenario's exact outcome distribution must equal its oracle
//! fixture (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{trapped, TurnError};
use lab_scenario::{load_scenario_file, run_decision, scenario_decision, scenario_positions};

/// `turn::trapped` for every active Pokémon of the scenario's position against Showdown's
/// `pokemon.trapped` (`oracle/trapped.cjs` → `oracle/expected/<name>.trapped.json`), and the
/// ruleset rejects exactly the trapped ones' switches.
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

/// Runs the scenario's turn (first position) and expects `Unsupported` naming `expected`.
fn assert_unsupported(name: &str, expected: &str) {
    let loaded =
        load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
    let position = scenario_positions(&loaded).unwrap().remove(0);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    match run_decision(&mut state, &decision) {
        Err(TurnError::Unsupported(what)) => assert!(what.contains(expected), "{what}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn heal_pulse_heals_half_or_three_quarters_with_mega_launcher() {
    assert_exact_parity("heal-pulse");
}

#[test]
fn heal_pulse_bounces_and_is_blocked_by_protect() {
    assert_exact_parity("heal-pulse-bounce");
}

/// The foes' moves aimed at a position hit whoever stands there after the swap; the partner's
/// queued move follows it.
#[test]
fn ally_switch_swaps_positions_and_queued_moves_follow() {
    assert_exact_parity("ally-switch");
}

/// The second use in a row succeeds with 1/3 (counter 3, then 9) or deletes the volatile.
#[test]
fn ally_switch_again_succeeds_one_time_in_three() {
    assert_exact_parity("ally-switch-again");
}

/// A Healing Wish waiting at the new position heals the ally moved into it (`onSwap`).
#[test]
fn ally_switch_into_a_waiting_healing_wish() {
    assert_exact_parity("ally-switch-healing-wish");
}

#[test]
fn ally_switch_fails_without_a_standing_partner() {
    assert_exact_parity("ally-switch-fail");
}

#[test]
fn ally_switch_under_a_snipe_shot_is_unsupported() {
    assert_unsupported("ally-switch-snipe-shot", "tracks its original target");
}

/// Crafty Shield blocks status moves (the side's own ally's too) before Magic Bounce acts;
/// damaging moves pass.
#[test]
fn crafty_shield_blocks_status_moves_before_magic_bounce() {
    assert_exact_parity("crafty-shield");
}

/// Mat Block on the first turn out blocks damaging moves (a spread move before its accuracy
/// roll), not status moves.
#[test]
fn mat_block_blocks_damaging_moves_on_the_first_turn() {
    assert_exact_parity("mat-block");
}

#[test]
fn mat_block_fails_after_the_first_turn_out() {
    assert_exact_parity("mat-block-late");
}

#[test]
fn feint_breaks_crafty_shield() {
    assert_exact_parity("feint-crafty-shield");
}

/// Protect's TryHit (priority 3) comes before Magic Bounce's (priority 1): nothing bounces.
#[test]
fn protect_stops_a_move_before_magic_bounce() {
    assert_exact_parity("magic-bounce-protect");
}

/// Heavy Slam at 80 base power (3x to 4x the target's weight).
#[test]
fn heavy_slam_power_from_the_weight_ratio() {
    assert_exact_parity("heavy-slam");
}

/// Heat Crash just over 2x the target's weight: 60 base power.
#[test]
fn heat_crash_power_at_a_bracket_edge() {
    assert_exact_parity("heat-crash");
}

/// Super Fang, Ruination, Nature's Madness take half the target's HP (floored); Revenge doubles
/// against a Pokémon that damaged its user this turn.
#[test]
fn half_hp_damage_callbacks_and_revenge_doubled() {
    assert_exact_parity("super-fang");
}

/// Super Fang on 1 HP deals 1; Revenge keeps its power against a Pokémon that did not hit.
#[test]
fn super_fang_minimum_and_revenge_plain() {
    assert_exact_parity("revenge-plain");
}

/// Mean Look, Block (a Ghost is immune), Spider Web; a trapped target cannot be trapped again.
#[test]
fn mean_look_block_spider_web_trap_with_linked_volatiles() {
    assert_exact_parity("mean-look");
}

/// The trapper switching out frees its target; the trapped Pokémon cannot choose to switch, a
/// Shed Shell holder can.
#[test]
fn mean_look_ends_when_the_trapper_switches_out() {
    assert_exact_parity("mean-look-switch");
    assert_trapped_parity("mean-look-switch");
}

#[test]
fn mean_look_ends_when_the_trapper_faints() {
    assert_exact_parity("mean-look-faint");
}

/// No Retreat from a trapped user boosts without adding its own volatile.
#[test]
fn no_retreat_of_a_trapped_user_only_boosts() {
    assert_exact_parity("no-retreat-trapped");
}
