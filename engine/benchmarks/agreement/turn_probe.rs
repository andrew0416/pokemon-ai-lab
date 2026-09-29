//! Cross-revision turn agreement, with one JSONL record per position/outcome.
//! Inject unchanged as engine/scenario/examples/ci_agreement_turn.rs.
//! Usage: ci_agreement_turn SCENARIO.json --rolls median|extremes|fixed7|full
//!        [--setup-rolls median|extremes|fixed7|full] [--factored off|on]
//!        [--before REPORT_OR_BEFORE.json]
//!
//! No implicit choice repairs, sampling, outcome sorting, clocks, or search.
//! Default setup uses scenario_positions, exactly as lab-check does. A scenario's
//! setupRolls/startState/setupStates retain their existing loader semantics.
//! Structured engine errors finish with status=error and exit zero so a controller
//! can classify them; they are not successful accuracy cases. Panic/assertion,
//! invalid CLI, broken output, or a missing final complete record fail the process.
//! With explicit --before, schema 2 names a requested canonical-parent scope.
//! Every additional parent is still run with the original choice and recorded.
//! Only five fixture-specific, previously recorded choice-domain rejections may
//! occur outside the requested scope without failing it. Their errors stay in
//! the whole-stream hash and all-position counters; they are never successes.
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
    canonical_json_hidden, canonical_value, format_slots, load_scenario_str_as,
    run_decision_mid_turn_with, scenario_decision, scenario_positions, scenario_positions_with,
    state_from_canonical, ScenarioError, ScenarioMeta,
};
use serde_json::{json, Value};

#[derive(Debug)]
struct Config {
    scenario: PathBuf,
    rolls: RollMode,
    setup_rolls: Option<RollMode>,
    factored: bool,
    before: Option<PathBuf>,
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
    let mut before = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag {
            "--rolls" if rolls.is_none() => rolls = Some(parse_rolls(value)?),
            "--setup-rolls" if setup_rolls.is_none() => setup_rolls = Some(parse_rolls(value)?),
            "--before" if before.is_none() => before = Some(PathBuf::from(value)),
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
        before,
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
    scope: Option<Scope>,
    contract: Option<&str>,
) {
    let mut record = json!({"kind": "error", "stage": stage, "category": category,
        "message": stable_error(message, scenario), "position": position,
        "input_restored": restored});
    if scope.is_some() {
        record["expected_choice_rejection"] = json!(contract.is_some());
        record["contract"] = json!(contract);
    }
    emit(scoped(record, scope));
}

fn category(error: &ScenarioError) -> &'static str {
    if error.is_unsupported() {
        "unsupported"
    } else {
        "invalid"
    }
}

#[derive(Clone, Copy, Default)]
struct Counts {
    positions: usize,
    outcomes: usize,
    errors: usize,
    hidden_diagnostics: usize,
    enumerations_restored: usize,
    position_completions: usize,
    empty_positions: usize,
}

fn status(counts: &Counts, strict_empty: bool) -> &'static str {
    if counts.errors != 0 {
        "error"
    } else if counts.positions == 0
        || counts.outcomes == 0
        || (strict_empty && counts.empty_positions != 0)
    {
        "empty"
    } else {
        "ok"
    }
}

fn count_record(counts: &Counts) -> Value {
    json!({"status": status(counts, true), "positions": counts.positions,
        "outcomes": counts.outcomes, "errors": counts.errors,
        "hidden_diagnostics": counts.hidden_diagnostics,
        "enumerations_restored": counts.enumerations_restored,
        "outcome_rollbacks": counts.outcomes,
        "position_completions": counts.position_completions,
        "empty_positions": counts.empty_positions})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Requested = 0,
    Additional = 1,
    Global = 2,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Additional => "additional",
            Self::Global => "global",
        }
    }
}

fn scoped(mut record: Value, scope: Option<Scope>) -> Value {
    if let Some(scope) = scope {
        record["scope"] = json!(scope.name());
    }
    record
}

#[derive(Default)]
struct Ledger {
    scopes: [Counts; 3],
    expected_choice_rejections: usize,
}

/// The table is deliberately exact. ScenarioError erases the original TurnError
/// type, so treating every Invalid as an expected choice rejection would hide
/// unsupported new paths or malformed scenarios. No choice is rewritten here.
fn expected_choice_rejection<'a>(
    scenario: &'a Path,
    scope: Scope,
    stage: &str,
    category: &str,
    message: &str,
) -> Option<&'a str> {
    if scope != Scope::Additional || category != "invalid" {
        return None;
    }
    let stem = scenario.file_stem()?.to_str()?;
    let matches = match stem {
        "bb-choicelock-struggle" | "red-card-drag-update" => {
            stage == "decision" && message == "\"move harden\": empty slot"
        }
        "hyper-beam-recharge" | "u-truant-recharge" => {
            stage == "enumeration" && message == "One slot 0: target 0 for Hyper Beam (Normal)"
        }
        "nn-pressure-locked-outrage" => {
            stage == "enumeration" && message == "One slot 0: target 1 for Outrage (RandomNormal)"
        }
        _ => false,
    };
    matches.then_some(stem)
}

fn before_value(value: Value) -> Result<Value, String> {
    let before = value.get("before").cloned().unwrap_or(value);
    if !before.is_object()
        || before.get("schema") != Some(&json!(1))
        || !before.get("sides").is_some_and(Value::is_array)
        || !before.get("field").is_some_and(Value::is_object)
    {
        return Err(
            "--before requires a canonical state object or a report with one in before".into(),
        );
    }
    Ok(before)
}

fn complete_record(counts: &Counts, pinned_status: Value, ledger: Option<&Ledger>) -> Value {
    let mut record = json!({"kind": "complete", "schema_version": 1, "status": status(counts, false),
        "positions": counts.positions, "outcomes": counts.outcomes,
        "errors": counts.errors, "hidden_diagnostics": counts.hidden_diagnostics,
        "enumerations_restored": counts.enumerations_restored,
        "outcome_rollbacks": counts.outcomes, "pinned_status": pinned_status});
    if let Some(ledger) = ledger {
        let [requested, additional, global] = ledger.scopes;
        let unexpected = additional
            .errors
            .checked_sub(ledger.expected_choice_rejections)
            .expect("rejections are additional errors");
        let requested_status =
            if global.errors > 0 || unexpected > 0 || additional.empty_positions > 0 {
                "error"
            } else if requested.positions == 0 {
                "no-matching-parent"
            } else {
                status(&requested, true)
            };
        record["schema_version"] = json!(2);
        record["status"] = json!(requested_status);
        record["success_scope"] = json!("requested");
        record["all_positions_status"] = json!(status(counts, true));
        record["scopes"] = json!({"requested": count_record(&requested),
            "additional": count_record(&additional), "global": count_record(&global)});
        record["expected_choice_rejections"] = json!(ledger.expected_choice_rejections);
        record["unexpected_additional_errors"] = json!(unexpected);
        record["selection"] = json!({"mode": "canonical-before", "matched_parents": requested.positions,
            "additional_parents": additional.positions, "unclassified_parents": global.positions});
    }
    record
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

fn update_counts(
    counts: &mut Counts,
    ledger: &mut Option<Ledger>,
    scope: Scope,
    update: impl Fn(&mut Counts),
) {
    update(counts);
    if let Some(ledger) = ledger {
        update(&mut ledger.scopes[scope as usize]);
    }
}

fn run<const N: usize>(config: &Config, text: &str, before: Option<&Value>) {
    let mut counts = Counts::default();
    let mut ledger = before.map(|_| Ledger::default());
    let base = config.scenario.parent().unwrap_or(Path::new("."));
    let loaded = match load_scenario_str_as::<N>(text, base) {
        Ok(loaded) => loaded,
        Err(error) => {
            update_counts(&mut counts, &mut ledger, Scope::Global, |c| c.errors += 1);
            error_record(
                "load",
                "load-error",
                error,
                None,
                &config.scenario,
                None,
                before.map(|_| Scope::Global),
                None,
            );
            emit(complete_record(
                &counts,
                json!({"status": "not-loaded"}),
                ledger.as_ref(),
            ));
            return;
        }
    };
    let mut pinned_status = json!({"status": "setup-pending", "start_state": loaded.start_state.is_some(),
        "setup_state_pins": loaded.setup_states.iter().filter(|state| state.is_some()).count()});
    let mut loaded_record = json!({"kind": "loaded", "slots": N, "state": state_record(&loaded.state),
        "setup_turns": loaded.setup_turns.len(), "pinned_status": pinned_status,
        "scenario_setup_rolls": loaded.setup_rolls.map(|rolls| format!("{rolls:?}")),
        "recorded_mid_turn_choices": [loaded.mid_turn[0].len(), loaded.mid_turn[1].len()]});
    if let Some(before) = before {
        loaded_record["before"] = before.clone();
        loaded_record["recorded_turn_choices"] = loaded
            .meta
            .turn
            .as_ref()
            .map(|turn| json!({"p1": turn.p1, "p2": turn.p2}))
            .unwrap_or(Value::Null);
    }
    emit(loaded_record);
    let initial = loaded.state.clone();
    let positions = match config.setup_rolls {
        Some(rolls) => scenario_positions_with(&loaded, EnumerateOptions { rolls }),
        None => scenario_positions(&loaded),
    };
    assert_eq!(loaded.state, initial, "scenario setup input restoration");
    let positions = match positions {
        Ok(positions) => positions,
        Err(error) => {
            update_counts(&mut counts, &mut ledger, Scope::Global, |c| c.errors += 1);
            pinned_status["status"] = json!("setup-error");
            error_record(
                "setup",
                category(&error),
                &error,
                None,
                &config.scenario,
                Some(true),
                before.map(|_| Scope::Global),
                None,
            );
            emit(complete_record(&counts, pinned_status, ledger.as_ref()));
            return;
        }
    };
    pinned_status["status"] = json!(if positions.is_empty() {
        "empty"
    } else {
        "positions-produced"
    });
    for (index, position) in positions.into_iter().enumerate() {
        assert!(
            position.probability.is_finite() && position.probability >= 0.0,
            "setup probability"
        );
        let canonical = before.map(|_| canonical_value(&position.state, &loaded.meta));
        let (scope, canonical) = match canonical {
            Some(Ok(value)) => (
                if Some(&value) == before {
                    Scope::Requested
                } else {
                    Scope::Additional
                },
                Some(value),
            ),
            Some(Err(error)) => {
                update_counts(&mut counts, &mut ledger, Scope::Global, |c| {
                    c.positions += 1;
                    c.errors += 1;
                });
                emit(scoped(
                    json!({"kind": "position", "position": index,
                    "setup_probability_bits": position.probability.to_bits(), "state": state_record(&position.state),
                    "party_order": format!("{:?}", position.order), "decision": null,
                    "canonical_state": null}),
                    Some(Scope::Global),
                ));
                error_record(
                    "parent-canonical",
                    "canonical-error",
                    &error,
                    Some(index),
                    &config.scenario,
                    Some(true),
                    Some(Scope::Global),
                    None,
                );
                continue;
            }
            None => (Scope::Requested, None),
        };
        update_counts(&mut counts, &mut ledger, scope, |c| c.positions += 1);
        let decision = scenario_decision(&loaded, &position);
        let mut record = json!({"kind": "position", "position": index,
            "setup_probability_bits": position.probability.to_bits(), "state": state_record(&position.state),
            "party_order": format!("{:?}", position.order),
            "decision": decision.as_ref().ok().map(|decision| format!("{decision:?}"))});
        if let Some(value) = canonical {
            record["canonical_state"] = value;
        }
        emit(scoped(record, before.map(|_| scope)));
        let decision = match decision {
            Ok(decision) => decision,
            Err(error) => {
                update_counts(&mut counts, &mut ledger, scope, |c| c.errors += 1);
                let contract = before.and_then(|_| {
                    expected_choice_rejection(
                        &config.scenario,
                        scope,
                        "decision",
                        "invalid",
                        &error,
                    )
                });
                if contract.is_some() {
                    ledger.as_mut().unwrap().expected_choice_rejections += 1;
                }
                error_record(
                    "decision",
                    "invalid",
                    error,
                    Some(index),
                    &config.scenario,
                    Some(true),
                    before.map(|_| scope),
                    contract,
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
        update_counts(&mut counts, &mut ledger, scope, |c| {
            c.enumerations_restored += 1
        });
        let outcomes = match outcomes {
            Ok(outcomes) => outcomes,
            Err(error) => {
                update_counts(&mut counts, &mut ledger, scope, |c| c.errors += 1);
                let contract = before.and_then(|_| {
                    expected_choice_rejection(
                        &config.scenario,
                        scope,
                        "enumeration",
                        category(&error),
                        error.message(),
                    )
                });
                if contract.is_some() {
                    ledger.as_mut().unwrap().expected_choice_rejections += 1;
                }
                error_record(
                    "enumeration",
                    category(&error),
                    &error,
                    Some(index),
                    &config.scenario,
                    Some(true),
                    before.map(|_| scope),
                    contract,
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
            update_counts(&mut counts, &mut ledger, scope, |c| {
                c.hidden_diagnostics += usize::from(!hidden_ok)
            });
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
            update_counts(&mut counts, &mut ledger, scope, |c| c.outcomes += 1);
            emit(scoped(record, before.map(|_| scope)));
        }
        update_counts(&mut counts, &mut ledger, scope, |c| {
            c.position_completions += 1;
            c.empty_positions += usize::from(outcome_count == 0);
        });
        emit(scoped(
            json!({"kind": "position-complete", "position": index, "outcomes": outcome_count,
            "enumeration_restored": true, "all_outcomes_reversed": true}),
            before.map(|_| scope),
        ));
    }
    emit(complete_record(&counts, pinned_status, ledger.as_ref()));
}

fn early_failure(config: &Config, stage: &str, category: &str, message: impl std::fmt::Display) {
    let counts = Counts {
        errors: 1,
        ..Counts::default()
    };
    let ledger = config.before.as_ref().map(|_| {
        let mut ledger = Ledger::default();
        ledger.scopes[Scope::Global as usize].errors = 1;
        ledger
    });
    error_record(
        stage,
        category,
        message,
        None,
        &config.scenario,
        None,
        config.before.as_ref().map(|_| Scope::Global),
        None,
    );
    emit(complete_record(
        &counts,
        json!({"status": "not-loaded"}),
        ledger.as_ref(),
    ));
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ci_agreement_turn: {error}\nusage: ci_agreement_turn SCENARIO.json --rolls median|extremes|fixed7|full [--setup-rolls MODE] [--factored off|on] [--before REPORT_OR_BEFORE.json]");
            return ExitCode::from(2);
        }
    };
    let mut case = json!({"kind": "case", "schema_version": 1,
        "scenario": config.scenario.file_name().map(|name| name.to_string_lossy()),
        "rolls": format!("{:?}", config.rolls),
        "setup_rolls": config.setup_rolls.map(|rolls| format!("{rolls:?}")),
        "setup_default": "scenario_positions (scenario setupRolls/pins preserved)",
        "factored": config.factored, "error_path_redaction": "scenario and scenario directory only"});
    if let Some(path) = &config.before {
        case["schema_version"] = json!(2);
        case["before_source"] = json!(path.file_name().map(|name| name.to_string_lossy()));
        case["scope_policy"] = json!("oracle-before-with-additional-observations");
        case["additional_scope_contract"] = json!(
            "all original choices recorded; only listed fixture choice-domain rejections tolerated"
        );
    }
    emit(case);
    let text = match std::fs::read_to_string(&config.scenario) {
        Ok(text) => text,
        Err(error) => {
            early_failure(&config, "read", "io", error);
            return ExitCode::SUCCESS;
        }
    };
    let before = if let Some(path) = &config.before {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                early_failure(&config, "before-read", "io", error);
                return ExitCode::SUCCESS;
            }
        };
        match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
            .map_err(|error| error.to_string())
            .and_then(before_value)
        {
            Ok(value) => Some(value),
            Err(error) => {
                early_failure(&config, "before-decode", "invalid-selector", error);
                return ExitCode::SUCCESS;
            }
        }
    } else {
        None
    };
    let slots = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|value| value["format"].as_str().and_then(format_slots));
    verify_position_hashes(true);
    let _scope = FactoredScope::new(config.factored);
    // Match lab-check: explicit singles use N=1; the loader handles invalid or
    // unsupported formats rather than silently translating the input document.
    if slots == Some(1) {
        run::<1>(&config, &text, before.as_ref());
    } else {
        run::<2>(&config, &text, before.as_ref());
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
        assert!(config.before.is_none());
        let config = parse_args(&args(&[
            "fixture.json",
            "--rolls",
            "full",
            "--setup-rolls",
            "fixed7",
            "--factored",
            "on",
            "--before",
            "report.json",
        ]))
        .unwrap();
        assert!(config.setup_rolls.is_some());
        assert!(config.factored);
        assert_eq!(config.before, Some(PathBuf::from("report.json")));
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
            vec!["fixture.json", "--rolls", "full", "--before"],
            vec![
                "fixture.json",
                "--rolls",
                "full",
                "--before",
                "a",
                "--before",
                "b",
            ],
        ] {
            assert!(parse_args(&args(&input)).is_err(), "{input:?}");
        }
    }

    #[test]
    fn selector_accepts_original_report_before_or_raw_canonical_only() {
        let before = json!({"schema": 1, "field": {}, "sides": [{}, {}]});
        assert_eq!(before_value(before.clone()).unwrap(), before);
        assert_eq!(
            before_value(json!({"before": before, "outcomes": []})).unwrap(),
            before
        );
        for bad in [
            json!(null),
            json!([]),
            json!({}),
            json!({"before": null}),
            json!({"schema": 2, "field": {}, "sides": []}),
        ] {
            assert!(before_value(bad).is_err());
        }
    }

    #[test]
    fn old_completion_contract_stays_schema_one_without_a_selector() {
        let counts = Counts {
            positions: 2,
            outcomes: 3,
            errors: 1,
            enumerations_restored: 2,
            ..Counts::default()
        };
        assert_eq!(
            complete_record(&counts, json!({"status": "positions-produced"}), None),
            json!({"kind": "complete", "schema_version": 1, "status": "error",
                "positions": 2, "outcomes": 3, "errors": 1, "hidden_diagnostics": 0,
                "enumerations_restored": 2, "outcome_rollbacks": 3,
                "pinned_status": {"status": "positions-produced"}})
        );
    }

    #[test]
    fn known_rejection_is_not_a_pass_for_all_parents_or_for_requested_parent() {
        let requested = Counts {
            positions: 1,
            outcomes: 2,
            enumerations_restored: 1,
            position_completions: 1,
            ..Counts::default()
        };
        let additional = Counts {
            positions: 1,
            errors: 1,
            ..Counts::default()
        };
        let counts = Counts {
            positions: 2,
            outcomes: 2,
            errors: 1,
            enumerations_restored: 1,
            position_completions: 1,
            ..Counts::default()
        };
        let mut ledger = Ledger {
            scopes: [requested, additional, Counts::default()],
            expected_choice_rejections: 1,
        };
        let result = complete_record(&counts, json!({}), Some(&ledger));
        assert_eq!(result["status"], "ok");
        assert_eq!(result["success_scope"], "requested");
        assert_eq!(result["all_positions_status"], "error");
        assert_eq!(result["scopes"]["additional"]["errors"], 1);
        assert_eq!(result["expected_choice_rejections"], 1);
        ledger.scopes[Scope::Requested as usize].errors = 1;
        assert_eq!(
            complete_record(&counts, json!({}), Some(&ledger))["status"],
            "error"
        );
    }

    #[test]
    fn unrecognized_extra_error_empty_output_and_missing_parent_fail_closed() {
        let counts = Counts {
            positions: 1,
            outcomes: 1,
            ..Counts::default()
        };
        let mut ledger = Ledger::default();
        ledger.scopes[Scope::Requested as usize] = counts;
        ledger.scopes[Scope::Additional as usize].errors = 1;
        assert_eq!(
            complete_record(&counts, json!({}), Some(&ledger))["status"],
            "error"
        );
        ledger.scopes[Scope::Additional as usize] = Counts {
            empty_positions: 1,
            ..Counts::default()
        };
        assert_eq!(
            complete_record(&counts, json!({}), Some(&ledger))["status"],
            "error"
        );
        ledger = Ledger::default();
        assert_eq!(
            complete_record(&Counts::default(), json!({}), Some(&ledger))["status"],
            "no-matching-parent"
        );
        ledger.scopes[Scope::Global as usize].errors = 1;
        assert_eq!(
            complete_record(&counts, json!({}), Some(&ledger))["status"],
            "error"
        );
    }

    #[test]
    fn expected_error_table_requires_exact_fixture_scope_stage_kind_and_message() {
        let path = Path::new("hyper-beam-recharge.json");
        let message = "One slot 0: target 0 for Hyper Beam (Normal)";
        assert_eq!(
            expected_choice_rejection(path, Scope::Additional, "enumeration", "invalid", message),
            Some("hyper-beam-recharge")
        );
        for scope in [Scope::Requested, Scope::Global] {
            assert!(
                expected_choice_rejection(path, scope, "enumeration", "invalid", message).is_none()
            );
        }
        assert!(
            expected_choice_rejection(path, Scope::Additional, "setup", "invalid", message)
                .is_none()
        );
        assert!(expected_choice_rejection(
            path,
            Scope::Additional,
            "enumeration",
            "unsupported",
            message
        )
        .is_none());
        assert!(expected_choice_rejection(
            path,
            Scope::Additional,
            "enumeration",
            "invalid",
            "unexpected error"
        )
        .is_none());
        assert!(expected_choice_rejection(
            Path::new("other.json"),
            Scope::Additional,
            "enumeration",
            "invalid",
            message
        )
        .is_none());
    }

    /// The five reports really contain usable before objects. Keep every setup
    /// position and its verbatim recorded choice: match the report only for the
    /// requested contract, and retain the known invalid extra choices as errors.
    #[test]
    fn five_existing_fixtures_validate_requested_parents_and_count_extra_rejections() {
        let _flat = FactoredScope::new(false);
        let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        for (name, parent_count, rejected_count) in [
            ("bb-choicelock-struggle", 32, 1),
            ("red-card-drag-update", 74, 2),
            ("hyper-beam-recharge", 30, 1),
            ("u-truant-recharge", 23, 1),
            ("nn-pressure-locked-outrage", 59, 1),
        ] {
            let path = engine.join(format!("oracle/scenarios/{name}.json"));
            let report =
                std::fs::read_to_string(engine.join(format!("oracle/expected/{name}.turn.json")))
                    .unwrap();
            let before = before_value(serde_json::from_str(&report).unwrap()).unwrap();
            let loaded = lab_scenario::load_scenario_file(&path).unwrap();
            let positions = scenario_positions(&loaded).unwrap();
            assert_eq!(
                positions.len(),
                parent_count,
                "{name}: original setup population"
            );
            let mut requested = 0;
            let mut rejected = 0;
            for position in positions {
                let canonical = canonical_value(&position.state, &loaded.meta).unwrap();
                let scope = if canonical == before {
                    requested += 1;
                    Scope::Requested
                } else {
                    Scope::Additional
                };
                let decision = match scenario_decision(&loaded, &position) {
                    Ok(decision) => decision,
                    Err(error) => {
                        assert_eq!(
                            expected_choice_rejection(&path, scope, "decision", "invalid", &error),
                            Some(name),
                            "{name}: {error}"
                        );
                        rejected += 1;
                        continue;
                    }
                };
                let mut state = position.state.clone();
                let outcomes = run_decision_mid_turn_with(
                    &mut state,
                    &position.order,
                    &decision,
                    &loaded.mid_turn,
                    EnumerateOptions {
                        rolls: RollMode::Median,
                    },
                );
                assert_eq!(state, position.state, "{name}: input restoration");
                match outcomes {
                    Ok(outcomes) => {
                        assert!(!outcomes.is_empty(), "{name}: empty success");
                        for outcome in outcomes {
                            state.apply(&outcome.instructions);
                            state.reverse(&outcome.instructions);
                            assert_eq!(state, position.state, "{name}: rollback");
                        }
                    }
                    Err(error) => {
                        assert_eq!(
                            expected_choice_rejection(
                                &path,
                                scope,
                                "enumeration",
                                category(&error),
                                error.message()
                            ),
                            Some(name),
                            "{name}: {error}"
                        );
                        rejected += 1;
                    }
                }
            }
            assert!(requested > 0, "{name}: no requested parent");
            assert_eq!(
                rejected, rejected_count,
                "{name}: all extra rejections must stay visible"
            );
        }
    }
}
