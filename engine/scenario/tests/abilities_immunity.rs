//! Parity of move-blocking, weather-suppressing, secondary-effect, status-passing and
//! ability-ignoring abilities (work plan units O48, O49, O60, O66, O67) with Showdown's exact
//! outcome distribution (`engine/oracle/expected/*.turn.json`).

mod common;

use common::{assert_exact_parity, fixture, start};
use lab_engine::dex::abilities;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::enumerate_turn;
use lab_scenario::scenario_choices;

// ---- O48 absorbing and immunity abilities ------------------------------------------------------

#[test]
fn healing_absorbers_match_showdown() {
    assert_exact_parity("o48-absorb-heal");
}

#[test]
fn boosting_absorbers_and_flash_fire_absorb_match_showdown() {
    assert_exact_parity("o48-boost-absorb");
}

#[test]
fn flash_fire_boost_matches_showdown() {
    assert_exact_parity("o48-flash-fire-boost");
}

/// Flash Fire's `move.accuracy = true` makes the rest of a spread move never miss.
#[test]
fn flash_fire_spread_accuracy_matches_showdown() {
    assert_exact_parity("o48-flash-fire-spread");
}

#[test]
fn bulletproof_soundproof_overcoat_telepathy_match_showdown() {
    assert_exact_parity("o48-immunity");
}

#[test]
fn wonder_guard_and_good_as_gold_match_showdown() {
    assert_exact_parity("o48-wonder-guard-gold");
}

#[test]
fn dazzling_and_armor_tail_match_showdown() {
    assert_exact_parity("o48-dazzling");
}

// ---- O49 Air Lock / Cloud Nine -----------------------------------------------------------------

/// Cloud Nine and Drought both lead (start expansion); the sun counts down but does nothing.
#[test]
fn cloud_nine_suppresses_sun_match_showdown() {
    assert_exact_parity("o49-cloud-nine-sun");
}

/// The suppression ends as soon as the Air Lock holder faints, within the turn.
#[test]
fn air_lock_holder_fainting_restores_sand_match_showdown() {
    assert_exact_parity("o49-air-lock-faint");
}

// ---- O60 secondary effects ---------------------------------------------------------------------

/// Inner Focus: no flinch, and a mid-turn Intimidate leaves its Attack alone.
#[test]
fn inner_focus_matches_showdown() {
    assert_exact_parity("o60-inner-focus");
}

/// Shield Dust drops flinch and burn secondaries (no roll); also Flash Fire's SpA boost.
#[test]
fn shield_dust_matches_showdown() {
    assert_exact_parity("o60-shield-dust");
}

#[test]
fn shield_dust_keeps_self_boosts_matches_showdown() {
    assert_exact_parity("o60-shield-dust-self-boost");
}

#[test]
fn serene_grace_matches_showdown() {
    assert_exact_parity("o60-serene-grace");
}

/// Sheer Force skips the secondaries, Life Orb recoil and the AfterMoveSecondary thaw.
#[test]
fn sheer_force_matches_showdown() {
    assert_exact_parity("o60-sheer-force");
}

// ---- O66 Synchronize ---------------------------------------------------------------------------

#[test]
fn synchronize_matches_showdown() {
    assert_exact_parity("o66-synchronize");
}

// ---- O67 Mold Breaker / Teravolt / Turboblaze ----------------------------------------------------

/// Mold Breaker ignores Levitate; Teravolt ignores Lightning Rod's redirection.
#[test]
fn mold_breaker_and_teravolt_match_showdown() {
    assert_exact_parity("o67-mold-breaker");
}

/// Turboblaze ignores Armor Tail; Mold Breaker ignores Purifying Salt's status block.
#[test]
fn turboblaze_and_mold_breaker_status_match_showdown() {
    assert_exact_parity("o67-turboblaze");
}

/// Against Water Veil, Mold Breaker's Will-O-Wisp burns through the suppressed ability and the
/// Update after the action cures the burn (`abilities::on_update`; the timing is checked against
/// Showdown by `o58-update-cures`): no outcome leaves the holder burned.
#[test]
fn mold_breaker_status_on_an_update_curing_ability_is_cured() {
    let fixture = fixture("o67-turboblaze");
    let (loaded, position) = start("o67-turboblaze", &fixture);
    let mut state = position.state;
    let choices = scenario_choices(&loaded, &state).unwrap();
    let garganacl = state
        .side(SideId::Two)
        .party
        .iter()
        .position(|p| p.species.data().name == "Garganacl")
        .unwrap();
    let mon = &mut state.side_mut(SideId::Two).party[garganacl];
    mon.ability = abilities::WATER_VEIL;
    mon.base_ability = abilities::WATER_VEIL;
    let outcomes = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    for outcome in &outcomes {
        state.apply(&outcome.instructions);
        assert_ne!(
            state.side(SideId::Two).party[garganacl].status,
            lab_engine::state::Status::Burn
        );
        state.reverse(&outcome.instructions);
    }
}
