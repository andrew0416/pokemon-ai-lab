//! Parity of held items (work plan units O82–O93) with Showdown's exact outcome distribution
//! (`engine/oracle/expected/*.turn.json`), plus the combinations the engine refuses on purpose.

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::action::SlotAction;
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::Ruleset;
use lab_engine::state::{Pokemon, SideId};
use lab_engine::turn::{enumerate_turn, TurnError};
use lab_engine::Doubles;
use lab_scenario::scenario_choices;

/// The fixture's start position with `change` applied to the named Pokémon, run as the
/// scenario's turn; the error the engine gives.
fn refused_with(
    name: &str,
    side: SideId,
    species: &str,
    change: impl FnOnce(&mut Pokemon),
) -> String {
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state: Doubles = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let mon = state
        .side_mut(side)
        .party
        .iter_mut()
        .find(|p| p.species.data().name == species)
        .unwrap_or_else(|| panic!("{species} is in {name}"));
    change(mon);
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::Unsupported(why)) => why,
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

// ---- O82 type-resist berries -------------------------------------------------------------------

#[test]
fn resist_berries_on_a_spread_move_match_showdown() {
    assert_exact_parity("o82-resist-berries");
}

#[test]
fn chilan_and_roseli_berries_match_showdown() {
    assert_exact_parity("o82-chilan-roseli");
}

/// Klutz makes Showdown ignore the holder's item (`ignoringItem`); no item handler checks it
/// yet, so the engine refuses the combination.
#[test]
fn klutz_holding_an_item_is_refused() {
    let why = refused_with("o82-resist-berries", SideId::Two, "Heatran", |mon| {
        mon.ability = lab_engine::dex::abilities::KLUTZ;
    });
    assert!(why.contains("Klutz"), "{why}");
}

// ---- O83 Choice items --------------------------------------------------------------------------

#[test]
fn choice_scarf_and_band_match_showdown() {
    assert_exact_parity("o83-choice-scarf-band");
}

#[test]
fn choice_lock_kept_and_ended_at_end_of_turn_matches_showdown() {
    assert_exact_parity("o83-choice-lock");
}

#[test]
fn choice_lock_ended_before_the_move_matches_showdown() {
    assert_exact_parity("o83-choice-knocked-before-move");
}

// ---- O84 Expert Belt / Life Orb ----------------------------------------------------------------

#[test]
fn expert_belt_matches_showdown() {
    assert_exact_parity("o84-expert-belt");
}

#[test]
fn life_orb_damage_and_recoil_match_showdown() {
    assert_exact_parity("o84-life-orb");
}

// ---- O89 Assault Vest / Eviolite ---------------------------------------------------------------

#[test]
fn assault_vest_and_eviolite_match_showdown() {
    assert_exact_parity("o89-assault-vest-eviolite");
}

#[test]
fn psyshock_ignores_assault_vest_and_eviolite_special_defense_match_showdown() {
    assert_exact_parity("o89-av-psyshock");
}

/// Assault Vest's `onDisableMove`: its holder cannot choose a status move.
#[test]
fn an_assault_vest_holder_cannot_choose_a_status_move() {
    let name = "o89-assault-vest-eviolite";
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state: Doubles = position.state;
    let mut choices = scenario_choices(&loaded, &state).unwrap();
    // Kingambit (Assault Vest) picks Protect (move 1).
    choices[1][0] = SlotAction::Move {
        index: 1,
        target: 0,
        gimmick: Gimmick::None,
    };
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::InvalidChoice { reason, .. }) => {
            assert!(reason.contains("Assault Vest"), "{reason}")
        }
        other => panic!("expected InvalidChoice, got {other:?}"),
    }
}

// ---- O91 accuracy, critical-hit and flinch items ----------------------------------------------

#[test]
fn wide_lens_and_zoom_lens_match_showdown() {
    assert_exact_parity("o91-wide-zoom-lens");
}

#[test]
fn zoom_lens_against_a_target_that_moves_later_matches_showdown() {
    assert_exact_parity("o91-zoom-lens-slower-target");
}

#[test]
fn scope_lens_and_focus_band_match_showdown() {
    assert_exact_parity("o91-scope-lens-focus-band");
}

#[test]
fn kings_rock_flinch_matches_showdown() {
    assert_exact_parity("o91-kings-rock");
}

// ---- O93 residual and after-move items ---------------------------------------------------------

#[test]
fn black_sludge_and_status_orbs_match_showdown() {
    assert_exact_parity("o93-residual-items");
}

#[test]
fn status_orb_immunity_magic_guard_and_sticky_barb_damage_match_showdown() {
    assert_exact_parity("o93-orb-immunity-sticky-barb");
}

#[test]
fn sticky_barb_transfer_shell_bell_and_throat_spray_match_showdown() {
    assert_exact_parity("o93-sticky-barb-shell-bell-throat-spray");
}

// ---- O87 Air Balloon, O88 Iron Ball / Lagging Tail / Full Incense / Quick Claw ------------------

#[test]
fn air_balloon_and_iron_ball_grounding_match_showdown() {
    assert_exact_parity("o87-o88-grounding");
}

#[test]
fn lagging_tail_full_incense_and_quick_claw_order_match_showdown() {
    assert_exact_parity("o88-lagging-tail-quick-claw");
}

/// Air Balloon pops on a damaging hit (`onDamagingHit`); that event is work plan F15, so the
/// engine refuses the hit instead of keeping the balloon.
#[test]
fn a_damaging_hit_on_an_air_balloon_is_refused() {
    let why = refused_with("o84-life-orb", SideId::Two, "Kingambit", |mon| {
        mon.item = lab_engine::dex::items::AIR_BALLOON;
    });
    assert!(why.contains("Air Balloon"), "{why}");
}

// ---- O90 Safety Goggles / Covert Cloak ---------------------------------------------------------

#[test]
fn safety_goggles_powder_redirection_and_sand_match_showdown() {
    assert_exact_parity("o90-safety-goggles");
}

#[test]
fn covert_cloak_blocks_flinch_secondaries_matches_showdown() {
    assert_exact_parity("o90-covert-cloak");
}

#[test]
fn covert_cloak_keeps_self_secondaries_matches_showdown() {
    assert_exact_parity("o90-covert-cloak-self-secondary");
}

/// A Pokémon locked by its Choice item cannot choose another move (`choicelock`'s
/// `onDisableMove`).
#[test]
fn a_choice_locked_pokemon_cannot_choose_another_move() {
    let name = "o83-choice-lock";
    let fixture = fixture(name);
    let (loaded, position) = start(name, &fixture);
    let mut state: Doubles = position.state;
    let mut choices = scenario_choices(&loaded, &state).unwrap();
    // Hydreigon is locked into Dragon Pulse (move 0); pick Protect (move 1).
    choices[0][0] = SlotAction::Move {
        index: 1,
        target: 0,
        gimmick: Gimmick::None,
    };
    match enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices) {
        Err(TurnError::InvalidChoice { reason, .. }) => {
            assert!(reason.contains("locked into Dragon Pulse"), "{reason}")
        }
        other => panic!("expected InvalidChoice, got {other:?}"),
    }
}
