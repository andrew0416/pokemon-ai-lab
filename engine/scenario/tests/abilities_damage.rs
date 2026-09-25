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
