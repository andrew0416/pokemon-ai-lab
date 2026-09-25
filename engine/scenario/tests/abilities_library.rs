//! Parity of the abilities the library teams use (O70 type changers, the auras, Flower Veil, No
//! Guard, Cursed Body, Protosynthesis / Quark Drive, Magic Bounce, the trapping abilities, Trace
//! on a mid-battle switch-in) with Showdown's outcome distribution
//! (`engine/oracle/expected/*.turn.json`).

mod common;

use common::{assert_exact_parity, assert_mc_parity};
use lab_engine::action::SlotAction;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{enumerate_turn, trapped, TurnError};
use lab_scenario::scenario_choices;

// ---- O70 onModifyType / onBasePower ------------------------------------------------------------

/// Pixilate and Aerilate turn Strength Fairy / Flying (a Ghost is no longer immune), 4915/4096.
#[test]
fn pixilate_and_aerilate_match_showdown() {
    assert_exact_parity("o70-pixilate-aerilate");
}

/// Galvanize's Electric Strength is absorbed by Volt Absorb; Refrigerate's Ice Strength.
#[test]
fn galvanize_and_refrigerate_match_showdown() {
    assert_exact_parity("o70-galvanize-refrigerate");
}

/// Normalize makes Dragon Pulse Normal (and boosts it); Liquid Voice's Water Sing is absorbed.
#[test]
fn normalize_and_liquid_voice_match_showdown() {
    assert_exact_parity("o70-normalize-liquid-voice");
}

// ---- Fairy Aura / Dark Aura / Aura Break -------------------------------------------------------

/// Two Fairy Aura holders boost a Fairy move once (`move.auraBooster`); Dark Aura its own move.
#[test]
fn fairy_and_dark_aura_match_showdown() {
    assert_exact_parity("aura-fairy-dark");
}

/// Aura Break reverses the aura (3072/4096) unless a Mold Breaker move skips it.
#[test]
fn aura_break_matches_showdown() {
    assert_exact_parity("aura-break");
}

// ---- Flower Veil -------------------------------------------------------------------------------

/// A Grass ally is spared a foe's status and stat drops; the non-Grass holder is not.
#[test]
fn flower_veil_status_and_drops_match_showdown() {
    assert_exact_parity("flower-veil-status-boost");
}

/// Yawn is blocked, a self-inflicted drop is not, a Mold Breaker move ignores Flower Veil.
#[test]
fn flower_veil_yawn_self_drop_mold_breaker_match_showdown() {
    assert_exact_parity("flower-veil-yawn-mold-breaker");
}

/// Flower Veil and the target's Mirror Armor run in their holders' Speed order.
#[test]
fn flower_veil_against_mirror_armor_matches_showdown() {
    assert_exact_parity("flower-veil-mirror-armor");
    assert_exact_parity("flower-veil-mirror-armor-faster");
}

// ---- No Guard and the Accuracy event -----------------------------------------------------------

/// Moves by and against a No Guard holder never miss.
#[test]
fn no_guard_matches_showdown() {
    assert_exact_parity("no-guard");
}

/// Micle Berry's Accuracy handler ends its volatile whatever the accuracy: for a move that
/// never misses, and after a Glaive Rush drawback (a faster holder) already answered `true`.
#[test]
fn micle_berry_ends_on_every_accuracy_event() {
    assert_exact_parity("micle-accuracy-true");
    assert_exact_parity("micle-glaive-rush");
}

// ---- Cursed Body -------------------------------------------------------------------------------

/// 30% Disable on the attacker's move, through Mold Breaker (not breakable).
#[test]
fn cursed_body_matches_showdown() {
    assert_exact_parity("cursed-body");
}

/// A multi-hit move rolls once per hit until the attacker is disabled (Monte Carlo fixture).
#[test]
fn cursed_body_multihit_matches_showdown() {
    assert_mc_parity("cursed-body-multihit");
}

// ---- O72 / O98 Protosynthesis, Quark Drive, Booster Energy -------------------------------------

/// Sun at the battle start: the attacker's best Attack and the defender's best Defense.
#[test]
fn protosynthesis_in_sun_matches_showdown() {
    assert_exact_parity("o72-protosynthesis-sun");
}

/// The best stat is picked with the stat stages Intimidate left (switch-in priority -2).
#[test]
fn protosynthesis_best_stat_after_intimidate_matches_showdown() {
    assert_exact_parity("o72-best-stat-intimidate");
}

/// Electric Surge's terrain for Quark Drive; Booster Energy used at once without sun, Speed as
/// the best stat (1.5x) changes the action order.
#[test]
fn quark_drive_and_booster_energy_match_showdown() {
    assert_exact_parity("o72-quark-drive-booster");
}

/// Rain replaces the sun: the condition ends, then Booster Energy starts it again.
#[test]
fn protosynthesis_ends_with_the_sun_matches_showdown() {
    assert_exact_parity("o72-sun-ends");
}

/// Knock Off cannot take Booster Energy from a Paradox Pokémon (and gets no boost), but can
/// from anyone else.
#[test]
fn booster_energy_knock_off_matches_showdown() {
    assert_exact_parity("o98-booster-knock-off");
}

// ---- O71 Trace on a switch during the turn -----------------------------------------------------

/// A uniformly random traceable foe, and the copied Intimidate starts at once.
#[test]
fn trace_on_a_turn_switch_matches_showdown() {
    assert_exact_parity("o71-trace-switch");
}

/// A `notrace` foe (Protosynthesis) is never copied.
#[test]
fn trace_on_a_turn_switch_skips_untraceable_foes() {
    assert_exact_parity("o71-trace-switch-notrace");
}

// ---- Magic Bounce ------------------------------------------------------------------------------

/// A Prankster status move bounces back without the Prankster boost; a spread status move
/// bounces at both foes and still hits the holder's ally.
#[test]
fn magic_bounce_matches_showdown() {
    assert_exact_parity("magic-bounce");
}

/// Stealth Rock bounces onto its user's side (`onAllyTryHitSide`); Mold Breaker gets through.
#[test]
fn magic_bounce_side_and_mold_breaker_match_showdown() {
    assert_exact_parity("magic-bounce-side");
}

// ---- Shadow Tag / Arena Trap / Magnet Pull -----------------------------------------------------

/// `turn::trapped` for every active Pokémon of the scenario's position against Showdown's
/// `pokemon.trapped` (`oracle/trapped.cjs` → `oracle/expected/<name>.trapped.json`), and a switch
/// choice of each trapped one is rejected (`ActionError::Trapped`) and never generated.
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
            // Every bench member as a switch candidate.
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
                let mut generated = Vec::new();
                let pass = [SlotAction::Pass];
                let mine = [switch];
                let candidates: [&[SlotAction]; 2] = if slot == 0 {
                    [&mine, &pass]
                } else {
                    [&pass, &mine]
                };
                Ruleset::CHAMPIONS_MC.joint_actions(&state, side, candidates, &mut generated);
                assert_eq!(generated.is_empty(), want, "{name}: {mon_name}");
            }
        }
    }
    assert_eq!(checked, expected_count(&fixture), "{name}");
}

fn expected_count(fixture: &serde_json::Value) -> usize {
    ["p1", "p2"]
        .iter()
        .map(|k| fixture["trapped"][k].as_object().unwrap().len())
        .sum()
}

/// Magnet Pull traps a Steel type, Arena Trap only a grounded foe, a Ghost type is immune; the
/// Ghost's legal switch.
#[test]
fn arena_trap_and_magnet_pull_match_showdown() {
    assert_trapped_parity("trap-arena-magnet");
    assert_exact_parity("trap-arena-magnet");
}

/// Shadow Tag does not trap another Shadow Tag holder, Shed Shell frees its holder.
#[test]
fn shadow_tag_and_shed_shell_match_showdown() {
    assert_trapped_parity("trap-shadow-tag");
    assert_exact_parity("trap-shadow-tag");
}

/// Levitate and Air Balloon escape Arena Trap, until Gravity grounds them.
#[test]
fn arena_trap_grounding_matches_showdown() {
    assert_trapped_parity("trap-arena-levitate");
    assert_exact_parity("trap-arena-levitate");
    assert_trapped_parity("trap-arena-gravity");
}

/// A trapped Pokémon's switch in the turn's choices is refused before anything runs.
#[test]
fn trapped_switch_is_an_invalid_choice() {
    let fixture = common::fixture("trap-arena-magnet");
    let (loaded, position) = common::start("trap-arena-magnet", &fixture);
    let mut state = position.state;
    let mut choices = scenario_choices(&loaded, &state).unwrap();
    // Skarmory (p2 slot 0, trapped by Magnet Pull) tries to switch to Snorlax (party 2).
    choices[1][0] = SlotAction::Switch { party_index: 2 };
    choices[1][1] = SlotAction::Move {
        index: 0,
        target: 0,
        gimmick: lab_engine::gimmick::Gimmick::None,
    };
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::Action {
            side: SideId::Two,
            error: ActionError::Trapped { slot: 0 },
        }) => {}
        other => panic!("expected a trapped switch to be refused, got {other:?}"),
    }
}
