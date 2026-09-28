//! Checks lab-engine against one `enumerate.cjs` report of a scenario: the engine replays the
//! scenario to the report's `before` state, enumerates the decision in the report's roll mode
//! (`full` → exact, `extremes` → min/max rolls, `fixed` with the report's `roll` k → every
//! damage roll at index k, `RollMode::Fixed(k)`) and compares the canonical outcome
//! distributions exactly (the comparator of the fixture tests, `lab_scenario::parity`).
//!
//! Usage: lab-check <scenario.json> <oracle-report.json> [--out verdict.json] [--tolerance p]
//!                  [--hash-keys]
//!        lab-check --batch <jobs.jsonl | -> [--jobs n] [--tolerance p] [--timeout s] [--hash-keys]
//!
//! Prints one JSON verdict (also written to `--out`). `status` is one of
//! - `match`: same canonical outcomes, probabilities within the tolerance (default 1e-9);
//! - `mismatch`: `differences` names the first canonical fields in which the most probable
//!   outcome only one side produced differs from the other side's closest outcome;
//! - `unsupported`: the engine refused the decision or a setup turn (`TurnError::Unsupported`);
//! - `engine-error`: any other engine failure (loading, parsing a choice, canonical output);
//! - `no-position`: no engine position after the setup turns has the report's `before` state;
//! - `ambiguous`: several engine positions share the `before` state (they differ in state the
//!   canonical form leaves out) and their distributions differ, so the oracle's single
//!   position cannot be matched to one of them. The comparison is against the first;
//! - `check-timeout`: `--timeout s` was given and the check took longer (the status
//!   `parity_corpus.py --per-report` gives a lab-check process it had to kill).
//!
//! Either file may be gzipped (a path ending in `.gz`; the parity corpus's files are).
//!
//! The report is read as a stream and its outcomes are folded into the distribution one at a
//! time (reports of the heaviest turns hold millions of outcomes, GBs of JSON). `--hash-keys`
//! (VB, GitHub Actions oracle runs) keys both distributions by a 128-bit hash of the canonical
//! key instead of the key itself: the memory of a turn with millions of outcomes, at the cost of
//! the field differences of a mismatch (not computed; the counts and TV are). Two different
//! canonical states with the same hash would merge; at 128 bits that is not expected.
//!
//! `--batch` (board P5) checks many pairs in one process: each input line is a JSON object
//! `{"scenario": path, "report": path, ...}` (from a file, or stdin with `-`), and each result is
//! one compact JSON line: the verdict with the input object's other fields (an `id`, say) copied
//! in, in completion order. `--jobs n` checks n lines at a time (threads). One process instead of
//! one per report, no temporary files: `engine/scripts/parity_corpus.py check` uses it.
//! `--timeout s` (board P5-t1) bounds each report: its check runs on a thread of its own, and when
//! it has not answered after `s` seconds the line gets a `check-timeout` verdict and the worker
//! moves on to the next line. The engine cannot be interrupted, so the abandoned check keeps its
//! thread (and a core) busy until it finishes or the batch ends (the process then exits without
//! waiting for it); many timeouts in one batch therefore slow the rest down.
//!
//! The exit code is 0 for `match` (in batch mode: every line matched), 1 for anything else, 2 for
//! a usage error.

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use lab_engine::instruction::Outcome;
use lab_engine::state::State;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::parity::{
    compare, engine_distribution, first_differences, json_diff, value_key, Distribution,
};
use lab_scenario::ScenarioMeta;

/// `--hash-keys`: distributions keyed by a hash of the canonical key.
static HASH_KEYS: AtomicBool = AtomicBool::new(false);

/// The distribution key of a canonical state: [`value_key`], or its 128-bit hash (hex) under
/// `--hash-keys`.
fn key_of(state: &Value) -> String {
    let key = value_key(state);
    if !HASH_KEYS.load(Ordering::Relaxed) {
        return key;
    }
    use std::hash::{DefaultHasher, Hasher};
    let half = |salt: u8| {
        let mut h = DefaultHasher::new();
        h.write_u8(salt);
        h.write(key.as_bytes());
        h.finish()
    };
    format!("{:016x}{:016x}", half(1), half(2))
}

/// [`engine_distribution`] keyed by [`key_of`] (the same when the keys are not hashed).
fn engine_distribution_keyed<const N: usize>(
    meta: &ScenarioMeta,
    state: &mut State<N>,
    outcomes: &[Outcome],
) -> Result<Distribution, String> {
    if !HASH_KEYS.load(Ordering::Relaxed) {
        return engine_distribution(meta, state, outcomes);
    }
    let mut out = Distribution::new();
    for outcome in outcomes {
        state.apply(&outcome.instructions);
        let value = canonical_value(state, meta);
        state.reverse(&outcome.instructions);
        let value = value.map_err(|e| e.to_string())?;
        *out.entry(key_of(&value)).or_insert(0.0) += outcome.probability;
    }
    Ok(out)
}
use lab_scenario::{
    canonical_value, format_slots, load_scenario_str_as, run_decision_mid_turn_with,
    scenario_decision, scenario_positions, LoadError, LoadedScenario, ScenarioError,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files = Vec::new();
    let mut out: Option<String> = None;
    let mut batch: Option<String> = None;
    let mut jobs = 1usize;
    let mut tolerance = 1e-9;
    let mut timeout: Option<std::time::Duration> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = args.get(i).cloned();
            }
            "--batch" => {
                i += 1;
                batch = args.get(i).cloned();
            }
            "--jobs" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(n) if n > 0 => jobs = n,
                    _ => {
                        eprintln!("lab-check: --jobs needs a positive number");
                        return ExitCode::from(2);
                    }
                }
            }
            "--timeout" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<f64>().ok()) {
                    Some(t) if t > 0.0 && t.is_finite() => {
                        timeout = Some(std::time::Duration::from_secs_f64(t))
                    }
                    _ => {
                        eprintln!("lab-check: --timeout needs a positive number of seconds");
                        return ExitCode::from(2);
                    }
                }
            }
            "--hash-keys" => HASH_KEYS.store(true, Ordering::Relaxed),
            "--tolerance" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(t) => tolerance = t,
                    None => {
                        eprintln!("lab-check: --tolerance needs a number");
                        return ExitCode::from(2);
                    }
                }
            }
            other => files.push(other.to_owned()),
        }
        i += 1;
    }
    if let Some(input) = batch {
        if !files.is_empty() || out.is_some() {
            eprintln!("lab-check: --batch takes no file arguments and no --out");
            return ExitCode::from(2);
        }
        return run_batch(&input, jobs, tolerance, timeout);
    }
    let [scenario, report] = files.as_slice() else {
        eprintln!(
            "usage: lab-check <scenario.json> <oracle-report.json> [--out verdict.json]\n       \
             lab-check --batch <jobs.jsonl | -> [--jobs n] [--timeout s]"
        );
        return ExitCode::from(2);
    };
    let verdict = bounded_check(scenario, report, tolerance, timeout);
    let text = serde_json::to_string_pretty(&verdict).expect("serializable");
    println!("{text}");
    if let Some(path) = out {
        if let Err(e) = std::fs::write(&path, &text) {
            eprintln!("lab-check: {path}: {e}");
            return ExitCode::from(2);
        }
    }
    if verdict["status"] == "match" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// [`check`] with the file names and the time it took (`engineMs`: reading, parsing and the
/// engine, as before P5).
fn timed_check(scenario: &str, report: &str, tolerance: f64) -> Value {
    let started = Instant::now();
    let mut verdict = check(scenario, report, tolerance);
    verdict["scenario"] = json!(scenario);
    verdict["report"] = json!(report);
    verdict["engineMs"] = json!((started.elapsed().as_secs_f64() * 1000.0).round());
    verdict
}

/// [`timed_check`] within `timeout`: on a detached thread, answered by `check-timeout` when it
/// has not finished in time (the thread is left to finish or to die with the process).
fn bounded_check(
    scenario: &str,
    report: &str,
    tolerance: f64,
    timeout: Option<std::time::Duration>,
) -> Value {
    let Some(timeout) = timeout else {
        return timed_check(scenario, report, tolerance);
    };
    let (send, receive) = std::sync::mpsc::channel();
    let (s, r) = (scenario.to_owned(), report.to_owned());
    std::thread::spawn(move || {
        // The receiver is gone after a timeout; the verdict is then dropped.
        let _ = send.send(timed_check(&s, &r, tolerance));
    });
    match receive.recv_timeout(timeout) {
        Ok(verdict) => verdict,
        Err(_) => json!({
            "status": "check-timeout",
            "error": format!("more than {} s", timeout.as_secs_f64()),
            "scenario": scenario,
            "report": report,
            "engineMs": (timeout.as_secs_f64() * 1000.0).round(),
        }),
    }
}

/// `--batch`: one verdict line per input line, `jobs` at a time.
fn run_batch(
    input: &str,
    jobs: usize,
    tolerance: f64,
    timeout: Option<std::time::Duration>,
) -> ExitCode {
    let reader: Box<dyn BufRead + Send> = if input == "-" {
        Box::new(std::io::BufReader::new(std::io::stdin()))
    } else {
        match std::fs::File::open(input) {
            Ok(f) => Box::new(std::io::BufReader::new(f)),
            Err(e) => {
                eprintln!("lab-check: {input}: {e}");
                return ExitCode::from(2);
            }
        }
    };
    let lines = Mutex::new(reader.lines());
    let stdout = Mutex::new(std::io::stdout());
    let all_match = Mutex::new(true);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let line = match lines.lock().expect("input lock").next() {
                    Some(Ok(line)) => line,
                    Some(Err(e)) => {
                        eprintln!("lab-check: {input}: {e}");
                        *all_match.lock().expect("flag lock") = false;
                        return;
                    }
                    None => return,
                };
                if line.trim().is_empty() {
                    continue;
                }
                let verdict = batch_line(&line, tolerance, timeout);
                if verdict["status"] != "match" {
                    *all_match.lock().expect("flag lock") = false;
                }
                let text = serde_json::to_string(&verdict).expect("serializable");
                let mut out = stdout.lock().expect("output lock");
                // A closed pipe (the reader gave up) ends this worker quietly.
                if writeln!(out, "{text}").and_then(|_| out.flush()).is_err() {
                    return;
                }
            });
        }
    });
    if *all_match.lock().expect("flag lock") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// One `--batch` line: the verdict, with the line's other fields copied in.
fn batch_line(line: &str, tolerance: f64, timeout: Option<std::time::Duration>) -> Value {
    let job: Value = match serde_json::from_str(line) {
        Ok(Value::Object(job)) => Value::Object(job),
        Ok(_) => {
            return failure(
                "engine-error",
                format!("batch line is not an object: {line}"),
            )
        }
        Err(e) => return failure("engine-error", format!("batch line: {e}")),
    };
    let (Some(scenario), Some(report)) = (job["scenario"].as_str(), job["report"].as_str()) else {
        let mut verdict = failure("engine-error", "batch line without scenario and report");
        verdict["job"] = job;
        return verdict;
    };
    let mut verdict = bounded_check(scenario, report, tolerance, timeout);
    if let (Value::Object(v), Value::Object(fields)) = (&mut verdict, &job) {
        for (k, value) in fields {
            if !v.contains_key(k) {
                v.insert(k.clone(), value.clone());
            }
        }
    }
    verdict
}

fn failure(status: &str, error: impl Into<String>) -> Value {
    json!({"status": status, "error": error.into()})
}

/// `unsupported` for what the engine does not implement ([`ScenarioError::Unsupported`]),
/// else `engine-error`; `prefix` goes before the message.
fn engine_failure(prefix: &str, error: ScenarioError) -> Value {
    let status = if error.is_unsupported() {
        "unsupported"
    } else {
        "engine-error"
    };
    failure(status, format!("{prefix}{error}"))
}

/// A file's text; a path ending in `.gz` is gunzipped (the parity corpus stores its scenarios
/// and reports compressed, board P5).
fn read_text(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    if path.ends_with(".gz") {
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .read_to_string(&mut text)
            .map_err(|e| format!("{path}: {e}"))?;
        Ok(text)
    } else {
        String::from_utf8(bytes).map_err(|e| format!("{path}: {e}"))
    }
}

/// What lab-check reads of an `enumerate.cjs` report. The other fields (per-outcome logs and
/// traces, branch counts, ...) are skipped by the parser instead of being built into a JSON tree:
/// most of a report's bytes are logs (board P5).
#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    roll: Option<u64>,
    #[serde(default)]
    before: Value,
    outcomes: FoldedOutcomes,
}

/// A report's outcomes folded into canonical key → probability while they are parsed (one
/// outcome's JSON tree at a time).
struct FoldedOutcomes(Distribution);

impl<'de> Deserialize<'de> for FoldedOutcomes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Fold;
        impl<'de> serde::de::Visitor<'de> for Fold {
            type Value = FoldedOutcomes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an array of outcomes")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<FoldedOutcomes, A::Error> {
                let mut dist = Distribution::new();
                while let Some(o) = seq.next_element::<ReportOutcome>()? {
                    *dist.entry(key_of(&o.state)).or_insert(0.0) += o.p;
                }
                Ok(FoldedOutcomes(dist))
            }
        }
        d.deserialize_seq(Fold)
    }
}

/// A report parsed from its file as a stream (gunzipped when the path ends in `.gz`).
fn read_report(path: &str) -> Result<Report, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let reader: Box<dyn Read> = if path.ends_with(".gz") {
        Box::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)))
    } else {
        Box::new(file)
    };
    let reader = std::io::BufReader::with_capacity(1 << 20, reader);
    serde_json::from_reader(reader).map_err(|e| format!("{path}: {e}"))
}

#[derive(Deserialize)]
struct ReportOutcome {
    p: f64,
    state: Value,
}

fn check(scenario: &str, report_path: &str, tolerance: f64) -> Value {
    let report: Report = match read_report(report_path) {
        Ok(r) => r,
        Err(e) => return failure("engine-error", e),
    };
    let mode = report.mode.as_str();
    let roll = report.roll;
    let options = EnumerateOptions {
        rolls: match mode {
            "full" => RollMode::Full,
            "extremes" => RollMode::Extremes,
            "fixed" => match roll
                .and_then(|k| u8::try_from(k).ok())
                .and_then(RollMode::fixed)
            {
                Some(rolls) => rolls,
                None => {
                    return failure(
                        "engine-error",
                        format!("report mode \"fixed\" with roll {roll:?}"),
                    )
                }
            },
            other => return failure("engine-error", format!("report mode {other:?}")),
        },
    };
    let oracle = report.outcomes.0;
    let text = match read_text(scenario) {
        Ok(t) => t,
        Err(e) => return failure("engine-error", e),
    };
    // II-t1: a singles format (`State<1>`) is checked by the same code at N = 1.
    let slots = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| v["format"].as_str().and_then(format_slots));
    let base = Path::new(scenario).parent().unwrap_or(Path::new("."));
    match slots {
        Some(1) => check_loaded(
            load_scenario_str_as::<1>(&text, base),
            &report.before,
            oracle,
            mode,
            roll,
            options,
            tolerance,
        ),
        _ => check_loaded(
            load_scenario_str_as::<2>(&text, base),
            &report.before,
            oracle,
            mode,
            roll,
            options,
            tolerance,
        ),
    }
}

/// [`check`] once the scenario is loaded at its slot count.
fn check_loaded<const N: usize>(
    loaded: Result<LoadedScenario<N>, LoadError>,
    before: &Value,
    oracle: Distribution,
    mode: &str,
    roll: Option<u64>,
    options: EnumerateOptions,
    tolerance: f64,
) -> Value {
    let loaded = match loaded {
        Ok(l) => l,
        Err(e) => return failure("engine-error", e.to_string()),
    };
    let positions = match scenario_positions(&loaded) {
        Ok(p) => p,
        Err(e) => return engine_failure("setup: ", e),
    };
    let mut matching = Vec::new();
    let mut closest: Option<Vec<String>> = None;
    for position in &positions {
        let value = match canonical_value(&position.state, &loaded.meta) {
            Ok(v) => v,
            Err(e) => return failure("engine-error", format!("canonical: {e}")),
        };
        if value == *before {
            matching.push(position.clone());
        } else {
            let d = json_diff(before, &value, 5);
            if closest.as_ref().is_none_or(|c| d.len() < c.len()) {
                closest = Some(d);
            }
        }
    }
    if matching.is_empty() {
        return json!({
            "status": "no-position",
            "error": format!("none of {} engine position(s) has the oracle's `before`", positions.len()),
            "differences": closest.unwrap_or_default(),
        });
    }
    let mut distributions: Vec<Distribution> = Vec::new();
    for position in &matching {
        let decision = match scenario_decision(&loaded, position) {
            Ok(d) => d,
            Err(e) => return failure("engine-error", format!("decision: {e}")),
        };
        let mut state = position.state.clone();
        let outcomes = match run_decision_mid_turn_with(
            &mut state,
            &position.order,
            &decision,
            &loaded.mid_turn,
            options,
        ) {
            Ok(o) => o,
            Err(e) => return engine_failure("", e),
        };
        match engine_distribution_keyed(&loaded.meta, &mut state, &outcomes) {
            Ok(d) => distributions.push(d),
            Err(e) => return failure("engine-error", format!("canonical: {e}")),
        }
    }
    let engine = &distributions[0];
    let variants_agree = distributions[1..]
        .iter()
        .all(|d| compare(d, engine).exact(tolerance));
    let comparison = compare(engine, &oracle);
    let status = if !variants_agree {
        "ambiguous"
    } else if comparison.exact(tolerance) {
        "match"
    } else {
        "mismatch"
    };
    let hashed = HASH_KEYS.load(Ordering::Relaxed);
    let mut differences = if hashed {
        Vec::new()
    } else {
        first_differences(&comparison, engine, &oracle, 8)
    };
    if hashed && !comparison.exact(tolerance) {
        differences.push("hash-keys: field differences not computed".to_owned());
    }
    if differences.is_empty() && !comparison.exact(tolerance) {
        differences.push(format!(
            "probability: largest difference {:e} over shared outcomes",
            comparison.max_shared_diff
        ));
    }
    json!({
        "status": status,
        "mode": mode,
        "roll": roll,
        "engineOutcomes": comparison.engine_outcomes,
        "oracleOutcomes": comparison.oracle_outcomes,
        "onlyEngine": comparison.only_engine.len(),
        "onlyOracle": comparison.only_oracle.len(),
        "maxSharedDiff": comparison.max_shared_diff,
        "tv": comparison.tv,
        "variants": matching.len(),
        "differences": differences,
        "hashKeys": hashed,
    })
}
