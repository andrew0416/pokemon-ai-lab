//! The VGC format (`gen9championsvgc2026regmc`, work plan O104): team preview keeps 4 members
//! (Showdown `Side.chooseTeam` with `pickedTeamSize`), `Adjust Level = 50`, and a real library
//! team against Showdown.

mod common;

use std::path::Path;

use lab_engine::state::SideId;
use lab_scenario::{
    canonical::format_ruleset, load_scenario_str, picked_order, picked_team_size, preview_order,
    DOUBLES_FORMAT, VGC_FORMAT,
};

/// The library team sand-owen (bring Tyranitar, Gholdengo, Indeedee, Excadrill) against a
/// 6-member team whose partial preview `3,1` is filled to 4; a switch counts positions among
/// the 4 brought; Mega Evolution; sand and Psychic Surge.
#[test]
fn vgc_library_team_turn_matches_showdown() {
    common::assert_exact_parity("o104-vgc-sand-owen");
}

/// Showdown `chooseTeam`: cut to the picked size, then filled from team positions
/// `0..picked` in order; entries past the cut are not checked.
#[test]
fn team_preview_picks_follow_choose_team() {
    assert_eq!(picked_order(Some("5,4,2,6"), 6, 4).unwrap(), [4, 3, 1, 5]);
    assert_eq!(picked_order(Some("31"), 6, 4).unwrap(), [2, 0, 1, 3]);
    assert_eq!(picked_order(Some("56"), 6, 4).unwrap(), [4, 5, 0, 1]);
    assert_eq!(picked_order(None, 6, 4).unwrap(), [0, 1, 2, 3]);
    assert_eq!(picked_order(Some("123456"), 6, 4).unwrap(), [0, 1, 2, 3]);
    // The fifth entry is cut before it is checked.
    assert_eq!(picked_order(Some("1234x"), 6, 4).unwrap(), [0, 1, 2, 3]);
    assert!(picked_order(Some("7"), 6, 4).is_err());
    assert!(picked_order(Some("11"), 6, 4).is_err());
    // A team smaller than the picked size brings everyone.
    assert_eq!(picked_order(Some("2"), 3, 4).unwrap(), [1, 0, 2]);
    // The custom game brings every member, as before.
    assert_eq!(preview_order(Some("21"), 6).unwrap(), [1, 0, 2, 3, 4, 5]);
    assert_eq!(picked_team_size(DOUBLES_FORMAT), Some(6));
    assert_eq!(picked_team_size(VGC_FORMAT), Some(4));
    assert_eq!(picked_team_size("gen9championsbssregmc"), None);
    assert!(format_ruleset(VGC_FORMAT).is_ok());
}

/// A VGC scenario keeps only the picked members (party and sidecar), and `Adjust Level = 50`
/// sets the library's level-100 sets to 50.
#[test]
fn vgc_scenarios_keep_four_members_at_level_50() {
    let engine = common::engine_dir();
    let team = std::fs::read_to_string(
        engine.join("../teams/library/doubles/m-c/kickoff-wolfey/team.json"),
    )
    .unwrap();
    assert!(team.contains(r#""level": 100"#));
    let scenario = format!(
        r#"{{"format": "{VGC_FORMAT}", "p1": {{"team": {team}, "order": "6,5"}},
            "p2": {{"team": {team}}}}}"#
    );
    let loaded = load_scenario_str(&scenario, Path::new(".")).unwrap();
    for side in [SideId::One, SideId::Two] {
        let party = &loaded.state.side(side).party;
        assert_eq!(party.iter().filter(|p| !p.species.is_none()).count(), 4);
        assert!(party
            .iter()
            .filter(|p| !p.species.is_none())
            .all(|p| p.level == 50));
        assert_eq!(loaded.meta.sides[side.index()].members.len(), 4);
    }
    let p1 = &loaded.meta.sides[0].members;
    let indices: Vec<u8> = p1.iter().map(|m| m.team_index).collect();
    assert_eq!(indices, [5, 4, 0, 1]);

    // The custom game still refuses a level other than 50.
    let custom = scenario.replace(VGC_FORMAT, DOUBLES_FORMAT);
    assert!(load_scenario_str(&custom, Path::new(".")).is_err());
}
