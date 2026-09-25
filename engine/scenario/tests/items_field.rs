//! Parity of held items and field effects (work plan units O86, O80/O81, O92/O102, O95,
//! O101, O103) with Showdown's outcome distribution (`engine/oracle/expected/`), plus the
//! combinations the engine refuses on purpose.

mod common;

use common::assert_exact_parity;

// ---- O86 items that boost when hit ---------------------------------------------------------------

/// A super-effective hit uses Weakness Policy; a fixed-damage move computes no `typeMod`.
#[test]
fn weakness_policy_matches_showdown() {
    assert_exact_parity("o86-weakness-policy");
}

#[test]
fn absorb_bulb_and_luminous_moss_match_showdown() {
    assert_exact_parity("o86-absorb-bulb-moss");
}

#[test]
fn cell_battery_and_snowball_match_showdown() {
    assert_exact_parity("o86-cell-battery-snowball");
}

/// Kee / Maranga Berry are eaten in AfterMoveSecondary, after the hit loop.
#[test]
fn kee_and_maranga_berries_match_showdown() {
    assert_exact_parity("o86-kee-maranga");
}

// ---- O80 / O81 remaining berries --------------------------------------------------------------

/// Lansat Berry adds `focusenergy` (crit ratio +2), Starf Berry raises a random stat by 2; both
/// eaten on the Update after a hit.
#[test]
fn lansat_and_starf_berries_match_showdown() {
    assert_exact_parity("o80-lansat-starf");
}

/// Micle Berry: eaten in the residual (setup turn), then its volatile's Accuracy handler.
#[test]
fn micle_berry_matches_showdown() {
    assert_exact_parity("o80-micle");
}

/// The Accuracy event is skipped for Toxic used by a Poison type (it cannot miss).
#[test]
fn toxic_from_a_poison_type_matches_showdown() {
    assert_exact_parity("o80-toxic-poison-user");
}

/// Custap Berry is eaten when the actions are queued and gives +0.1 priority.
#[test]
fn custap_berry_matches_showdown() {
    assert_exact_parity("o81-custap");
}

/// Enigma Berry (runEvent('Hit'), super-effective) and Jaboca Berry (DamagingHit, physical).
#[test]
fn enigma_and_jaboca_berries_match_showdown() {
    assert_exact_parity("o81-enigma-jaboca");
}

/// Rowap Berry is eaten by a holder the hit fainted; Jaboca Berry is not eaten against Magic
/// Guard.
#[test]
fn rowap_berry_at_zero_hp_and_jaboca_against_magic_guard_match_showdown() {
    assert_exact_parity("o81-rowap-fainted");
}

// ---- O92 / O102 Seeds and the TerrainChange event ----------------------------------------------

/// A Seed is used by TerrainChange at battle start (Psychic Surge) and after a terrain move,
/// and by its own switch-in handler (priority -1) under a terrain that is already up.
#[test]
fn seeds_on_terrain_change_and_switch_in_match_showdown() {
    assert_exact_parity("o92-seeds-psychic-electric");
}

/// Grassy Seed at battle start, Misty Seed after Misty Terrain replaces Grassy Terrain, and
/// Ice Spinner's `clearTerrain` (TerrainChange without a terrain).
#[test]
fn seeds_with_a_replaced_and_cleared_terrain_match_showdown() {
    assert_exact_parity("o92-seeds-grassy-misty");
}

// ---- O95 White Herb, Mirror Herb, Adrenaline Orb, Room Service ----------------------------------

/// White Herb restores its holder's drops at the AfterMove of the move that caused them (a
/// foe's Growl, its own Close Combat).
#[test]
fn white_herb_after_moves_matches_showdown() {
    assert_exact_parity("o95-white-herb");
}

/// At battle start White Herb answers Intimidate in the same switch-in batch, and Adrenaline
/// Orb activates although Clear Body blocked the drop; a later Intimidate meets neither.
#[test]
fn white_herb_and_adrenaline_orb_against_intimidate_match_showdown() {
    assert_exact_parity("o95-intimidate-herb-orb");
}

/// Room Service on Trick Room's start (PseudoWeatherChange).
#[test]
fn room_service_on_trick_room_matches_showdown() {
    assert_exact_parity("o95-room-service");
}

/// Room Service in its holder's switch-in (priority -1) under Trick Room.
#[test]
fn room_service_at_switch_in_matches_showdown() {
    assert_exact_parity("o95-room-service-switch-in");
}

/// Mirror Herb copies both raises of a foe's Howl and uses them at the AfterMove.
#[test]
fn mirror_herb_matches_showdown() {
    assert_exact_parity("o95-mirror-herb");
}

/// A foe's raise after the last trigger of a stage leaves Mirror Herb `ready` across stages,
/// which the engine refuses: here the burn (residual order 10) brings Hariyama to 1/4 HP, and
/// its Liechi Berry is eaten on the Update after the residual, after Mirror Herb's own residual
/// handler (order 29).
#[test]
fn mirror_herb_ready_at_the_end_of_a_stage_is_refused() {
    use lab_engine::action::SlotAction;
    use lab_engine::dex::items;
    use lab_engine::gimmick::Gimmick;
    use lab_engine::rules::Ruleset;
    use lab_engine::state::{SideId, Status};
    use lab_engine::turn::{enumerate_turn, TurnError};

    let name = "o95-mirror-herb";
    let fixture = common::fixture(name);
    let (loaded, position) = common::start(name, &fixture);
    let mut state = position.state;
    let mut choices = lab_scenario::scenario_choices(&loaded, &state).unwrap();
    let hariyama = state
        .side_mut(SideId::One)
        .party
        .iter_mut()
        .find(|p| p.species.data().name == "Hariyama")
        .unwrap();
    hariyama.item = items::LIECHI_BERRY;
    hariyama.status = Status::Burn;
    hariyama.hp = hariyama.max_hp / 4 + 8;
    // Both p1 Pokémon protect: no raise before the residual.
    let protect = SlotAction::Move {
        index: 1,
        target: 0,
        gimmick: Gimmick::None,
    };
    choices[0] = [protect, protect];
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::Unsupported(why)) => assert!(why.contains("Mirror Herb"), "{why}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
