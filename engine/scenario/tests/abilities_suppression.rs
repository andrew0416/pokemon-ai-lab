//! Ability suppression (Opus U): Gastro Acid's volatile and Neutralizing Gas make
//! `Battle::ability` `NONE` (Showdown `ignoringAbility`); Neutralizing Gas leaving restarts the
//! abilities it suppressed. Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`, `<name>.trapped.json`).

mod common;

use common::assert_exact_parity;
use lab_engine::action::SlotAction;
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, SlotRef};
use lab_engine::turn::trapped;

/// Gastro Acid on a Levitate holder: the volatile is added and the following Ground move hits.
#[test]
fn gastro_acid_grounds_a_levitate_holder() {
    assert_exact_parity("u-gastro-acid-levitate");
}

/// Gastro Acid fails on a `cantsuppress` ability (Disguise) and on an Ability Shield holder.
#[test]
fn gastro_acid_fails_on_cantsuppress_and_ability_shield() {
    assert_exact_parity("u-gastro-acid-fails");
}

/// Neutralizing Gas at the battle start keeps Sand Stream from starting; switching out, its End
/// restarts it before the newcomer arrives.
#[test]
fn neutralizing_gas_silences_sand_stream_until_it_switches_out() {
    assert_exact_parity("u-neutralizing-gas-sand");
}

/// A fainting Neutralizing Gas holder's End (in `faintMessages`) restarts Intimidate.
#[test]
fn neutralizing_gas_fainting_restarts_intimidate() {
    assert_exact_parity("u-neutralizing-gas-faint");
}

/// Gastro Acid on the Neutralizing Gas holder runs its End: Intimidate restarts and hits both
/// foes, the holder included.
#[test]
fn gastro_acid_on_neutralizing_gas_restarts_the_others() {
    assert_exact_parity("u-gastro-acid-neutralizing-gas");
}

/// Shadow Tag does not trap while Neutralizing Gas is on the field (`turn::trapped`, whose foe
/// pre-check reads the abilities as they act), and the switch goes through.
#[test]
fn neutralizing_gas_suppresses_shadow_tag() {
    let name = "u-neutralizing-gas-shadow-tag";
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
        }
    }
    let snorlax = SlotRef {
        side: SideId::One,
        slot: 1,
    };
    let switch = SlotAction::Switch { party_index: 2 };
    assert_eq!(
        Ruleset::CHAMPIONS_MC.validate_slot_action(&state, snorlax, switch),
        Ok(())
    );
    assert_exact_parity(name);
}
