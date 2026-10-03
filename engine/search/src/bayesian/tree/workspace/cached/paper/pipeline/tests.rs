use super::*;
fn game(scale: f64, zero: bool) -> Tree {
    fn add(nodes: &mut Vec<Node>, node: Node) -> usize {
        let n = nodes.len();
        nodes.push(node);
        n
    }
    let mut nodes = vec![Node::Terminal(0.)];
    let mut worlds = vec![];
    for world in 0..2 {
        let start = add(&mut nodes, Node::Terminal(0.));
        worlds.push((if zero { world as f64 } else { 0.5 }, start));
        let mut own = vec![];
        for a in 0..2 {
            let other = add(&mut nodes, Node::Terminal(0.));
            own.push(other);
            let mut opponent = vec![];
            for b in 0..2 {
                let signal = add(&mut nodes, Node::Terminal(0.));
                opponent.push(signal);
                let mut edges = vec![];
                for s in 0..2 {
                    let last = add(&mut nodes, Node::Terminal(0.));
                    edges.push((if s == world { 0.7 } else { 0.3 }, last));
                    let children = (0..2)
                        .map(|c| {
                            add(
                                &mut nodes,
                                Node::Terminal(
                                    (((world * 17 + a * 11 + b * 7 + s * 5 + c * 3) % 13) as f64
                                        - 6.)
                                        * scale,
                                ),
                            )
                        })
                        .collect();
                    nodes[last] = Node::Decision {
                        player: 0,
                        information: format!("own-{a}-signal-{s}"),
                        actions: vec!["c0".into(), "c1".into()],
                        children,
                    };
                }
                nodes[signal] = Node::Chance(edges);
            }
            nodes[other] = Node::Decision {
                player: 1,
                information: format!("private-world-{world}"),
                actions: vec!["b0".into(), "b1".into()],
                children: opponent,
            };
        }
        nodes[start] = Node::Decision {
            player: 0,
            information: "start".into(),
            actions: vec!["a0".into(), "a1".into()],
            children: own,
        };
    }
    nodes[0] = Node::Chance(worlds);
    Tree::new(nodes, 0).unwrap()
}
#[test]
fn compressed_best_responses_preserve_information_and_recall() {
    for scale in [1., 1e200, 1e-200] {
        for zero in [false, true] {
            let tree = game(scale, zero);
            let mut kernel = sequence::Kernel::new(&tree).unwrap();
            for seed in 0..41 {
                let policy: Policy = tree
                    .information
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        let p = ((i * 19 + seed * 11) % 37) as f64 / 36.;
                        vec![p, 1. - p]
                    })
                    .collect();
                let original = tree.assess(&policy).unwrap().gap / scale;
                let compressed = kernel.gap(&tree, &policy).unwrap() / scale;
                assert!(
                    (original - compressed).abs() < 1e-11,
                    "original {original}, compressed {compressed}"
                );
            }
        }
    }
}
#[test]
fn cap_and_candidate_always_return_original_tree_certificates() {
    let tree = game(1., false);
    for rule in [
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
    ] {
        for tolerance in [0., 0.1, 100.] {
            let cfg = Config {
                iterations: 257,
                check_every: 7,
                tolerance,
            };
            let settings = Settings {
                rule,
                sequence: true,
                ..Default::default()
            };
            let reference = super::super::solve(&tree, cfg, settings).unwrap();
            let mut metrics = Metrics::default();
            let candidate = solve_from(
                &tree,
                cfg,
                settings,
                None,
                PipelineSettings {
                    compressed_checks: true,
                    ..Default::default()
                },
                None,
                &mut Context::default(),
                &mut metrics,
            )
            .unwrap();
            assert_eq!(
                format!("{:?}", candidate.solution.assessment),
                format!("{:?}", tree.assess(&candidate.solution.policy).unwrap())
            );
            assert_eq!(
                format!("{:?}", candidate.solution),
                format!("{:?}", reference.solution)
            );
            assert!(candidate.stats.assessments >= 1);
            assert!(candidate.stats.assessments <= reference.stats.assessments);
            if tolerance == 0. {
                assert!(metrics.compressed_rejections > 0);
                // Near-zero numerical gaps deliberately trigger reference checks
                // again; the filter must not bypass the original zero tolerance.
                assert!(candidate.stats.assessments < reference.stats.assessments);
            }
        }
    }
}
#[test]
fn guarded_subnormal_check_falls_back_instead_of_publishing_fast_bound() {
    let tree = game(1., false);
    let mut kernel = sequence::Kernel::new(&tree).unwrap();
    let policy: Policy = tree.information.iter().map(|_| vec![1e-200, 1.]).collect();
    assert!(kernel.gap(&tree, &policy).is_none());
}
