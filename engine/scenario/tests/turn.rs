//! Turn engine parity with Showdown: lab-engine's exact outcome distribution for an oracle
//! scenario must equal the oracle's `full` enumeration (`engine/oracle/expected/*.turn.json`,
//! made by `enumerate.cjs` + `strip-report.cjs`): the same canonical end states with the same
//! probabilities. Helpers live in `common/mod.rs`; feature-specific parity tests go in their own
//! files (one per work unit) so parallel work does not collide here.

mod common;

use common::{assert_exact_parity, distribution, fixture, start};
use lab_engine::rules::Ruleset;
use lab_engine::turn::{enumerate_turn, sample_turn};
use lab_scenario::scenario_choices;

#[test]
fn single_hit_matches_showdown_exactly() {
    assert_exact_parity("single-hit");
}

#[test]
fn hypnosis_under_gravity_matches_showdown_exactly() {
    assert_exact_parity("hypnosis-gravity");
}

#[test]
fn sampling_agrees_with_enumeration() {
    let fixture = fixture("single-hit");
    let (loaded, mut state) = start("single-hit", &fixture);
    let choices = scenario_choices(&loaded, &state).unwrap();
    let exact = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, choices).unwrap();
    let exact = distribution(&loaded, &mut state, &exact);
    let samples = 40_000;
    let sampled = sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, samples, 11).unwrap();
    let sampled = distribution(&loaded, &mut state, &sampled);
    let mut tv = 0.0;
    for (k, p) in &exact {
        tv += (p - sampled.get(k).copied().unwrap_or(0.0)).abs() / 2.0;
    }
    for k in sampled.keys() {
        assert!(exact.contains_key(k), "sampled an impossible outcome");
    }
    // Noise ~ sqrt(k / (2 pi n)) with k = 271 outcomes.
    let noise = (exact.len() as f64 / (2.0 * std::f64::consts::PI * samples as f64)).sqrt();
    assert!(tv < 3.0 * noise, "TV {tv} vs noise {noise}");
}
