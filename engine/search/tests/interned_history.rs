use lab_engine::{
    eval::Heuristic,
    rules::Ruleset,
    state::SideId,
    turn::{EnumerateOptions, FactoredScope},
};
use lab_search::{
    bayesian::{
        self,
        engine::{EngineWorld, Knowledge},
        tree::{
            self,
            builder::{self, growing, history_observer as observer},
        },
    },
    budgeted::Position,
};

fn worlds(name: &str) -> Vec<EngineWorld<2>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios")
        .join(format!("{name}.json"));
    let loaded = lab_scenario::load_scenario_file(path).unwrap();
    let p = lab_scenario::scenario_positions_with(&loaded, EnumerateOptions::default())
        .unwrap()
        .remove(0);
    vec![EngineWorld {
        id: "world\"한글\\1".into(),
        weight: 1.,
        position: Position {
            state: p.state,
            suspension: None,
        },
    }]
}
fn result(r: Result<growing::ResultTree, bayesian::Error>) -> String {
    match r {
        Err(e) => format!("error:{e:?}"),
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
#[test]
fn engine_wrappers_activate_sharing_for_both_seats_and_preserve_all_tree_fields() {
    let _flat = FactoredScope::new(false);
    for name in ["psych-up-speed-swap", "eject-button-uturn"] {
        let worlds = worlds(name);
        let before = worlds[0].position.state.clone();
        for side in [SideId::One, SideId::Two] {
            let limits = builder::Limits {
                turns: 1,
                ..Default::default()
            };
            let knowledge = Knowledge::default();
            let a = tree::engine::build_owned(
                &worlds,
                side,
                Ruleset::CHAMPIONS_MC,
                &Heuristic,
                &knowledge,
                limits,
            )
            .unwrap();
            observer::reset();
            let b = tree::engine::build_interned(
                &worlds,
                side,
                Ruleset::CHAMPIONS_MC,
                &Heuristic,
                &knowledge,
                limits,
            )
            .unwrap();
            assert_eq!(
                format!("{:?}|{:?}", a.tree, a.stats),
                format!("{:?}|{:?}", b.tree, b.stats)
            );
            let counts = observer::counts();
            assert!(counts.advances > 0 && counts.key_requests > 0 && counts.key_formats > 0);
            assert!(counts.key_formats <= counts.key_requests);
            println!("S26E_HISTORY_ENGINE fixture={name} side={side:?} {counts:?}");
        }
        assert_eq!(worlds[0].position.state, before);
    }
}
#[test]
fn growth_wrappers_preserve_commits_and_errors_when_budgets_interrupt_expansion() {
    let _flat = FactoredScope::new(false);
    let worlds = worlds("psych-up-speed-swap");
    let before = worlds[0].position.state.clone();
    for side in [SideId::One, SideId::Two] {
        for (expansions, nodes, transitions) in [
            (1, 5000, 5000),
            (2, 5000, 5000),
            (50, 400, 5000),
            (50, 5000, 64),
            (50, 8, 5000),
            (50, 5000, 1),
        ] {
            let limits = builder::Limits {
                turns: 2,
                max_nodes: nodes,
                max_transitions: transitions,
                max_decisions: 16,
            };
            let settings = growing::reuse::Settings {
                growth: growing::Config {
                    max_expansions: expansions,
                    max_walks: 1000,
                    solver: bayesian::Config {
                        iterations: 32,
                        check_every: 16,
                        tolerance: 0.1,
                    },
                    ..Default::default()
                },
                storage: growing::reuse::Options {
                    in_place: true,
                    workspace: true,
                    compiler: true,
                    static_values: true,
                    direct_write: true,
                },
            };
            let knowledge = Knowledge::default();
            let a = tree::engine::growing_owned(
                &worlds,
                side,
                Ruleset::CHAMPIONS_MC,
                &Heuristic,
                &knowledge,
                limits,
                settings,
            );
            observer::reset();
            let b = tree::engine::growing_interned(
                &worlds,
                side,
                Ruleset::CHAMPIONS_MC,
                &Heuristic,
                &knowledge,
                limits,
                settings,
            );
            assert_eq!(result(a), result(b));
            if nodes == 5000 && transitions == 5000 {
                let c = observer::counts();
                assert!(c.advances > 0 && c.key_formats < c.key_requests);
            }
        }
    }
    assert_eq!(worlds[0].position.state, before);
}
