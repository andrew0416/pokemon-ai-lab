//! P8f: one presence snapshot per non-factored enumeration. Empty values enable stats.
//! Changes to LAB_ENGINE_STATS during a call take effect on the next enumeration.

use std::time::{Duration, Instant};

pub(super) struct StageStats {
    enabled: bool,
}

impl StageStats {
    pub(super) fn snapshot() -> Self {
        #[cfg(feature = "experiment-stats-off-cost-observer")]
        observer::record(|counts| counts.env_reads += 1);
        Self {
            enabled: std::env::var_os("LAB_ENGINE_STATS").is_some(),
        }
    }

    pub(super) fn start(&self) -> Option<Instant> {
        self.enabled.then(|| {
            #[cfg(feature = "experiment-stats-off-cost-observer")]
            observer::record(|counts| counts.clock_starts += 1);
            Instant::now()
        })
    }

    pub(super) fn finish(&self, started: Option<Instant>) -> Option<Duration> {
        started.map(|started| {
            #[cfg(feature = "experiment-stats-off-cost-observer")]
            observer::record(|counts| counts.clock_elapsed += 1);
            started.elapsed()
        })
    }
}

/// Isolated mechanism proof. Absent from ordinary P8f timing builds.
#[cfg(feature = "experiment-stats-off-cost-observer")]
pub mod observer {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub env_reads: usize,
        pub clock_starts: usize,
        pub clock_elapsed: usize,
    }

    thread_local! {
        static COUNTS: Cell<Counts> = Cell::new(Counts::default());
    }

    pub fn reset() {
        COUNTS.with(|counts| counts.set(Counts::default()));
    }

    pub fn counts() -> Counts {
        COUNTS.with(Cell::get)
    }

    pub(super) fn record(update: impl FnOnce(&mut Counts)) {
        COUNTS.with(|cell| {
            let mut counts = cell.get();
            update(&mut counts);
            cell.set(counts);
        });
    }
}

#[cfg(all(test, feature = "experiment-stats-off-cost-observer"))]
mod tests {
    use super::observer::{self, Counts};
    use crate::instruction::Instruction;
    use crate::state::State;
    use crate::turn::{enumerate_stages, EnumerateOptions, FactoredScope, StageEnd, TurnError};
    use std::process::Command;

    const CHILD: &str = "LAB_P8F_TEST_CHILD";
    const TEST_NAME: &str = "turn::stats_off_cost::tests::subprocess_probe";

    fn run<const N: usize>(factored: bool, mutate_env: bool) {
        let _scope = FactoredScope::new(factored);
        let mut state = State::<N>::default();
        let original = state.clone();
        let endings = enumerate_stages(&mut state, 0u8, EnumerateOptions::default(), |b, step| {
            if mutate_env && *step == 0 {
                // This child is launched with --test-threads=1; no other work shares its env.
                std::env::remove_var("LAB_ENGINE_STATS");
            }
            let old = b.state.turn;
            b.apply(Instruction::SetTurn { old, new: old + 1 });
            *step += 1;
            Ok(if *step == 3 {
                StageEnd::Finished
            } else {
                StageEnd::Continue
            })
        })
        .unwrap();
        assert_eq!(state, original);
        let result: Vec<_> = endings.into_iter().flatten().collect();
        assert_eq!(result.len(), 1);
        let (end, pending, probability, hash) = &result[0];
        let mut expected = original;
        expected.turn = 3;
        assert_eq!(*end, expected);
        assert_eq!(*pending, None);
        assert_eq!(*probability, 1.0);
        assert_eq!(*hash, end.position_hash());
    }

    #[test]
    fn subprocess_probe() {
        let Ok(mode) = std::env::var(CHILD) else {
            return;
        };
        observer::reset();
        match mode.as_str() {
            "off" => {
                run::<1>(false, false);
                assert_eq!(
                    observer::counts(),
                    Counts {
                        env_reads: 1,
                        clock_starts: 0,
                        clock_elapsed: 0
                    }
                );
            }
            "on" | "empty" => {
                run::<2>(false, false);
                assert_eq!(
                    observer::counts(),
                    Counts {
                        env_reads: 1,
                        clock_starts: 3,
                        clock_elapsed: 3
                    }
                );
            }
            "refresh" => {
                run::<1>(false, true);
                run::<2>(false, false);
                std::env::set_var("LAB_ENGINE_STATS", "");
                run::<1>(false, false);
                assert_eq!(
                    observer::counts(),
                    Counts {
                        env_reads: 3,
                        clock_starts: 6,
                        clock_elapsed: 6
                    }
                );
            }
            "factored" => {
                run::<2>(true, false);
                assert_eq!(observer::counts(), Counts::default());
            }
            "error" => {
                let _scope = FactoredScope::new(false);
                let mut state = State::<1>::default();
                let original = state.clone();
                let result =
                    enumerate_stages(&mut state, (), EnumerateOptions::default(), |_b, _step| {
                        Err(TurnError::Unsupported("p8f-test".into()))
                    });
                assert_eq!(result.unwrap_err().to_string(), "not implemented: p8f-test");
                assert_eq!(state, original);
                assert_eq!(
                    observer::counts(),
                    Counts {
                        env_reads: 1,
                        clock_starts: 1,
                        clock_elapsed: 0
                    }
                );
            }
            _ => panic!("unknown child mode"),
        }
    }

    #[test]
    fn env_presence_snapshot_and_diagnostics_are_process_isolated() {
        for (mode, value, expected_lines) in [
            ("off", None, 0),
            ("on", Some("1"), 3),
            ("empty", Some(""), 3),
            ("refresh", Some("1"), 6),
            ("factored", Some("1"), 0),
            ("error", Some("1"), 0),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
                .env(CHILD, mode)
                .env_remove("LAB_ENGINE_STATS")
                .env_remove("LAB_ENGINE_FACTORED");
            if let Some(value) = value {
                child.env("LAB_ENGINE_STATS", value);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stderr = String::from_utf8(output.stderr).unwrap();
            let lines: Vec<_> = stderr
                .lines()
                .filter(|line| line.starts_with("lab-engine: stage "))
                .collect();
            assert_eq!(lines.len(), expected_lines, "{mode}: {stderr}");
            for (index, line) in lines.iter().enumerate() {
                let prefix = if index % 3 == 2 {
                    "lab-engine: stage frontier 0 states, 1 finished; 1 replays in "
                } else {
                    "lab-engine: stage frontier 1 states, 0 finished; 1 replays in "
                };
                let elapsed = line
                    .strip_prefix(prefix)
                    .unwrap()
                    .strip_suffix(" ms")
                    .unwrap();
                assert!(elapsed.parse::<f64>().unwrap() >= 0.0);
                assert_eq!(elapsed.split('.').nth(1).unwrap().len(), 1);
            }
        }
    }
}
