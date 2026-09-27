//! Board PY2 / R2-t1: canonical JSON → `State` (`lab_scenario::state_from_canonical`) and the
//! `x-hidden` extension (`canonical_json_hidden`), checked on the oracle fixtures' own states
//! (Showdown's `before` and outcomes) and on the engine's outcomes of the same turns.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use lab_engine::state::SideId;
use lab_scenario::{
    canonical_json, canonical_json_hidden, initial_order, load_scenario_file,
    run_decision_mid_turn, scenario_decision, scenario_positions, state_from_canonical,
    LoadedScenario, Position, ScenarioError, HIDDEN_KEY,
};

fn fixture_names() -> Vec<String> {
    let dir = common::engine_dir().join("oracle/expected");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".turn.json").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn scenario_path(name: &str) -> PathBuf {
    common::engine_dir().join(format!("oracle/scenarios/{name}.json"))
}

/// The fixture's scenario and the position its `before` names, if the scenario still loads.
fn start(name: &str, fixture: &Value) -> Option<(LoadedScenario, Position)> {
    let loaded = load_scenario_file(scenario_path(name)).ok()?;
    let before = common::key(&fixture["before"]);
    let position = scenario_positions(&loaded).ok()?.into_iter().find(|p| {
        canonical_json(&p.state, &loaded.meta)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .is_some_and(|v| common::key(&v) == before)
    })?;
    Some((loaded, position))
}

fn reason(e: &ScenarioError) -> String {
    // The kind of what is missing, without names and slots.
    let m = e.message();
    for key in [
        "volatile",
        "transformed",
        "future move",
        "mid-turn",
        "side condition",
        "weather",
        "without a duration",
    ] {
        if m.contains(key) {
            return key.to_owned();
        }
    }
    m.chars().take(80).collect()
}

/// Every canonical state Showdown wrote in the fixtures (`before` and each outcome) rebuilds
/// into a state that prints it again, or is refused as `Unsupported` for a documented reason
/// (hidden state with no default); nothing is `Invalid`.
#[test]
fn oracle_states_rebuild_to_the_same_canonical_state() {
    let mut ok = 0usize;
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut invalid = Vec::new();
    for name in fixture_names() {
        let fixture = common::fixture(&name);
        let Ok(loaded) = load_scenario_file(scenario_path(&name)) else {
            continue;
        };
        let mut states = vec![&fixture["before"]];
        if let Some(outcomes) = fixture["outcomes"].as_array() {
            states.extend(outcomes.iter().map(|o| &o["state"]));
        }
        for value in states {
            match state_from_canonical(&loaded.state, &loaded.meta, value) {
                Ok(rebuilt) => {
                    let text = canonical_json(&rebuilt.state, &loaded.meta).unwrap();
                    let printed: Value = serde_json::from_str(&text).unwrap();
                    assert_eq!(&printed, value, "{name}");
                    ok += 1;
                }
                Err(e) if e.is_unsupported() => *refused.entry(reason(&e)).or_default() += 1,
                Err(e) => invalid.push(format!("{name}: {}", e.message())),
            }
        }
    }
    eprintln!("oracle states: {ok} rebuilt, refused {refused:?}");
    assert!(
        invalid.is_empty(),
        "{} invalid: {:#?}",
        invalid.len(),
        &invalid[..invalid.len().min(10)]
    );
    assert!(ok > 5000, "only {ok} states rebuilt");
}

/// The engine's own outcome states: the canonical string rebuilds into a state that prints the
/// same bytes; with the `x-hidden` extension the rebuild is the state itself (and the party
/// orders); the extension's canonical part is byte-identical to `canonical_json`.
#[test]
fn engine_states_round_trip_exactly_with_the_extension() {
    let mut checked = 0usize;
    let mut plain_refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut hidden_keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut fixtures = 0usize;
    for name in fixture_names() {
        let fixture = common::fixture(&name);
        // Heavy turns are covered by the oracle-state test; keep this one quick.
        if fixture["outcomes"].as_array().is_none_or(|o| o.len() > 300) {
            continue;
        }
        let Some((loaded, position)) = start(&name, &fixture) else {
            continue;
        };
        let Ok(decision) = scenario_decision(&loaded, &position) else {
            continue;
        };
        let mut state = position.state.clone();
        let Ok(outcomes) =
            run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn)
        else {
            continue;
        };
        fixtures += 1;
        for outcome in outcomes.iter().take(40) {
            if outcome.suspension.is_some() {
                continue;
            }
            let mut end = position.state.clone();
            end.apply(&outcome.instructions);
            let mut order = position.order.clone();
            lab_scenario::advance_order(&mut order, &outcome.instructions);
            let Ok(text) = canonical_json(&end, &loaded.meta) else {
                continue;
            };

            // Plain canonical: same bytes back.
            let value: Value = serde_json::from_str(&text).unwrap();
            match state_from_canonical(&position.state, &loaded.meta, &value) {
                Ok(rebuilt) => assert_eq!(
                    canonical_json(&rebuilt.state, &loaded.meta).unwrap(),
                    text,
                    "{name}"
                ),
                Err(e) if e.is_unsupported() => *plain_refused.entry(reason(&e)).or_default() += 1,
                Err(e) => panic!("{name}: {}", e.message()),
            }

            // With the extension: the state itself.
            let hidden = canonical_json_hidden(&end, &loaded.meta, Some(&order))
                .unwrap_or_else(|e| panic!("{name}: {}", e.message()));
            let with: Value = serde_json::from_str(&hidden).unwrap();
            if let Some(h) = with.get(HIDDEN_KEY) {
                assert!(hidden.starts_with(&text[..text.len() - 1]), "{name}");
                count_keys(h, "", &mut hidden_keys);
            } else {
                assert_eq!(hidden, text);
            }
            let back = state_from_canonical(&position.state, &loaded.meta, &with)
                .unwrap_or_else(|e| panic!("{name}: {}", e.message()));
            assert_eq!(back.state, end, "{name}");
            assert_eq!(back.order, order, "{name}");
            checked += 1;
        }
    }
    eprintln!(
        "engine states: {checked} from {fixtures} fixtures; plain canonical refused {plain_refused:?}"
    );
    eprintln!("x-hidden fields used: {hidden_keys:#?}");
    assert!(checked > 2000, "only {checked} states checked");
}

fn count_keys(value: &Value, prefix: &str, out: &mut BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                // Names and ids below these keys vary; count the key itself.
                let leaf = matches!(prefix, "pokemon" | "volatiles");
                let key = if leaf { prefix.to_owned() } else { k.clone() };
                if leaf {
                    if let Value::Object(fields) = v {
                        for f in fields.keys() {
                            *out.entry(format!("pokemon.{f}")).or_default() += 1;
                        }
                        continue;
                    }
                    *out.entry(format!("volatile.{k}")).or_default() += 1;
                    continue;
                }
                count_keys(v, &key, out);
            }
        }
        Value::Array(list) if matches!(prefix, "sides" | "slots") => {
            for v in list {
                count_keys(v, prefix, out);
            }
        }
        _ => *out.entry(prefix.to_owned()).or_default() += 1,
    }
}

/// A hand-written position: the hypnosis-gravity leads with edited HP, a status, a boost and
/// Tailwind, built from the scenario's loaded state as the base.
#[test]
fn a_hand_edited_position_rebuilds() {
    let loaded = load_scenario_file(scenario_path("hypnosis-gravity")).unwrap();
    let position = &scenario_positions(&loaded).unwrap()[0];
    let text = canonical_json(&position.state, &loaded.meta).unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    let mon = &mut value["sides"][1]["pokemon"][0];
    let max = mon["maxhp"].as_i64().unwrap();
    mon["hp"] = (max / 3).into();
    mon["status"] = "par".into();
    mon["boosts"]["spe"] = 2.into();
    value["sides"][0]["conditions"]["tailwind"] = serde_json::json!({"duration": 3});
    let rebuilt = state_from_canonical(&loaded.state, &loaded.meta, &value).unwrap();
    let side = rebuilt.state.side(SideId::Two);
    let edited = side.party.iter().filter(|m| m.hp == max as i16 / 3).count();
    assert!(edited >= 1);
    assert_eq!(rebuilt.order[0], initial_order(&rebuilt.state, SideId::One));
    // The scenario's own turn still parses and runs from the edited position.
    let pos = Position {
        probability: 1.0,
        state: rebuilt.state.clone(),
        order: rebuilt.order.clone(),
    };
    let decision = scenario_decision(&loaded, &pos).unwrap();
    let mut state = pos.state.clone();
    let outcomes =
        run_decision_mid_turn(&mut state, &pos.order, &decision, &loaded.mid_turn).unwrap();
    let total: f64 = outcomes.iter().map(|o| o.probability).sum();
    assert!((total - 1.0).abs() < 1e-9);
}

/// What the canonical form cannot say is refused, not guessed: a mid-turn switch request, a
/// lossy volatile without the extension, an unknown name.
#[test]
fn missing_hidden_state_is_refused() {
    let loaded = load_scenario_file(scenario_path("hypnosis-gravity")).unwrap();
    let position = &scenario_positions(&loaded).unwrap()[0];
    let text = canonical_json(&position.state, &loaded.meta).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();

    let mut seeded = value.clone();
    seeded["sides"][0]["pokemon"][0]["volatiles"]["leechseed"] = serde_json::json!({});
    let active = seeded["sides"][0]["pokemon"][0]["slot"].clone();
    if !active.is_null() {
        let e = state_from_canonical(&loaded.state, &loaded.meta, &seeded).unwrap_err();
        assert!(e.is_unsupported(), "{}", e.message());
        assert!(e.message().contains(HIDDEN_KEY), "{}", e.message());
    }

    let mut renamed = value.clone();
    renamed["sides"][0]["pokemon"][0]["name"] = "Nobody".into();
    let e = state_from_canonical(&loaded.state, &loaded.meta, &renamed).unwrap_err();
    assert!(!e.is_unsupported());

    let mut switch = value;
    switch["sides"][0]["request"] = "switch".into();
    let e = state_from_canonical(&loaded.state, &loaded.meta, &switch).unwrap_err();
    assert!(e.message().contains("request"), "{}", e.message());
}

/// Board R2-t1: the entry hazard order is hidden from the canonical form but decides the turn
/// (tt-hazard-order-toxic-first: Toxic Spikes set before Stealth Rock, so Arbok absorbs them
/// before the rocks knock it out). With `x-hidden` the rebuilt position gives the oracle's
/// distribution; from the plain canonical state it gets the default (id) order, in which the
/// rocks come first, and a different distribution.
#[test]
fn hazard_order_needs_the_extension() {
    let name = "tt-hazard-order-toxic-first";
    let fixture = common::fixture(name);
    let (loaded, position) = start(name, &fixture).unwrap();
    let decision = scenario_decision(&loaded, &position).unwrap();
    let oracle = common::oracle_distribution(&fixture);
    let run = |rebuilt: &lab_scenario::Rebuilt<2>| {
        let mut state = rebuilt.state.clone();
        let outcomes =
            run_decision_mid_turn(&mut state, &rebuilt.order, &decision, &loaded.mid_turn).unwrap();
        common::distribution(&loaded, &mut state, &outcomes)
    };

    let text = canonical_json_hidden(&position.state, &loaded.meta, Some(&position.order)).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value[HIDDEN_KEY]["sides"][1]["hazardOrder"],
        serde_json::json!(["toxicspikes", "stealthrock"]),
        "{text}"
    );
    let exact = state_from_canonical(&loaded.state, &loaded.meta, &value).unwrap();
    assert_eq!(run(&exact), oracle);

    let mut plain = value.clone();
    plain.as_object_mut().unwrap().remove(HIDDEN_KEY);
    let default = state_from_canonical(&loaded.state, &loaded.meta, &plain).unwrap();
    assert_ne!(run(&default), oracle);
}
