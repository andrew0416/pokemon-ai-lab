//! Parity of Eject Pack (Opus W unit 1) with Showdown: a stat drop sets the pack's flag
//! (`onAfterBoost`), the next trigger uses it and the turn suspends for the holder's switch.

mod common;

use common::{assert_exact_parity, assert_extremes_parity, engine_dir};
use lab_engine::dex::abilities;
use lab_engine::state::{SideId, SlotRef};
use lab_scenario::{expand_switch_ins, load_scenario_file, SwitchInError};

/// A lead's Intimidate using a foe's Eject Pack at the battle start asks for a switch before
/// turn 1, which the start cannot suspend for: refused.
#[test]
fn eject_pack_at_the_battle_start_is_refused() {
    let loaded =
        load_scenario_file(engine_dir().join("oracle/scenarios/w-eject-pack-intimidate.json"))
            .unwrap();
    let mut state = loaded.state.clone();
    let swampert = SlotRef {
        side: SideId::Two,
        slot: 0,
    };
    let lead = state.active_mut(swampert).unwrap();
    lead.ability = abilities::INTIMIDATE;
    lead.base_ability = abilities::INTIMIDATE;
    match expand_switch_ins(&state) {
        Err(SwitchInError::Unsupported { what }) => {
            assert!(
                what.contains("Eject Pack") && what.contains("battle start"),
                "{what}"
            )
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

/// Intimidate from a switch-in: the pack is used in the same switch-in batch (`onAnySwitchIn`).
#[test]
fn eject_pack_after_intimidate_matches_showdown() {
    assert_exact_parity("w-eject-pack-intimidate");
}

/// The holder's own Close Combat drops: used at the move's AfterMove, before anyone else moves.
#[test]
fn eject_pack_after_a_self_drop_matches_showdown() {
    assert_exact_parity("w-eject-pack-close-combat");
}

/// Two holders dropped by one Icy Wind: the faster switches first; the other's flag waits for
/// the replacement's switch-in, which asks for a second switch.
#[test]
fn two_eject_packs_after_icy_wind_match_showdown() {
    assert_extremes_parity("w-eject-pack-icy-wind");
}

/// Intimidate from a Mega Evolution: the pack is used at the AfterMega (`onAnyAfterMega`).
#[test]
fn eject_pack_after_a_mega_intimidate_matches_showdown() {
    assert_exact_parity("w-eject-pack-mega");
}

/// Drops from Parting Shot do not count; only the Parting Shot user switches.
#[test]
fn eject_pack_ignores_parting_shot_matches_showdown() {
    assert_exact_parity("w-eject-pack-parting-shot");
}
