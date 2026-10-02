use super::builder::{self, Cursor, Limits, Observation, ObservedDomain, Seed};
use super::*;
use crate::budgeted::{Domain, Phase};

fn cfg() -> Config {
    Config {
        iterations: 30_000,
        tolerance: 0.005,
        check_every: 64,
    }
}
fn decision(player: usize, key: &str, children: Vec<usize>) -> Node {
    Node::Decision {
        player,
        information: key.into(),
        actions: (0..children.len()).map(|i| format!("a{i}")).collect(),
        children,
    }
}
fn near(a: f64, b: f64, eps: f64) {
    assert!((a - b).abs() < eps, "{a} vs {b}");
}

#[test]
fn simultaneous_choices_do_not_observe_each_other() {
    let t = Tree::new(
        vec![
            decision(0, "us", vec![1, 2]),
            decision(1, "them", vec![3, 4]),
            decision(1, "them", vec![5, 6]),
            Node::Terminal(1.),
            Node::Terminal(-1.),
            Node::Terminal(-1.),
            Node::Terminal(1.),
        ],
        0,
    )
    .unwrap();
    let s = solve(&t, cfg()).unwrap();
    assert!(s.converged);
    near(s.assessment.value, 0., 0.006);
    near(s.policy[0][0], 0.5, 0.006);
    near(s.policy[1][0], 0.5, 0.006);
    assert_eq!(t.information[1].nodes.len(), 2);
}

#[cfg(feature = "experiment-belief-workspace")]
#[test]
fn scratch_reuse_matches_reference_kuhn_at_every_check_schedule() {
    let tree = kuhn();
    for iterations in [1, 2, 3, 31, 32, 127, 511] {
        for check_every in [1, 7, 32] {
            for tolerance in [0., 0.1] {
                let config = Config {
                    iterations,
                    check_every,
                    tolerance,
                };
                let reference = solve(&tree, config).unwrap();
                let candidate = workspace::solve(&tree, config).unwrap();
                let cached = workspace::cached::solve(&tree, config).unwrap();
                assert_eq!(format!("{reference:?}"), format!("{cached:?}"));
                assert_eq!(format!("{reference:?}"), format!("{candidate:?}"));
            }
        }
    }
}

#[cfg(feature = "experiment-belief-workspace")]
#[test]
fn scratch_reuse_matches_scaled_hidden_games_and_invalid_configs() {
    for scale in [0., 1e-300, 1., 1e100] {
        for mass in [0., 0.01, 0.5, 1.] {
            let t = Tree::new(
                vec![
                    Node::Chance(vec![(mass, 1), (1. - mass, 2)]),
                    decision(0, "same", vec![3, 4]),
                    decision(0, "same", vec![5, 6]),
                    Node::Terminal(scale),
                    Node::Terminal(-scale),
                    Node::Terminal(-scale * 0.7),
                    Node::Terminal(scale * 0.3),
                ],
                0,
            )
            .unwrap();
            for config in [
                Config {
                    iterations: 100,
                    check_every: 7,
                    tolerance: 0.,
                },
                Config {
                    iterations: 0,
                    ..cfg()
                },
                Config {
                    check_every: 0,
                    ..cfg()
                },
                Config {
                    tolerance: f64::NAN,
                    ..cfg()
                },
            ] {
                assert_eq!(
                    format!("{:?}", solve(&t, config)),
                    format!("{:?}", workspace::solve(&t, config))
                );
                assert_eq!(
                    format!("{:?}", solve(&t, config)),
                    format!("{:?}", workspace::cached::solve(&t, config))
                );
            }
        }
    }
}

#[test]
fn best_response_shares_actions_across_indistinguishable_histories() {
    let t = Tree::new(
        vec![
            Node::Chance(vec![(0.5, 1), (0.5, 2)]),
            decision(0, "same", vec![3, 4]),
            decision(0, "same", vec![5, 6]),
            Node::Terminal(1.),
            Node::Terminal(-1.),
            Node::Terminal(-1.),
            Node::Terminal(1.),
        ],
        0,
    )
    .unwrap();
    let (value, chosen) = t.best_response(&t.uniform(), 0).unwrap();
    assert_eq!(value, 0.);
    assert_eq!(chosen.len(), 1);
    assert_eq!(t.assess(&t.uniform()).unwrap().gap, 0.);
}

#[test]
fn invalid_trees_and_imperfect_recall_are_rejected() {
    assert!(Tree::new(vec![Node::Chance(vec![(0.9, 1)]), Node::Terminal(0.)], 0).is_err());
    assert!(Tree::new(vec![Node::Chance(vec![(1., 0)])], 0).is_err());
    assert!(Tree::new(
        vec![Node::Chance(vec![(0.5, 1), (0.5, 1)]), Node::Terminal(0.)],
        0
    )
    .is_err());
    assert!(Tree::new(vec![Node::Terminal(0.), Node::Terminal(1.)], 0).is_err());
    assert!(Tree::new(
        vec![
            decision(0, "start", vec![1, 2]),
            decision(0, "forgot", vec![3]),
            decision(0, "forgot", vec![4]),
            Node::Terminal(0.),
            Node::Terminal(0.)
        ],
        0
    )
    .is_err());
    assert!(Tree::new(
        vec![
            Node::Chance(vec![(0.5, 1), (0.5, 2)]),
            decision(0, "same", vec![3]),
            decision(1, "same", vec![4]),
            Node::Terminal(0.),
            Node::Terminal(0.)
        ],
        0
    )
    .is_err());
}

fn kuhn() -> Tree {
    fn add(nodes: &mut Vec<Node>, cards: [usize; 2], history: &str) -> usize {
        let sign = if cards[0] > cards[1] { 1. } else { -1. };
        let value = match history {
            "cc" => Some(sign),
            "bf" => Some(1.),
            "bc" => Some(2. * sign),
            "cbf" => Some(-1.),
            "cbc" => Some(2. * sign),
            _ => None,
        };
        let id = nodes.len();
        nodes.push(Node::Terminal(value.unwrap_or(0.)));
        if value.is_some() {
            return id;
        }
        let player = if history.is_empty() || history == "cb" {
            0
        } else {
            1
        };
        let actions = if history.ends_with('b') {
            ["f", "c"]
        } else {
            ["c", "b"]
        };
        let children = actions
            .iter()
            .map(|a| add(nodes, cards, &format!("{history}{a}")))
            .collect();
        nodes[id] = Node::Decision {
            player,
            information: format!("p{player}:card{}:{history}", cards[player]),
            actions: actions.iter().map(|s| s.to_string()).collect(),
            children,
        };
        id
    }
    let mut nodes = vec![Node::Terminal(0.)];
    let mut edges = Vec::new();
    for a in 0..3 {
        for b in 0..3 {
            if a != b {
                let n = add(&mut nodes, [a, b], "");
                edges.push((1. / 6., n));
            }
        }
    }
    nodes[0] = Node::Chance(edges);
    Tree::new(nodes, 0).unwrap()
}

#[test]
fn kuhn_poker_matches_known_equilibrium_with_perfect_recall() {
    let t = kuhn();
    let s = solve(&t, cfg()).unwrap();
    assert!(s.converged, "gap {}", s.assessment.gap);
    near(s.assessment.value, -1. / 18., 0.005);
    assert!(s.assessment.lower <= -1. / 18. + 1e-9 && s.assessment.upper >= -1. / 18. - 1e-9);
}

// The first simultaneous choice commits an opponent action, then a pivot asks us to
// guess it. Physical histories differ but the pending action is not observable.
#[derive(Clone, Debug)]
struct Hidden {
    world: usize,
    stage: usize,
    commit: usize,
    value: f32,
}
struct Pivot {
    reveal: bool,
    signal: bool,
}
impl Domain for Pivot {
    type Position = Hidden;
    type Action = usize;
    fn phase(&self, p: &Hidden) -> Result<Phase, String> {
        Ok(match p.stage {
            0 => Phase::Turn,
            1 => Phase::Switch,
            _ => Phase::Terminal,
        })
    }
    fn actions(&self, p: &Hidden, player: usize) -> Result<Vec<usize>, String> {
        Ok(
            if (p.stage == 0 && player == 1) || (p.stage == 1 && player == 0) {
                vec![0, 1]
            } else {
                vec![0]
            },
        )
    }
    fn value(&self, p: &Hidden) -> f32 {
        p.value
    }
    fn transitions(&self, p: &Hidden, a: [&usize; 2]) -> Result<Vec<(f64, Hidden)>, String> {
        let mut c = p.clone();
        c.stage += 1;
        if p.stage == 0 {
            c.commit = *a[1];
        } else {
            c.value = if *a[0] == if self.signal { p.world } else { p.commit } {
                1.
            } else {
                -1.
            };
        }
        Ok(vec![(1., c)])
    }
}
impl ObservedDomain for Pivot {
    fn observation(&self, p: &Hidden) -> Result<Observation, String> {
        Ok(Observation {
            public: if p.stage == 1 && (self.reveal || self.signal) {
                format!("switch-signal{}", p.commit)
            } else {
                format!("stage{}", p.stage)
            },
            private: [String::new(), String::new()],
        })
    }
    fn action_id(&self, _: &Hidden, _: usize, a: &usize) -> String {
        format!("a{a}")
    }
}
fn seed(id: &str, world: usize, weight: f64) -> Seed<Hidden> {
    Seed {
        id: id.into(),
        weight,
        position: Hidden {
            world,
            stage: 0,
            commit: 0,
            value: 0.,
        },
    }
}

#[test]
fn pending_commitment_stays_hidden_until_an_observation_reveals_it() {
    for reveal in [false, true] {
        let d = Pivot {
            reveal,
            signal: false,
        };
        let b = builder::build(
            &d,
            &[seed("one", 0, 1.)],
            Limits {
                turns: 1,
                ..Limits::default()
            },
        )
        .unwrap();
        let s = solve(&b.tree, cfg()).unwrap();
        near(s.assessment.value, if reveal { 1. } else { 0. }, 0.006);
        let pivot_infos: Vec<_> = b
            .tree
            .information()
            .iter()
            .filter(|i| i.player == 0 && i.actions.len() == 2)
            .collect();
        assert_eq!(pivot_infos.len(), if reveal { 2 } else { 1 });
        assert_eq!(b.stats.switch_decisions, 2);
        if !reveal {
            assert_eq!(pivot_infos[0].nodes.len(), 2);
        }
    }
}

#[test]
fn public_belief_depends_on_world_specific_action_likelihoods() {
    let d = Pivot {
        reveal: false,
        signal: true,
    };
    let b = builder::build(&d, &[seed("a", 0, 1.), seed("b", 1, 3.)], Limits::default()).unwrap();
    let mut policy = b.tree.uniform();
    for (i, info) in b.tree.information().iter().enumerate() {
        if info.player == 1 && info.actions.len() == 2 {
            policy[i] = if info.key.contains("Type(\"a\")") {
                vec![0.75, 0.25]
            } else {
                vec![0.25, 0.75]
            };
        }
    }
    let beliefs = b.tree.public_beliefs(&policy).unwrap();
    let red = beliefs
        .iter()
        .find(|b| b.key.contains("switch-signal0"))
        .unwrap();
    near(red.posterior.as_ref().unwrap()[0], 0.5, 1e-12);
    near(red.reach, 0.375, 1e-12);
    let blue = beliefs
        .iter()
        .find(|b| b.key.contains("switch-signal1"))
        .unwrap();
    near(blue.posterior.as_ref().unwrap()[0], 0.1, 1e-12);
    for (i, info) in b.tree.information().iter().enumerate() {
        if info.player == 1 && info.actions.len() == 2 {
            policy[i] = vec![1., 0.];
        }
    }
    let absent = b
        .tree
        .public_beliefs(&policy)
        .unwrap()
        .into_iter()
        .find(|b| b.key.contains("switch-signal1"))
        .unwrap();
    assert_eq!(absent.reach, 0.);
    assert!(absent.posterior.is_none());
}

#[test]
fn continuation_cursor_needs_only_our_action_and_observation() {
    let d = Pivot {
        reveal: false,
        signal: false,
    };
    let s = seed("world", 0, 1.);
    let initial = d.observation(&s.position).unwrap();
    assert!(Cursor::new(0, Some("world"), &initial).is_err());
    let b = builder::build(&d, &[s.clone()], Limits::default()).unwrap();
    let mut cursor = Cursor::new(0, None, &initial).unwrap();
    assert!(cursor.information(&b.tree).is_some());
    let after = d.transitions(&s.position, [&0, &1]).unwrap().remove(0).1;
    cursor.advance("a0", &d.observation(&after).unwrap());
    let id = cursor.information(&b.tree).unwrap();
    assert_eq!(b.tree.information()[id].nodes.len(), 2);
}

#[test]
fn budgets_fail_closed_instead_of_returning_a_partial_belief_tree() {
    let d = Pivot {
        reveal: false,
        signal: false,
    };
    let seeds = [seed("x", 0, 1.)];
    for limits in [
        Limits {
            max_nodes: 2,
            ..Limits::default()
        },
        Limits {
            max_transitions: 1,
            ..Limits::default()
        },
        Limits {
            max_decisions: 1,
            ..Limits::default()
        },
    ] {
        assert!(builder::build(&d, &seeds, limits).is_err());
    }
    assert_eq!(seeds[0].position.stage, 0);
}

#[derive(Clone)]
struct TwoTurn;
impl Domain for TwoTurn {
    type Position = (u32, usize);
    type Action = usize;
    fn phase(&self, _: &Self::Position) -> Result<Phase, String> {
        Ok(Phase::Turn)
    }
    fn actions(&self, _: &Self::Position, player: usize) -> Result<Vec<usize>, String> {
        Ok(if player == 0 { vec![0, 1] } else { vec![0] })
    }
    fn value(&self, p: &Self::Position) -> f32 {
        p.1 as f32
    }
    fn transitions(
        &self,
        p: &Self::Position,
        a: [&usize; 2],
    ) -> Result<Vec<(f64, Self::Position)>, String> {
        Ok(vec![(1., (p.0 + 1, p.1 + *a[0]))])
    }
}
impl ObservedDomain for TwoTurn {
    fn observation(&self, p: &Self::Position) -> Result<Observation, String> {
        Ok(Observation {
            public: format!("turn{}", p.0),
            private: [String::new(), String::new()],
        })
    }
    fn action_id(&self, _: &Self::Position, _: usize, a: &usize) -> String {
        format!("{a}")
    }
}
#[test]
fn multi_turn_horizon_counts_turns_and_own_memory_is_preserved() {
    let b = builder::build(
        &TwoTurn,
        &[Seed {
            id: "w".into(),
            weight: 1.,
            position: (0, 0),
        }],
        Limits {
            turns: 2,
            ..Limits::default()
        },
    )
    .unwrap();
    let s = solve(&b.tree, cfg()).unwrap();
    near(s.assessment.value, 2., 0.006);
    assert_eq!(b.stats.turn_decisions, 3);
    assert_eq!(b.stats.leaves, 4);
    assert_eq!(
        b.tree
            .information()
            .iter()
            .filter(|i| i.player == 0)
            .count(),
        3
    );
}

fn position(name: &str) -> crate::budgeted::Position<2> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios")
        .join(name);
    let loaded = lab_scenario::load_scenario_file(path).unwrap();
    let mut p = lab_scenario::scenario_positions_with(
        &loaded,
        lab_engine::turn::EnumerateOptions::default(),
    )
    .unwrap();
    assert_eq!(p.len(), 1);
    crate::budgeted::Position {
        state: p.remove(0).state,
        suspension: None,
    }
}
fn ew(id: &str, p: crate::budgeted::Position<2>) -> crate::bayesian::engine::EngineWorld<2> {
    crate::bayesian::engine::EngineWorld {
        id: id.into(),
        weight: 1.,
        position: p,
    }
}

#[test]
fn actual_engine_two_turns_preserve_inputs_and_shared_root_policy() {
    use lab_engine::{eval::Heuristic, rules::Ruleset, state::SideId};
    let p = position("psych-up-speed-swap.json");
    let before = p.state.clone();
    let worlds = [ew("a", p.clone()), ew("b", p)];
    let b = engine::build(
        &worlds,
        SideId::One,
        Ruleset::CHAMPIONS_MC,
        &Heuristic,
        &Default::default(),
        Limits::default(),
    )
    .unwrap();
    assert!(b.stats.turn_decisions > 2);
    assert_eq!(b.stats.switch_decisions, 0);
    let roots: Vec<_> = b
        .tree
        .boundaries()
        .iter()
        .filter(|b| b.public == 0)
        .map(|n| b.tree.information_at(n.node).unwrap())
        .collect();
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0], roots[1]);
    let s = solve(
        &b.tree,
        Config {
            iterations: 500,
            tolerance: 0.1,
            check_every: 16,
        },
    )
    .unwrap();
    assert!(
        s.assessment.lower <= s.assessment.value + 1e-9
            && s.assessment.upper >= s.assessment.value - 1e-9
    );
    assert_eq!(worlds[0].position.state, before);
    assert_eq!(worlds[1].position.state, before);
}

#[test]
fn actual_engine_pivot_and_forced_replacements_are_resolved_at_horizon() {
    use lab_engine::{eval::Heuristic, rules::Ruleset, state::SideId};
    for fixture in ["eject-button-uturn.json", "memento-final-gambit.json"] {
        let mut p = position(fixture);
        if fixture == "memento-final-gambit.json" {
            // Ensure fainted slots have a legal reserve; its set is already known to the engine.
            for side in &mut p.state.sides {
                side.party[2] = side.party[1].clone();
            }
        }
        let before = p.state.clone();
        let worlds = [ew("w", p)];
        let b = engine::build(
            &worlds,
            SideId::One,
            Ruleset::CHAMPIONS_MC,
            &Heuristic,
            &Default::default(),
            Limits {
                turns: 1,
                max_nodes: 100_000,
                ..Limits::default()
            },
        )
        .unwrap();
        assert!(b.stats.switch_decisions > 0, "{fixture}");
        assert!(
            b.tree
                .public_keys
                .iter()
                .any(|k| k.contains(if fixture.starts_with("eject") {
                    "MidTurn"
                } else {
                    "Replacement"
                })),
            "{fixture}"
        );
        let assessment = b.tree.assess(&b.tree.uniform()).unwrap();
        assert!(assessment.gap.is_finite());
        assert_eq!(worlds[0].position.state, before);
    }
}

#[test]
fn engine_snapshot_cannot_read_the_suspended_opponent_command() {
    use crate::{
        budgeted::{EngineDomain, Position},
        Choice, Pruning,
    };
    use lab_engine::{eval::Heuristic, rules::Ruleset, state::SideId, turn::EnumerateOptions};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios/eject-button-uturn.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    v["p2"]["team"][1]["moves"] = serde_json::json!(["Harden", "Calm Mind"]);
    let loaded =
        lab_scenario::load_scenario_str(&v.to_string(), std::path::Path::new(".")).unwrap();
    let p = Position {
        state: lab_scenario::scenario_positions_with(&loaded, EnumerateOptions::default())
            .unwrap()
            .remove(0)
            .state,
        suspension: None,
    };
    let d = engine::SnapshotDomain {
        inner: EngineDomain {
            ruleset: Ruleset::CHAMPIONS_MC,
            options: EnumerateOptions::default(),
            pruning: Pruning::All,
            us: SideId::One,
            evaluator: &Heuristic,
        },
    };
    let named = |side, a: &Choice<2>| {
        let Choice::Turn(a) = a else { panic!() };
        crate::format_choice(&p.state, side, &[0, 1, 2], a)
    };
    let a = d
        .actions(&p, 0)
        .unwrap()
        .into_iter()
        .find(|a| named(SideId::One, a) == "move uturn 1, move harden")
        .unwrap();
    let theirs = d.actions(&p, 1).unwrap();
    let x = theirs
        .iter()
        .find(|a| named(SideId::Two, a) == "move harden, move harden")
        .unwrap();
    let y = theirs
        .iter()
        .find(|a| named(SideId::Two, a) == "move harden, move calmmind")
        .unwrap();
    let xs = d.transitions(&p, [&a, x]).unwrap();
    let ys = d.transitions(&p, [&a, y]).unwrap();
    let pair = xs
        .iter()
        .flat_map(|(_, x)| ys.iter().map(move |(_, y)| (x, y)))
        .find(|(x, y)| x.state == y.state && x.suspension.is_some() && x.suspension != y.suspension)
        .unwrap();
    assert_eq!(
        d.observation(pair.0).unwrap(),
        d.observation(pair.1).unwrap()
    );
}

struct XorObservation;
impl Domain for XorObservation {
    type Position = Hidden;
    type Action = usize;
    fn phase(&self, _: &Hidden) -> Result<Phase, String> {
        Ok(Phase::Turn)
    }
    fn actions(&self, _: &Hidden, p: usize) -> Result<Vec<usize>, String> {
        Ok(if p == 0 { vec![0, 1] } else { vec![0] })
    }
    fn value(&self, p: &Hidden) -> f32 {
        p.world as f32
    }
    fn transitions(&self, p: &Hidden, a: [&usize; 2]) -> Result<Vec<(f64, Hidden)>, String> {
        let mut next = p.clone();
        next.stage = 1;
        next.commit = *a[0];
        Ok(vec![(1., next)])
    }
}
impl ObservedDomain for XorObservation {
    fn observation(&self, p: &Hidden) -> Result<Observation, String> {
        Ok(Observation {
            public: if p.stage == 0 {
                "start".into()
            } else {
                format!("xor{}", p.world ^ p.commit)
            },
            private: [String::new(), String::new()],
        })
    }
    fn action_id(&self, _: &Hidden, _: usize, a: &usize) -> String {
        format!("a{a}")
    }
}
#[test]
fn horizon_posterior_conditions_on_private_own_action_not_just_public_signal() {
    let d = XorObservation;
    let a = seed("a", 0, 1.);
    let b = seed("b", 1, 1.);
    let built = builder::build(
        &d,
        &[a.clone(), b],
        Limits {
            turns: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    let policy = built.tree.uniform();
    let public = built
        .tree
        .public_beliefs(&policy)
        .unwrap()
        .into_iter()
        .find(|b| b.key.contains("xor0"))
        .unwrap();
    assert_eq!(public.posterior, Some(vec![0.5, 0.5]));
    let mut cursor = Cursor::new(0, None, &d.observation(&a.position).unwrap()).unwrap();
    let after = d.transitions(&a.position, [&0, &0]).unwrap().remove(0).1;
    cursor.advance("a0", &d.observation(&after).unwrap());
    assert!(cursor.information(&built.tree).is_none()); // horizon, but posterior is retained
    let private = cursor.belief(&built.tree, &policy).unwrap().unwrap();
    assert_eq!(private.posterior, Some(vec![1., 0.]));
    assert_eq!(private.histories.len(), 1);
    near(private.world_values[0].unwrap(), 0., 1e-12);
}
