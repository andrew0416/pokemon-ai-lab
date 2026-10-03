use lab_engine::{
    action::SlotAction,
    eval::Heuristic,
    field::{Effect, FieldEffect},
    rules::Ruleset,
    state::{BattleResult, SideId},
    turn::{final_state_observer as observer, EnumerateOptions, FactoredScope, RollMode},
};
use lab_search::{
    bayesian::{self, engine::EngineWorld, tree},
    budgeted::{Domain, EngineDomain, Phase, Position},
    Choice, Pruning,
};

fn fixture(name: &str, rolls: RollMode) -> (Position<2>, [Choice<2>; 2]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios")
        .join(format!("{name}.json"));
    let loaded = lab_scenario::load_scenario_file(path).unwrap();
    let positions =
        lab_scenario::scenario_positions_with(&loaded, EnumerateOptions { rolls }).unwrap();
    // The setup can also miss Hyper Beam, which makes the recorded recharge choice illegal.
    // Select the successful-hit setup branch for this explicit recharge coverage case.
    let p = positions
        .into_iter()
        .find(|p| {
            name != "hyper-beam-recharge"
                || p.state.sides[0].slots[0]
                    .volatiles
                    .get(lab_engine::volatile::Volatile::MustRecharge)
                    .active
        })
        .expect("fixture must provide the intended setup branch");
    let lab_scenario::Decision::Turn(pair) = lab_scenario::scenario_decision(&loaded, &p).unwrap()
    else {
        panic!("normal turn fixture required: {name}")
    };
    (
        Position {
            state: p.state,
            suspension: None,
        },
        pair.map(Choice::Turn),
    )
}

fn domain(us: SideId, rolls: RollMode) -> EngineDomain<'static, 2, Heuristic> {
    EngineDomain {
        ruleset: Ruleset::CHAMPIONS_MC,
        options: EnumerateOptions { rolls },
        pruning: Pruning::All,
        us,
        evaluator: &Heuristic,
    }
}

fn compare(
    d: &EngineDomain<'_, 2, Heuristic>,
    p: &Position<2>,
    pair: [Choice<2>; 2],
) -> Vec<(f64, Position<2>)> {
    let before = p.clone();
    let actions = [&pair[0], &pair[1]];
    let expected = d.transitions(p, actions);
    let actual = d.transitions_owned(p, actions);
    assert_eq!(p.state, before.state);
    assert_eq!(p.suspension, before.suspension);
    match (expected, actual) {
        (Err(a), Err(b)) => {
            assert_eq!(a, b);
            Vec::new()
        }
        (Ok(a), Ok(b)) => {
            assert_eq!(a.len(), b.len());
            for ((pa, a), (pb, b)) in a.iter().zip(&b) {
                assert_eq!(pa.to_bits(), pb.to_bits());
                assert_eq!(a.state, b.state);
                assert_eq!(a.state.position_hash(), b.state.position_hash());
                assert_eq!(a.suspension, b.suspension);
            }
            b
        }
        (a, b) => panic!("different result/error kinds: {a:?} / {b:?}"),
    }
}

#[test]
fn complete_hidden_state_order_and_probability_match_across_mechanics_and_seats() {
    let _flat = FactoredScope::new(false);
    let fixtures = [
        "single-hit",
        "hypnosis-gravity",
        "psych-up-speed-swap",
        "mega-tyranitar",
        "ee-transform-mega",
        "hyper-beam-recharge",
        "o28-struggle",
        "substitute-status",
    ];
    let mut pairs = 0;
    let mut endings = 0;
    for name in fixtures {
        for rolls in [RollMode::Median, RollMode::Extremes, RollMode::Full] {
            let (p, physical) = fixture(name, rolls);
            for us in [SideId::One, SideId::Two] {
                let pair = if us == SideId::One {
                    physical
                } else {
                    [physical[1], physical[0]]
                };
                let children = compare(&domain(us, rolls), &p, pair);
                assert!(
                    !children.is_empty(),
                    "fixture unexpectedly rejected: {name}"
                );
                pairs += 1;
                endings += children.len();
            }
        }
    }
    println!(
        "S26E_EXACT mechanics={} pairs={pairs} endings={endings}",
        fixtures.len()
    );
}

#[test]
fn owned_path_removes_instruction_materialization_and_factored_path_falls_back() {
    for factored in [false, true] {
        let _scope = FactoredScope::new(factored);
        let (p, pair) = fixture("single-hit", RollMode::Full);
        let d = domain(SideId::One, RollMode::Full);
        observer::reset();
        let reference = d.transitions(&p, [&pair[0], &pair[1]]).unwrap();
        let before = observer::counts();
        assert_eq!(before.materialized_outcomes, reference.len());
        assert!(before.emitted_instructions > 0);
        observer::reset();
        let candidate = d.transitions_owned(&p, [&pair[0], &pair[1]]).unwrap();
        let after = observer::counts();
        assert_eq!(candidate.len(), reference.len());
        if factored {
            assert_eq!(after, before);
        } else {
            assert_eq!(after.materialized_outcomes, 0);
            assert_eq!(after.emitted_instructions, 0);
            assert_eq!(after.batches, 1);
            assert_eq!(after.visits, candidate.len());
        }
        compare(&d, &p, pair);
        println!("S26E_ACTIVATION factored={factored} reference={before:?} candidate={after:?}");
    }
}

#[test]
fn suspended_and_fainted_endings_continue_via_the_original_adapter() {
    let _flat = FactoredScope::new(false);
    let mut mid_turns = 0;
    let mut replacements = 0;
    for name in ["eject-button-uturn", "memento-final-gambit"] {
        let (p, physical) = fixture(name, RollMode::Full);
        for us in [SideId::One, SideId::Two] {
            let d = domain(us, RollMode::Full);
            let pair = if us == SideId::One {
                physical
            } else {
                [physical[1], physical[0]]
            };
            let children = compare(&d, &p, pair);
            let (_, child) = children
                .iter()
                .find(|(_, p)| d.phase(p).unwrap() == Phase::Switch)
                .expect("fixture must reach a switch boundary");
            if child.suspension.is_some() {
                mid_turns += 1;
            } else {
                replacements += 1;
            }
            let choices = [d.actions(child, 0).unwrap(), d.actions(child, 1).unwrap()];
            for left in choices[0].iter().take(2) {
                for right in choices[1].iter().take(2) {
                    observer::reset();
                    let actual = d.transitions_owned(child, [left, right]).unwrap();
                    assert_eq!(observer::counts().batches, 0);
                    assert_eq!(observer::counts().visits, 0);
                    assert!(!actual.is_empty());
                    compare(&d, child, [*left, *right]);
                }
            }
        }
    }
    assert!(mid_turns > 0 && replacements > 0);
    println!("S26E_CONTINUATIONS mid_turns={mid_turns} replacements={replacements}");
}

#[test]
fn invalid_choices_rules_and_state_errors_preserve_error_order_and_inputs() {
    for factored in [false, true] {
        let _scope = FactoredScope::new(factored);
        let (mut p, pair) = fixture("mega-tyranitar", RollMode::Median);
        let mut d = domain(SideId::One, RollMode::Median);
        d.ruleset = Ruleset::NO_GIMMICKS;
        assert!(d.transitions_owned(&p, [&pair[0], &pair[1]]).is_err());
        compare(&d, &p, pair);
        d.ruleset = Ruleset::CHAMPIONS_MC;
        let mut invalid = pair;
        for choice in &mut invalid {
            let Choice::Turn(actions) = choice else {
                unreachable!()
            };
            actions[0] = SlotAction::Pass;
        }
        p.state.field[FieldEffect::Gravity as usize] = Effect {
            turns: Effect::PERMANENT,
            value: 0,
        };
        for choices in [
            pair,
            invalid,
            [invalid[0], pair[1]],
            [pair[0], invalid[1]],
            [Choice::Switches([None; 2]), pair[1]],
        ] {
            assert!(d.transitions_owned(&p, [&choices[0], &choices[1]]).is_err());
            compare(&d, &p, choices);
        }
        p.state.result = BattleResult::Tie;
        compare(&d, &p, pair);
    }
}

#[test]
fn exhaustive_tree_backend_activates_ownership_without_changing_the_tree() {
    let _flat = FactoredScope::new(false);
    let (p, _) = fixture("psych-up-speed-swap", RollMode::Full);
    let worlds = [EngineWorld {
        id: "normal".into(),
        weight: 1.,
        position: p,
    }];
    let limits = tree::builder::Limits {
        turns: 1,
        ..Default::default()
    };
    observer::reset();
    let a = tree::engine::build_writing(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Default::default(),
        limits,
    )
    .unwrap();
    assert!(observer::counts().materialized_outcomes > 0);
    observer::reset();
    let b = tree::engine::build_owned(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Default::default(),
        limits,
    )
    .unwrap();
    let counts = observer::counts();
    assert_eq!(counts.batches, b.stats.transitions);
    assert!(counts.batches > 0);
    assert_eq!(counts.visits, b.stats.chance_outcomes);
    assert_eq!(counts.materialized_outcomes, 0);
    assert_eq!(format!("{:?}", a.tree), format!("{:?}", b.tree));
    println!("S26E_FULL_BACKEND {counts:?}");
}

#[test]
fn partial_growth_limits_return_the_identical_committed_tree_and_solution() {
    let _flat = FactoredScope::new(false);
    let (p, _) = fixture("psych-up-speed-swap", RollMode::Full);
    let worlds = [EngineWorld {
        id: "normal".into(),
        weight: 1.,
        position: p.clone(),
    }];
    for (expansions, nodes, transitions) in [
        (1, 5000, 5000),
        (2, 5000, 5000),
        (50, 400, 5000),
        (50, 5000, 64),
        (50, 8, 5000),
        (50, 5000, 1),
    ] {
        let limits = tree::builder::Limits {
            turns: 2,
            max_nodes: nodes,
            max_transitions: transitions,
            max_decisions: 16,
        };
        let settings = tree::builder::growing::reuse::Settings {
            growth: tree::builder::growing::Config {
                max_expansions: expansions,
                max_walks: 1000,
                solver: bayesian::Config {
                    iterations: 32,
                    check_every: 16,
                    tolerance: 0.1,
                },
                ..Default::default()
            },
            storage: tree::builder::growing::reuse::Options {
                in_place: true,
                workspace: true,
                compiler: true,
                static_values: true,
                direct_write: true,
            },
        };
        let args = (
            &worlds,
            SideId::One,
            Ruleset::CHAMPIONS_MC,
            &Heuristic,
            &Default::default(),
        );
        let a =
            tree::engine::growing_reusing(args.0, args.1, args.2, args.3, args.4, limits, settings);
        observer::reset();
        let b =
            tree::engine::growing_owned(args.0, args.1, args.2, args.3, args.4, limits, settings);
        if nodes == 5000 && transitions == 5000 {
            let counts = observer::counts();
            assert!(counts.batches > 0 && counts.visits > 0);
            assert_eq!(counts.materialized_outcomes, 0);
        }
        match (a, b) {
            (Err(a), Err(b)) => assert_eq!(a, b),
            (Ok(a), Ok(b)) => {
                assert_eq!(format!("{:?}", a.built.tree), format!("{:?}", b.built.tree));
                assert_eq!(
                    format!("{:?}", a.built.stats),
                    format!("{:?}", b.built.stats)
                );
                assert_eq!(format!("{:?}", a.solution), format!("{:?}", b.solution));
                assert_eq!(format!("{:?}", a.work), format!("{:?}", b.work));
                assert_eq!(a.stop, b.stop);
                assert_eq!(
                    (
                        a.frontier_histories,
                        a.frontier_public_groups,
                        a.horizon_complete
                    ),
                    (
                        b.frontier_histories,
                        b.frontier_public_groups,
                        b.horizon_complete
                    )
                );
            }
            _ => panic!("different growth success/error result"),
        }
        assert_eq!(worlds[0].position.state, p.state);
    }
}
