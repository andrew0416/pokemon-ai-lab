//! Board P5-t1: `lab-check --batch --timeout s` answers a report that takes longer than `s`
//! seconds with a `check-timeout` verdict (the status `parity_corpus.py --per-report` uses) and
//! goes on with the next line; the one-pair mode takes the option too.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

fn engine_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn batch(timeout: &str, jobs: &[(&str, &str)]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lab-check"))
        .args(["--batch", "-", "--jobs", "2", "--timeout", timeout])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for (name, id) in jobs {
            let line = serde_json::json!({
                "id": id,
                "scenario": engine_dir().join(format!("oracle/scenarios/{name}.json")),
                "report": engine_dir().join(format!("oracle/expected/{name}.turn.json")),
            });
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let mut verdicts: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    verdicts.sort_by_key(|v| v["id"].as_str().unwrap().to_owned());
    verdicts
}

#[test]
fn a_report_past_the_timeout_is_a_check_timeout() {
    // A microsecond: nothing can load, replay and enumerate that fast.
    let verdicts = batch("0.000001", &[("single-hit", "a"), ("quash", "b")]);
    assert_eq!(verdicts.len(), 2);
    for v in &verdicts {
        assert_eq!(v["status"], "check-timeout", "{v}");
        assert!(v["error"].as_str().unwrap().starts_with("more than"), "{v}");
        assert!(v["report"].as_str().is_some());
    }
    // A generous one changes nothing.
    let verdicts = batch("600", &[("single-hit", "a"), ("quash", "b")]);
    assert!(
        verdicts.iter().all(|v| v["status"] == "match"),
        "{verdicts:?}"
    );
}

#[test]
fn the_one_pair_mode_takes_a_timeout_and_bad_values_are_usage_errors() {
    let run = |timeout: &str| {
        Command::new(env!("CARGO_BIN_EXE_lab-check"))
            .arg(engine_dir().join("oracle/scenarios/quash.json"))
            .arg(engine_dir().join("oracle/expected/quash.turn.json"))
            .args(["--timeout", timeout])
            .output()
            .unwrap()
    };
    let out = run("600");
    assert!(out.status.success());
    let out = run("0.000001");
    assert_eq!(out.status.code(), Some(1));
    let verdict: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(verdict["status"], "check-timeout");
    for bad in ["0", "-1", "x"] {
        assert_eq!(run(bad).status.code(), Some(2), "{bad}");
    }
}
