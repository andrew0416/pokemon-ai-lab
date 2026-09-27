//! Board A4-loader-refusal-audit: the loader refusals `engine/scripts/refusals.py` classifies as
//! `unsupported` (a battle Showdown plays that the loader refuses) each have a test here that
//! reaches them, and some `input` ones (a malformed scenario file) too. The audit checks that
//! every test it names exists.

mod common;

use lab_scenario::{
    load_scenario_str, picked_order, LoadError, ScenarioError, SetProblem, TeamProblem,
};

fn set(species: &str, name: Option<&str>, level: Option<u8>) -> serde_json::Value {
    let mut set = serde_json::json!({
        "species": species, "item": "", "ability": "Honey Gather", "nature": "Hardy",
        "evs": {"hp": 32}, "moves": ["Harden"]
    });
    if let Some(name) = name {
        set["name"] = name.into();
    }
    if let Some(level) = level {
        set["level"] = level.into();
    }
    set
}

fn scenario(format: &str, p1: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "format": format,
        "p1": {"team": p1},
        "p2": {"team": [set("Snorlax", None, Some(50)), set("Blissey", None, Some(50))]},
        "turn": {"p1": "move harden, move harden", "p2": "move harden, move harden"}
    })
}

fn load(json: &serde_json::Value) -> Result<lab_scenario::LoadedScenario, LoadError> {
    load_scenario_str(&json.to_string(), &common::engine_dir())
}

fn two(format: &str, a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    scenario(format, serde_json::json!([a, b]))
}

// ---- unsupported ---------------------------------------------------------------------------

/// Champions singles (`gen9championsbssregmc`) is a format Showdown plays; the loader builds
/// doubles only (board II-singles-loader).
#[test]
fn a_singles_format_is_refused() {
    let json = two(
        "gen9championsbssregmc",
        set("Chansey", None, Some(50)),
        set("Machamp", None, Some(50)),
    );
    match load(&json) {
        Err(LoadError::UnsupportedFormat(format)) => assert_eq!(format, "gen9championsbssregmc"),
        other => panic!("{other:?}"),
    }
}

/// The custom game plays a set without `level` at level 100 (board B31) and any level a set
/// gives; the engine's stats are the level-50 ones.
#[test]
fn a_level_other_than_50_is_refused() {
    for level in [None, Some(100), Some(1)] {
        let json = two(
            lab_scenario::DOUBLES_FORMAT,
            set("Chansey", None, level),
            set("Machamp", None, Some(50)),
        );
        match load(&json) {
            Err(LoadError::Set {
                problem: SetProblem::UnsupportedLevel(l),
                ..
            }) => assert_eq!(l, level.unwrap_or(100)),
            other => panic!("{level:?}: {other:?}"),
        }
    }
    // VGC's `Adjust Level = 50` makes every set level 50.
    let json = two(
        lab_scenario::VGC_FORMAT,
        set("Chansey", None, Some(100)),
        set("Machamp", None, None),
    );
    load(&json).unwrap();
}

/// A temporary in-battle forme as a set's species (Showdown without a validator keeps it as the
/// base species; the engine's state cannot tell it from the forme reached in battle).
#[test]
fn a_temporary_forme_as_species_is_refused() {
    let json = two(
        lab_scenario::DOUBLES_FORMAT,
        set("Darmanitan-Zen", None, Some(50)),
        set("Machamp", None, Some(50)),
    );
    match load(&json) {
        Err(LoadError::Set {
            problem: SetProblem::TemporaryForme(species),
            ..
        }) => assert_eq!(species, "Darmanitan-Zen"),
        other => panic!("{other:?}"),
    }
}

/// Two members of a side with the same name (a nickname, or a species written twice in the
/// custom game): Showdown plays it, canonical states key Pokémon by name.
#[test]
fn a_name_twice_on_a_side_is_refused() {
    let json = two(
        lab_scenario::DOUBLES_FORMAT,
        set("Chansey", Some("Tank"), Some(50)),
        set("Machamp", Some("Tank"), Some(50)),
    );
    match load(&json) {
        Err(LoadError::Team {
            problem: TeamProblem::DuplicateName(name),
            ..
        }) => assert_eq!(name, "Tank"),
        other => panic!("{other:?}"),
    }
}

// ---- no longer refused -----------------------------------------------------------------------

/// Bracketed team preview choices (`team [1,2,3,4]`, Showdown `Side.chooseTeam`): brackets
/// stripped, always split on commas, not cut, then filled; a list longer than the picked size
/// is an error (it was refused as unsupported before A4). The oracle fixture checks the leads
/// Showdown picks for `[6, 4,1]` and `[2,1,5,3]`.
#[test]
fn bracketed_team_preview_follows_choose_team() {
    assert_eq!(picked_order(Some("[5,4,2,6]"), 6, 4).unwrap(), [4, 3, 1, 5]);
    assert_eq!(picked_order(Some("[6, 4,1]"), 6, 4).unwrap(), [5, 3, 0, 1]);
    // Two digits without a comma are one position in brackets (`split(',')`).
    assert!(picked_order(Some("[12]"), 6, 4).is_err());
    // Not cut: five entries for four picks.
    let err = picked_order(Some("[1,2,3,4,5]"), 6, 4).unwrap_err();
    assert!(err.contains("exactly 4"), "{err}");
    // Empty brackets name the whole team (`autoChoose`), which is too many for VGC.
    assert!(picked_order(Some("[]"), 6, 4).is_err());
    assert_eq!(picked_order(Some("[]"), 6, 6).unwrap(), [0, 1, 2, 3, 4, 5]);
    // An unclosed bracket is not bracketed: "[1" is not a position.
    assert!(picked_order(Some("[1,2"), 6, 4).is_err());
    common::assert_exact_parity("vv-bracketed-preview");
}

// ---- input -----------------------------------------------------------------------------------

/// A scenario field the loader does not know is an error (it could change the battle).
#[test]
fn an_unknown_scenario_field_is_refused() {
    let mut json = two(
        lab_scenario::DOUBLES_FORMAT,
        set("Chansey", None, Some(50)),
        set("Machamp", None, Some(50)),
    );
    json["weatherx"] = "sun".into();
    assert!(matches!(load(&json), Err(LoadError::Json { .. })));
}

/// `setupRolls` is `full` or `extremes`; `team` is a path or an inline array; no more pinned
/// setup states than setup turns.
#[test]
fn scenario_field_values_are_checked() {
    let base = two(
        lab_scenario::DOUBLES_FORMAT,
        set("Chansey", None, Some(50)),
        set("Machamp", None, Some(50)),
    );
    let mut rolls = base.clone();
    rolls["setupRolls"] = "median".into();
    let mut team = base.clone();
    team["p1"]["team"] = 5.into();
    let mut states = base.clone();
    states["setupStates"] = serde_json::json!([{}]);
    for (json, field) in [
        (rolls, "setupRolls"),
        (team, "team"),
        (states, "setupStates"),
    ] {
        match load(&json) {
            Err(LoadError::Unsupported { field: f, .. }) => assert_eq!(f, field),
            other => panic!("{field}: {other:?}"),
        }
    }
}

/// A patch the scenario cannot express is the scenario's fault (`ScenarioError::Invalid`), as
/// T1 types it.
#[test]
fn a_bad_patch_is_invalid() {
    let mut json = two(
        lab_scenario::DOUBLES_FORMAT,
        set("Chansey", None, Some(50)),
        set("Machamp", None, Some(50)),
    );
    json["patch"] = serde_json::json!({"field": {"weather": "desolateland", "weatherDuration": 5}});
    let loaded = load(&json).unwrap();
    match lab_scenario::scenario_positions(&loaded) {
        Err(ScenarioError::Invalid(why)) => assert!(why.contains("not supported"), "{why}"),
        other => panic!("{other:?}"),
    }
}
