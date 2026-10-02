use super::engine::{EngineWorld, Knowledge};
use super::*;
use crate::budgeted::{Domain, EngineDomain, Position};
use crate::{nash, Pruning};
use lab_engine::{eval::Heuristic, rules::Ruleset, state::SideId, turn::EnumerateOptions};
use std::path::Path;

fn world(id: &str, weight: f64, cols: usize, payoffs: &[f64]) -> World {
    World {
        id: id.into(),
        weight,
        columns: (0..cols).map(|i| format!("c{i}")).collect(),
        payoffs: payoffs.to_vec(),
    }
}
fn game(rows: usize, worlds: Vec<World>) -> Game {
    Game::new((0..rows).map(|i| format!("r{i}")).collect(), worlds).unwrap()
}
fn config() -> Config {
    Config {
        iterations: 60_000,
        tolerance: 0.003,
        check_every: 64,
    }
}
fn near(a: f64, b: f64, eps: f64) {
    assert!((a - b).abs() <= eps, "{a} != {b} within {eps}");
}

#[test]
fn shared_policy_cannot_use_the_hidden_world() {
    let g = game(
        2,
        vec![world("a", 1., 1, &[1., -1.]), world("b", 1., 1, &[-1., 1.])],
    );
    let s = solve(&g, config()).unwrap();
    assert_eq!(s.ours, vec![0.5, 0.5]);
    assert_eq!(s.assessment.value, 0.0);
    assert_eq!(s.assessment.gap, 0.0);
    // Solving each world with a different row policy would incorrectly claim value +1.
    let omniscient: f64 = g
        .worlds()
        .iter()
        .map(|w| w.weight * w.payoffs.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        .sum();
    assert_eq!(omniscient, 1.0);
}

#[test]
fn informed_opponent_cannot_be_replaced_by_an_average_matrix() {
    let g = game(
        1,
        vec![world("a", 1., 2, &[1., -1.]), world("b", 1., 2, &[-1., 1.])],
    );
    let s = solve(&g, config()).unwrap();
    near(s.assessment.value, -1., 1e-12);
    assert_eq!(s.theirs, vec![vec![0., 1.], vec![1., 0.]]);
    // The average matrix [0,0] loses the opponent's private type and falsely returns 0.
    assert_eq!((g.worlds[0].payoffs[0] + g.worlds[1].payoffs[0]) / 2., 0.);
}

#[test]
fn posterior_changes_the_shared_decision_and_preserves_world_ids() {
    let g = game(
        2,
        vec![world("a", 1., 1, &[1., -1.]), world("b", 1., 1, &[-1., 1.])],
    );
    let post = g
        .conditioned(&[("b".into(), 0.1), ("a".into(), 0.9)])
        .unwrap();
    assert_eq!(post.worlds[0].id, "a");
    near(post.worlds[0].weight, 0.9, 1e-14);
    let s = solve(&post, config()).unwrap();
    assert_eq!(s.ours, vec![1., 0.]);
    near(s.assessment.value, 0.8, 1e-12);
    assert_eq!(g.worlds[0].weight, 0.5); // immutable original belief
}

#[test]
fn one_world_matching_pennies_and_nonuniform_equilibrium() {
    for (a, p, value) in [
        ([1., -1., -1., 1.], 0.5, 0.),
        ([3., 0., 0., 1.], 0.25, 0.75),
    ] {
        let s = solve(&game(2, vec![world("one", 1., 2, &a)]), config()).unwrap();
        assert!(s.converged, "gap {}", s.assessment.gap);
        near(s.ours[0], p, 0.004);
        near(s.theirs[0][0], p, 0.004);
        near(s.assessment.value, value, 0.004);
    }
}

// Independent normal-form construction: a pure column specifies an action IN EVERY
// hidden world. Used only for tiny tests; production CFR never builds this product.
fn expanded(g: &Game) -> nash::Matrix {
    let count: usize = g.worlds.iter().map(|w| w.columns.len()).product();
    let mut values = Vec::new();
    for r in 0..g.rows.len() {
        for index in 0..count {
            let mut code = index;
            let mut value = 0.;
            for w in &g.worlds {
                let c = code % w.columns.len();
                code /= w.columns.len();
                value += w.weight * w.payoffs[r * w.columns.len() + c];
            }
            values.push(value as f32);
        }
    }
    nash::Matrix::new(g.rows.len(), count, values)
}

#[test]
fn generated_games_match_independent_expanded_normal_form() {
    let mut seed = 731u64;
    for case in 0..24 {
        let n = 2 + case % 2;
        let mut worlds = Vec::new();
        for w in 0..2 {
            let m = 2 + w;
            let values: Vec<_> = (0..n * m)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((seed >> 32) % 9) as f64 - 4.
                })
                .collect();
            worlds.push(world(
                &format!("w{w}"),
                (w + case % 3 + 1) as f64,
                m,
                &values,
            ));
        }
        let g = game(n, worlds);
        let s = solve(&g, config()).unwrap();
        assert!(s.assessment.gap < 0.02, "case {case}: {}", s.assessment.gap);
        let flat = expanded(&g);
        let reference = nash::solve(&flat, 100_000, 0.0001);
        assert!(reference.exploitability < 0.01);
        assert!(f64::from(reference.value) >= s.assessment.lower - 0.011);
        assert!(f64::from(reference.value) <= s.assessment.upper + 0.011);
        // Recompute the lower bound directly over ALL contingent pure responses.
        let lower = (0..flat.cols)
            .map(|c| {
                (0..n)
                    .map(|r| s.ours[r] * f64::from(flat.at(r, c)))
                    .sum::<f64>()
            })
            .fold(f64::INFINITY, f64::min);
        near(lower, s.assessment.lower, 1e-6);
    }
}

#[test]
fn assess_uses_shared_row_response_and_world_specific_column_response() {
    let g = game(
        2,
        vec![
            world("a", 1., 2, &[2., -1., 0., 3.]),
            world("b", 3., 3, &[1., 4., -2., -1., 2., 5.]),
        ],
    );
    let p = [0.3, 0.7];
    let q = vec![vec![0.8, 0.2], vec![0.2, 0.3, 0.5]];
    let s = g.assess(&p, &q).unwrap();
    let flat = expanded(&g);
    let products: Vec<_> = (0..flat.cols).map(|c| q[0][c % 2] * q[1][c / 2]).collect();
    let upper = (0..2)
        .map(|r| {
            (0..flat.cols)
                .map(|c| products[c] * f64::from(flat.at(r, c)))
                .sum::<f64>()
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let lower = (0..flat.cols)
        .map(|c| (0..2).map(|r| p[r] * f64::from(flat.at(r, c))).sum::<f64>())
        .fold(f64::INFINITY, f64::min);
    near(s.upper, upper, 1e-12);
    near(s.lower, lower, 1e-12);
    near(s.gap, upper - lower, 1e-12);
}

#[test]
fn permutations_preserve_equilibrium_and_zero_worlds_keep_their_indices() {
    let g = game(
        2,
        vec![
            world("a", 1., 2, &[3., 0., 0., 1.]),
            world("zero", 0., 1, &[1e4, -1e4]),
            world("b", 3., 1, &[0., 0.]),
        ],
    );
    let s = solve(&g, config()).unwrap();
    assert_eq!(s.theirs[1], vec![1.]);
    near(s.ours[0], 0.25, 0.006);
    let reversed = game(
        2,
        vec![
            world("b", 3., 1, &[0., 0.]),
            world("zero", 0., 1, &[-1e4, 1e4]),
            world("a", 1., 2, &[1., 0., 0., 3.]),
        ],
    );
    let t = solve(&reversed, config()).unwrap();
    near(s.ours[0], t.ours[1], 0.006);
    near(s.assessment.value, t.assessment.value, 0.003);
}

#[test]
fn scale_and_weight_normalization_are_finite_and_do_not_drop_positive_worlds() {
    let g = game(
        1,
        vec![
            world("a", f64::MAX, 1, &[1e300]),
            world("b", f64::MAX, 1, &[-1e300]),
        ],
    );
    assert_eq!(g.worlds[0].weight, 0.5);
    let s = solve(&g, config()).unwrap();
    assert_eq!(s.assessment.value, 0.);
    assert!(s.assessment.gap.is_finite());
    assert!(Game::new(
        vec!["r".into()],
        vec![
            world("a", f64::MAX, 1, &[0.]),
            world("b", f64::MIN_POSITIVE, 1, &[0.])
        ]
    )
    .is_err());
    let tiny = game(
        1,
        vec![world("a", 1e-300, 1, &[1.]), world("b", 1., 1, &[2.])],
    );
    let post = tiny
        .conditioned(&[("a".into(), 1.), ("b".into(), 1e-300)])
        .unwrap();
    near(post.worlds[0].weight, 0.5, 1e-12);
}

#[test]
fn invalid_games_policies_beliefs_and_solver_configs_fail() {
    assert!(Game::new(vec![], vec![]).is_err());
    for weight in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(Game::new(vec!["r".into()], vec![world("a", weight, 1, &[1.])]).is_err());
    }
    for a in [f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(Game::new(vec!["r".into()], vec![world("a", 1., 1, &[a])]).is_err());
    }
    assert!(Game::new(vec!["r".into()], vec![world("a", 1., 2, &[1.])]).is_err());
    assert!(Game::new(
        vec!["r".into(), "r".into()],
        vec![world("a", 1., 1, &[1., 2.])]
    )
    .is_err());
    assert!(Game::new(
        vec!["r".into()],
        vec![world("a", 1., 1, &[1.]), world("a", 1., 1, &[2.])]
    )
    .is_err());
    let g = game(1, vec![world("a", 1., 1, &[0.]), world("b", 0., 1, &[1.])]);
    for ls in [
        vec![("a".into(), 0.), ("b".into(), 1.)],
        vec![("a".into(), 1.)],
        vec![("a".into(), 1.), ("a".into(), 1.)],
        vec![("a".into(), 1.), ("other".into(), 1.)],
        vec![("a".into(), f64::NAN), ("b".into(), 1.)],
    ] {
        assert!(g.conditioned(&ls).is_err());
    }
    for cfg in [
        Config {
            iterations: 0,
            ..config()
        },
        Config {
            check_every: 0,
            ..config()
        },
        Config {
            tolerance: -1.,
            ..config()
        },
        Config {
            tolerance: f64::NAN,
            ..config()
        },
    ] {
        assert!(solve(&g, cfg).is_err());
    }
    assert!(g.assess(&[0.9], &[vec![1.], vec![1.]]).is_err());
    assert!(g.assess(&[1.], &[vec![1.]]).is_err());
    assert!(g.assess(&[1.], &[vec![-1.], vec![1.]]).is_err());
}

#[test]
fn exhaustion_is_not_reported_as_convergence() {
    let g = game(2, vec![world("a", 1., 2, &[3., 0., 0., 1.])]);
    let s = solve(
        &g,
        Config {
            iterations: 1,
            tolerance: 0.,
            check_every: 32,
        },
    )
    .unwrap();
    assert!(!s.converged);
    assert_eq!(s.iterations, 1);
    assert!(s.assessment.gap > 0.);
}

fn fixture(name: &str) -> Position<2> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios")
        .join(name);
    let loaded = lab_scenario::load_scenario_file(path).unwrap();
    let mut ps =
        lab_scenario::scenario_positions_with(&loaded, EnumerateOptions::default()).unwrap();
    assert_eq!(ps.len(), 1);
    Position {
        state: ps.remove(0).state,
        suspension: None,
    }
}
fn ew(id: &str, p: Position<2>) -> EngineWorld<2> {
    EngineWorld {
        id: id.into(),
        weight: 1.,
        position: p,
    }
}

#[test]
fn engine_full_chance_matches_direct_enumeration_for_both_sides() {
    let p = fixture("super-fang.json");
    let before = p.state.clone();
    for us in [SideId::One, SideId::Two] {
        let worlds = vec![ew("true", p.clone()), ew("same", p.clone())];
        let b = engine::one_turn(
            &worlds,
            us,
            Ruleset::CHAMPIONS_MC,
            &Heuristic,
            &Knowledge::default(),
            100_000,
        )
        .unwrap();
        let domain = EngineDomain {
            ruleset: Ruleset::CHAMPIONS_MC,
            options: EnumerateOptions::default(),
            pruning: Pruning::All,
            us,
            evaluator: &Heuristic,
        };
        for (r, row) in b.ours.iter().enumerate() {
            for (c, col) in b.theirs[0].iter().enumerate() {
                let children = domain.transitions(&p, [row, col]).unwrap();
                let direct: f64 = children
                    .iter()
                    .map(|(p, s)| p * f64::from(domain.value(s)))
                    .sum();
                assert_eq!(b.game.worlds[0].payoffs[r * b.theirs[0].len() + c], direct);
            }
        }
        assert_eq!(worlds[0].position.state, before);
        assert_eq!(worlds[1].position.state, before);
        assert!(b.outcomes >= b.transitions);
    }
}

#[test]
fn engine_rejects_known_state_changes_and_hiding_active_occupants() {
    let p = fixture("super-fang.json");
    let mut altered = p.clone();
    altered.state.sides[0].party[0].hp -= 1;
    let worlds = vec![ew("a", p.clone()), ew("b", altered)];
    assert!(engine::one_turn(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge::default(),
        100_000
    )
    .err()
    .unwrap()
    .0
    .contains("known state"));
    assert!(engine::one_turn(
        &[ew("a", p)],
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge {
            hidden_stats: false,
            unrevealed_reserves: vec![0]
        },
        100_000
    )
    .is_err());
}

#[test]
fn engine_accepts_declared_stat_variants_but_not_undeclared_ones() {
    let p = fixture("super-fang.json");
    let mut variant = p.clone();
    variant.state.sides[1].party[0].stats[4] += 1;
    variant.state.sides[1].party[0].stat_points[5] += 1;
    let worlds = vec![ew("a", p), ew("b", variant)];
    assert!(engine::one_turn(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge::default(),
        100_000
    )
    .is_err());
    assert!(engine::one_turn(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge {
            hidden_stats: true,
            ..Knowledge::default()
        },
        100_000
    )
    .is_ok());
}

#[test]
fn engine_cell_cap_and_intermediate_switch_fail_without_mutation() {
    let p = fixture("super-fang.json");
    let worlds = vec![ew("a", p)];
    let before = worlds[0].position.state.clone();
    assert!(engine::one_turn(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge::default(),
        1
    )
    .err()
    .unwrap()
    .0
    .contains("max_cells"));
    assert_eq!(worlds[0].position.state, before);
    let p = fixture("eject-button-uturn.json");
    let before = p.state.clone();
    let worlds = vec![ew("pivot", p)];
    let e = engine::one_turn(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge::default(),
        100_000,
    )
    .err()
    .unwrap();
    assert!(e.0.contains("intermediate switch"), "{e}");
    assert_eq!(worlds[0].position.state, before);
}

#[test]
fn opening_hidden_reserves_can_have_different_opponent_menus() {
    let mon = |species: &str| serde_json::json!({"species":species,"moves":["Harden"],"ability":"Honey Gather","nature":"Serious","level":50});
    let mut v = serde_json::json!({"format":"gen9championsdoublescustomgame","p1":{"team":[mon("Raticate"),mon("Blissey")],"order":"12"},
        "p2":{"team":[mon("Snorlax"),mon("Machamp"),mon("Pikachu")],"order":"123"},"turn":{"p1":"move harden, move harden","p2":"move harden, move harden"}});
    let load = |v: &serde_json::Value| {
        let l = lab_scenario::load_scenario_str(&v.to_string(), Path::new(".")).unwrap();
        let ps = lab_scenario::scenario_positions_with(&l, EnumerateOptions::default()).unwrap();
        assert_eq!(ps.len(), 1);
        Position {
            state: ps[0].state.clone(),
            suspension: None,
        }
    };
    let a = load(&v);
    v["p2"]["team"]
        .as_array_mut()
        .unwrap()
        .push(mon("Clefable"));
    v["p2"]["order"] = serde_json::json!("1234");
    let b = load(&v);
    let worlds = [ew("three", a), ew("four", b)];
    let result = engine::one_turn(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Knowledge {
            hidden_stats: false,
            unrevealed_reserves: vec![2, 3],
        },
        100_000,
    )
    .unwrap();
    assert_ne!(result.theirs[0].len(), result.theirs[1].len());
    assert_eq!(
        solve(&result.game, config()).unwrap().ours.len(),
        result.ours.len()
    );
}
