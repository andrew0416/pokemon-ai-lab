//! Parity of Commander (Opus S unit 3: `onStart` / `onAnySwitchIn` / `onUpdate` and the
//! `commanding` / `commanded` conditions) with Showdown: each scenario's exact outcome
//! distribution must equal its oracle fixture, and the trapping and forced pass must match.

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::{ActionError, Ruleset};
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::{enumerate_turn, legal_joint_actions, trapped, TurnError};
use lab_scenario::scenario_choices;

/// Tatsugiri and Dondozo lead: commanded at the start (+2 all); moves at Tatsugiri miss, spread
/// moves skip it; the commanded Dondozo's U-turn and Eject Button do not switch it out.
#[test]
fn commander_at_the_battle_start_matches_showdown() {
    assert_exact_parity("s-commander-start");
}

/// Dondozo switching in next to Tatsugiri: commanded in the runSwitch, Tatsugiri's queued move
/// cancelled.
#[test]
fn commander_on_dondozo_switch_in_matches_showdown() {
    assert_exact_parity("s-commander-switch-in");
}

/// Dondozo fainting ends commanding at the action's Update: Tatsugiri can be hit again.
#[test]
fn commander_ends_when_dondozo_faints_matches_showdown() {
    assert_exact_parity("s-commander-dondozo-faints");
}

/// Perish Song misses the commanding Tatsugiri; Roar fails on the commanded Dondozo.
#[test]
fn commander_against_perish_song_and_roar_matches_showdown() {
    assert_exact_parity("s-commander-roar-perish");
}

/// Perish Song's `Invulnerability` check (shared with Commander) also skips a Pokémon in the
/// middle of Fly.
#[test]
fn perish_song_misses_a_flying_pokemon() {
    assert_exact_parity("s-perish-song-fly");
}

/// Both Commander Pokémon are trapped (the commanded Dondozo despite Shed Shell), as Showdown's
/// `pokemon.trapped` says (`oracle/trapped.cjs`); a switch choice is refused; the commanding
/// Tatsugiri can only pass.
#[test]
fn commander_traps_and_forces_a_pass() {
    let name = "s-commander-trapped";
    let path = common::engine_dir().join(format!("oracle/expected/{name}.trapped.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let fixture: serde_json::Value = serde_json::from_str(&text).unwrap();
    let (loaded, position) = common::start(name, &fixture);
    let state = position.state;
    for (key, side) in [("p1", SideId::One), ("p2", SideId::Two)] {
        let expected = fixture["trapped"][key].as_object().unwrap();
        for slot in 0..2u8 {
            let r = SlotRef { side, slot };
            let party = state.slot(r).party_index.expect("both slots are occupied");
            let mon_name = loaded.meta.sides[side.index()].name(party).unwrap();
            let want = expected[mon_name].as_bool().unwrap();
            assert_eq!(trapped(&state, r), want, "{mon_name}");
            let bench = SlotAction::Switch { party_index: 2 };
            let result = Ruleset::CHAMPIONS_MC.validate_slot_action(&state, r, bench);
            if want {
                assert_eq!(result, Err(ActionError::Trapped { slot }), "{mon_name}");
            } else {
                assert_eq!(result, Ok(()), "{mon_name}");
            }
        }
    }
    // Tatsugiri (p1 slot 0) only passes; Dondozo only moves (it is trapped).
    let legal = legal_joint_actions(&state, Ruleset::CHAMPIONS_MC, SideId::One);
    assert!(!legal.is_empty());
    for action in &legal {
        assert_eq!(action[0], SlotAction::Pass, "{action:?}");
        assert!(matches!(action[1], SlotAction::Move { .. }), "{action:?}");
    }
    // A move chosen for the commanding Tatsugiri is refused.
    let loaded_turn = common::fixture("s-commander-start");
    let (loaded_start, start) = common::start("s-commander-start", &loaded_turn);
    let mut state = start.state;
    let mut choices = scenario_choices(&loaded_start, &state).unwrap();
    choices[0][0] = SlotAction::Move {
        index: 0,
        target: 1,
        gimmick: Gimmick::None,
    };
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::InvalidChoice {
            side: SideId::One,
            slot: 0,
            ..
        }) => {}
        other => panic!("expected the commanding Tatsugiri's move to be refused, got {other:?}"),
    }
}
