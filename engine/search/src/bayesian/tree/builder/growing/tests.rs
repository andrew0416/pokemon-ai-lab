use super::*;
use crate::budgeted::Domain;
use std::cell::Cell;

#[derive(Clone)]
struct Position {
    world: usize,
    stage: usize,
    own: usize,
    other: usize,
    signal: usize,
}
struct Game {
    calls: Cell<usize>,
    switch: bool,
    bad_mass: bool,
}
impl Game {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
            switch: false,
            bad_mass: false,
        }
    }
}
impl Domain for Game {
    type Position = Position;
    type Action = usize;
    fn phase(&self, p: &Position) -> Result<Phase, String> {
        Ok(if p.stage == 2 {
            Phase::Terminal
        } else if self.switch && p.stage == 1 {
            Phase::Switch
        } else {
            Phase::Turn
        })
    }
    fn actions(&self, p: &Position, player: usize) -> Result<Vec<usize>, String> {
        Ok(if p.stage == 1 && player == 1 {
            vec![0]
        } else {
            vec![0, 1]
        })
    }
    fn value(&self, p: &Position) -> f32 {
        if p.stage < 2 {
            0.
        } else {
            (if p.own == p.world { 1. } else { -1. }) + if p.other == p.world { 0.2 } else { -0.2 }
        }
    }
    fn transitions(&self, p: &Position, a: [&usize; 2]) -> Result<Vec<(f64, Position)>, String> {
        self.calls.set(self.calls.get() + 1);
        if self.bad_mass {
            return Ok(vec![(0.9, p.clone())]);
        }
        if p.stage == 0 {
            Ok((0..2)
                .map(|signal| {
                    (
                        0.5,
                        Position {
                            stage: 1,
                            own: *a[0],
                            other: *a[1],
                            signal,
                            ..p.clone()
                        },
                    )
                })
                .collect())
        } else {
            Ok(vec![(
                1.,
                Position {
                    stage: 2,
                    own: *a[0],
                    ..p.clone()
                },
            )])
        }
    }
}
impl ObservedDomain for Game {
    fn observation(&self, p: &Position) -> Result<Observation, String> {
        Ok(Observation {
            public: format!("stage{}-signal{}", p.stage, p.signal),
            private: [String::new(), String::new()],
        })
    }
    fn action_id(&self, _: &Position, _: usize, a: &usize) -> String {
        format!("a{a}")
    }
}
fn seeds() -> Vec<Seed<Position>> {
    (0..2)
        .map(|world| Seed {
            id: format!("w{world}"),
            weight: 1.,
            position: Position {
                world,
                stage: 0,
                own: 0,
                other: 0,
                signal: 0,
            },
        })
        .collect()
}
fn config(expansions: usize) -> Config {
    Config {
        max_expansions: expansions,
        max_walks: 1000,
        solver: CfrConfig {
            iterations: 2048,
            tolerance: 0.01,
            check_every: 32,
        },
        ..Config::default()
    }
}

#[test]
fn full_growth_matches_exhaustive_value_without_repeating_transitions() {
    let game = Game::new();
    let seeds = seeds();
    let limits = Limits::default();
    let result = search(&game, &seeds, limits, config(100), &Uniform).unwrap();
    assert!(result.horizon_complete);
    assert_eq!(result.stop, Stop::HorizonComplete);
    assert_eq!(result.work.committed_expansions, 3);
    assert_eq!(result.work.attempted_transitions, game.calls.get());
    let calls = game.calls.get();
    let exhaustive = super::super::build(&game, &seeds, limits).unwrap();
    assert_eq!(calls, exhaustive.stats.transitions);
    assert_eq!(result.built.tree.node_count(), exhaustive.tree.node_count());
    let oracle = tree::solve(&exhaustive.tree, config(100).solver).unwrap();
    assert!((result.solution.assessment.value - oracle.assessment.value).abs() < 0.02);
    assert_eq!(seeds[0].position.stage, 0);
}

#[test]
fn root_only_is_a_surrogate_not_a_complete_horizon_certificate() {
    let game = Game::new();
    let r = search(&game, &seeds(), Limits::default(), config(1), &Uniform).unwrap();
    assert_eq!(r.stop, Stop::ExpansionLimit);
    assert!(!r.horizon_complete);
    assert_eq!(r.frontier_public_groups, 2);
    assert_eq!(r.frontier_histories, 16);
    assert_eq!(r.work.attempted_transitions, 8);
    assert!(r.solution.converged);
    let root: Vec<_> = r
        .built
        .tree
        .boundaries
        .iter()
        .filter(|b| b.public == 0)
        .map(|b| r.built.tree.information_at(b.node))
        .collect();
    assert_eq!(root[0], root[1]);
}

#[test]
fn public_admission_includes_all_worlds_and_unreached_action_histories() {
    let r = search(
        &Game::new(),
        &seeds(),
        Limits::default(),
        config(2),
        &Uniform,
    )
    .unwrap();
    assert_eq!(r.work.committed_expansions, 2);
    assert_eq!(r.frontier_public_groups, 1);
    assert_eq!(r.frontier_histories, 8);
    assert_eq!(r.work.attempted_transitions, 24);
    // Each second-stage own action memory groups both worlds and both hidden columns.
    let infos: Vec<_> = r
        .built
        .tree
        .information()
        .iter()
        .filter(|i| i.player == 0 && !i.own_sequence.is_empty())
        .collect();
    assert_eq!(infos.len(), 2);
    assert!(infos.iter().all(|i| i.nodes.len() == 4));
}

#[test]
fn interrupted_admission_preserves_last_policy_and_charges_attempts() {
    let base = search(
        &Game::new(),
        &seeds(),
        Limits::default(),
        config(1),
        &Uniform,
    )
    .unwrap();
    let g = Game::new();
    let limits = Limits {
        max_transitions: 9,
        ..Limits::default()
    };
    let r = search(&g, &seeds(), limits, config(100), &Uniform).unwrap();
    assert_eq!(r.stop, Stop::TransitionLimit);
    assert_eq!(r.work.attempted_transitions, 9);
    assert_eq!(g.calls.get(), 9);
    assert_eq!(r.built.stats.transitions, 8);
    assert_eq!(r.work.committed_expansions, 1);
    assert_eq!(r.work.solves, 1);
    assert_eq!(r.solution.policy, base.solution.policy);
    assert_eq!(r.built.tree.node_count(), base.built.tree.node_count());
    assert_eq!(r.frontier_public_groups, base.frontier_public_groups);
}

#[test]
fn incomplete_root_never_returns_a_strategy() {
    assert!(search(
        &Game::new(),
        &seeds(),
        Limits {
            max_transitions: 7,
            ..Limits::default()
        },
        config(100),
        &Uniform
    )
    .unwrap_err_text()
    .contains("root incomplete"));
}
trait ErrText {
    fn unwrap_err_text(self) -> String;
}
impl ErrText for Result<ResultTree, Error> {
    fn unwrap_err_text(self) -> String {
        match self {
            Err(e) => e.0,
            Ok(_) => panic!("expected error"),
        }
    }
}

#[test]
fn forced_switches_finish_at_horizon_or_whole_admission_is_rejected() {
    let g = Game {
        switch: true,
        ..Game::new()
    };
    let limits = Limits {
        turns: 1,
        ..Limits::default()
    };
    let r = search(&g, &seeds(), limits, config(100), &Uniform).unwrap();
    assert!(r.horizon_complete);
    assert_eq!(r.built.stats.switch_decisions, 16);
    assert!(search(&g, &seeds(), limits, config(1), &Uniform).is_err());
}

#[test]
fn bad_probability_and_observation_menu_contracts_fail_closed() {
    let g = Game {
        bad_mass: true,
        ..Game::new()
    };
    assert!(search(&g, &seeds(), Limits::default(), config(100), &Uniform).is_err());
    let mut s = seeds();
    s[1].position.signal = 1;
    assert!(search(&Game::new(), &s, Limits::default(), config(100), &Uniform).is_err());
}

#[test]
fn deterministic_seed_and_small_walk_budget_are_reported() {
    let cfg = Config {
        max_walks: 1,
        ..config(100)
    };
    let a = search(&Game::new(), &seeds(), Limits::default(), cfg, &Uniform).unwrap();
    let b = search(&Game::new(), &seeds(), Limits::default(), cfg, &Uniform).unwrap();
    assert_eq!(a.stop, Stop::WalkLimit);
    assert_eq!(a.solution.policy, b.solution.policy);
    assert_eq!(a.work.attempted_transitions, b.work.attempted_transitions);
}

#[test]
fn selection_q_is_shared_across_hidden_worlds() {
    let t = Tree::new(
        vec![
            Node::Chance(vec![(0.5, 1), (0.5, 2)]),
            Node::Decision {
                player: 0,
                information: "same".into(),
                actions: vec!["a".into(), "b".into()],
                children: vec![3, 4],
            },
            Node::Decision {
                player: 0,
                information: "same".into(),
                actions: vec!["a".into(), "b".into()],
                children: vec![5, 6],
            },
            Node::Terminal(1.),
            Node::Terminal(-1.),
            Node::Terminal(-1.),
            Node::Terminal(1.),
        ],
        0,
    )
    .unwrap();
    assert_eq!(scores(&t, &t.uniform()).unwrap(), vec![vec![0., 0.]]);
}

#[test]
fn zero_prior_world_is_retained_without_fake_belief() {
    let mut s = seeds();
    s[1].weight = 0.;
    let r = search(&Game::new(), &s, Limits::default(), config(1), &Uniform).unwrap();
    assert_eq!(r.work.attempted_transitions, 8);
    assert_eq!(
        r.built.tree.public_beliefs(&r.solution.policy).unwrap()[0].posterior,
        Some(vec![1., 0.])
    );
}

#[test]
fn node_and_decision_caps_keep_the_completed_root() {
    let base = search(
        &Game::new(),
        &seeds(),
        Limits::default(),
        config(1),
        &Uniform,
    )
    .unwrap();
    for (limits, stop) in [
        (
            Limits {
                max_nodes: base.built.tree.node_count() + 2,
                ..Limits::default()
            },
            Stop::NodeLimit,
        ),
        (
            Limits {
                max_decisions: 1,
                ..Limits::default()
            },
            Stop::DecisionLimit,
        ),
    ] {
        let r = search(&Game::new(), &seeds(), limits, config(100), &Uniform).unwrap();
        assert_eq!(r.stop, stop);
        assert_eq!(r.solution.policy, base.solution.policy);
        assert_eq!(r.built.tree.node_count(), base.built.tree.node_count());
        assert_eq!(r.work.solves, 1);
    }
}

#[test]
fn invalid_solver_fails_before_engine_work() {
    let game = Game::new();
    let mut c = config(100);
    c.solver.iterations = 0;
    assert!(search(&game, &seeds(), Limits::default(), c, &Uniform).is_err());
    assert_eq!(game.calls.get(), 0);
}

#[cfg(feature = "experiment-belief-workspace")]
fn exact_result(r: Result<ResultTree, Error>) -> String {
    match r {
        Err(e) => format!("error: {e:?}"),
        Ok(r) => format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}",
            r.built.tree,
            r.built.stats,
            r.solution,
            r.stop,
            r.work,
            r.frontier_histories,
            r.frontier_public_groups,
            r.horizon_complete
        ),
    }
}

#[cfg(feature = "experiment-belief-workspace")]
#[test]
fn allocation_candidates_match_all_admission_failure_boundaries() {
    let mut trials = Vec::new();
    for cap in 0..=42 {
        trials.push((
            Limits {
                max_transitions: cap,
                ..Limits::default()
            },
            config(100),
        ));
    }
    for cap in (0..=180).step_by(3) {
        trials.push((
            Limits {
                max_nodes: cap,
                ..Limits::default()
            },
            config(100),
        ));
    }
    for cap in 0..=3 {
        trials.push((
            Limits {
                max_decisions: cap,
                ..Limits::default()
            },
            config(100),
        ));
    }
    for cap in 0..=4 {
        trials.push((Limits::default(), config(cap)));
    }
    for cap in 0..=4 {
        trials.push((
            Limits::default(),
            Config {
                max_walks: cap,
                ..config(100)
            },
        ));
    }
    for switch in [false, true] {
        for (limits, cfg) in &trials {
            let g = Game {
                switch,
                ..Game::new()
            };
            let expected = exact_result(search(&g, &seeds(), *limits, *cfg, &Uniform));
            let expected_calls = g.calls.get();
            for options in [
                reuse::Options {
                    in_place: false,
                    workspace: true,
                    compiler: false,
                    static_values: true,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: true,
                    workspace: true,
                    compiler: true,
                    static_values: true,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: false,
                    workspace: false,
                    compiler: true,
                    static_values: false,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: true,
                    workspace: false,
                    compiler: true,
                    static_values: false,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: true,
                    workspace: true,
                    compiler: true,
                    static_values: false,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: true,
                    workspace: false,
                    compiler: false,
                    static_values: false,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: false,
                    workspace: true,
                    compiler: false,
                    static_values: false,
                    direct_write: false,
                },
                reuse::Options {
                    in_place: true,
                    workspace: true,
                    compiler: false,
                    static_values: false,
                    direct_write: false,
                },
            ] {
                let g = Game {
                    switch,
                    ..Game::new()
                };
                let actual = exact_result(reuse::search(
                    &g,
                    &seeds(),
                    *limits,
                    *cfg,
                    &Uniform,
                    options,
                ));
                #[cfg(feature = "experiment-shared-final-passes")]
                {
                    let g = Game {
                        switch,
                        ..Game::new()
                    };
                    let shared = exact_result(reuse::search_shared(
                        &g,
                        &seeds(),
                        *limits,
                        *cfg,
                        &Uniform,
                        options,
                    ));
                    assert_eq!(actual, shared, "shared final passes");
                    assert_eq!(g.calls.get(), expected_calls);
                }
                #[cfg(feature = "experiment-incremental-compilation")]
                {
                    let g = Game {
                        switch,
                        ..Game::new()
                    };
                    let shared = exact_result(reuse::search_incremental(
                        &g,
                        &seeds(),
                        *limits,
                        *cfg,
                        &Uniform,
                        options,
                    ));
                    assert_eq!(actual, shared, "shared final passes");
                    assert_eq!(g.calls.get(), expected_calls);
                }
                #[cfg(feature = "experiment-growth-cadence")]
                {
                    let g = Game {
                        switch,
                        ..Game::new()
                    };
                    let shared = exact_result(reuse::cadence::search(
                        &g,
                        &seeds(),
                        *limits,
                        *cfg,
                        &Uniform,
                        options,
                        1,
                    ));
                    assert_eq!(actual, shared, "shared final passes");
                    assert_eq!(g.calls.get(), expected_calls);
                }
                assert_eq!(
                    actual, expected,
                    "{limits:?}, {cfg:?}, {options:?}, switch={switch}"
                );
                assert_eq!(g.calls.get(), expected_calls);
            }
        }
    }
}

#[cfg(feature = "experiment-belief-workspace")]
#[test]
fn allocation_candidates_keep_zero_mass_and_malformed_domain_behavior() {
    let mut s = seeds();
    s[1].weight = 0.;
    for bad_mass in [false, true] {
        for seed in [1, 7, 31, u64::MAX] {
            let cfg = Config {
                seed,
                ..config(100)
            };
            let a = Game {
                bad_mass,
                ..Game::new()
            };
            let b = Game {
                bad_mass,
                ..Game::new()
            };
            assert_eq!(
                exact_result(search(&a, &s, Limits::default(), cfg, &Uniform)),
                exact_result(reuse::search(
                    &b,
                    &s,
                    Limits::default(),
                    cfg,
                    &Uniform,
                    reuse::Options {
                        in_place: true,
                        workspace: true,
                        compiler: true,
                        static_values: true,
                        direct_write: false,
                    }
                ))
            );
            assert_eq!(a.calls.get(), b.calls.get());
        }
    }
    assert_eq!(s[0].position.stage, 0);
}

#[cfg(feature = "experiment-parallel-transitions")]
struct TestBatch;
#[cfg(feature = "experiment-parallel-transitions")]
impl Batch<Game> for TestBatch {
    fn width(&self) -> usize {
        4
    }
    fn run(&self, d: &Game, p: &Position, a: &[[&usize; 2]]) -> Vec<Outcomes<Position>> {
        a.iter().map(|a| d.transitions(p, *a)).collect()
    }
}
#[cfg(feature = "experiment-parallel-transitions")]
#[test]
fn batched_attempts_are_real_bounded_and_failed_admissions_keep_solved_snapshot() {
    let options = reuse::Options {
        in_place: true,
        workspace: true,
        compiler: true,
        static_values: true,
        direct_write: true,
    };
    for switch in [false, true] {
        for cap in 1..50 {
            let g = Game {
                switch,
                ..Game::new()
            };
            let limits = Limits {
                max_transitions: cap,
                ..Limits::default()
            };
            let expected = search(
                &Game {
                    switch,
                    ..Game::new()
                },
                &seeds(),
                limits,
                config(100),
                &Uniform,
            );
            let actual = reuse::search_batched(
                &g,
                &seeds(),
                limits,
                config(100),
                &Uniform,
                options,
                &TestBatch,
            );
            assert!(g.calls.get() <= cap);
            match (expected, actual) {
                (Ok(a), Ok(mut b)) => {
                    assert_eq!(b.work.attempted_transitions, g.calls.get());
                    assert!(b.work.attempted_transitions >= a.work.attempted_transitions);
                    b.work.attempted_transitions = a.work.attempted_transitions;
                    assert_eq!(exact_result(Ok(a)), exact_result(Ok(b)));
                }
                (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
                _ => panic!("batch budget changed publication boundary"),
            }
        }
    }
}
#[cfg(feature = "experiment-parallel-transitions")]
#[test]
fn batched_node_caps_and_bad_mass_remain_fail_closed() {
    let options = reuse::Options {
        in_place: true,
        workspace: true,
        compiler: true,
        static_values: true,
        direct_write: true,
    };
    for cap in 1..140 {
        let limits = Limits {
            max_nodes: cap,
            ..Limits::default()
        };
        let g = Game::new();
        let expected = search(&Game::new(), &seeds(), limits, config(100), &Uniform);
        let actual = reuse::search_batched(
            &g,
            &seeds(),
            limits,
            config(100),
            &Uniform,
            options,
            &TestBatch,
        );
        match (expected, actual) {
            (Ok(a), Ok(mut b)) => {
                assert_eq!(b.work.attempted_transitions, g.calls.get());
                b.work.attempted_transitions = a.work.attempted_transitions;
                assert_eq!(exact_result(Ok(a)), exact_result(Ok(b)));
            }
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            _ => panic!("batch node budget changed snapshot"),
        }
    }
    let g = Game {
        bad_mass: true,
        ..Game::new()
    };
    let result = reuse::search_batched(
        &g,
        &seeds(),
        Limits::default(),
        config(100),
        &Uniform,
        options,
        &TestBatch,
    );
    assert!(result.err().unwrap().to_string().contains("chance mass"));
    assert_eq!(g.calls.get(), 4);
}

#[cfg(feature = "experiment-growth-cadence")]
#[test]
fn cadence_complete_game_and_policies_match_with_fewer_solves() {
    let options = reuse::Options {
        in_place: true,
        workspace: true,
        compiler: true,
        static_values: true,
        direct_write: true,
    };
    for seed in [1, 7, 31, u64::MAX] {
        let cfg = Config {
            seed,
            ..config(100)
        };
        let reference = reuse::cadence::search(
            &Game::new(),
            &seeds(),
            Limits::default(),
            cfg,
            &Uniform,
            options,
            1,
        )
        .unwrap();
        let policy = |r: &ResultTree| {
            r.built
                .tree
                .information()
                .iter()
                .enumerate()
                .map(|(i, n)| (n.key.clone(), r.solution.policy[i].clone()))
                .collect::<BTreeMap<_, _>>()
        };
        for every in [2, 4] {
            let game = Game::new();
            let actual = reuse::cadence::search(
                &game,
                &seeds(),
                Limits::default(),
                cfg,
                &Uniform,
                options,
                every,
            )
            .unwrap();
            assert!(actual.horizon_complete);
            assert_eq!(actual.work.attempted_transitions, game.calls.get());
            assert_eq!(
                actual.work.attempted_transitions,
                reference.work.attempted_transitions
            );
            assert_eq!(policy(&reference), policy(&actual));
            assert_eq!(
                format!("{:?}", reference.solution.assessment),
                format!("{:?}", actual.solution.assessment)
            );
            assert_eq!(actual.work.solves, 2);
            assert_eq!(reference.work.solves, 3);
        }
    }
}
#[cfg(feature = "experiment-growth-cadence")]
#[test]
fn cadence_failure_after_a_whole_unsolved_group_returns_previous_solved_root() {
    let options = reuse::Options {
        in_place: true,
        workspace: true,
        compiler: true,
        static_values: true,
        direct_write: true,
    };
    let base = reuse::cadence::search(
        &Game::new(),
        &seeds(),
        Limits::default(),
        config(1),
        &Uniform,
        options,
        4,
    )
    .unwrap();
    let one = reuse::cadence::search(
        &Game::new(),
        &seeds(),
        Limits::default(),
        config(2),
        &Uniform,
        options,
        1,
    )
    .unwrap();
    let cap = one.work.attempted_transitions + 1;
    let game = Game::new();
    let actual = reuse::cadence::search(
        &game,
        &seeds(),
        Limits {
            max_transitions: cap,
            ..Limits::default()
        },
        config(100),
        &Uniform,
        options,
        4,
    )
    .unwrap();
    assert_eq!(actual.stop, Stop::TransitionLimit);
    assert_eq!(actual.work.attempted_transitions, cap);
    assert_eq!(game.calls.get(), cap);
    assert_eq!(actual.work.solves, 1);
    assert_eq!(actual.work.committed_expansions, 1);
    assert!(!actual.horizon_complete);
    assert_eq!(
        format!("{:?}", actual.built.tree),
        format!("{:?}", base.built.tree)
    );
    assert_eq!(
        format!("{:?}", actual.solution),
        format!("{:?}", base.solution)
    );
}
#[cfg(feature = "experiment-growth-cadence")]
#[test]
fn invalid_cadence_never_touches_domain() {
    for n in [0, 3, usize::MAX] {
        let game = Game::new();
        assert!(reuse::cadence::search(
            &game,
            &seeds(),
            Limits::default(),
            config(100),
            &Uniform,
            Default::default(),
            n
        )
        .is_err());
        assert_eq!(game.calls.get(), 0);
    }
}

#[cfg(feature = "experiment-growth-pipeline")]
#[test]
fn pipeline_frontier_exact_parity_across_atomic_budgets() {
    for cadence in [1, 2, 4] {
        for seed in [0, 1, 999] {
            for owned_compiler in [false, true] {
                for budget in [7, 8, 9, 23, 24, 25, 40, 1000] {
                    for switch in [false, true] {
                        let a = Game {
                            switch,
                            ..Game::new()
                        };
                        let b = Game {
                            switch,
                            ..Game::new()
                        };
                        let limits = Limits {
                            max_transitions: budget,
                            ..Limits::default()
                        };
                        let cfg = Config {
                            seed,
                            ..config(100)
                        };
                        let options = reuse::Options {
                            in_place: true,
                            compiler: true,
                            workspace: true,
                            static_values: true,
                            direct_write: true,
                        };
                        let settings = tree::paper::Settings {
                            sequence: true,
                            ..Default::default()
                        };
                        let ra = reuse::paper::search(
                            &a,
                            &seeds(),
                            limits,
                            cfg,
                            &Uniform,
                            options,
                            cadence,
                            settings,
                        );
                        let rb = reuse::pipeline::search(
                            &b,
                            &seeds(),
                            limits,
                            cfg,
                            &Uniform,
                            options,
                            cadence,
                            settings,
                            tree::pipeline::Settings {
                                frontier_index: true,
                                owned_compiler,
                                ..Default::default()
                            },
                        );
                        assert_eq!(a.calls.get(), b.calls.get());
                        match (ra, rb) {
                            (Ok(a), Ok(b)) => {
                                assert_eq!(exact_result(Ok(a.search)), exact_result(Ok(b.search)));
                                assert_eq!(format!("{:?}", a.stats), format!("{:?}", b.stats));
                            }
                            (Err(a), Err(b)) => assert_eq!(a.0, b.0),
                            _ => panic!("pipeline changed success/error"),
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "experiment-growth-pipeline")]
#[test]
fn pipeline_delta_game_and_certificates_survive_growth_and_budget_abort() {
    for cadence in [1, 4] {
        for expansions in [1, 2, 100] {
            for cap in [9, 25, 1000] {
                let game = Game::new();
                let limits = Limits {
                    max_transitions: cap,
                    ..Limits::default()
                };
                let options = reuse::Options {
                    in_place: true,
                    compiler: true,
                    workspace: true,
                    static_values: true,
                    direct_write: true,
                };
                let settings = tree::paper::Settings {
                    sequence: true,
                    ..Default::default()
                };
                let reference = reuse::paper::search(
                    &Game::new(),
                    &seeds(),
                    limits,
                    config(expansions),
                    &Uniform,
                    options,
                    cadence,
                    settings,
                )
                .unwrap();
                let actual = reuse::pipeline::search(
                    &game,
                    &seeds(),
                    limits,
                    config(expansions),
                    &Uniform,
                    options,
                    cadence,
                    settings,
                    tree::pipeline::Settings {
                        frontier_index: true,
                        owned_compiler: true,
                        incremental_sequence: true,
                        ..Default::default()
                    },
                )
                .unwrap();
                let a = &actual.search;
                assert_eq!(a.work.attempted_transitions, game.calls.get());
                assert_eq!(a.stop, reference.search.stop);
                assert_eq!(
                    format!("{:?}", a.built.tree),
                    format!("{:?}", reference.search.built.tree)
                );
                assert_eq!(
                    format!("{:?}", a.built.tree.assess(&a.solution.policy).unwrap()),
                    format!("{:?}", a.solution.assessment)
                );
                assert!(
                    (a.solution.assessment.value - reference.search.solution.assessment.value)
                        .abs()
                        < 0.02
                );
                assert_eq!(actual.metrics.sequence_full_builds, 1);
                assert_eq!(actual.metrics.sequence_delta_updates, a.work.solves - 1);
                assert_eq!(actual.metrics.sequence_fallbacks, 0);
            }
        }
    }
}
