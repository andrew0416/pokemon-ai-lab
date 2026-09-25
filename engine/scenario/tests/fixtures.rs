//! Loader tests against the oracle fixtures (`engine/oracle/scenarios/`) and the lab's
//! gravity team (`teams/gravity-original.json`, which holds two Mega Stones).

use std::path::{Path, PathBuf};

use lab_engine::dex::{abilities, items, moves, species, Type};
use lab_engine::gimmick::{Gimmick, GimmickSet};
use lab_engine::rules::Ruleset;
use lab_engine::state::{MoveSlot, Pokemon, SideId, SlotRef, Status};
use lab_scenario::{
    load_scenario_file, load_scenario_str, parse_team, state_from_teams, LoadError, SetProblem,
    TeamProblem, TeamSet, DOUBLES_FORMAT,
};

fn scenarios() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios")
}

fn team(path: &Path) -> Vec<TeamSet> {
    parse_team(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn p1_team() -> Vec<TeamSet> {
    team(&scenarios().join("hypnosis-gravity.p1.json"))
}

fn p2_team() -> Vec<TeamSet> {
    team(&scenarios().join("hypnosis-gravity.p2.json"))
}

fn gravity_team() -> Vec<TeamSet> {
    team(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teams/gravity-original.json"))
}

fn move_slots(pokemon: &Pokemon) -> Vec<(lab_engine::dex::MoveId, u8)> {
    pokemon.moves.iter().map(|m| (m.id, m.pp)).collect()
}

fn slot(side: SideId, slot: u8) -> SlotRef {
    SlotRef { side, slot }
}

#[test]
fn single_hit_scenario_loads_the_hypnosis_gravity_teams() {
    let loaded = load_scenario_file(scenarios().join("single-hit.json")).unwrap();
    let state = &loaded.state;
    assert_eq!(loaded.meta.format, DOUBLES_FORMAT);
    assert_eq!(state.turn, 1);
    assert_eq!(
        loaded.meta.turn.as_ref().map(|t| t.p1.as_str()),
        Some("move protect, move grassyglide 1")
    );

    // Order "12" on both sides: team order, first two active.
    for side in [SideId::One, SideId::Two] {
        let s = state.side(side);
        assert_eq!(s.slots[0].party_index, Some(0));
        assert_eq!(s.slots[1].party_index, Some(1));
        assert!(s.party[2..].iter().all(|p| *p == Pokemon::default()));
        assert!(s.gimmicks_used.is_empty());
    }

    let gardevoir = state.active(slot(SideId::One, 0)).unwrap();
    let rillaboom = state.active(slot(SideId::One, 1)).unwrap();
    let tyranitar = state.active(slot(SideId::Two, 0)).unwrap();
    let excadrill = state.active(slot(SideId::Two, 1)).unwrap();

    assert_eq!(rillaboom.species, species::RILLABOOM);
    assert_eq!(rillaboom.level, 50);
    assert_eq!(rillaboom.types, [Type::Grass, Type::None]);
    assert_eq!((rillaboom.hp, rillaboom.max_hp), (207, 207));
    assert_eq!(rillaboom.stats, [194, 112, 80, 90, 94]);
    assert_eq!(rillaboom.status, Status::None);
    assert_eq!(rillaboom.item, items::MIRACLE_SEED);
    assert_eq!(rillaboom.ability, abilities::GRASSY_SURGE);
    assert_eq!(
        move_slots(rillaboom),
        [
            (moves::FAKE_OUT, 12),
            (moves::GRASSY_GLIDE, 20),
            (moves::WOOD_HAMMER, 16),
            (moves::PROTECT, 8), // Champions: base PP 5
        ]
    );

    assert_eq!(tyranitar.species, species::TYRANITAR);
    assert_eq!(tyranitar.types, [Type::Rock, Type::Dark]);
    assert_eq!((tyranitar.hp, tyranitar.max_hp), (207, 207));
    assert_eq!(tyranitar.stats, [204, 130, 103, 122, 81]);
    assert_eq!(tyranitar.item, items::LEFTOVERS);
    assert_eq!(tyranitar.ability, abilities::SAND_STREAM);
    assert_eq!(
        move_slots(tyranitar),
        [
            (moves::ROCK_SLIDE, 12),
            (moves::KNOCK_OFF, 20),
            (moves::PROTECT, 8),
            (moves::LOW_KICK, 20),
        ]
    );

    // Modest, SP hp 2 / spa 32 / spe 32.
    assert_eq!(gardevoir.max_hp, 145);
    assert_eq!(gardevoir.stats, [76, 85, 194, 135, 132]);
    assert_eq!(
        move_slots(gardevoir),
        [
            (moves::HYPNOSIS, 20),
            (moves::HYPER_VOICE, 12),
            (moves::PROTECT, 8),
            (moves::FOCUS_BLAST, 8),
        ]
    );
    // Jolly, SP hp 2 / atk 32 / spe 32.
    assert_eq!(excadrill.max_hp, 187);
    assert_eq!(excadrill.stats, [187, 80, 63, 85, 154]);

    // No Mega Stones in these teams: nothing is eligible, and M-C unlocks nothing.
    for r in lab_engine::Doubles::slot_refs() {
        assert!(state.active(r).unwrap().gimmicks.is_empty());
        assert!(Ruleset::CHAMPIONS_MC
            .available_gimmicks(state, r)
            .is_empty());
    }

    let p1 = &loaded.meta.sides[0];
    assert_eq!(p1.name(0), Some("Gardevoir"));
    assert_eq!(p1.party_index("Rillaboom"), Some(1));
    assert_eq!(p1.party_index("Tyranitar"), None);
    assert_eq!(loaded.meta.sides[1].canonical_order(), [1, 0]); // Excadrill < Tyranitar
}

#[test]
fn patches_load_and_setup_turns_are_rejected() {
    let loaded = load_scenario_file(scenarios().join("hypnosis-gravity.json")).unwrap();
    let patch = loaded.patch.as_ref().expect("hypnosis-gravity has a patch");
    assert_eq!(patch.field.pseudo_weather.get("gravity"), Some(&Some(4)));

    let base = scenarios();
    let with = |extra: &str| {
        format!(
            r#"{{"format": "{DOUBLES_FORMAT}",
                "p1": {{"team": "hypnosis-gravity.p1.json", "order": "12"}},
                "p2": {{"team": "hypnosis-gravity.p2.json", "order": "12"}}{extra}}}"#
        )
    };
    let setup = with(r#", "setupTurns": [["move protect", "move protect"]]"#);
    let err = load_scenario_str(&setup, &base).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::Unsupported {
                field: "setupTurns",
                ..
            }
        ),
        "{err}"
    );
    // Empty setup and patch are no-ops, as in enumerate.cjs.
    load_scenario_str(&with(r#", "setupTurns": [], "patch": {}"#), &base).unwrap();

    let err = load_scenario_str(&with(r#", "seedx": 1"#), &base).unwrap_err();
    assert!(matches!(err, LoadError::Json { .. }), "{err}");
    // Unknown patch fields could change the position: rejected, not dropped.
    let err =
        load_scenario_str(&with(r#", "patch": {"field": {"gravityx": 1}}"#), &base).unwrap_err();
    assert!(matches!(err, LoadError::Json { .. }), "{err}");

    let vgc = with("").replace(DOUBLES_FORMAT, "gen9championsvgc2026regmc");
    assert!(matches!(
        load_scenario_str(&vgc, &base),
        Err(LoadError::UnsupportedFormat(_))
    ));
}

#[test]
fn lead_order_follows_team_preview() {
    let p1 = p1_team();
    let p2 = p2_team();
    let (state, meta) = state_from_teams::<2>([(&p1, Some("21")), (&p2, Some("123456"))]).unwrap();
    let side = state.side(SideId::One);
    assert_eq!(side.party[0].species, species::RILLABOOM);
    assert_eq!(side.party[1].species, species::GARDEVOIR);
    assert_eq!(meta[0].members[0].name, "Rillaboom");
    assert_eq!(meta[0].members[0].team_index, 1);
    // Showdown cuts a long order to the team size, so enumerate.cjs's default "123456" works.
    assert_eq!(state.side(SideId::Two).party[0].species, species::TYRANITAR);

    let gravity = gravity_team();
    let (state, meta) = state_from_teams::<2>([(&gravity, Some("3,4")), (&p2, None)]).unwrap();
    let names: Vec<&str> = meta[0].members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Gardevoir",
            "Charizard",
            "Sableye",
            "Milotic",
            "Flapple",
            "Maushold"
        ]
    );
    let team_indices: Vec<u8> = meta[0].members.iter().map(|m| m.team_index).collect();
    assert_eq!(team_indices, [2, 3, 0, 1, 4, 5]);
    assert_eq!(state.side(SideId::One).slots[0].party_index, Some(0));
    assert_eq!(state.side(SideId::One).slots[1].party_index, Some(1));

    // Singles builds from the same code: one active slot.
    let (single, _) = state_from_teams::<1>([(&gravity, Some("5")), (&p2, None)]).unwrap();
    assert_eq!(
        single.active(slot(SideId::One, 0)).unwrap().species,
        species::FLAPPLE
    );

    for (order, reason) in [
        ("13", "no Pokémon in slot 3"),
        ("11", "only switch in once"),
        ("x", "not a team position"),
    ] {
        let err = state_from_teams::<2>([(&p1, Some(order)), (&p2, None)]).unwrap_err();
        match err {
            LoadError::Team {
                side: SideId::One,
                problem: TeamProblem::Order { reason: r, .. },
            } => assert!(r.contains(reason), "{order}: {r}"),
            other => panic!("{order}: {other}"),
        }
    }
}

#[test]
fn mega_eligibility_is_structural_and_only_mega_is_unlocked() {
    let gravity = gravity_team();
    let p2 = p2_team();
    let (mut state, _) = state_from_teams::<2>([(&gravity, Some("34")), (&p2, None)]).unwrap();
    let side = state.side(SideId::One);

    let gardevoir = &side.party[0];
    assert_eq!(gardevoir.species, species::GARDEVOIR);
    assert_eq!(gardevoir.item, items::GARDEVOIRITE);
    assert_eq!(gardevoir.gimmicks, GimmickSet::MEGA);
    // Modest, SP hp 32 / def 10 / spa 24.
    assert_eq!(gardevoir.max_hp, 175);
    assert_eq!(gardevoir.stats, [76, 95, 185, 135, 100]);

    let charizard = &side.party[1];
    assert_eq!(charizard.item, items::CHARIZARDITE_Y);
    assert_eq!(charizard.gimmicks, GimmickSet::MEGA);
    assert_eq!(charizard.types, [Type::Fire, Type::Flying]);
    assert_eq!(charizard.max_hp, 155);
    assert_eq!(charizard.stats, [93, 98, 177, 105, 152]);

    let maushold = &side.party[5];
    assert!(maushold.gimmicks.is_empty());
    assert_eq!(maushold.moves[1], MoveSlot::full(moves::POPULATION_BOMB));
    assert_eq!(maushold.moves[1].pp, 12);
    assert!(side.party[2..].iter().all(|p| p.gimmicks.is_empty()));
    assert!(!side.party.iter().any(|p| p.gigantamax_factor));

    let lead = slot(SideId::One, 0);
    assert_eq!(
        Ruleset::CHAMPIONS_MC.available_gimmicks(&state, lead),
        GimmickSet::MEGA
    );
    assert!(Ruleset::NO_GIMMICKS
        .available_gimmicks(&state, lead)
        .is_empty());
    // Enabling another mode in the ruleset unlocks nothing: only Mega is derived.
    assert_eq!(
        Ruleset::CHAMPIONS_MC
            .enabling(Gimmick::Tera)
            .enabling(Gimmick::ZMove)
            .available_gimmicks(&state, lead),
        GimmickSet::MEGA
    );
    state.side_mut(SideId::One).gimmicks_used = GimmickSet::MEGA;
    assert!(Ruleset::CHAMPIONS_MC
        .available_gimmicks(&state, lead)
        .is_empty());
}

fn set_error(json: &str) -> (usize, String, SetProblem) {
    let team = parse_team(json).unwrap();
    let p2 = p2_team();
    match state_from_teams::<2>([(&team, None), (&p2, None)]).unwrap_err() {
        LoadError::Set {
            side: SideId::One,
            index,
            name,
            problem,
        } => (index, name, problem),
        other => panic!("{other}"),
    }
}

const RILLA: &str = r#"{"species": "Rillaboom", "ability": "Grassy Surge", "nature": "Brave",
    "evs": {"hp": 32, "atk": 32, "def": 2}, "moves": ["Fake Out"]}"#;

fn team_with(second: &str) -> String {
    format!("[{RILLA}, {second}]")
}

#[test]
fn unknown_names_and_invalid_sets_are_rejected_with_context() {
    let cases = [
        (
            r#"{"species": "Gardevior", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"]}"#,
            SetProblem::UnknownSpecies("Gardevior".into()),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosys"]}"#,
            SetProblem::UnknownMove("Hypnosys".into()),
        ),
        (
            r#"{"species": "Gardevoir", "item": "Gardevoirnite", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"]}"#,
            SetProblem::UnknownItem("Gardevoirnite".into()),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Tracer", "nature": "Modest", "moves": ["Hypnosis"]}"#,
            SetProblem::UnknownAbility("Tracer".into()),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modesty", "moves": ["Hypnosis"]}"#,
            SetProblem::UnknownNature("Modesty".into()),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "moves": ["Hypnosis"]}"#,
            SetProblem::MissingNature,
        ),
        (
            r#"{"species": "Gardevoir", "nature": "Modest", "moves": ["Hypnosis"]}"#,
            SetProblem::MissingAbility,
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": []}"#,
            SetProblem::NoMoves,
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis", "Protect", "hypnosis"]}"#,
            SetProblem::DuplicateMove("hypnosis".into()),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"], "level": 100}"#,
            SetProblem::UnsupportedLevel(100),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"], "gender": "X"}"#,
            SetProblem::UnknownGender("X".into()),
        ),
        (
            r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"], "ivs": {"spe": 32}}"#,
            SetProblem::InvalidIv {
                stat: "spe",
                value: 32,
            },
        ),
    ];
    for (set, expected) in cases {
        let (index, _, problem) = set_error(&team_with(set));
        assert_eq!(index, 1);
        assert_eq!(problem, expected, "{set}");
    }

    // SP limits come from lab_engine::stats.
    let (_, _, problem) = set_error(&team_with(
        r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"], "evs": {"hp": 33}}"#,
    ));
    assert!(matches!(problem, SetProblem::StatPoints(_)), "{problem}");
    let (_, _, problem) = set_error(&team_with(
        r#"{"species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"], "evs": {"hp": 32, "spa": 32, "spe": 3}}"#,
    ));
    assert!(matches!(problem, SetProblem::StatPoints(_)), "{problem}");

    // The message names side, position and Pokémon.
    let team = parse_team(&team_with(
        r#"{"name": "Gardy", "species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosys"]}"#,
    ))
    .unwrap();
    let p2 = p2_team();
    let err = state_from_teams::<2>([(&p2, None), (&team, None)]).unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"p2 team[1] "Gardy": unknown move "Hypnosys""#
    );

    // Unknown set fields and stat keys fail in serde instead of being dropped.
    assert!(parse_team(&team_with(r#"{"species": "Gardevoir", "shiny": true}"#)).is_err());
    assert!(parse_team(&team_with(
        r#"{"species": "Gardevoir", "evs": {"speed": 4}}"#
    ))
    .is_err());
}

#[test]
fn names_default_to_species_are_truncated_and_must_be_unique() {
    let p2 = p2_team();
    let team = parse_team(&team_with(
        r#"{"name": "A very long nickname indeed", "species": "Gardevoir", "ability": "Trace", "nature": "Modest", "moves": ["Hypnosis"], "gender": "F", "teraType": "Fairy"}"#,
    ))
    .unwrap();
    let (_, meta) = state_from_teams::<2>([(&team, None), (&p2, None)]).unwrap();
    assert_eq!(meta[0].members[0].name, "Rillaboom");
    assert_eq!(meta[0].members[1].name, "A very long nickname");
    assert_eq!(meta[0].members[1].name.chars().count(), 20);
    assert_eq!(meta[0].members[1].tera_type, Type::Fairy);
    assert_eq!(meta[0].members[1].gender, lab_engine::dex::Gender::Female);

    let twins = parse_team(&format!("[{RILLA}, {RILLA}]")).unwrap();
    let err = state_from_teams::<2>([(&twins, None), (&p2, None)]).unwrap_err();
    assert!(matches!(
        err,
        LoadError::Team {
            problem: TeamProblem::DuplicateName(_),
            ..
        }
    ));
    let empty: Vec<TeamSet> = Vec::new();
    assert!(matches!(
        state_from_teams::<2>([(&empty, None), (&p2, None)]),
        Err(LoadError::Team {
            problem: TeamProblem::Empty,
            ..
        })
    ));
}
