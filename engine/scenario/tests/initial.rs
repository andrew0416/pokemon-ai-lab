//! Initial switch-in expansion and canonical serialization against the oracle fixture
//! `engine/oracle/expected/single-hit.initial.json` (from `engine/oracle/initial.cjs`).

use std::path::{Path, PathBuf};

use serde_json::Value;

use lab_engine::dex::{abilities, items, AbilityId};
use lab_engine::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use lab_engine::gimmick::GimmickSet;
use lab_engine::state::{SideId, SlotRef, State};
use lab_engine::Doubles;
use lab_scenario::{
    canonical_json, canonical_value, expand_switch_ins, initial_outcomes, load_scenario_file,
    parse_team, state_from_teams, CanonicalError, InitialOutcome, LoadedScenario, ScenarioMeta,
    SwitchInError, DOUBLES_FORMAT,
};

fn engine_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn single_hit() -> LoadedScenario {
    load_scenario_file(engine_dir().join("oracle/scenarios/single-hit.json")).unwrap()
}

fn fixture() -> Value {
    let path = engine_dir().join("oracle/expected/single-hit.initial.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

const GARDEVOIR: SlotRef = SlotRef {
    side: SideId::One,
    slot: 0,
};

fn ability(state: &Doubles, r: SlotRef) -> AbilityId {
    state.active(r).unwrap().ability
}

fn weather(state: &Doubles) -> Effect {
    state.field[FieldEffect::Weather as usize]
}

fn terrain(state: &Doubles) -> Effect {
    state.field[FieldEffect::Terrain as usize]
}

fn sand(turns: u8) -> Effect {
    Effect {
        value: Weather::Sand as u8,
        turns,
    }
}

fn total(outcomes: &[InitialOutcome<2>]) -> f64 {
    outcomes.iter().map(|o| o.probability).sum()
}

#[test]
fn single_hit_trace_branches_over_both_foes() {
    let loaded = single_hit();
    let before = loaded.state.clone();
    let outcomes = initial_outcomes(&loaded).unwrap();
    assert_eq!(loaded.state, before, "the loaded state is not modified");
    assert_eq!(outcomes.len(), 2);
    assert_eq!(total(&outcomes), 1.0);

    let mut traced: Vec<AbilityId> = Vec::new();
    for o in &outcomes {
        assert_eq!(o.probability, 0.5);
        assert_eq!(weather(&o.state), sand(5));
        assert_eq!(
            terrain(&o.state),
            Effect {
                value: Terrain::Grassy as u8,
                turns: 5
            }
        );
        traced.push(ability(&o.state, GARDEVOIR));

        // Nothing else changes: only Gardevoir's ability and the field.
        let mut rest = o.state.clone();
        rest.field = before.field;
        rest.side_mut(SideId::One).party[0].ability = abilities::TRACE;
        assert_eq!(rest, before);
    }
    traced.sort();
    let mut expected = vec![abilities::SAND_STREAM, abilities::SAND_RUSH];
    expected.sort();
    assert_eq!(traced, expected);
}

fn outcome_values(loaded: &LoadedScenario) -> Vec<(f64, Value)> {
    initial_outcomes(loaded)
        .unwrap()
        .iter()
        .map(|o| {
            (
                o.probability,
                canonical_value(&o.state, &loaded.meta).unwrap(),
            )
        })
        .collect()
}

#[test]
fn initial_outcomes_match_the_oracle_fixture() {
    let loaded = single_hit();
    let ours = outcome_values(&loaded);
    let fixture = fixture();
    let expected = fixture["outcomes"].as_array().unwrap();
    assert_eq!(ours.len(), expected.len());
    let expected_total: f64 = expected.iter().map(|o| o["p"].as_f64().unwrap()).sum();
    assert!((expected_total - 1.0).abs() < 1e-12);
    for o in expected {
        let p = o["p"].as_f64().unwrap();
        let found = ours.iter().find(|(_, state)| *state == o["state"]);
        let (q, _) = found.unwrap_or_else(|| panic!("no engine outcome equals {}", o["state"]));
        assert!((p - q).abs() < 1e-12, "p {p} vs {q}");
    }
}

/// A start-of-battle Speed tie (Gardevoir's Trace vs Ninetales' Drought, both 132) is broken
/// uniformly at random by Showdown's `speedSort`, not by switch-in order: the oracle
/// (`initial.cjs`, 64 branches) gives Drought/sun 5 and Drought/sun 8 a quarter each and
/// Shell Armor/sun 8 a half.
#[test]
fn start_speed_ties_are_uniform_like_the_oracle() {
    let loaded = load_scenario_file(engine_dir().join("oracle/scenarios/tie-start.json")).unwrap();
    let ours = outcome_values(&loaded);
    let path = engine_dir().join("oracle/expected/tie-start.initial.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let expected = fixture["outcomes"].as_array().unwrap();
    assert_eq!(expected.len(), 3);
    assert_eq!(ours.len(), expected.len());
    for o in expected {
        let p = o["p"].as_f64().unwrap();
        let found = ours.iter().find(|(_, state)| *state == o["state"]);
        let (q, _) = found.unwrap_or_else(|| panic!("no engine outcome equals {}", o["state"]));
        assert!((p - q).abs() < 1e-12, "p {p} vs {q}");
    }
}

#[test]
fn oracle_before_is_one_of_the_initial_outcomes() {
    let fixture = fixture();
    let before = &fixture["before"];
    assert!(
        !before.is_null(),
        "fixture has no oracle `before` ({}); regenerate it with \
         `node engine/oracle/initial.cjs engine/oracle/scenarios/single-hit.json \
         --out engine/oracle/expected/single-hit.initial.json`",
        fixture["provenance"]
    );
    let loaded = single_hit();
    let ours = outcome_values(&loaded);
    assert_eq!(ours.iter().filter(|(_, s)| s == before).count(), 1);
    assert_eq!(ours.iter().map(|(p, _)| p).sum::<f64>(), 1.0);
}

#[test]
fn canonical_json_matches_the_oracle_layout() {
    let loaded = single_hit();
    // Before switch-in effects: no weather or terrain yet.
    let text = canonical_json(&loaded.state, &loaded.meta).unwrap();
    assert!(text.starts_with(
        r#"{"schema":1,"turn":1,"ended":false,"winner":"","field":{"weather":"","weatherDuration":null,"terrain":"","terrainDuration":null,"pseudoWeather":{}},"sides":[{"request":"move","conditions":{},"slotConditions":[{},{}],"pokemon":[{"name":"Gardevoir","species":"Gardevoir","hp":145,"maxhp":145,"status":"","item":"focussash","ability":"trace","slot":0,"pp":{"hypnosis":20,"hypervoice":12,"protect":8,"focusblast":8},"boosts":{},"volatiles":{}},"#
    ));
    // p2 is sorted by name: Excadrill (slot 1) before Tyranitar (slot 0).
    assert!(text.ends_with(
        r#"{"name":"Tyranitar","species":"Tyranitar","hp":207,"maxhp":207,"status":"","item":"leftovers","ability":"sandstream","slot":0,"pp":{"rockslide":12,"knockoff":20,"protect":8,"lowkick":20},"boosts":{},"volatiles":{}}]}]}"#
    ));
    assert!(text.contains(r#"[{"name":"Excadrill","#));
    // Round trip through serde keeps it valid JSON.
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["sides"][1]["pokemon"][0]["slot"], 1);

    // After the start: the Trace -> Sand Stream outcome prints sand and Grassy Terrain.
    let outcomes = initial_outcomes(&loaded).unwrap();
    let stream = outcomes
        .iter()
        .find(|o| ability(&o.state, GARDEVOIR) == abilities::SAND_STREAM)
        .unwrap();
    let text = canonical_json(&stream.state, &loaded.meta).unwrap();
    assert!(text.contains(
        r#""field":{"weather":"sandstorm","weatherDuration":5,"terrain":"grassyterrain","terrainDuration":5,"pseudoWeather":{}}"#
    ));
}

fn unrepresentable(state: &Doubles, meta: &ScenarioMeta) -> String {
    match canonical_json(state, meta) {
        Err(CanonicalError::Unrepresentable { what }) => what,
        other => panic!("expected Unrepresentable, got {other:?}"),
    }
}

#[test]
fn state_without_a_canonical_form_is_an_error() {
    let loaded = single_hit();
    let meta = &loaded.meta;

    let mut s = loaded.state.clone();
    s.slot_mut(GARDEVOIR).substitute_hp = 30;
    assert!(unrepresentable(&s, meta).contains("substitute"));

    let mut s = loaded.state.clone();
    s.side_mut(SideId::Two).effects[SideEffect::StealthRock as usize] = Effect {
        value: 1,
        turns: Effect::PERMANENT,
    };
    assert!(unrepresentable(&s, meta).contains("side effect"));

    let mut s = loaded.state.clone();
    s.field[FieldEffect::MagicRoom as usize] = Effect { value: 0, turns: 5 };
    assert!(unrepresentable(&s, meta).contains("pseudo-weather"));

    let mut s = loaded.state.clone();
    s.field[FieldEffect::Weather as usize] = sand(Effect::PERMANENT);
    assert!(unrepresentable(&s, meta).contains("duration"));

    let mut other = meta.clone();
    other.format = "gen9championsvgc2026regmc".into();
    assert!(matches!(
        canonical_json(&loaded.state, &other),
        Err(CanonicalError::UnknownFormat(_))
    ));
}

#[test]
fn can_mega_follows_eligibility_and_side_usage() {
    let root = engine_dir().join("..");
    let read = |p: &str| parse_team(&std::fs::read_to_string(root.join(p)).unwrap()).unwrap();
    let gravity = read("teams/gravity-original.json");
    let p2 = read("engine/oracle/scenarios/hypnosis-gravity.p2.json");
    let (mut state, sides) = state_from_teams::<2>([(&gravity, Some("34")), (&p2, None)]).unwrap();
    let meta = ScenarioMeta {
        format: DOUBLES_FORMAT.into(),
        description: String::new(),
        sides,
        turn: None,
    };
    let text = canonical_json(&state, &meta).unwrap();
    // Gardevoir (Gardevoirite) and Charizard (Charizardite Y), active or not.
    assert_eq!(text.matches(r#""canMega":true"#).count(), 2);
    assert!(text.contains(r#""slot":0,"pp":{"#));

    state.side_mut(SideId::One).gimmicks_used = GimmickSet::MEGA;
    let text = canonical_json(&state, &meta).unwrap();
    assert!(!text.contains("canMega"));
}

#[test]
fn unsupported_start_handlers_are_rejected() {
    let loaded = single_hit();
    let rillaboom = SlotRef {
        side: SideId::One,
        slot: 1,
    };

    let mut s = loaded.state.clone();
    s.active_mut(rillaboom).unwrap().ability = abilities::DOWNLOAD;
    match expand_switch_ins(&s) {
        Err(SwitchInError::UnsupportedAbility {
            slot,
            ability,
            handler: "onStart",
        }) => {
            assert_eq!(slot, rillaboom);
            assert_eq!(ability, abilities::DOWNLOAD);
        }
        other => panic!("{other:?}"),
    }

    // Intimidate is implemented (2026-09-26): every foe loses one Atk stage.
    let mut s = loaded.state.clone();
    s.active_mut(rillaboom).unwrap().ability = abilities::INTIMIDATE;
    let outcomes = expand_switch_ins(&s).unwrap();
    assert!(!outcomes.is_empty());
    for o in &outcomes {
        for slot in 0..2 {
            assert_eq!(o.state.side(SideId::Two).slots[slot].boosts[0], -1);
        }
    }

    let mut s = loaded.state.clone();
    s.active_mut(rillaboom).unwrap().item = items::CHOICE_SCARF;
    assert!(matches!(
        expand_switch_ins(&s),
        Err(SwitchInError::UnsupportedItem { .. })
    ));

    // Air Lock is implemented (O49): its start only runs `WeatherChange`, which has no
    // implemented handler; the sand it suppresses is still set.
    let mut s = loaded.state.clone();
    s.active_mut(rillaboom).unwrap().ability = abilities::AIR_LOCK;
    let outcomes = expand_switch_ins(&s).unwrap();
    assert!(!outcomes.is_empty());
    for o in &outcomes {
        assert!(o.state.field[FieldEffect::Weather as usize].is_active());
    }

    // A started position is not an initial one.
    let mut s = loaded.state.clone();
    s.field[FieldEffect::Weather as usize] = sand(5);
    assert!(matches!(
        expand_switch_ins(&s),
        Err(SwitchInError::NotInitial { .. })
    ));
}

#[test]
fn trace_with_only_untraceable_foes_is_rejected() {
    let loaded = single_hit();
    let mut s = loaded.state.clone();
    // Gardevoir moves first and faces two Trace users (Trace has `notrace`).
    s.active_mut(GARDEVOIR).unwrap().stats[4] = 200;
    for mon in &mut s.side_mut(SideId::Two).party[..2] {
        mon.ability = abilities::TRACE;
    }
    match expand_switch_ins(&s) {
        Err(SwitchInError::Unsupported { what }) => assert!(what.contains("Trace"), "{what}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn speed_ties_branch_and_smooth_rock_extends_sand() {
    let loaded = single_hit();
    let mut s: State<2> = loaded.state.clone();
    let tyranitar = SlotRef {
        side: SideId::Two,
        slot: 0,
    };
    // Tie Tyranitar with Gardevoir (132) and give it Smooth Rock. Whoever sets sand first
    // decides the duration: a second Sand Stream fails.
    let mon = s.active_mut(tyranitar).unwrap();
    mon.stats[4] = 132;
    mon.item = items::SMOOTH_ROCK;

    let outcomes = expand_switch_ins(&s).unwrap();
    assert_eq!(total(&outcomes), 1.0);
    let mut summary: Vec<(AbilityId, u8, f64)> = outcomes
        .iter()
        .map(|o| {
            assert_eq!(weather(&o.state).value, Weather::Sand as u8);
            (
                ability(&o.state, GARDEVOIR),
                weather(&o.state).turns,
                o.probability,
            )
        })
        .collect();
    summary.sort_by_key(|a| (a.0, a.1));
    let mut expected = vec![
        (abilities::SAND_STREAM, 5, 0.25), // Gardevoir first, traces Sand Stream
        (abilities::SAND_STREAM, 8, 0.25), // Tyranitar first, Gardevoir's copy fails
        (abilities::SAND_RUSH, 8, 0.5),    // either order
    ];
    expected.sort_by_key(|a| (a.0, a.1));
    assert_eq!(summary, expected);
}
