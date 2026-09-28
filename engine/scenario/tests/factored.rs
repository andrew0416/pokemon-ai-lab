//! The factored enumeration (WORKPLAN P1b; DESIGN.md "Full 모드의 HP 인수분해"): lazy HP units
//! split at thresholds, expanded where the HP is read as a number, and compacted into products.
//! It must give exactly the distribution of the flat enumeration: the oracle fixtures through the
//! factored path (`FactoredScope`), and `spread-damage` (two spread moves, whose flat Full
//! enumeration does not fit in memory) against the flat enumeration under the reduced roll modes
//! and against sampling under Full.

mod common;

use std::collections::HashMap;

use common::{assert_exact_parity, assert_extremes_parity, assert_fixed_parity, engine_dir};
use lab_engine::rules::Ruleset;
use lab_engine::state::{PokemonRef, SideId};
use lab_engine::turn::{
    enumerate_turn_factored, enumerate_turn_factored_with, sample_turn, EnumerateOptions,
    FactoredOptions, FactoredScope, RollMode,
};
use lab_engine::Doubles;
use lab_scenario::{
    canonical_json, load_scenario_file, run_decision_with, scenario_decision, scenario_positions,
    Decision, LoadedScenario, Position,
};

/// Fixtures whose turns read HP: KO thresholds, berries, Focus Sash / Sturdy / Endure, heals and
/// their caps, HP-scaled power, HP arithmetic (Pain Split, Endeavor, Super Fang, Final Gambit),
/// ability thresholds (pinch abilities, Multiscale, Gale Wings, Anger Shell, Emergency Exit,
/// Schooling, Shields Down, Zen Mode), recoil and drain, substitutes, spread moves.
const HP_FIXTURES: &[&str] = &[
    "single-hit",
    "belly-drum",
    "brine",
    "brine-above-half",
    "clangorous-soul",
    "crush-grip-hard-press",
    "dd-emergency-exit-recoil-eject-pack",
    "dd-emergency-exit-recoil-ko",
    "ee-life-orb",
    "ee-recoil",
    "ee-spread-done",
    "ee-spread-failed",
    "emergency-exit",
    "emergency-exit-below",
    "emergency-exit-hazard",
    "emergency-exit-residual",
    "endeavor",
    "eruption",
    "hex-reversal",
    "healing-wish",
    "heal-pulse",
    "history-tantrum-assurance",
    "jungle-healing-floral",
    "memento-final-gambit",
    "micle-accuracy-true",
    "o25-heal-sand",
    "o25-heal-snow-full",
    "o27-endure",
    "o38-instruct-spread",
    "o42-blaze-torrent",
    "o42-overgrow-swarm",
    "o46-filter-multiscale",
    "o48-absorb-heal",
    "o48-flash-fire-spread",
    "o53-sturdy",
    "o55-anger-shell-berserk",
    "o61-gale-wings-damaged",
    "o81-custap",
    "o84-life-orb",
    "o91-scope-lens-focus-band",
    "o92-seeds-grassy-misty",
    "o93-sticky-barb-shell-bell-throat-spray",
    "q-fire-mane-defeatist",
    "q-moxie-spread",
    "r-spread-eject-buttons",
    "r-spread-red-cards",
    "revival-blessing",
    "schooling-residual",
    "shields-down-residual",
    "sitrus-lum",
    "spiky-shield-spread-ko",
    "strength-sap-pain-split",
    "substitute-recoil-drain",
    "substitute-spread",
    "super-fang",
    "switch-update-sitrus",
    "u-poison-heal",
    "uturn-emergency-exit",
    "w-berry-juice",
    "wish-heal",
    "y-shed-tail",
    "z-dragon-darts-emergency-exit",
    "zen-mode-residual",
];

#[test]
fn hp_fixtures_match_through_the_factored_path() {
    let _factored = FactoredScope::new(true);
    for name in HP_FIXTURES {
        assert_exact_parity(name);
    }
}

/// Real library turns (oracle `--mode extremes`) and a mirror of spread moves (oracle fixed
/// rolls), through the factored path.
#[test]
fn heavy_turns_match_through_the_factored_path() {
    let _factored = FactoredScope::new(true);
    for name in [
        "cc-lib-balance-ddee-vs-crown-cecil9",
        "cc-lib-coaching-panda-vs-psy-sand-udon",
        "cc-lib-crown-cecil9-vs-perish-mrada",
        "cc-lib-psy-cona-vs-sand-owen",
        "cc-lib-psy-sand-udon-vs-balance-ddee",
        "cc-lib-sand-owen-vs-coaching-panda",
        "jj-floette-mirror-spread",
    ] {
        assert_extremes_parity(name);
    }
    for roll in [0, 7, 15] {
        assert_fixed_parity("jj-floette-mirror-spread", roll);
    }
}

fn spread_damage() -> (LoadedScenario, Vec<Position>) {
    let loaded =
        load_scenario_file(engine_dir().join("oracle/scenarios/spread-damage.json")).unwrap();
    let positions = scenario_positions(&loaded).unwrap();
    (loaded, positions)
}

/// Canonical end state → probability of `position`'s turn, flat or factored.
fn distribution(
    loaded: &LoadedScenario,
    position: &Position,
    rolls: RollMode,
    factored: bool,
) -> HashMap<String, f64> {
    let _scope = FactoredScope::new(factored);
    let mut state = position.state.clone();
    let decision = scenario_decision(loaded, position).unwrap();
    let outcomes = run_decision_with(&mut state, &decision, EnumerateOptions { rolls }).unwrap();
    let mut out = HashMap::new();
    for o in &outcomes {
        state.apply(&o.instructions);
        *out.entry(canonical_json(&state, &loaded.meta).unwrap())
            .or_insert(0.0) += o.probability;
        state.reverse(&o.instructions);
    }
    out
}

/// `spread-damage` under every reduced roll mode: the factored enumeration gives the flat one's
/// distribution.
#[test]
fn spread_damage_reduced_rolls_match_the_flat_enumeration() {
    let (loaded, positions) = spread_damage();
    for position in &positions {
        for rolls in [
            RollMode::Extremes,
            RollMode::Quartiles,
            RollMode::Fixed(0),
            RollMode::Fixed(9),
        ] {
            let flat = distribution(&loaded, position, rolls, false);
            let factored = distribution(&loaded, position, rolls, true);
            assert_eq!(flat.len(), factored.len(), "{rolls:?}: outcome count");
            for (key, p) in &flat {
                let q = factored.get(key).copied().unwrap_or(-1.0);
                assert!((p - q).abs() < 1e-12, "{rolls:?}: {p} vs {q} for\n{key}");
            }
        }
    }
}

/// The HP marginals of every party member under an outcome list: member → hp → probability.
type Marginals = HashMap<PokemonRef, HashMap<i16, f64>>;

fn members() -> impl Iterator<Item = PokemonRef> {
    [SideId::One, SideId::Two]
        .into_iter()
        .flat_map(|side| (0..2u8).map(move |party| PokemonRef { side, party }))
}

/// `spread-damage` under Full: the factored enumeration completes (the flat one runs out of
/// memory), its probabilities sum to 1, and every member's exact HP marginal agrees with 40,000
/// samples of the turn within sampling noise.
#[test]
fn spread_damage_full_matches_sampling() {
    let (loaded, positions) = spread_damage();
    let position = &positions[0];
    let Decision::Turn(choices) = scenario_decision(&loaded, position).unwrap() else {
        panic!("a turn");
    };
    let mut state: Doubles = position.state.clone();
    let factored = enumerate_turn_factored(
        &mut state,
        Ruleset::CHAMPIONS_MC,
        choices,
        EnumerateOptions::default(),
    )
    .unwrap();
    let total: f64 = factored.iter().map(|o| o.probability).sum();
    assert!((total - 1.0).abs() < 1e-9, "total {total}");
    let flat: f64 = factored.iter().map(|o| o.flat_count()).sum();
    assert!(flat > 1e6, "{flat} flat outcomes");
    let mut exact: Marginals = HashMap::new();
    for o in &factored {
        state.apply(&o.instructions);
        for member in members() {
            let listed = o.hp.iter().find(|(p, _)| *p == member);
            let values = match listed {
                Some((_, values)) => values.clone(),
                None => vec![(state.pokemon(member).hp, 1.0)],
            };
            let marginal = exact.entry(member).or_default();
            for (hp, p) in values {
                *marginal.entry(hp).or_insert(0.0) += o.probability * p;
            }
        }
        state.reverse(&o.instructions);
    }
    let samples = 40_000;
    let sampled = sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, samples, 7).unwrap();
    let mut seen: Marginals = HashMap::new();
    for o in &sampled {
        state.apply(&o.instructions);
        for member in members() {
            *seen
                .entry(member)
                .or_default()
                .entry(state.pokemon(member).hp)
                .or_insert(0.0) += o.probability;
        }
        state.reverse(&o.instructions);
    }
    let n = samples as f64;
    for member in members() {
        let exact = &exact[&member];
        for (hp, &q) in &seen[&member] {
            assert!(
                exact.contains_key(hp),
                "{member:?}: sampled HP {hp} is not in the exact distribution"
            );
            let _ = q;
        }
        for (hp, &p) in exact {
            let q = seen[&member].get(hp).copied().unwrap_or(0.0);
            let sigma = (p * (1.0 - p) / n).sqrt();
            assert!(
                (p - q).abs() <= 5.0 * sigma + 2.0 / n,
                "{member:?} HP {hp}: exact {p}, sampled {q} (σ {sigma})"
            );
        }
    }
}

/// Canonical end state → probability of factored outcomes (expanded).
fn expanded(
    loaded: &LoadedScenario,
    state: &mut Doubles,
    outcomes: &[lab_engine::turn::FactoredOutcome],
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for o in outcomes.iter().flat_map(|o| o.expand()) {
        state.apply(&o.instructions);
        *out.entry(canonical_json(state, &loaded.meta).unwrap())
            .or_insert(0.0) += o.probability;
        state.reverse(&o.instructions);
    }
    out
}

/// P1c: with `max_support`, every member keeps at most that many HP values, the probabilities
/// still sum to 1, and the reported bound holds for the actual total variation distance from
/// the exact distribution (`spread-damage` under Quartiles, whose exact distribution expands).
#[test]
fn max_support_bounds_the_distance_from_the_exact_distribution() {
    let (loaded, positions) = spread_damage();
    let position = &positions[0];
    let Decision::Turn(choices) = scenario_decision(&loaded, position).unwrap() else {
        panic!("a turn");
    };
    let mut state: Doubles = position.state.clone();
    let run = |state: &mut Doubles, max_support| {
        enumerate_turn_factored_with(
            state,
            Ruleset::CHAMPIONS_MC,
            choices,
            FactoredOptions {
                rolls: RollMode::Quartiles,
                max_support,
            },
        )
        .unwrap()
    };
    let exact = run(&mut state, None);
    assert_eq!(exact.tv_bound, 0.0);
    let exact_dist = expanded(&loaded, &mut state, &exact.outcomes);
    for k in [1, 2, 3, 5] {
        let approx = run(&mut state, Some(k));
        for o in &approx.outcomes {
            assert!(o.hp.iter().all(|(_, values)| values.len() <= k));
        }
        let dist = expanded(&loaded, &mut state, &approx.outcomes);
        let total: f64 = dist.values().sum();
        assert!((total - 1.0).abs() < 1e-9, "k {k}: total {total}");
        let mut tv = 0.0;
        for (key, p) in &exact_dist {
            tv += (p - dist.get(key).copied().unwrap_or(0.0)).abs() / 2.0;
        }
        for (key, q) in &dist {
            if !exact_dist.contains_key(key) {
                tv += q / 2.0;
            }
        }
        assert!(
            tv <= approx.tv_bound + 1e-9,
            "k {k}: TV {tv} above the bound {}",
            approx.tv_bound
        );
        assert!(approx.tv_bound > 0.0, "k {k}: something was merged");
    }
    // Full: 32 values per member keep the bound small.
    let full = enumerate_turn_factored_with(
        &mut state,
        Ruleset::CHAMPIONS_MC,
        choices,
        FactoredOptions {
            rolls: RollMode::Full,
            max_support: Some(32),
        },
    )
    .unwrap();
    let total: f64 = full.outcomes.iter().map(|o| o.probability).sum();
    assert!((total - 1.0).abs() < 1e-9);
    assert!(full.tv_bound < 0.05, "bound {}", full.tv_bound);
}
