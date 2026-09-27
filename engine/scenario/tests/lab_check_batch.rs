//! Board P5: `lab-check --batch` checks many (scenario, report) pairs in one process, and either
//! file may be gzipped (the parity corpus's files are). One verdict line per input line, with the
//! input's other fields (`id`) copied in; the same verdict as the one-pair mode.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

fn engine_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn gzip(from: &std::path::Path, to: &std::path::Path) {
    let data = std::fs::read(from).unwrap();
    let mut encoder =
        flate2::write::GzEncoder::new(std::fs::File::create(to).unwrap(), Default::default());
    encoder.write_all(&data).unwrap();
    encoder.finish().unwrap();
}

#[test]
fn batch_mode_reads_gzipped_files_and_keeps_the_ids() {
    let dir = std::env::temp_dir().join(format!("lab-check-batch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let scenario = engine_dir().join("oracle/scenarios/single-hit.json");
    let report = engine_dir().join("oracle/expected/single-hit.turn.json");
    // A gzipped scenario is read from its own directory, so this one has its teams inline (as
    // the corpus's scenarios do).
    let scenario_gz = dir.join("quash.json.gz");
    let report_gz = dir.join("quash.report.json.gz");
    gzip(&engine_dir().join("oracle/scenarios/quash.json"), &scenario_gz);
    gzip(&engine_dir().join("oracle/expected/quash.turn.json"), &report_gz);
    // A report of another scenario: the `before` state is not among the positions.
    let other = engine_dir().join("oracle/expected/quash.turn.json");
    let jobs = [
        (scenario.clone(), report.clone(), "plain"),
        (scenario_gz, report_gz, "gz"),
        (scenario, other, "other"),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_lab-check"))
        .args(["--batch", "-", "--jobs", "2"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for (s, r, id) in &jobs {
            let line = serde_json::json!({"id": id, "scenario": s, "report": r});
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1), "one pair does not match");
    let verdicts: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(verdicts.len(), 3);
    let status = |id: &str| {
        verdicts
            .iter()
            .find(|v| v["id"] == id)
            .map(|v| v["status"].as_str().unwrap().to_owned())
            .unwrap()
    };
    assert_eq!(status("plain"), "match");
    assert_eq!(status("gz"), "match");
    assert_eq!(status("other"), "no-position");
    let gz = verdicts.iter().find(|v| v["id"] == "gz").unwrap();
    assert_eq!(gz["engineOutcomes"], 291);
    std::fs::remove_dir_all(&dir).ok();
}
