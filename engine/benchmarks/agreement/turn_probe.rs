//! Cross-revision turn agreement, with one JSONL record per position/outcome.
//! Inject unchanged as engine/scenario/examples/ci_agreement_turn.rs.
//! Usage: ci_agreement_turn SCENARIO.json --rolls median|extremes|fixed7|full
//!        [--setup-rolls median|extremes|fixed7|full] [--factored off|on]
//!
//! No implicit choice repairs, sampling, outcome sorting, clocks, or search.
//! Default setup uses scenario_positions, exactly as lab-check does. A scenario's
//! setupRolls/startState/setupStates retain their existing loader semantics.
//! Structured engine errors finish with status=error and exit zero so a controller
//! can classify them; they are not successful accuracy cases. Panic/assertion,
//! invalid CLI, broken output, or a missing final complete record fail the process.
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lab_engine::hash::KeyHasher;
use lab_engine::instruction::Instruction;
use lab_engine::state::State;
use lab_engine::turn::{verify_position_hashes, EnumerateOptions, FactoredScope, RollMode};
use lab_scenario::decision::{advance_order, PartyOrder};
use lab_scenario::{
    canonical_json_hidden, format_slots, load_scenario_str_as, run_decision_mid_turn_with,
    scenario_decision, scenario_positions, scenario_positions_with, state_from_canonical,
    ScenarioError, ScenarioMeta,
};
use serde_json::{json, Value};

#[derive(Debug)]
struct Config {
    scenario: PathBuf,
    rolls: RollMode,
    setup_rolls: Option<RollMode>,
    factored: bool,
}

fn parse_rolls(value: &str) -> Result<RollMode, String> {
    match value {
        "median" => Ok(RollMode::Median),
        "extremes" => Ok(RollMode::Extremes),
        "fixed7" => Ok(RollMode::fixed(7).expect("fixed damage roll 7 exists")),
        "full" => Ok(RollMode::Full),
        _ => Err(format!("unsupported roll mode {value:?}")),
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let Some(scenario) = args.first().filter(|arg| !arg.starts_with("--")) else {
        return Err("scenario JSON path must be the first argument".into());
    };
    let mut rolls = None;
    let mut setup_rolls = None;
    let mut factored = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag {
            "--rolls" if rolls.is_none() => rolls = Some(parse_rolls(value)?),
            "--setup-rolls" if setup_rolls.is_none() => setup_rolls = Some(parse_rolls(value)?),
            "--factored" if factored.is_none() => {
                factored = Some(match value.as_str() {
                    "off" => false,
                    "on" => true,
                    _ => return Err("--factored must be off or on".into()),
                });
            }
            _ => return Err(format!("unknown or duplicate option {flag:?}")),
        }
        index += 2;
    }
    Ok(Config {
        scenario: PathBuf::from(scenario),
        rolls: rolls.ok_or("explicit --rolls is required")?,
        setup_rolls,
        factored: factored.unwrap_or(false),
    })
}

fn emit(record: Value) {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &record).expect("JSONL output");
    out.write_all(b"\n").expect("JSONL newline");
    out.flush().expect("JSONL flush");
}

/// Only input-location text is redacted; rule/choice/error details are preserved.
fn stable_error(message: impl std::fmt::Display, scenario: &Path) -> String {
    let mut text = message.to_string();
    let path = scenario.to_string_lossy();
    if !path.is_empty() {
        text = text.replace(path.as_ref(), "<scenario>");
    }
    if let Some(parent) = scenario
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        let parent = parent.to_string_lossy();
        // Do not replace every slash when a scenario lives directly in /.
        if parent.len() > 1 {
            text = text.replace(parent.as_ref(), "<scenario-dir>");
        }
    }
    text
}

fn state_record<const N: usize>(state: &State<N>) -> Value {
    let mut key = KeyHasher::new();
    state.hash(&mut key);
    // The full logical Debug is retained, so agreement does not rely on a 64-bit
    // hash collision assumption. P10 compact Volatiles explicitly prints the
    // same dense tuple Debug as the original representation.
    json!({"debug": format!("{state:?}"), "key_hash": key.finish(),
           "position_hash": state.position_hash()})
}

fn error_record(
    stage: &str,
    category: &str,
    message: impl std::fmt::Display,
    position: Option<usize>,
    scenario: &Path,
    restored: Option<bool>,
) {
    emit(
        json!({"kind": "error", "stage": stage, "category": category,
        "message": stable_error(message, scenario), "position": position,
        "input_restored": restored}),
    );
}

fn category(error: &ScenarioError) -> &'static str {
    if error.is_unsupported() {
        "unsupported"
    } else {
        "invalid"
    }
}

#[derive(Default)]
struct Counts {
    positions: usize,
    outcomes: usize,
    errors: usize,
    hidden_diagnostics: usize,
    enumerations_restored: usize,
}

fn complete(counts: &Counts, pinned_status: Value) {
    let status = if counts.errors != 0 {
        "error"
    } else if counts.positions == 0 || counts.outcomes == 0 {
        "empty"
    } else {
        "ok"
    };
    emit(
        json!({"kind": "complete", "schema_version": 1, "status": status,
        "positions": counts.positions, "outcomes": counts.outcomes,
        "errors": counts.errors, "hidden_diagnostics": counts.hidden_diagnostics,
        "enumerations_restored": counts.enumerations_restored,
        "outcome_rollbacks": counts.outcomes, "pinned_status": pinned_status}),
    );
}

/// Codec limitations are diagnostics. Full logical State output and mandatory
/// apply/reverse restoration remain available even if this codec rejects a state.
fn hidden_record<const N: usize>(
    state: &State<N>,
    meta: &ScenarioMeta,
    order: &[PartyOrder; 2],
    scenario: &Path,
) -> (Value, bool) {
    let text = match canonical_json_hidden(state, meta, Some(order)) {
        Ok(text) => text,
        Err(error) => {
            return (
                json!({"status": "encode-error", "category": category(&error),
            "message": stable_error(&error, scenario), "canonical_hidden": null}),
                false,
            )
        }
    };
    let value: Value =
        serde_json::from_str(&text).expect("canonical_json_hidden returns valid JSON");
    match state_from_canonical(state, meta, &value) {
        Ok(rebuilt) => {
            let state_equal = rebuilt.state == *state;
            let order_equal = rebuilt.order == *order;
            let agrees = state_equal && order_equal;
            (
                json!({"status": if agrees { "ok" } else { "roundtrip-mismatch" },
                "canonical_hidden": text, "state_equal": state_equal, "order_equal": order_equal,
                "rebuilt_state": if agrees { Value::Null } else { state_record(&rebuilt.state) },
                "rebuilt_order": if agrees { None } else { Some(format!("{:?}", rebuilt.order)) }}),
                agrees,
            )
        }
        Err(error) => (
            json!({"status": "decode-error", "category": category(&error),
            "message": stable_error(&error, scenario), "canonical_hidden": text}),
            false,
        ),
    }
}

fn apply_checked<const N: usize>(state: &mut State<N>, instructions: &[Instruction]) {
    for instruction in instructions {
        let before = state.position_hash();
        let old_cell = state.instruction_hash(instruction);
        state.apply_one(instruction);
        assert_eq!(
            state.position_hash(),
            before
                .wrapping_sub(old_cell)
                .wrapping_add(state.instruction_hash(instruction)),
            "apply incremental position hash"
        );
    }
}

fn reverse_checked<const N: usize>(state: &mut State<N>, instructions: &[Instruction]) {
    for instruction in instructions.iter().rev() {
        let before = state.position_hash();
        let old_cell = state.instruction_hash(instruction);
        state.reverse_one(instruction);
        assert_eq!(
            state.position_hash(),
            before
                .wrapping_sub(old_cell)
                .wrapping_add(state.instruction_hash(instruction)),
            "reverse incremental position hash"
        );
    }
}

fn run<const N: usize>(config: &Config, text: &str) {
    let mut counts = Counts::default();
    let base = config.scenario.parent().unwrap_or(Path::new("."));
    let loaded = match load_scenario_str_as::<N>(text, base) {
        Ok(loaded) => loaded,
        Err(error) => {
            counts.errors += 1;
            error_record("load", "load-error", error, None, &config.scenario, None);
            complete(&counts, json!({"status": "not-loaded"}));
            return;
        }
    };
    let mut pinned_status = json!({"status": "setup-pending", "start_state": loaded.start_state.is_some(),
        "setup_state_pins": loaded.setup_states.iter().filter(|state| state.is_some()).count()});
    emit(
        json!({"kind": "loaded", "slots": N, "state": state_record(&loaded.state),
        "setup_turns": loaded.setup_turns.len(), "pinned_status": pinned_status,
        "scenario_setup_rolls": loaded.setup_rolls.map(|rolls| format!("{rolls:?}")),
        "recorded_mid_turn_choices": [loaded.mid_turn[0].len(), loaded.mid_turn[1].len()]}),
    );
    let initial = loaded.state.clone();
    let positions = match config.setup_rolls {
        Some(rolls) => scenario_positions_with(&loaded, EnumerateOptions { rolls }),
        None => scenario_positions(&loaded),
    };
    assert_eq!(loaded.state, initial, "scenario setup input restoration");
    let positions = match positions {
        Ok(positions) => positions,
        Err(error) => {
            counts.errors += 1;
            pinned_status["status"] = json!("setup-error");
            error_record(
                "setup",
                category(&error),
                &error,
                None,
                &config.scenario,
                Some(true),
            );
            complete(&counts, pinned_status);
            return;
        }
    };
    pinned_status["status"] = json!(if positions.is_empty() {
        "empty"
    } else {
        "positions-produced"
    });
    for (index, position) in positions.into_iter().enumerate() {
        counts.positions += 1;
        assert!(
            position.probability.is_finite() && position.probability >= 0.0,
            "setup probability"
        );
        let decision = scenario_decision(&loaded, &position);
        emit(json!({"kind": "position", "position": index,
            "setup_probability_bits": position.probability.to_bits(), "state": state_record(&position.state),
            "party_order": format!("{:?}", position.order),
            "decision": decision.as_ref().ok().map(|decision| format!("{decision:?}"))}));
        let decision = match decision {
            Ok(decision) => decision,
            Err(error) => {
                counts.errors += 1;
                error_record(
                    "decision",
                    "invalid",
                    error,
                    Some(index),
                    &config.scenario,
                    Some(true),
                );
                continue;
            }
        };
        let mut work = position.state.clone();
        let outcomes = run_decision_mid_turn_with(
            &mut work,
            &position.order,
            &decision,
            &loaded.mid_turn,
            EnumerateOptions {
                rolls: config.rolls,
            },
        );
        assert_eq!(
            work, position.state,
            "decision enumeration input restoration"
        );
        counts.enumerations_restored += 1;
        let outcomes = match outcomes {
            Ok(outcomes) => outcomes,
            Err(error) => {
                counts.errors += 1;
                error_record(
                    "enumeration",
                    category(&error),
                    &error,
                    Some(index),
                    &config.scenario,
                    Some(true),
                );
                continue;
            }
        };
        let outcome_count = outcomes.len();
        for (outcome_index, outcome) in outcomes.into_iter().enumerate() {
            assert!(
                outcome.probability.is_finite() && outcome.probability >= 0.0,
                "outcome probability"
            );
            apply_checked(&mut work, &outcome.instructions);
            let mut order = position.order.clone();
            advance_order(&mut order, &outcome.instructions);
            let (hidden, hidden_ok) = hidden_record(&work, &loaded.meta, &order, &config.scenario);
            counts.hidden_diagnostics += usize::from(!hidden_ok);
            let record = json!({"kind": "outcome", "position": index, "outcome": outcome_index,
                "probability_bits": outcome.probability.to_bits(),
                "instructions": format!("{:?}", outcome.instructions),
                "suspension": format!("{:?}", outcome.suspension),
                "state": state_record(&work), "party_order": format!("{order:?}"),
                "hidden": hidden, "input_restored": true, "incremental_hash_checked": true});
            reverse_checked(&mut work, &outcome.instructions);
            assert_eq!(
                work, position.state,
                "outcome reverse full-State restoration"
            );
            counts.outcomes += 1;
            emit(record);
        }
        emit(
            json!({"kind": "position-complete", "position": index, "outcomes": outcome_count,
            "enumeration_restored": true, "all_outcomes_reversed": true}),
        );
    }
    complete(&counts, pinned_status);
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ci_agreement_turn: {error}\nusage: ci_agreement_turn SCENARIO.json --rolls median|extremes|fixed7|full [--setup-rolls MODE] [--factored off|on]");
            return ExitCode::from(2);
        }
    };
    emit(json!({"kind": "case", "schema_version": 1,
        "scenario": config.scenario.file_name().map(|name| name.to_string_lossy()),
        "rolls": format!("{:?}", config.rolls),
        "setup_rolls": config.setup_rolls.map(|rolls| format!("{rolls:?}")),
        "setup_default": "scenario_positions (scenario setupRolls/pins preserved)",
        "factored": config.factored, "error_path_redaction": "scenario and scenario directory only"}));
    let text = match std::fs::read_to_string(&config.scenario) {
        Ok(text) => text,
        Err(error) => {
            error_record("read", "io", error, None, &config.scenario, None);
            complete(
                &Counts {
                    errors: 1,
                    ..Counts::default()
                },
                json!({"status": "not-loaded"}),
            );
            return ExitCode::SUCCESS;
        }
    };
    let slots = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|value| value["format"].as_str().and_then(format_slots));
    verify_position_hashes(true);
    let _scope = FactoredScope::new(config.factored);
    // Match lab-check: explicit singles use N=1; the loader handles invalid or
    // unsupported formats rather than silently translating the input document.
    if slots == Some(1) {
        run::<1>(&config, &text);
    } else {
        run::<2>(&config, &text);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn decision_rolls_do_not_override_default_setup() {
        let config = parse_args(&args(&["fixture.json", "--rolls", "median"])).unwrap();
        assert!(config.setup_rolls.is_none());
        assert!(!config.factored);
        let config = parse_args(&args(&[
            "fixture.json",
            "--rolls",
            "full",
            "--setup-rolls",
            "fixed7",
            "--factored",
            "on",
        ]))
        .unwrap();
        assert!(config.setup_rolls.is_some());
        assert!(config.factored);
    }

    #[test]
    fn incomplete_unknown_and_duplicate_options_fail_closed() {
        for input in [
            vec!["fixture.json"],
            vec!["fixture.json", "--rolls"],
            vec!["fixture.json", "--rolls", "fixed16"],
            vec!["fixture.json", "--rolls", "full", "--rolls", "median"],
            vec!["fixture.json", "--rolls", "full", "--factored", "auto"],
            vec!["fixture.json", "--rolls", "full", "--unknown", "value"],
        ] {
            assert!(parse_args(&args(&input)).is_err(), "{input:?}");
        }
    }
}
