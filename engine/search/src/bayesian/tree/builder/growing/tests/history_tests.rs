use super::*;
use std::cell::RefCell;
struct Traced {
    game: Game,
    calls: RefCell<Vec<String>>,
}
impl Traced {
    fn log(&self, p: &Position, operation: &str) {
        self.calls.borrow_mut().push(format!(
            "{operation}:{}:{}:{}:{}:{}",
            p.world, p.stage, p.own, p.other, p.signal
        ));
    }
}
impl Domain for Traced {
    type Position = Position;
    type Action = usize;
    fn phase(&self, p: &Position) -> Result<Phase, String> {
        self.log(p, "phase");
        self.game.phase(p)
    }
    fn actions(&self, p: &Position, side: usize) -> Result<Vec<usize>, String> {
        self.log(p, &format!("actions:{side}"));
        self.game.actions(p, side)
    }
    fn value(&self, p: &Position) -> f32 {
        self.log(p, "value");
        self.game.value(p)
    }
    fn transitions(&self, p: &Position, a: [&usize; 2]) -> Result<Vec<(f64, Position)>, String> {
        self.log(p, &format!("transition:{}:{}", a[0], a[1]));
        self.game.transitions(p, a)
    }
}
impl ObservedDomain for Traced {
    fn observation(&self, p: &Position) -> Result<Observation, String> {
        self.log(p, "observation");
        let mut obs = self.game.observation(p)?;
        obs.public.push_str("-메가\"\\\n]");
        obs.private = ["우리\0".into(), "상대\t".into()];
        Ok(obs)
    }
    fn action_id(&self, p: &Position, side: usize, a: &usize) -> String {
        self.log(p, &format!("action_id:{side}:{a}"));
        format!("new-mechanic-{a}-\"\\\t")
    }
}
#[derive(Default)]
struct TracedPrior(RefCell<Vec<String>>);
impl Prior for TracedPrior {
    fn weights(&self, information: &Information) -> Vec<f64> {
        self.0.borrow_mut().push(format!("{information:?}"));
        vec![1.; information.actions.len()]
    }
}

#[test]
fn encoded_history_preserves_every_callback_and_atomic_result() {
    for switch in [false, true] {
        for cap in [8, 25, 10000] {
            for budget in [1, 2, 99] {
                for all in [false, true] {
                    let make = || Traced {
                        game: Game {
                            switch,
                            ..Game::new()
                        },
                        calls: RefCell::default(),
                    };
                    let left = make();
                    let right = make();
                    let lp = TracedPrior::default();
                    let rp = TracedPrior::default();
                    let run = |domain: &Traced, prior: &TracedPrior, enabled| {
                        reuse::pipeline::search(
                            domain,
                            &seeds(),
                            Limits {
                                max_transitions: cap,
                                ..Limits::default()
                            },
                            Config {
                                solver: CfrConfig {
                                    iterations: 128,
                                    tolerance: 0.03,
                                    check_every: 32,
                                },
                                ..config(budget)
                            },
                            prior,
                            reuse::Options {
                                in_place: all,
                                ..Default::default()
                            },
                            4,
                            tree::paper::Settings {
                                sequence: true,
                                ..Default::default()
                            },
                            tree::pipeline::Settings {
                                frontier_index: all,
                                owned_compiler: all,
                                incremental_sequence: all,
                                compressed_checks: all,
                                encoded_history: enabled,
                            },
                        )
                    };
                    match (run(&left, &lp, false), run(&right, &rp, true)) {
                        (Ok(a), Ok(b)) => {
                            assert_eq!(
                                format!("{:?}", a.search.built.tree),
                                format!("{:?}", b.search.built.tree)
                            );
                            assert_eq!(
                                format!("{:?}", a.search.solution),
                                format!("{:?}", b.search.solution)
                            );
                            assert_eq!(
                                format!("{:?}", a.search.work),
                                format!("{:?}", b.search.work)
                            );
                            assert_eq!(
                                format!("{:?}", a.search.built.stats),
                                format!("{:?}", b.search.built.stats)
                            );
                            assert_eq!(format!("{:?}", a.stats), format!("{:?}", b.stats));
                            assert_eq!(format!("{:?}", a.metrics), format!("{:?}", b.metrics));
                            assert_eq!(a.search.stop, b.search.stop);
                            assert_eq!(
                                (
                                    a.search.frontier_histories,
                                    a.search.frontier_public_groups,
                                    a.search.horizon_complete
                                ),
                                (
                                    b.search.frontier_histories,
                                    b.search.frontier_public_groups,
                                    b.search.horizon_complete
                                )
                            );
                        }
                        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
                        _ => panic!("encoded history changed success/error behavior"),
                    }
                    assert_eq!(*left.calls.borrow(), *right.calls.borrow());
                    assert_eq!(*lp.0.borrow(), *rp.0.borrow());
                }
            }
        }
    }
}
