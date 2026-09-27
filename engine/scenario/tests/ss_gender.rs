//! Opus SS (wave 15 L1, board R13b): undecided genders in the initial distribution. Showdown
//! draws `this.battle.sample(['M', 'F'])` for a set with neither a gender nor a species' fixed
//! one; `lab_scenario::decide_genders` splits the start into every assignment at 1/2^k when the
//! battle can read a gender (Attract, Cute Charm, Rivalry). The fixtures are Showdown's reports
//! mixed over the same assignments (`oracle/gender-mix.cjs`: one enumerate.cjs run per
//! assignment, weight 1/2^k), so the engine side mixes every position with the oracle's `before`
//! (they differ only in the genders the canonical state leaves out).

mod common;

use std::collections::HashMap;

use common::{engine_dir, fixture, key};
use lab_engine::dex::Gender;
use lab_engine::state::SideId;
use lab_scenario::{
    canonical_value, load_scenario_file, run_decision_mid_turn, scenario_decision,
    scenario_positions,
};

/// The turn from every position with the fixture's `before`, weighted by the positions'
/// probabilities, against the oracle's mixed distribution.
fn assert_mixed_parity(name: &str) -> usize {
    let fixture = fixture(name);
    let loaded =
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    let before = key(&fixture["before"]);
    let positions: Vec<_> = scenario_positions(&loaded)
        .unwrap()
        .into_iter()
        .filter(|p| key(&canonical_value(&p.state, &loaded.meta).unwrap()) == before)
        .collect();
    assert!(
        !positions.is_empty(),
        "{name}: no position has the oracle's `before`"
    );
    let total: f64 = positions.iter().map(|p| p.probability).sum();
    let mut engine: HashMap<String, f64> = HashMap::new();
    for position in &positions {
        let mut state = position.state.clone();
        let decision = scenario_decision(&loaded, position).unwrap();
        let outcomes =
            run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        for (k, p) in common::distribution(&loaded, &mut state, &outcomes) {
            *engine.entry(k).or_default() += p * position.probability / total;
        }
    }
    let oracle = common::oracle_distribution(&fixture);
    assert_eq!(engine.len(), oracle.len(), "{name}: number of outcomes");
    for (state, p) in &oracle {
        let q = engine
            .get(state)
            .unwrap_or_else(|| panic!("{name}: engine lacks an oracle outcome:\n{state}"));
        assert!(
            (p - q).abs() < 1e-12,
            "{name}: p {p} vs engine {q} for\n{state}"
        );
    }
    positions.len()
}

/// Snorlax's Attract on Umbreon, four sets without a gender: attracted exactly when the two
/// differ (1/2).
#[test]
fn attract_with_undecided_genders() {
    assert_eq!(assert_mixed_parity("rr-attract-undecided-gender"), 16);
}

/// Cute Charm (30% on contact) between a Garchomp and a Milotic of undecided gender.
#[test]
fn cute_charm_with_undecided_genders() {
    assert_mixed_parity("rr-cute-charm-undecided-gender");
}

/// Rivalry's 1.25x / 0.75x against a target of undecided gender.
#[test]
fn rivalry_with_undecided_genders() {
    assert_mixed_parity("rr-rivalry-undecided-gender");
}

/// A Rivalry holder switching in next to Pokémon of undecided gender.
#[test]
fn rivalry_switching_in_with_undecided_genders() {
    assert_mixed_parity("rr-rivalry-switch-in-undecided-gender");
}

/// Without a reader nothing is decided: one start, the genders stay undecided.
#[test]
fn genders_nothing_reads_stay_undecided() {
    let loaded =
        load_scenario_file(engine_dir().join("oracle/scenarios/ss-supersweet-syrup-once.json"))
            .unwrap();
    let outcomes = lab_scenario::initial_outcomes(&loaded).unwrap();
    assert_eq!(outcomes.len(), 1);
    let state = &outcomes[0].state;
    // Snorlax has a gender ratio and no set gender.
    assert_eq!(state.side(SideId::One).party[1].gender, Gender::Random);
}
