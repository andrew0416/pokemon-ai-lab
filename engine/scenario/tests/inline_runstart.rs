//! P11 bounded differential probe. Run this unchanged test in separately compiled feature-OFF
//! and feature-ON binaries, writing LAB_P11_RECORDS to different paths, then compare bytes.
//! No reference selector or probe branch is added to the production turn engine.

use std::hash::{Hash, Hasher};
use std::io::Write;

use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::State;
use lab_engine::turn::{
    sample_turn, verify_position_hashes, EnumerateOptions, FactoredScope, RollMode,
};
use lab_scenario::{
    advance_order, load_scenario_file_as, run_decision_mid_turn_with, scenario_choices,
    scenario_decision, scenario_positions_with,
};
use serde_json::{json, Value};

/// Complete Hash input, not a lossy fingerprint: method, length and every payload byte are
/// retained. This includes hidden Slot/Volatile payloads; full Debug is recorded alongside it
/// for raw representation fields deliberately ignored by equality/hash (such as LazyTag).
#[derive(Default)]
struct TraceHasher(Vec<(&'static str, Vec<u8>)>);

macro_rules! trace_write {
    ($name:ident, $ty:ty) => {
        fn $name(&mut self, value: $ty) {
            self.0
                .push((stringify!($name), value.to_le_bytes().to_vec()));
        }
    };
}

impl Hasher for TraceHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, value: &[u8]) {
        self.0.push(("write", value.to_vec()));
    }
    trace_write!(write_u8, u8);
    trace_write!(write_u16, u16);
    trace_write!(write_u32, u32);
    trace_write!(write_u64, u64);
    trace_write!(write_u128, u128);
    trace_write!(write_usize, usize);
    trace_write!(write_i8, i8);
    trace_write!(write_i16, i16);
    trace_write!(write_i32, i32);
    trace_write!(write_i64, i64);
    trace_write!(write_i128, i128);
    trace_write!(write_isize, isize);
}

fn trace(value: &impl Hash) -> Value {
    let mut hasher = TraceHasher::default();
    value.hash(&mut hasher);
    json!(hasher.0)
}

fn outcome_record<const N: usize>(before: &State<N>, outcome: &Outcome) -> Value {
    assert!(outcome.probability.is_finite() && outcome.probability > 0.0);
    let mut state = before.clone();
    let mut hash = state.position_hash();
    for instruction in &outcome.instructions {
        hash = hash.wrapping_add(state.apply_hashed(instruction));
        assert_eq!(
            hash,
            state.position_hash(),
            "hash after each applied instruction"
        );
    }
    let end = state.clone();
    for instruction in outcome.instructions.iter().rev() {
        let old = state.instruction_hash(instruction);
        state.reverse_one(instruction);
        hash = hash.wrapping_add(state.instruction_hash(instruction).wrapping_sub(old));
        assert_eq!(
            hash,
            state.position_hash(),
            "hash after each reversed instruction"
        );
    }
    assert_eq!(
        &state, before,
        "complete caller State restored, including hidden fields"
    );
    json!({
        "probability_bits":outcome.probability.to_bits(),
        "instructions":trace(&outcome.instructions),
        "instructions_debug":format!("{:?}",outcome.instructions),
        "suspension":trace(&outcome.suspension),
        "suspension_debug":format!("{:?}",outcome.suspension),
        "end_state":trace(&end),
        "end_state_debug":format!("{end:?}"),
        "end_position_hash":end.position_hash(),
    })
}

#[derive(Default)]
struct Coverage {
    positions: usize,
    outcomes: usize,
    errors: usize,
    successes: usize,
    suspended: usize,
    resumed: usize,
    samples: usize,
}

fn run<const N: usize>(
    records: &mut Vec<Value>,
    coverage: &mut Coverage,
    name: &str,
    rolls: RollMode,
    factored: bool,
    resume: bool,
    sample: bool,
) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../oracle/scenarios/{name}.json"));
    let loaded = load_scenario_file_as::<N>(&path).unwrap();
    let _scope = FactoredScope::new(factored);
    let positions = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap();
    assert!(
        !positions.is_empty(),
        "{name}: fixture must supply a real position"
    );
    let empty_mid_turn = [Vec::new(), Vec::new()];
    let mid_turn = if resume {
        &loaded.mid_turn
    } else {
        &empty_mid_turn
    };
    for (position_index, position) in positions.iter().enumerate() {
        coverage.positions += 1;
        let decision = scenario_decision(&loaded, position).unwrap();
        let mut state = position.state.clone();
        let result = run_decision_mid_turn_with(
            &mut state,
            &position.order,
            &decision,
            mid_turn,
            EnumerateOptions { rolls },
        );
        assert_eq!(
            state, position.state,
            "{name}: caller restored on success or error"
        );
        let result = match result {
            Ok(outcomes) => {
                coverage.successes += 1;
                assert!(!outcomes.is_empty());
                let mass: f64 = outcomes.iter().map(|outcome| outcome.probability).sum();
                assert!(
                    (mass - 1.0).abs() < 1e-10,
                    "{name}: probability mass {mass}"
                );
                let outcomes: Vec<_> = outcomes.iter().map(|outcome| {
                    coverage.outcomes += 1;
                    coverage.suspended += usize::from(outcome.suspension.is_some());
                    if name == "uturn-switch" && resume {
                        assert!(outcome.suspension.is_none(), "recorded choices must resume U-turn");
                        coverage.resumed += 1;
                    }
                    let mut order = position.order.clone();
                    advance_order(&mut order, &outcome.instructions);
                    json!({"outcome":outcome_record(&position.state, outcome),"party_order":order})
                }).collect();
                json!({"ok":outcomes})
            }
            Err(error) => {
                coverage.errors += 1;
                json!({"error_debug":format!("{error:?}"),"error_display":error.to_string()})
            }
        };
        records.push(json!({
            "kind":"enumeration", "case":name,"slots":N,"position_index":position_index,
            "position_probability_bits":position.probability.to_bits(),
            "rolls":format!("{rolls:?}"),"factored":factored,"resume":resume,
            "before_state":trace(&position.state),"before_position_hash":position.state.position_hash(),
            "before_state_debug":format!("{:?}",position.state),
            "before_party_order":position.order,"decision":format!("{decision:?}"),"result":result,
        }));
        if sample {
            let choices = scenario_choices(&loaded, &position.state).unwrap();
            for seed in [0, 7, 42] {
                let mut state = position.state.clone();
                let outcomes =
                    sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, 4, seed).unwrap();
                assert_eq!(state, position.state);
                coverage.samples += 1;
                records.push(json!({
                    "kind":"sample","case":name,"slots":N,"position_index":position_index,
                    "seed":seed,"sample_count":4,"factored_scope":factored,
                    "outcomes":outcomes.iter().map(|outcome| outcome_record(&position.state,outcome)).collect::<Vec<_>>(),
                }));
            }
        }
    }
}

#[test]
fn bounded_runstart_records_preserve_full_logical_behavior() {
    verify_position_hashes(true);
    #[cfg(feature = "experiment-inline-runstart-observer")]
    lab_engine::turn::inline_runstart_observer::reset();
    let mut records = Vec::new();
    let mut coverage = Coverage::default();
    // In-action speed snapshots, subsequent action speeds, Trick Room/raw speed, forme/speed
    // changes, random priority, multihit resumes, and initial/setup stages.
    for name in [
        "single-hit",
        "f-trick-room-speed-wrap",
        "f-trick-room-raw-speed",
        "f-speed-snapshot-eject-pack",
        "electro-ball-gyro-ball",
        "o44-quickfeet",
        "stance-change-raw-speed",
        "speed-swap-switch",
        "aa-quick-draw",
        "double-hit",
        "after-you",
        "quash",
        "aa-power-construct-faint",
    ] {
        run::<2>(
            &mut records,
            &mut coverage,
            name,
            RollMode::Median,
            false,
            true,
            name == "single-hit",
        );
    }
    // One bounded Full distribution; do not turn spread damage into a giant Full workload.
    run::<2>(
        &mut records,
        &mut coverage,
        "single-hit",
        RollMode::Full,
        false,
        true,
        false,
    );
    for factored in [false, true] {
        for resume in [false, true] {
            run::<2>(
                &mut records,
                &mut coverage,
                "uturn-switch",
                RollMode::Median,
                factored,
                resume,
                false,
            );
        }
        run::<1>(
            &mut records,
            &mut coverage,
            "ae-singles-hit",
            RollMode::Median,
            factored,
            true,
            true,
        );
    }
    for name in ["single-hit", "f-speed-snapshot-eject-pack", "double-hit"] {
        run::<2>(
            &mut records,
            &mut coverage,
            name,
            RollMode::Median,
            true,
            true,
            false,
        );
    }
    assert!(
        coverage.errors > 0 && coverage.successes > coverage.errors,
        "real error and success paths required"
    );
    assert!(coverage.suspended > 0 && coverage.resumed > 0);
    assert!(coverage.samples >= 9);
    #[cfg(feature = "experiment-inline-runstart-observer")]
    {
        let counts = lab_engine::turn::inline_runstart_observer::counts();
        assert!(counts.captures > 0 && counts.inline_snapshots > 0);
        assert_eq!(
            counts.spilled_snapshots, 0,
            "selected fixtures had no spilled RunStart snapshots"
        );
        eprintln!("P11 activation: {counts:?}");
    }
    records.push(json!({
        "kind":"coverage","positions":coverage.positions,"outcomes":coverage.outcomes,
        "errors":coverage.errors,"successes":coverage.successes,"suspended":coverage.suspended,
        "resumed":coverage.resumed,"sample_calls":coverage.samples,
    }));
    if let Some(path) = std::env::var_os("LAB_P11_RECORDS") {
        let mut output = std::io::BufWriter::new(
            std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap(),
        );
        for record in &records {
            serde_json::to_writer(&mut output, record).unwrap();
            output.write_all(b"\n").unwrap();
        }
        output.flush().unwrap();
    }
    eprintln!("P11 deterministic records: {}", records.len());
    verify_position_hashes(false);
}
