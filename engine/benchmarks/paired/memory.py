#!/usr/bin/env python3
"""Separate Linux peak-RSS pass after a successful paired timing benchmark.

GNU /usr/bin/time %M reports maximum resident set size in KiB for a fresh
harness process. This is process peak RSS (including setup), not allocations,
live engine heap, or a sum of simultaneous process peaks. Two AB/BA pairs per
case are diagnostic samples, not confidence intervals. No time from this pass
is accepted as a CPU/wall measurement or added to the original timing data.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location("paired_memory_run", Path(__file__).with_name("run.py"))
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)
GNU_TIME = Path("/usr/bin/time")
PAIRS = 2


def read_json(path):
    # Artifact metadata includes timing floats; harness outputs use strict_json.
    return json.loads(path.read_text(encoding="utf-8"),
                      object_pairs_hook=bench.unique_object,
                      parse_constant=bench.reject_number)


def contained_file(root, relative):
    bench.require(isinstance(relative, str) and not Path(relative).is_absolute(),
                  "artifact reference must be relative")
    return bench.within_root(root / relative, root)


def fixed_environment(threads):
    return {"LAB_ENGINE_FACTORED": "0", "RAYON_NUM_THREADS": str(threads),
            "PYTHONUTF8": "1", "PYTHONHASHSEED": "0",
            "OPENBLAS_NUM_THREADS": "1", "OMP_NUM_THREADS": "1"}


def measurement_environment(threads):
    env = os.environ.copy()
    removed = sorted(k for k in env if k.startswith(("LAB_ENGINE_", "LAB_SEARCH_", "RAYON_")))
    for key in removed:
        del env[key]
    env.update(fixed_environment(threads))
    return env, removed


def time_identity():
    bench.require(sys.platform.startswith("linux"), "peak RSS pass requires Linux GNU /usr/bin/time")
    path = GNU_TIME.resolve(strict=True)
    env = os.environ.copy()
    env["LC_ALL"] = "C"
    probe = subprocess.run([str(path), "--version"], capture_output=True,
                           text=True, encoding="utf-8", timeout=10, env=env, check=False)
    version = probe.stdout + probe.stderr
    bench.require(probe.returncode == 0 and re.search(r"GNU [Tt]ime", version),
                  "/usr/bin/time did not identify itself as GNU time")
    return {"path": str(path), "sha256": bench.sha256(path), "version": version.strip(),
            "format": "%M", "unit": "KiB", "method": "GNU time maximum resident set size"}


def parse_peak_rss(text):
    # Successful GNU %M output is one positive decimal integer and one newline.
    bench.require(re.fullmatch(r"[0-9]+\n?", text) is not None,
                  "invalid GNU time %M output: expected one KiB integer")
    value = int(text)
    bench.require(value > 0, "GNU time reported zero peak RSS; measurement unavailable")
    return value


def memory_schedule(cases):
    return [{**entry, "phase": "memory"} for entry in bench.schedule(cases, PAIRS)
            if entry["phase"] == "measure"]


def check_inputs(args, cases, fixtures, executables):
    """Fail before any harness execution if the accepted timing workload differs."""
    source = args.benchmark_dir.resolve(strict=True)
    manifest_path, result_path, records_path = (source / name for name in
                                               ("manifest.json", "result.json", "records.jsonl"))
    manifest, result = read_json(manifest_path), read_json(result_path)
    records = [json.loads(line, object_pairs_hook=bench.unique_object,
                          parse_constant=bench.reject_number)
               for line in records_path.read_text(encoding="utf-8").splitlines()]
    bench.require(result.get("schema") == manifest.get("schema") == 1,
                  "unsupported timing artifact schema")
    bench.require(result.get("status") == "ok", "timing benchmark must finish successfully first")
    for field, expected in (("suite", args.suite), ("threads", args.threads), ("cases", cases)):
        bench.require(manifest.get(field) == expected, f"timing manifest {field} differs")
    bench.require(result.get("suite") == args.suite and result.get("threads") == args.threads,
                  "timing result suite/threads differs")
    pairs = manifest["pairs_per_case"]
    bench.integer(pairs, "timing pairs", 2, 20)
    bench.require(pairs % 2 == 0 and result.get("pairs_per_case") == pairs,
                  "invalid timing pairs")
    expected_schedule = bench.schedule(cases, pairs)
    bench.require(manifest.get("schedule") == expected_schedule and
                  len(records) == result.get("completed_invocations") == len(expected_schedule),
                  "incomplete or changed timing schedule")
    bench.require(manifest.get("warmups_per_case_and_build") == 1,
                  "unexpected timing warmup policy")
    bench.require(Path(manifest["shared_checkout"]).resolve() == bench.ROOT.resolve(),
                  "shared checkout differs from the timing benchmark")
    bench.require(manifest.get("environment") == fixed_environment(args.threads),
                  "timing environment differs")
    paths = set(fixtures) | set(executables.values()) | {
        bench.HERE / "harness.rs", bench.HERE / "suites.json", Path(bench.__file__),
        manifest_path, result_path, records_path}
    hashes = {path: bench.sha256(path) for path in paths}
    for label, path in executables.items():
        bench.require(Path(manifest["binary_paths"][label]).resolve() == path,
                      f"{label} executable path differs")
        bench.require(manifest["binary_sha256"][label] == hashes[path],
                      f"{label} executable hash differs")
    expected_inputs = {path.relative_to(bench.ROOT).as_posix(): hashes[path] for path in fixtures}
    bench.require(manifest.get("input_sha256") == expected_inputs, "shared input hashes differ")
    for key, path in (("harness_sha256", bench.HERE / "harness.rs"),
                      ("suites_sha256", bench.HERE / "suites.json"),
                      ("runner_sha256", Path(bench.__file__))):
        bench.require(manifest.get(key) == hashes[path], f"timing {key} differs")
    case_by_name = {case["name"]: case for case in cases}
    references, reference_records = {}, {}
    for index, (record, planned) in enumerate(zip(records, expected_schedule)):
        bench.require(all(record.get(k) == value for k, value in planned.items()) and
                      record.get("invocation") == index, "timing record schedule differs")
        bench.require(record.get("status") == "ok" and record.get("returncode") == 0,
                      "timing record was not successful")
        case = case_by_name[record["case"]]
        expected_command = [str(executables[record["variant"]]), str(bench.ROOT / case["scenario"]),
                            str(args.threads), case["position"]]
        bench.require(record.get("command") == expected_command, "timing executable/arguments differ")
        stdout = contained_file(source, record["stdout"])
        stderr = contained_file(source, record["stderr"])
        raw = stdout.read_bytes()
        encoded = bench.canonical(bench.validate_output(bench.strict_json(raw.decode("utf-8"))))
        semantic_hash = hashlib.sha256(encoded.encode()).hexdigest()
        bench.require(record.get("semantic_sha256") == semantic_hash, "timing semantic hash differs")
        original = references.setdefault(record["case"], {"raw": raw, "canonical": encoded})
        bench.require(original == {"raw": raw, "canonical": encoded},
                      "timing outputs are not byte/canonical identical within a case")
        reference_records.setdefault(record["case"], {"stdout": record["stdout"],
                                                     "sha256": bench.sha256(stdout),
                                                     "semantic_sha256": semantic_hash})
        hashes[stdout], hashes[stderr] = bench.sha256(stdout), bench.sha256(stderr)
    bench.require([row["case"] for row in result["cases"]] == [c["name"] for c in cases],
                  "timing summary cases differ")
    return manifest, references, reference_records, hashes


def summarize(records, cases):
    output = []
    for case in cases:
        selected = [r for r in records if r["case"] == case["name"]]
        bench.require(len(selected) == PAIRS * 2 and all(r["status"] == "ok" for r in selected),
                      "incomplete peak RSS pass")
        pairs = []
        for pair in range(PAIRS):
            rows = {r["variant"]: r for r in selected if r["pair"] == pair}
            a, b = rows["baseline"]["peak_rss_kib"], rows["candidate"]["peak_rss_kib"]
            pairs.append({"pair": pair, "order": rows["baseline"]["order"],
                          "baseline_peak_rss_kib": a, "candidate_peak_rss_kib": b,
                          "ratio_candidate_over_baseline": b / a})
        entry = {"case": case["name"], "pairs": pairs}
        for label in ("baseline", "candidate"):
            values = [r["peak_rss_kib"] for r in selected if r["variant"] == label]
            entry[label + "_peak_rss_kib"] = {"samples": values, "median": statistics.median(values),
                                               "min": min(values), "max": max(values)}
        output.append(entry)
    return output


def write_summary(path, result):
    lines = ["## Separate Linux peak RSS pass", "", f"Status: **{result['status']}**.", "",
             "GNU time %M, KiB; fresh processes; two AB/BA pairs per case; no warmup.",
             "CPU/wall values from the process helper are discarded and never merged into timing results.", ""]
    if result["status"] == "ok":
        lines += ["| Case | Baseline peak RSS median KiB | Candidate peak RSS median KiB |",
                  "|---|---:|---:|"]
        for row in result["cases"]:
            lines.append(f"| {row['case']} | {row['baseline_peak_rss_kib']['median']} | "
                         f"{row['candidate_peak_rss_kib']['median']} |")
        lines += ["", "Process high-water RSS includes setup and runtime/library overhead; it is not live heap or allocation count.",
                  "Two pairs are a narrow diagnostic, not a confidence interval. Raw outputs matched the successful timing benchmark byte for byte."]
    else:
        lines += ["Failure: " + result.get("error", "incomplete").replace("\n", " "),
                  "No complete memory comparison is accepted. Partial raw files and the journal are retained."]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def run(args):
    source, out = args.benchmark_dir.resolve(), args.out_dir.resolve()
    bench.require(not out.is_relative_to(source) and not source.is_relative_to(out),
                  "memory output and timing artifact directories must be separate")
    out.mkdir(parents=True, exist_ok=False)
    (out / "raw").mkdir()
    result = {"schema": 1, "status": "running", "started_utc": bench.utc_now(),
              "suite": args.suite, "threads": args.threads, "pairs_per_case": PAIRS,
              "completed_invocations": 0, "metric": "peak_rss_kib", "timing_measurement": False}
    bench.write_json(out / "result.json", result)
    records = []
    try:
        identity = time_identity()
        cases, fixtures = bench.load_cases(args.suite)
        executables = {label: getattr(args, label + "_exe").resolve(strict=True)
                       for label in ("baseline", "candidate")}
        prior, references, reference_records, hashes = check_inputs(args, cases, fixtures, executables)
        hashes[Path(identity["path"])] = identity["sha256"]
        hashes[Path(__file__).resolve()] = bench.sha256(Path(__file__).resolve())
        env, removed = measurement_environment(args.threads)
        machine = bench.machine_info()
        bench.require(machine["hostname"] == prior["machine"]["hostname"],
                      "memory pass must use the timing benchmark host")
        planned_runs = memory_schedule(cases)
        manifest = {"schema": 1, "started_utc": result["started_utc"], "suite": args.suite,
                    "cases": cases, "threads": args.threads, "pairs_per_case": PAIRS,
                    "warmups_per_case_and_build": 0, "per_process_timeout_seconds": args.timeout,
                    "machine": machine, "method": identity, "environment": fixed_environment(args.threads),
                    "removed_environment_variable_names": removed, "benchmark_dir": str(source),
                    "binary_paths": {name: str(path) for name, path in executables.items()},
                    "binary_sha256": {name: hashes[path] for name, path in executables.items()},
                    "timing_reference_outputs": reference_records, "schedule": planned_runs,
                    "source_sha256": {str(path): digest for path, digest in hashes.items()},
                    "timing_measurement": False,
                    "discarded_execute_fields": ["wall_ns", "cpu_seconds"],
                    "comparison": "byte equality and strict canonical harness output equality to original timing outputs"}
        bench.write_json(out / "manifest.json", manifest)
        case_by_name = {case["name"]: case for case in cases}
        with (out / "records.jsonl").open("x", encoding="utf-8") as journal:
            for index, planned in enumerate(planned_runs):
                record = dict(planned, invocation=index, started_utc=bench.utc_now())
                stem = f"{index:04d}-{record['case']}-{record['order']}-{record['variant']}"
                paths = {key: out / "raw" / (stem + suffix)
                         for key, suffix in (("stdout", ".stdout"), ("stderr", ".stderr"),
                                             ("timefile", ".time"))}
                record.update({key: path.relative_to(out).as_posix() for key, path in paths.items()})
                case = case_by_name[record["case"]]
                child = [str(executables[record["variant"]]), str(bench.ROOT / case["scenario"]),
                         str(args.threads), case["position"]]
                command = [identity["path"], "-f", "%M", "-o", str(paths["timefile"]), "--", *child]
                record.update(command=command, child_command=child,
                              binary_sha256=hashes[executables[record["variant"]]])
                try:
                    outcome = bench.execute(command, cwd=bench.ROOT, env=env, timeout=args.timeout,
                                            stdout_path=paths["stdout"], stderr_path=paths["stderr"])
                    # Helper timing exists only for timeout/process management; never report it.
                    record.update({key: outcome[key] for key in ("status", "error", "returncode", "pid")})
                    if record["status"] == "ok":
                        raw = paths["stdout"].read_bytes()
                        encoded = bench.canonical(bench.validate_output(bench.strict_json(raw.decode("utf-8"))))
                        bench.require(raw == references[record["case"]]["raw"] and
                                      encoded == references[record["case"]]["canonical"],
                                      f"memory workload differs from timing output: {record['case']}")
                        record["semantic_sha256"] = hashlib.sha256(encoded.encode()).hexdigest()
                        record["peak_rss_kib"] = parse_peak_rss(paths["timefile"].read_text(encoding="ascii"))
                except (Exception, KeyboardInterrupt) as error:
                    record.update(status="validation_error", error=str(error) or type(error).__name__,
                                  error_type=type(error).__name__)
                record["raw_sha256"] = {key: bench.sha256(path) for key, path in paths.items() if path.is_file()}
                journal.write(json.dumps(record, sort_keys=True, allow_nan=False) + "\n")
                journal.flush()
                os.fsync(journal.fileno())
                records.append(record)
                result["completed_invocations"] = len(records)
                bench.write_json(out / "result.json", result)
                bench.require(record["status"] == "ok", f"invocation {index}: {record['error']}")
        for path, digest in hashes.items():
            bench.require(bench.sha256(path) == digest, f"source/binary changed during memory pass: {path}")
        result.update(status="ok", cases=summarize(records, cases),
                      interpretation="Separate process peak RSS; no timing conclusion, no allocation count, no universal memory verdict.")
    except (Exception, KeyboardInterrupt) as error:
        result.update(status="failed", error=str(error) or type(error).__name__, error_type=type(error).__name__)
    finally:
        result["finished_utc"] = bench.utc_now()
        bench.write_json(out / "result.json", result)
        write_summary(out / "summary.md", result)
    print(json.dumps(result, indent=2, allow_nan=False))
    return 0 if result["status"] == "ok" else 1


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--baseline-exe", type=Path, required=True)
    p.add_argument("--candidate-exe", type=Path, required=True)
    p.add_argument("--suite", choices=("smoke", "narrow"), required=True)
    p.add_argument("--threads", type=int, choices=(1, 2, 4), default=1)
    p.add_argument("--timeout", type=bench.bounded_timeout, default=600.0)
    p.add_argument("--benchmark-dir", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    return p


def main():
    try:
        return run(parser().parse_args())
    except (OSError, bench.BenchmarkError) as error:
        print(f"memory pass could not create a separate new output directory: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
