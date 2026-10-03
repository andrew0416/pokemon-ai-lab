use super::*;
fn matrix(a: &[&[f64]]) -> Tree {
    let rows = a.len();
    let cols = a[0].len();
    let mut nodes = vec![Node::Terminal(0.); 1 + rows];
    nodes[0] = Node::Decision {
        player: 0,
        information: "row".into(),
        actions: (0..rows).map(|i| format!("r{i}")).collect(),
        children: (1..=rows).collect(),
    };
    for (r, values) in a.iter().enumerate() {
        let mut children = vec![];
        for &v in *values {
            children.push(nodes.len());
            nodes.push(Node::Terminal(v));
        }
        nodes[1 + r] = Node::Decision {
            player: 1,
            information: "col".into(),
            actions: (0..cols).map(|i| format!("c{i}")).collect(),
            children,
        };
    }
    Tree::new(nodes, 0).unwrap()
}
const RULES: [Rule; 10] = [
    Rule::Lcfr,
    Rule::Cfr,
    Rule::CfrSimultaneous,
    Rule::Dcfr,
    Rule::PcfrPlus,
    Rule::SapcfrPlus,
    Rule::HsDcfr15,
    Rule::HsDcfr30,
    Rule::HsPcfr15,
    Rule::HsPcfr30,
];
#[test]
fn lcfr_control_retains_reference_arithmetic_and_checks() {
    let tree = matrix(&[&[1., -2., 7.], &[-3., 2., 0.], &[4., 0., -1.]]);
    for every in [1, 7, 32] {
        let cfg = Config {
            iterations: 257,
            check_every: every,
            tolerance: 0.,
        };
        let a = super::super::solve(&tree, cfg).unwrap();
        let b = solve(&tree, cfg, Settings::default()).unwrap().solution;
        assert_eq!(a.policy, b.policy);
        assert_eq!(a.iterations, b.iterations);
        assert_eq!(a.assessment.gap.to_bits(), b.assessment.gap.to_bits());
    }
}
#[test]
fn substitute_regrets_use_new_scale_and_reject_infeasible_virtual_history() {
    let tree = matrix(&[&[1e200, 0.], &[0., 2e200]]);
    let policy = vec![vec![2. / 3., 1. / 3.], vec![2. / 3., 1. / 3.]];
    let snapshot = reuse::Snapshot::capture(&tree, &policy).unwrap();
    let initial = warm::initialize(&tree, &policy, 8).unwrap().unwrap();
    assert!(initial.root_sum <= 0.);
    assert!(initial.regrets.iter().flatten().all(|r| r.is_finite()));
    let small = matrix(&[&[1., 0.], &[0., 2.]]);
    let small_initial = warm::initialize(&small, &policy, 8).unwrap().unwrap();
    assert_eq!(initial.regrets, small_initial.regrets);
    let run = solve_from(
        &small,
        Config {
            iterations: 32768,
            tolerance: 0.01,
            check_every: 32,
        },
        Settings {
            rule: Rule::CfrSimultaneous,
            warm_iterations: 8,
            checks: Checks::Geometric,
            ..Settings::default()
        },
        Some(&snapshot),
    )
    .unwrap();
    assert!(run.stats.warm_applied && run.solution.converged);
    assert_eq!(run.stats.virtual_iterations, 8);
    assert!((run.solution.assessment.value - 2. / 3.).abs() < 0.01);
    // Poor seed + huge T cannot satisfy the joint root condition: cold fallback.
    let bad = vec![vec![1., 0.], vec![0., 1.]];
    assert!(warm::initialize(&small, &bad, 1_000_000).unwrap().is_none());
    let seed = reuse::Snapshot::capture(&small, &bad).unwrap();
    let settings = Settings {
        rule: Rule::CfrSimultaneous,
        warm_iterations: 1_000_000,
        ..Settings::default()
    };
    let a = solve_from(&small, Config::default(), settings, Some(&seed)).unwrap();
    let b = solve(
        &small,
        Config::default(),
        Settings {
            warm_iterations: 0,
            ..settings
        },
    )
    .unwrap();
    assert!(a.stats.warm_attempted && !a.stats.warm_applied);
    assert_eq!(a.solution.policy, b.solution.policy);
}
#[test]
fn warm_configuration_is_explicit_and_average_uses_own_realization_reach() {
    let tree = matrix(&[&[0., 0.], &[0., 0.]]);
    assert!(solve(
        &tree,
        Config::default(),
        Settings {
            warm_iterations: 8,
            ..Settings::default()
        }
    )
    .is_err());
    let mut nodes = tree.export_nodes();
    let old = nodes.len();
    nodes.push(Node::Decision {
        player: 0,
        information: "later".into(),
        actions: vec!["x".into(), "y".into()],
        children: vec![old + 1, old + 2],
    });
    nodes.push(Node::Terminal(1.));
    nodes.push(Node::Terminal(2.));
    // Replace one former leaf by the continuation, keeping a reachable tree.
    nodes.swap(3, old);
    nodes.remove(old);
    // Removal shifts the two new leaves by one.
    if let Node::Decision { children, .. } = &mut nodes[3] {
        for c in children {
            *c -= 1;
        }
    }
    let t = Tree::new(nodes, 0).unwrap();
    let mut p = t.uniform();
    p[0] = vec![0.25, 0.75];
    let mut sums: Policy = p.iter().map(|x| vec![0.; x.len()]).collect();
    let mut total = vec![0.; p.len()];
    warm::average(&t, &p, &mut sums, &mut total, 8.).unwrap();
    let later = t.information.iter().position(|i| i.key == "later").unwrap();
    assert_eq!(total[later], 2.);
    assert_eq!(sums[later], vec![1., 1.]);
    assert!(warm::initialize(&t, &p, 1).unwrap().is_some());
}
#[test]
fn all_rules_certify_asymmetric_mixed_equilibrium_on_original_tree() {
    // Unique equilibrium is (2/3,1/3) for both players, value 2/3.
    let tree = matrix(&[&[1., 0.], &[0., 2.]]);
    for rule in RULES {
        for (compact, sequence) in [(false, false), (true, false), (false, true)] {
            let run = solve(
                &tree,
                Config {
                    iterations: 16384,
                    check_every: 16,
                    tolerance: 0.01,
                },
                Settings {
                    rule,
                    compact,
                    sequence,
                    checks: Checks::Geometric,
                    ..Settings::default()
                },
            )
            .unwrap();
            assert!(
                run.solution.converged,
                "{rule:?} {compact} {}",
                run.solution.assessment.gap
            );
            assert!((run.solution.assessment.value - 2. / 3.).abs() < 0.01);
            assert_eq!(run.stats.compact, compact);
            assert_eq!(run.stats.sequence, sequence);
            assert_eq!(
                run.solution.assessment.gap.to_bits(),
                tree.assess(&run.solution.policy).unwrap().gap.to_bits()
            );
        }
    }
}
#[test]
fn sequence_kernel_matches_multi_decision_counterfactual_updates() {
    // Hidden first action, then own-action recall and branch-specific continuation.
    let mut raw = matrix(&[&[1., -2.], &[3., 0.]]).export_nodes();
    for n in [3usize, 4, 5, 6] {
        let next = raw.len();
        raw[n] = Node::Decision {
            player: 0,
            information: format!("continuation-{}", (n - 3) / 2),
            actions: vec!["left".into(), "right".into()],
            children: vec![next, next + 1],
        };
        raw.push(Node::Terminal(n as f64 - 4.));
        raw.push(Node::Terminal(7. - 2. * n as f64));
    }
    let tree = Tree::new(raw, 0).unwrap();
    let mut kernel = sequence::Kernel::new(&tree).unwrap();
    for seed in 0..12 {
        let policy: Policy = tree
            .information
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let p = ((i + seed) % 5) as f64 / 4.;
                vec![p, 1. - p]
            })
            .collect();
        for player in 0..2 {
            let mut fast: Policy = policy.iter().map(|p| vec![0.; p.len()]).collect();
            let mut slow = fast.clone();
            assert!(kernel.prepare(&tree, &policy, player));
            kernel.accumulate(&tree, &policy, player, &mut fast, 0.37);
            let cf = tree.reach(&policy, Some(player)).unwrap();
            let values = tree.values(&policy);
            let sign = if player == 0 { 1. } else { -1. };
            for (i, info) in tree.information.iter().enumerate() {
                if info.player != player {
                    continue;
                }
                for &n in &info.nodes {
                    let Compiled::Decision { children, .. } = &tree.nodes[n] else {
                        unreachable!()
                    };
                    for (a, &child) in children.iter().enumerate() {
                        slow[i][a] += 0.37 * cf[n] * sign * (values[child] - values[n]);
                    }
                }
            }
            for (a, b) in fast.iter().flatten().zip(slow.iter().flatten()) {
                assert!((a - b).abs() < 1e-12, "{player} {seed} {a} {b}");
            }
        }
    }
}
#[test]
fn mapping_uses_action_labels_and_new_tree_certificate() {
    let tree = matrix(&[&[1., 0.], &[0., 2.]]);
    let policy = vec![vec![2. / 3., 1. / 3.], vec![2. / 3., 1. / 3.]];
    let snapshot = reuse::Snapshot::capture(&tree, &policy).unwrap();
    let mut raw = tree.export_nodes();
    for node in &mut raw {
        if let Node::Decision {
            actions, children, ..
        } = node
        {
            actions.reverse();
            children.reverse();
        }
    }
    // Move the root so compilation assigns a different information index order.
    let old_last = raw.len() - 1;
    raw.swap(0, old_last);
    for node in &mut raw {
        let remap = |n: &mut usize| {
            if *n == 0 {
                *n = old_last
            } else if *n == old_last {
                *n = 0
            }
        };
        match node {
            Node::Decision { children, .. } => children.iter_mut().for_each(remap),
            Node::Chance(edges) => edges.iter_mut().for_each(|(_, n)| remap(n)),
            _ => {}
        }
    }
    let next = Tree::new(raw, old_last).unwrap();
    let cfg = Config {
        iterations: 64,
        tolerance: 1e-12,
        check_every: 32,
    };
    let settings = Settings {
        reuse_policy: true,
        ..Settings::default()
    };
    let reused = solve_from(&next, cfg, settings, Some(&snapshot)).unwrap();
    assert!(reused.stats.reused);
    assert_eq!(reused.solution.iterations, 0);
    assert_eq!(reused.stats.mapped_information, 2);
    for p in &reused.solution.policy {
        assert_eq!(*p, vec![1. / 3., 2. / 3.]);
    }
    // Same keys, changed utility/scale: stale convergence must not be accepted.
    let changed = matrix(&[&[20., 0.], &[0., 1.]]);
    let restart = solve_from(&changed, cfg, settings, Some(&snapshot)).unwrap();
    assert!(restart.stats.reuse_attempted && !restart.stats.reused);
    assert!(restart.solution.iterations > 0);
    let cold = solve(&changed, cfg, Settings::default()).unwrap();
    assert_eq!(restart.solution.policy, cold.solution.policy);
}
#[test]
fn compact_rejects_information_richer_games_and_preserves_underflow() {
    let tree = matrix(&[&[1., -1.], &[-1., 1.]]);
    let mut raw = tree.export_nodes();
    if let Node::Decision { information, .. } = &mut raw[2] {
        *information = "col-sees-row".into();
    }
    let richer = Tree::new(raw, 0).unwrap();
    let settings = Settings {
        compact: true,
        ..Settings::default()
    };
    let run = solve(&richer, Config::default(), settings).unwrap();
    assert!(!run.stats.compact);
    let mut raw = tree.export_nodes();
    let root = raw.len();
    let middle = root + 1;
    let zero = root + 2;
    raw.extend([
        Node::Chance(vec![(1e-200, middle), (1., zero)]),
        Node::Chance(vec![(1e-200, 0), (1., zero + 1)]),
        Node::Terminal(0.),
        Node::Terminal(0.),
    ]);
    let tiny = Tree::new(raw, root).unwrap();
    assert!(solve(&tiny, Config::default(), settings)
        .unwrap_err()
        .0
        .contains("underflow"));
    assert!(super::super::solve(&tiny, Config::default())
        .unwrap_err()
        .0
        .contains("underflow"));
}
#[test]
fn invalid_settings_cannot_be_bypassed_by_a_reused_policy() {
    let tree = matrix(&[&[0., 0.], &[0., 0.]]);
    let seed = reuse::Snapshot::capture(&tree, &tree.uniform()).unwrap();
    let cfg = Config {
        iterations: 0,
        ..Config::default()
    };
    assert!(solve_from(
        &tree,
        cfg,
        Settings {
            reuse_policy: true,
            ..Settings::default()
        },
        Some(&seed)
    )
    .is_err());
}
