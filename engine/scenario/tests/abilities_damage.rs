//! Parity of the damage-path abilities (WORKPLAN O40–O47) with Showdown: each scenario's exact
//! outcome distribution must equal its oracle fixture (`engine/oracle/expected/*.turn.json`).

mod common;

use common::assert_exact_parity;

// O40: base power abilities.

#[test]
fn punk_rock_and_technician_match_showdown() {
    assert_exact_parity("o40-punkrock-technician");
}

#[test]
fn steely_spirit_and_tough_claws_match_showdown() {
    assert_exact_parity("o40-steelyspirit-toughclaws");
}

#[test]
fn iron_fist_and_mega_launcher_match_showdown() {
    assert_exact_parity("o40-ironfist-megalauncher");
}

#[test]
fn reckless_and_sharpness_match_showdown() {
    assert_exact_parity("o40-reckless-sharpness");
}

#[test]
fn strong_jaw_matches_showdown() {
    assert_exact_parity("o40-strongjaw");
}

#[test]
fn technician_with_priority_and_weight_power_matches_showdown() {
    assert_exact_parity("o40-technician");
}

// O41: STAB.

#[test]
fn adaptability_matches_showdown() {
    assert_exact_parity("o41-adaptability");
}

// O42: pinch abilities (ModifyAtk / ModifySpA).

#[test]
fn blaze_and_torrent_at_the_hp_boundary_match_showdown() {
    assert_exact_parity("o42-blaze-torrent");
}

#[test]
fn overgrow_and_swarm_match_showdown() {
    assert_exact_parity("o42-overgrow-swarm");
}

// O43: Hustle (attack and accuracy).

#[test]
fn hustle_matches_showdown() {
    assert_exact_parity("o43-hustle");
}

// O44: status-boosted stats.

#[test]
fn guts_and_marvel_scale_match_showdown() {
    assert_exact_parity("o44-guts-marvelscale");
}

#[test]
fn quick_feet_under_paralysis_matches_showdown() {
    assert_exact_parity("o44-quickfeet");
}

// O45: type-based defensive abilities.

#[test]
fn water_bubble_matches_showdown() {
    assert_exact_parity("o45-waterbubble");
}

#[test]
fn thick_fat_and_heatproof_match_showdown() {
    assert_exact_parity("o45-thickfat-heatproof");
}

#[test]
fn purifying_salt_and_moongeist_beam_match_showdown() {
    assert_exact_parity("o45-purifyingsalt");
}

#[test]
fn dry_skin_in_rain_matches_showdown() {
    assert_exact_parity("o45-dryskin-rain");
}

#[test]
fn dry_skin_in_sun_matches_showdown() {
    assert_exact_parity("o45-dryskin-sun");
}

// O46: final damage reductions (onSourceModifyDamage).

#[test]
fn filter_and_multiscale_match_showdown() {
    assert_exact_parity("o46-filter-multiscale");
}

#[test]
fn solid_rock_and_fluffy_match_showdown() {
    assert_exact_parity("o46-solidrock-fluffy");
}

#[test]
fn prism_armor_and_shadow_shield_against_ability_ignoring_moves_match_showdown() {
    assert_exact_parity("o46-prismarmor-shadowshield");
}

#[test]
fn ice_scales_and_aura_guard_match_showdown() {
    assert_exact_parity("o46-icescales-auraguard");
}
