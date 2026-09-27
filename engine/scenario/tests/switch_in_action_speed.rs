//! Parity of the switch-in handlers' order (board B30): `switchIn` queues the newcomer's
//! `runSwitch` with `BattleQueue.insertChoice`, which calls `pokemon.updateSpeed()`, so the
//! batched `fieldEvent('SwitchIn')` sorts its handlers by the newcomers' *action* Speed (Choice
//! Scarf, paralysis, Tailwind, and Trick Room's `10000 - speed`), not by the raw stored Speed.

mod common;

use common::assert_exact_parity;
use lab_scenario::{canonical_value, initial_outcomes, load_scenario_file};
use serde_json::Value;

/// Two replacements at equal raw Speed 85; Pelipper's Choice Scarf makes its Drizzle run first
/// every time, so Abomasnow's snow remains (raw Speeds would tie and draw the order).
#[test]
fn a_choice_scarf_orders_the_replacements_start_handlers() {
    assert_exact_parity("f-switch-in-scarf-replace");
}

/// Under Trick Room the slower replacement's handler runs first: Snow Warning, then Drizzle,
/// and rain remains.
#[test]
fn trick_room_reverses_the_replacements_start_handlers() {
    assert_exact_parity("f-switch-in-trick-room-replace");
}

/// The same at the battle start: the leads' `runSwitch` actions are queued one by one, each
/// with its action Speed, so the start has one outcome (snow), not a drawn order
/// (`initial.cjs`: 2 branches, 1 outcome). The turn fixture then starts from that position.
#[test]
fn a_choice_scarf_orders_the_leads_start_handlers() {
    let engine = common::engine_dir();
    let loaded =
        load_scenario_file(engine.join("oracle/scenarios/f-switch-in-scarf-start.json")).unwrap();
    let ours: Vec<(f64, Value)> = initial_outcomes(&loaded)
        .unwrap()
        .iter()
        .map(|o| {
            (
                o.probability,
                canonical_value(&o.state, &loaded.meta).unwrap(),
            )
        })
        .collect();
    let path = engine.join("oracle/expected/f-switch-in-scarf-start.initial.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let expected = fixture["outcomes"].as_array().unwrap();
    assert_eq!(expected.len(), 1);
    assert_eq!(ours.len(), expected.len());
    for o in expected {
        let p = o["p"].as_f64().unwrap();
        let found = ours.iter().find(|(_, state)| *state == o["state"]);
        let (q, _) = found.unwrap_or_else(|| panic!("no engine outcome equals {}", o["state"]));
        assert!((p - q).abs() < 1e-12, "p {p} vs {q}");
    }
    assert_exact_parity("f-switch-in-scarf-start");
}
