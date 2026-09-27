//! Turn throughput on real positions (Opus GG unit GG-turn-throughput): what one turn of the
//! engine costs, per position, for the search's hot calls.
//!
//! Positions: the committed library lead turns (`oracle/scenarios/cc-lib-*.json`, six library
//! teams' turn 1, VGC Reg M-C) and three hand scenarios with spread moves (Rock Slide, Heat
//! Wave, Earthquake). The joint action measured is each scenario's own `turn`. Metrics:
//!
//! - `legal`: `legal_joint_actions` for both sides (counts p1 x p2, time for the pair of calls);
//! - `clone`: one `State<2>` clone;
//! - `sample`: `sample_turn(.., 1, seed)` per call with a fresh seed each time (what
//!   `lab-rollout` does per turn), checks and instruction diff included;
//! - `median` / `extremes` / `full`: `enumerate_turn_with` at that `RollMode`, with the outcome
//!   count. `spread-damage`'s exact distribution does not fit in memory (WORKPLAN F18), so its
//!   `full` column is skipped.
//!
//! Usage: `cargo bench -p lab-scenario --bench turn [-- <name filter>...]`. Each metric gets
//! one warm-up call and then five batches of calls, each at least a fifth of `LAB_BENCH_MS`
//! milliseconds (default 500), and prints the fastest batch's mean. Numbers and the machine they
//! come from: `benches/README.md`. `LAB_BENCH_METRICS=median,sample` (a comma list of the
//! column names `legal`, `clone`, `sample`, `median`, `extremes`, `full`) runs only those, e.g.
//! under a sampling profiler.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{
    enumerate_turn_with, legal_joint_actions, sample_turn, EnumerateOptions, RollMode,
};
use lab_engine::Doubles;
use lab_scenario::{load_scenario_file, scenario_decision, scenario_positions, Decision};

struct Case {
    scenario: &'static str,
    position: usize,
    full: bool,
}

const CASES: &[Case] = &[
    Case {
        scenario: "cc-lib-sand-owen-vs-coaching-panda",
        position: 0,
        full: true,
    },
    Case {
        scenario: "cc-lib-coaching-panda-vs-psy-sand-udon",
        position: 0,
        full: true,
    },
    Case {
        scenario: "cc-lib-psy-cona-vs-sand-owen",
        position: 0,
        full: true,
    },
    Case {
        scenario: "cc-lib-psy-sand-udon-vs-balance-ddee",
        position: 0,
        full: true,
    },
    Case {
        scenario: "cc-lib-balance-ddee-vs-crown-cecil9",
        position: 0,
        full: true,
    },
    Case {
        scenario: "cc-lib-crown-cecil9-vs-perish-mrada",
        position: 0,
        full: true,
    },
    // Rock Slide + Hyper Voice + two single-target attacks, no protection.
    Case {
        scenario: "spread-damage",
        position: 0,
        full: false,
    },
    // Heat Wave into Flash Fire.
    Case {
        scenario: "o48-flash-fire-spread",
        position: 0,
        full: true,
    },
    // Rock Slide + Earthquake into Wide Guard.
    Case {
        scenario: "o21-wide-guard",
        position: 0,
        full: true,
    },
];

/// Batches per metric; the fastest batch's mean is reported, which keeps other load on the
/// machine (a compile in another shell) out of the numbers as far as possible.
const BATCHES: u32 = 5;

/// Time per call of `f`: one warm-up call, then [`BATCHES`] batches of calls, each until a
/// fifth of `budget` has passed; the smallest batch mean.
fn time<T>(budget: Duration, mut f: impl FnMut() -> T) -> Duration {
    black_box(f());
    let batch = budget / BATCHES;
    let mut best = Duration::MAX;
    for _ in 0..BATCHES {
        let started = Instant::now();
        let mut calls = 0u32;
        loop {
            black_box(f());
            calls += 1;
            let elapsed = started.elapsed();
            if elapsed >= batch {
                best = best.min(elapsed / calls);
                break;
            }
        }
    }
    best
}

fn micros(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1e6)
}

fn millis(d: Duration) -> String {
    format!("{:.3}", d.as_secs_f64() * 1e3)
}

fn main() {
    // `cargo bench` passes `--bench`; everything else is a name filter.
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let budget = Duration::from_millis(
        std::env::var("LAB_BENCH_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(500),
    );
    // `LAB_BENCH_METRICS=median,sample` runs only those columns (for a profiler).
    let metrics = std::env::var("LAB_BENCH_METRICS").ok();
    let wanted = |metric: &str| {
        metrics
            .as_deref()
            .is_none_or(|list| list.split(',').any(|m| m.trim() == metric))
    };
    let scenarios = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios");
    if cfg!(debug_assertions) {
        println!("warning: debug assertions are on; numbers are not release numbers");
    }
    println!(
        "{:<40} {:>9} {:>9} {:>8} {:>10} {:>15} {:>15} {:>17}",
        "position",
        "legal",
        "legal us",
        "clone ns",
        "sample us",
        "median ms (n)",
        "extremes ms (n)",
        "full ms (n)"
    );
    let mut checksum = 0u64;
    for case in CASES {
        if !filters.is_empty() && !filters.iter().any(|f| case.scenario.contains(f.as_str())) {
            continue;
        }
        let loaded = load_scenario_file(scenarios.join(format!("{}.json", case.scenario)))
            .unwrap_or_else(|e| panic!("{}: {e}", case.scenario));
        let positions = scenario_positions(&loaded).expect("positions");
        let position = &positions[case.position];
        let Decision::Turn(choices) = scenario_decision(&loaded, position).expect("decision")
        else {
            panic!("{}: not a turn decision", case.scenario);
        };
        let mut state: Doubles = position.state.clone();
        let ruleset = Ruleset::CHAMPIONS_MC;

        let counts =
            [SideId::One, SideId::Two].map(|side| legal_joint_actions(&state, ruleset, side).len());
        let skipped = || "-".to_string();
        let legal = if wanted("legal") {
            micros(time(budget, || {
                legal_joint_actions(&state, ruleset, SideId::One).len()
                    + legal_joint_actions(&state, ruleset, SideId::Two).len()
            }))
        } else {
            skipped()
        };
        let clone = if wanted("clone") {
            format!(
                "{:.0}",
                time(budget, || black_box(&state).clone()).as_secs_f64() * 1e9
            )
        } else {
            skipped()
        };
        let sample = if wanted("sample") {
            let mut seed = 0u64;
            micros(time(budget, || {
                seed += 1;
                let outcomes = sample_turn(&mut state, ruleset, choices, 1, seed).expect("sample");
                checksum = checksum.wrapping_add(outcomes[0].instructions.len() as u64);
            }))
        } else {
            skipped()
        };
        let mut enumerate = |metric: &str, rolls: RollMode| {
            if !wanted(metric) {
                return skipped();
            }
            let options = EnumerateOptions { rolls };
            let mut count = 0usize;
            let per_call = time(budget, || {
                let outcomes =
                    enumerate_turn_with(&mut state, ruleset, choices, options).expect("enumerate");
                count = outcomes.len();
            });
            checksum = checksum.wrapping_add(count as u64);
            format!("{} ({count})", millis(per_call))
        };
        let median = enumerate("median", RollMode::Median);
        let extremes = enumerate("extremes", RollMode::Extremes);
        let full = if case.full {
            enumerate("full", RollMode::Full)
        } else {
            "skipped".to_string()
        };
        assert_eq!(state, position.state, "the state is left unchanged");
        println!(
            "{:<40} {:>9} {:>9} {:>8} {:>10} {:>15} {:>15} {:>17}",
            format!("{}#{}", case.scenario, case.position),
            format!("{}x{}", counts[0], counts[1]),
            legal,
            clone,
            sample,
            median,
            extremes,
            full
        );
    }
    println!("checksum: {checksum}");
}
