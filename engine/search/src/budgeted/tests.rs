use super::*;
use crate::nash::{self, Matrix};
use std::cell::Cell as Counter;

struct ToyNode {
    phase: Phase,
    heuristic: f32,
    rows: usize,
    cols: usize,
    children: Vec<Vec<(f64, usize)>>,
}
struct Toy {
    nodes: Vec<ToyNode>,
    calls: Counter<u64>,
}
impl Domain for Toy {
    type Position = usize;
    type Action = usize;
    fn phase(&self, p: &usize) -> Result<Phase, String> {
        Ok(self.nodes[*p].phase)
    }
    fn actions(&self, p: &usize, player: usize) -> Result<Vec<usize>, String> {
        Ok((0..if player == 0 {
            self.nodes[*p].rows
        } else {
            self.nodes[*p].cols
        })
            .collect())
    }
    fn value(&self, p: &usize) -> f32 {
        self.nodes[*p].heuristic
    }
    fn transitions(&self, p: &usize, a: [&usize; 2]) -> Result<Vec<(f64, usize)>, String> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.nodes[*p].children[*a[0] * self.nodes[*p].cols + *a[1]].clone())
    }
}
fn terminal(value: f32) -> ToyNode {
    ToyNode {
        phase: Phase::Terminal,
        heuristic: value,
        rows: 0,
        cols: 0,
        children: vec![],
    }
}

#[test]
fn observer_sees_only_committed_backups_and_can_stop() {
    let game = Toy {
        nodes: vec![node(Phase::Turn, 0., 1, 1, vec![vec![(1., 1)]]), terminal(7.)],
        calls: Counter::new(0),
    };
    let mut seen = Vec::new();
    let r = search_with_observer(&game, &Uniform, 0, cfg(20), |p, s| {
        seen.push((p.equilibrium.value, s.transitions, s.stored_nodes));
        true
    }).unwrap();
    assert_eq!(r.stop, Stop::Observer);
    assert_eq!(seen, vec![(7., 1, 2)]);
    assert_eq!(r.policy.unwrap().equilibrium.value, 7.);
}

#[test]
fn observer_does_not_publish_an_unfinished_initial_matrix() {
    let game = Toy {
        nodes: vec![node(Phase::Turn, 0., 1, 1, vec![vec![(1., 1)]]), terminal(7.)],
        calls: Counter::new(0),
    };
    let r = search_with_observer(&game, &Uniform, 0, cfg(0), |_, _| panic!("unfinished policy")).unwrap();
    assert!(r.policy.is_none());
    assert_eq!(r.stop, Stop::Budget);
}
fn node(
    phase: Phase,
    value: f32,
    rows: usize,
    cols: usize,
    children: Vec<Vec<(f64, usize)>>,
) -> ToyNode {
    ToyNode {
        phase,
        heuristic: value,
        rows,
        cols,
        children,
    }
}
fn cfg(budget: u64) -> Config {
    Config {
        budget,
        turn_cost: 1,
        switch_cost: 1,
        max_turns: 2,
        walks_before_scan: 4,
        exploration: 1.0,
        matrix_iterations: 20_000,
        matrix_tolerance: 0.001,
        ..Config::default()
    }
}
fn delayed() -> Toy {
    let mut nodes = vec![node(
        Phase::Turn,
        0.0,
        2,
        2,
        (1..=4).map(|i| vec![(1.0, i)]).collect(),
    )];
    for (i, v) in [4., 3., 1., 0.].into_iter().enumerate() {
        nodes.push(node(Phase::Turn, v, 1, 1, vec![vec![(1.0, i + 5)]]));
    }
    nodes.extend([-4., -3., 2., 1.].into_iter().map(terminal));
    Toy {
        nodes,
        calls: Counter::new(0),
    }
}
fn reference_gap(matrix: &Matrix, policy: &Policy<usize>) -> f32 {
    gap(matrix, &policy.equilibrium.rows, &policy.equilibrium.cols)
}

#[cfg(feature = "experiment-response-sweeps")]
#[test]
fn response_sweeps_find_initially_unlikely_counters_on_either_side() {
    // A pure shallow equilibrium hides a losing continuation in a low-probability
    // response. Transposition/sign reversal tests both players, with several menus.
    for n in [4, 8, 12] {
        for transpose in [false, true] {
            let (rows, cols) = if transpose { (n, 2) } else { (2, n) };
            let mut nodes = vec![node(Phase::Turn, 0., rows, cols, vec![])];
            let mut exact = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    let (a, b) = if transpose { (c, r) } else { (r, c) };
                    let sign = if transpose { -1. } else { 1. };
                    let shallow = if a == 0 { if b == 0 { 5. } else { 6. } } else { 0. };
                    let deep = if a == 0 && b == n - 1 { -10. } else { shallow };
                    let child = nodes.len();
                    nodes.push(node(Phase::Turn, sign * shallow, 1, 1, vec![vec![(1., child + 1)]]));
                    nodes.push(terminal(sign * deep));
                    nodes[0].children.push(vec![(1., child)]);
                    exact.push(sign * deep);
                }
            }
            let game = Toy { nodes, calls: Counter::new(0) };
            let mut c = cfg((3 * n + 2) as u64);
            c.matrix_tolerance = 0.001;
            let result = search(&game, &Uniform, 0, c).unwrap();
            let gap = reference_gap(&Matrix::new(rows, cols, exact), &result.policy.unwrap());
            assert!(gap < 0.02, "n={n} transpose={transpose} gap={gap}");
            assert!(result.stats.skipped_backups > 0);
            assert!(result.stats.transitions <= (3 * n + 2) as u64);
        }
    }
}

#[cfg(feature = "experiment-response-sweeps")]
#[test]
fn completion_releases_positions_even_when_payoffs_never_change() {
    // Three turns, chance, and a forced switch at the horizon. Every backup is
    // value-neutral, but all continuations must finish and all positions be freed.
    let game = Toy { nodes: vec![
        node(Phase::Turn, 3., 1, 1, vec![vec![(0.25, 1), (0.75, 1)]]),
        node(Phase::Turn, 3., 1, 1, vec![vec![(1., 2)]]),
        node(Phase::Turn, 3., 1, 1, vec![vec![(1., 3)]]),
        node(Phase::Switch, 3., 2, 1, vec![vec![(1., 4)], vec![(1., 4)]]),
        terminal(3.),
    ], calls: Counter::new(0) };
    let mut c = cfg(100); c.max_turns = 3;
    let result = search(&game, &Uniform, 0, c).unwrap();
    assert_eq!(result.stop, Stop::FrontierExhausted);
    assert_eq!(result.policy.unwrap().equilibrium.value, brute(&game, &0, 3));
    assert_eq!(result.stats.retained_positions, 0);
    assert_eq!(result.stats.transitions, 9);
    assert!(result.stats.peak_positions < result.stats.stored_nodes);
    assert!(result.stats.skipped_backups > 0);
}
fn gap(matrix: &Matrix, x: &[f32], y: &[f32]) -> f32 {
    let upper = (0..matrix.rows)
        .map(|r| {
            (0..matrix.cols)
                .map(|c| f64::from(matrix.at(r, c)) * f64::from(y[c]))
                .sum::<f64>()
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let lower = (0..matrix.cols)
        .map(|c| {
            (0..matrix.rows)
                .map(|r| f64::from(x[r]) * f64::from(matrix.at(r, c)))
                .sum::<f64>()
        })
        .fold(f64::INFINITY, f64::min);
    (upper - lower).max(0.) as f32
}

#[test]
fn budget_is_strict_and_initial_incomplete_has_no_policy() {
    for budget in 0..12 {
        let game = delayed();
        let mut c = cfg(budget);
        c.double_oracle = false;
        let r = search(&game, &Uniform, 0, c).unwrap();
        assert!(r.stats.cost_used <= budget);
        assert_eq!(r.stats.transitions, game.calls.get());
        assert_eq!(r.stats.cost_used, r.stats.transitions);
        assert_eq!(r.policy.is_some(), budget >= 4);
        if budget >= 8 {
            assert_eq!(r.stop, Stop::FrontierExhausted);
        }
    }
}

#[test]
fn deeper_search_corrects_misleading_leaf_and_matches_reference() {
    let game = delayed();
    let shallow = search(&game, &Uniform, 0, cfg(4)).unwrap();
    let full = search(&game, &Uniform, 0, cfg(100)).unwrap();
    let reference = Matrix::new(2, 2, vec![-4., -3., 2., 1.]);
    let p = shallow.policy.unwrap();
    assert!(p.equilibrium.exploitability < 0.01);
    assert!(
        reference_gap(&reference, &p) > 3.0,
        "local gap is not a quality certificate"
    );
    let p = full.policy.unwrap();
    assert!(reference_gap(&reference, &p) < 0.01);
    assert!((p.equilibrium.value - 1.0).abs() < 0.01);
    assert!(p.equilibrium.rows[1] > 0.999);
    assert_eq!(full.stop, Stop::FrontierExhausted);
    assert_eq!(full.stats.transitions, 8);
}

#[test]
fn interrupted_ancestor_backup_returns_previous_complete_root() {
    let mut nodes = vec![node(
        Phase::Turn,
        0.,
        3,
        3,
        (1..=9).map(|i| vec![(1., i)]).collect(),
    )];
    nodes.push(node(Phase::Turn, 0., 1, 1, vec![vec![(1., 10)]]));
    nodes.extend([1., 1., -1., 2., 2., -1., 2., 2.].into_iter().map(terminal));
    nodes.push(terminal(-10.));
    let game = Toy {
        nodes,
        calls: Counter::new(0),
    };
    let mut c = cfg(5);
    c.exploration = 0.;
    let old = search(&game, &Uniform, 0, c.clone()).unwrap();
    c.budget = 6;
    let partial = search(&game, &Uniform, 0, c).unwrap();
    let a = old.policy.unwrap();
    let b = partial.policy.unwrap();
    assert_eq!(a.equilibrium, b.equilibrium);
    assert_eq!(b.equilibrium.value, 0.);
    assert_eq!(partial.stats.cost_used, 6);
    assert_eq!(partial.stats.committed_updates, 0);
    assert_eq!(partial.stats.attempted_updates, 1);
    assert_eq!(partial.stop, Stop::Budget);
}

#[test]
fn chance_mass_and_rare_bad_outcome_are_not_dropped() {
    let game = Toy {
        nodes: vec![
            node(Phase::Turn, 0., 1, 1, vec![vec![(0.99, 1), (0.01, 2)]]),
            terminal(1.),
            terminal(-99.),
        ],
        calls: Counter::new(0),
    };
    let r = search(&game, &Uniform, 0, cfg(1)).unwrap();
    assert!(r.policy.unwrap().equilibrium.value.abs() < 1e-6);
    assert_eq!(r.stats.stored_nodes, 3);
}

#[test]
fn forced_switch_is_resolved_at_horizon_and_has_separate_cost() {
    let game = Toy {
        nodes: vec![
            node(Phase::Turn, 0., 1, 1, vec![vec![(1., 1)]]),
            node(
                Phase::Switch,
                999.,
                2,
                1,
                vec![vec![(1., 2)], vec![(1., 3)]],
            ),
            terminal(-2.),
            terminal(3.),
        ],
        calls: Counter::new(0),
    };
    let mut c = cfg(12);
    c.max_turns = 1;
    c.turn_cost = 10;
    let full = search(&game, &Uniform, 0, c.clone()).unwrap();
    assert!((full.policy.unwrap().equilibrium.value - 3.).abs() < 0.01);
    assert_eq!(full.stats.turn_transitions, 1);
    assert_eq!(full.stats.switch_transitions, 2);
    assert_eq!(full.stats.cost_used, 12);
    c.budget = 11;
    assert!(search(&game, &Uniform, 0, c).unwrap().policy.is_none());
}

#[test]
fn deterministic_seed_and_cached_transitions() {
    let game = delayed();
    let a = search(&game, &Uniform, 0, cfg(7)).unwrap();
    let b = search(&game, &Uniform, 0, cfg(7)).unwrap();
    assert_eq!(a.stats, b.stats);
    assert_eq!(a.policy.unwrap().equilibrium, b.policy.unwrap().equilibrium);
    for lazy in [false, true] {
        let mut c = cfg(100);
        c.double_oracle = lazy;
        let r = search(&game, &Uniform, 0, c).unwrap();
        assert_eq!(
            r.stats.transitions, 8,
            "backups reused each edge's transition"
        );
    }
}

#[test]
fn node_limit_including_forced_switch_siblings_is_strict() {
    let game = Toy {
        nodes: vec![
            node(Phase::Turn, 0., 1, 1, vec![vec![(0.5, 1), (0.5, 2)]]),
            node(Phase::Switch, 0., 1, 1, vec![vec![(1., 3)]]),
            terminal(2.),
            terminal(1.),
        ],
        calls: Counter::new(0),
    };
    for limit in 1..=4 {
        let mut c = cfg(100);
        c.max_nodes = limit;
        let r = search(&game, &Uniform, 0, c).unwrap();
        assert!(r.stats.stored_nodes <= limit);
        assert_eq!(r.policy.is_some(), limit == 4);
        assert_eq!(
            r.stop,
            if limit == 4 {
                Stop::FrontierExhausted
            } else {
                Stop::NodeLimit
            }
        );
    }
}

#[test]
fn invalid_values_mass_priors_and_configs_fail_closed() {
    let mut game = Toy {
        nodes: vec![
            node(Phase::Turn, 0., 1, 1, vec![vec![(0.8, 1)]]),
            terminal(1.),
        ],
        calls: Counter::new(0),
    };
    assert_eq!(
        search(&game, &Uniform, 0, cfg(10)).unwrap_err(),
        Error::InvalidProbabilities
    );
    game.nodes[0].children[0][0].0 = 1.;
    game.nodes[1].heuristic = f32::NAN;
    assert_eq!(
        search(&game, &Uniform, 0, cfg(10)).unwrap_err(),
        Error::InvalidValue
    );
    game.nodes[1].heuristic = 1.;
    struct Bad;
    impl Prior<Toy> for Bad {
        fn weights(&self, _: &usize, _: usize, _: &[usize]) -> Vec<f32> {
            vec![0.]
        }
    }
    assert_eq!(
        search(&game, &Bad, 0, cfg(10)).unwrap_err(),
        Error::InvalidPrior
    );
    let mut c = cfg(10);
    c.turn_cost = 0;
    assert!(matches!(
        search(&game, &Uniform, 0, c),
        Err(Error::InvalidConfig(_))
    ));
}

#[test]
fn cyclic_switch_backend_is_rejected() {
    let game = Toy {
        nodes: vec![node(Phase::Switch, 0., 1, 1, vec![vec![(1., 0)]])],
        calls: Counter::new(0),
    };
    assert_eq!(
        search(&game, &Uniform, 0, cfg(1000)).unwrap_err(),
        Error::SwitchChainLimit
    );
}

#[test]
fn terminal_root_needs_no_budget_or_policy() {
    let game = Toy {
        nodes: vec![terminal(12.)],
        calls: Counter::new(0),
    };
    let r = search(&game, &Uniform, 0, cfg(0)).unwrap();
    assert_eq!(r.terminal_value, Some(12.));
    assert!(r.policy.is_none());
    assert_eq!(r.stats.transitions, 0);
}

#[test]
fn exhausted_tree_does_not_claim_solver_tolerance_was_met() {
    let game = Toy {
        nodes: vec![
            node(
                Phase::Turn,
                0.,
                2,
                2,
                (1..=4).map(|i| vec![(1., i)]).collect(),
            ),
            terminal(4.),
            terminal(3.),
            terminal(1.),
            terminal(0.),
        ],
        calls: Counter::new(0),
    };
    let mut c = cfg(4);
    c.matrix_iterations = 1;
    c.double_oracle = false;
    let r = search(&game, &Uniform, 0, c).unwrap();
    assert_eq!(r.stop, Stop::FrontierExhausted);
    let p = r.policy.unwrap();
    assert!(!p.local_tolerance_met);
    assert!(p.equilibrium.exploitability > 0.001);
}

#[test]
fn backend_errors_are_not_silently_omitted_from_the_game() {
    struct Broken;
    impl Domain for Broken {
        type Position = usize;
        type Action = usize;
        fn phase(&self, _: &usize) -> Result<Phase, String> {
            Ok(Phase::Turn)
        }
        fn actions(&self, _: &usize, _: usize) -> Result<Vec<usize>, String> {
            Ok(vec![0])
        }
        fn value(&self, _: &usize) -> f32 {
            0.
        }
        fn transitions(&self, _: &usize, _: [&usize; 2]) -> Result<Vec<(f64, usize)>, String> {
            Err("unsupported future mechanic".into())
        }
    }
    assert_eq!(
        search(&Broken, &Uniform, 0, cfg(1)).unwrap_err(),
        Error::Domain("unsupported future mechanic".into())
    );
}

#[test]
fn matrix_double_oracle_matches_full_action_reference_and_saves_cells() {
    let n = 12;
    let values: Vec<f32> = (0..n)
        .flat_map(|r| (0..n).map(move |c| c as f32 - r as f32))
        .collect();
    let mut count = 0;
    let solution = matrix::solve(
        n,
        n,
        20_000,
        0.001,
        true,
        &mut matrix::Counts::default(),
        |i| {
            count += 1;
            Ok::<_, ()>(values[i])
        },
    )
    .unwrap();
    assert_eq!(count, 2 * n - 1);
    let certified = gap(
        &Matrix::new(n, n, values),
        &solution.equilibrium.rows,
        &solution.equilibrium.cols,
    );
    assert_eq!(certified, 0.);
    assert_eq!(solution.equilibrium.exploitability, 0.);
}

#[test]
fn simultaneous_mixed_policy_is_preserved() {
    let values = [1., -1., -1., 1.];
    let solution = matrix::solve(
        2,
        2,
        20_000,
        0.001,
        true,
        &mut matrix::Counts::default(),
        |i| Ok::<_, ()>(values[i]),
    )
    .unwrap();
    for p in solution
        .equilibrium
        .rows
        .iter()
        .chain(&solution.equilibrium.cols)
    {
        assert!((*p - 0.5).abs() < 0.001);
    }
    assert!(solution.equilibrium.exploitability < 0.001);
}

#[test]
fn quality_grid_uses_a_common_exhaustive_reference_not_its_own_gap() {
    let mut records = Vec::new();
    for case in 0..8u64 {
        let mut rng = Rng(case + 99);
        let mut nodes = vec![node(Phase::Turn, 0., 2, 2, vec![])];
        for _ in 0..8 {
            nodes.push(node(
                Phase::Turn,
                (rng.unit() * 10. - 5.) as f32,
                2,
                2,
                vec![],
            ));
        }
        for child in 1..=8 {
            for _ in 0..4 {
                let id = nodes.len();
                nodes.push(terminal((rng.unit() * 20. - 10.) as f32));
                nodes[child].children.push(vec![(1., id)]);
            }
        }
        nodes[0].children = (0..4)
            .map(|i| vec![(0.25, 1 + 2 * i), (0.75, 2 + 2 * i)])
            .collect();
        let game = Toy {
            nodes,
            calls: Counter::new(0),
        };
        let values: Vec<f32> = game.nodes[0]
            .children
            .iter()
            .map(|outcomes| {
                outcomes
                    .iter()
                    .map(|(p, c)| p * f64::from(brute(&game, c, 1)))
                    .sum::<f64>() as f32
            })
            .collect();
        let reference = Matrix::new(2, 2, values);
        let exact = nash::solve(&reference, 20_000, 0.0001);
        for budget in [4, 8, 16, 24, 36] {
            for seed in [1, 7, 19] {
                let mut c = cfg(budget);
                c.seed = seed;
                let report = search(&game, &Uniform, 0, c).unwrap();
                let p = report.policy.unwrap();
                let reference_gap = reference_gap(&reference, &p);
                if budget == 36 {
                    assert_eq!(report.stop, Stop::FrontierExhausted);
                    assert!(reference_gap < 0.02, "case={case} gap={reference_gap}");
                    assert!((p.equilibrium.value - exact.value).abs() < 0.02);
                }
                records.push(serde_json::json!({"case":case,"budget":budget,"seed":seed,
                "transitions":report.stats.transitions,"local_leaf_matrix_gap":p.equilibrium.exploitability,
                "reference_root_br_gap":reference_gap,"estimated_value":p.equilibrium.value,
                "reference_value":exact.value,"reference_solver_gap":exact.exploitability,
                "reference_value_error":(p.equilibrium.value-exact.value).abs(),
                "committed_updates":report.stats.committed_updates,"stop":format!("{:?}",report.stop)}));
            }
        }
    }
    assert_eq!(records.len(), 120);
    if let Ok(path) = std::env::var("LAB_BUDGET_EVIDENCE") {
        let evidence = serde_json::json!({"schema":1,"scope":"synthetic perfect-information depth-2 games; NOT Pokemon strength or timing",
            "cases":8,"records":records,"reference":"independent all-action recursion, same horizon/evaluator; RM+ tolerance recorded"});
        std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    }
}

/// Independent, all-action recursion. No selection, cached cells, DO, or budget logic.
fn brute<D: Domain>(game: &D, p: &D::Position, remaining: u32) -> f32 {
    let phase = game.phase(p).unwrap();
    if phase == Phase::Terminal || (phase == Phase::Turn && remaining == 0) {
        return game.value(p);
    }
    let rows = game.actions(p, 0).unwrap();
    let cols = game.actions(p, 1).unwrap();
    assert!(
        !rows.is_empty() && !cols.is_empty(),
        "reference menu empty: phase={phase:?} remaining={remaining} rows={} cols={}",
        rows.len(),
        cols.len()
    );
    let next = remaining - u32::from(phase == Phase::Turn);
    let mut values = Vec::new();
    for a in &rows {
        for b in &cols {
            values.push(
                game.transitions(p, [a, b])
                    .unwrap()
                    .into_iter()
                    .map(|(q, s)| q * f64::from(brute(game, &s, next)))
                    .sum::<f64>() as f32,
            );
        }
    }
    nash::solve(&Matrix::new(rows.len(), cols.len(), values), 20_000, 0.0001).value
}

#[test]
fn engine_full_distribution_and_input_restoration() {
    use lab_engine::{
        eval::Heuristic,
        rules::Ruleset,
        state::SideId,
        turn::{EnumerateOptions, RollMode},
    };
    use lab_scenario::{load_scenario_file, scenario_positions_with};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios/eject-button-uturn.json");
    let loaded = load_scenario_file(path).unwrap();
    let options = EnumerateOptions {
        rolls: RollMode::Full,
    };
    let initial = scenario_positions_with(&loaded, options).unwrap().remove(0);
    let position = Position {
        state: initial.state.clone(),
        suspension: None,
    };
    let domain = EngineDomain {
        ruleset: Ruleset::CHAMPIONS_MC,
        options,
        pruning: crate::Pruning::Sensible,
        us: SideId::One,
        evaluator: &Heuristic,
    };
    let mut c = cfg(1000);
    c.max_turns = 1;
    let expected = brute(&domain, &position, 1);
    let r = search(&domain, &Uniform, position.clone(), c).unwrap();
    assert_eq!(r.stop, Stop::FrontierExhausted);
    assert!((r.policy.unwrap().equilibrium.value - expected).abs() < 0.02);
    assert_eq!(position.state, initial.state);
    assert!(
        r.stats.stored_nodes > r.stats.transitions as usize + 1,
        "actual stochastic outcomes"
    );
}

#[test]
fn engine_pivot_and_replacement_match_independent_recursion() {
    use lab_engine::{
        eval::Heuristic,
        rules::Ruleset,
        state::SideId,
        turn::{EnumerateOptions, RollMode},
    };
    use lab_scenario::{load_scenario_file, scenario_positions_with};
    for name in ["eject-button-uturn", "ko-replace"] {
        eprintln!("independent engine reference: {name}");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../oracle/scenarios/{name}.json"));
        let loaded = load_scenario_file(path).unwrap();
        let options = EnumerateOptions {
            rolls: RollMode::Median,
        };
        let mut initial = scenario_positions_with(&loaded, options).unwrap().remove(0);
        if name == "ko-replace" {
            // Explicitly derived narrow fixture: preserve the KO/replacement/entry state,
            // replace the next turn's moves by Harden so this is a bounded logic check.
            for side in &mut initial.state.sides {
                for mon in &mut side.party {
                    if mon.species.is_none() {
                        continue;
                    }
                    mon.moves = [lab_engine::state::MoveSlot::default(); 4];
                    mon.moves[0] =
                        lab_engine::state::MoveSlot::full(lab_engine::dex::moves::HARDEN);
                }
            }
        }
        let position = Position {
            state: initial.state.clone(),
            suspension: None,
        };
        let domain = EngineDomain {
            ruleset: Ruleset::CHAMPIONS_MC,
            options,
            pruning: crate::Pruning::Sensible,
            us: SideId::One,
            evaluator: &Heuristic,
        };
        let mut c = cfg(10_000);
        c.max_turns = 1;
        c.max_nodes = 20_000;
        let expected = brute(&domain, &position, 1);
        let r = search(&domain, &Uniform, position.clone(), c).unwrap();
        assert_eq!(r.stop, Stop::FrontierExhausted, "{name}");
        assert!(
            (r.policy.unwrap().equilibrium.value - expected).abs() < 0.02,
            "{name}"
        );
        assert!(r.stats.switch_transitions > 0, "{name}");
        assert_eq!(position.state, initial.state);
    }
}
