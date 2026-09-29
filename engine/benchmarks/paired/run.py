#!/usr/bin/env python3
"""Sequential, same-host paired process measurements with an exact workload guard.

Standard library only. The shared checkout supplies both refs' scenario/team inputs.
This is a narrowly scoped benchmark, not an independent proof of battle rules.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
U32 = (1 << 32) - 1
BASE_FIELDS = {
    "decision", "ours", "theirs", "matrix", "equilibrium", "maximin",
    "nodes", "turns", "depth", "unsupported", "omitted",
}
STATS_FIELDS = {
    "tt_hits", "tt_misses", "nash_solves", "nash_iterations",
    "deep_tt_hits", "deep_tt_misses", "split_cells",
}


class BenchmarkError(Exception):
    """Invalid input, failed process, or workload mismatch: no success report."""


def require(condition, message):
    if not condition:
        raise BenchmarkError(message)


def keys(value, expected, where):
    require(type(value) is dict, f"{where}: expected an object")
    require(set(value) == set(expected), f"{where}: missing or unknown fields")


def integer(value, where, minimum=0, maximum=None):
    require(type(value) is int and value >= minimum, f"{where}: invalid integer")
    require(maximum is None or value <= maximum, f"{where}: integer out of range")


def float_bits(value, where):
    integer(value, where, maximum=U32)
    require(value & 0x7F800000 != 0x7F800000, f"{where}: non-finite f32 bits")


def array(value, where, length=None):
    require(type(value) is list, f"{where}: expected an array")
    require(length is None or len(value) == length, f"{where}: wrong array length")


def reject_number(value):
    raise BenchmarkError(f"JSON float or non-finite number is forbidden: {value}")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON field: {key}")
        result[key] = value
    return result


def strict_json(text):
    try:
        return json.loads(text, object_pairs_hook=unique_object,
                          parse_float=reject_number, parse_constant=reject_number)
    except (ValueError, UnicodeError) as error:
        raise BenchmarkError(f"invalid JSON: {error}") from error


def validate_analysis(value, *, deep):
    extra = {"beam", "outcome_cap", "levels", "shallow", "stats"} if deep else set()
    keys(value, BASE_FIELDS | extra, "analysis")
    for field in ("decision", "ours", "theirs"):
        require(type(value[field]) is str and value[field], f"{field}: empty or invalid")
    integer(value["depth"], "depth", 1)
    require(value["depth"] == (2 if deep else 1), "unexpected search depth")
    integer(value["nodes"], "nodes")
    integer(value["turns"], "turns", 1)
    require(value["unsupported"] == [], "unsupported effects in benchmark workload")
    array(value["omitted"], "omitted", 2)
    for count in value["omitted"]:
        integer(count, "omitted", maximum=0)
    matrix = value["matrix"]
    keys(matrix, {"rows", "cols", "values"}, "matrix")
    for field in ("rows", "cols"):
        integer(matrix[field], f"matrix.{field}", 1)
    array(matrix["values"], "matrix.values", matrix["rows"] * matrix["cols"])
    for item in matrix["values"]:
        float_bits(item, "matrix.values")
    equilibrium = value["equilibrium"]
    keys(equilibrium, {"rows", "cols", "value", "exploitability", "iterations"}, "equilibrium")
    for field in ("rows", "cols"):
        array(equilibrium[field], f"equilibrium.{field}", matrix[field])
        for item in equilibrium[field]:
            float_bits(item, f"equilibrium.{field}")
    for field in ("value", "exploitability"):
        float_bits(equilibrium[field], f"equilibrium.{field}")
    integer(equilibrium["iterations"], "equilibrium.iterations")
    array(value["maximin"], "maximin", 2)
    integer(value["maximin"][0], "maximin.row", maximum=matrix["rows"] - 1)
    float_bits(value["maximin"][1], "maximin.value")
    if deep:
        integer(value["beam"], "beam")
        integer(value["outcome_cap"], "outcome_cap")
        require(value["beam"] == value["outcome_cap"] == 2, "unexpected beam/outcome cap")
        array(value["levels"], "levels", 1)
        keys(value["levels"][0], {"beam", "outcomes"}, "levels[0]")
        for field in ("beam", "outcomes"):
            integer(value["levels"][0][field], f"levels[0].{field}")
            require(value["levels"][0][field] == 2, "unexpected level configuration")
        validate_analysis(value["shallow"], deep=False)
        keys(value["stats"], STATS_FIELDS, "stats")
        for field, count in value["stats"].items():
            integer(count, f"stats.{field}")
        require(value["stats"]["split_cells"] == 0, "split scheduling must be disabled")


def validate_output(value):
    keys(value, {"schema", "state_restored", "analysis"}, "output")
    integer(value["schema"], "schema")
    require(value["schema"] == 1, "unsupported output schema")
    require(value["state_restored"] is True, "State restoration assertion missing")
    validate_analysis(value["analysis"], deep=True)
    return value


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n",
                         encoding="utf-8")
    temporary.replace(path)


def utc_now():
    return datetime.now(timezone.utc).isoformat()


def within_root(path, root):
    path = path.resolve(strict=True)
    require(path.is_relative_to(root), f"fixture escapes the shared checkout: {path}")
    require(path.is_file(), f"not a file: {path}")
    return path


def load_cases(suite, root=ROOT):
    manifest = strict_json((HERE / "suites.json").read_text(encoding="utf-8"))
    keys(manifest, {"schema", "suites"}, "suite manifest")
    integer(manifest["schema"], "suite manifest.schema")
    require(manifest["schema"] == 1, "unsupported suite schema")
    keys(manifest["suites"], {"smoke", "narrow"}, "suites")
    cases = manifest["suites"][suite]
    array(cases, "cases")
    require(cases, "suite has no cases")
    names = set()
    inputs = set()
    for case in cases:
        keys(case, {"name", "scenario", "position"}, "case")
        name = case["name"]
        require(type(name) is str and name and name.isascii()
                and all(char.isalnum() or char == "-" for char in name), "invalid case name")
        require(name not in names, "duplicate case name")
        names.add(name)
        position = case["position"]
        require(type(position) is str and (position == "max" or
                (position.isascii() and position.isdecimal())), "invalid position")
        require(type(case["scenario"]) is str, "scenario must be a path")
        scenario = within_root(root / case["scenario"], root)
        inputs.add(scenario)
        # The loader supports inline teams or paths relative to the scenario. Both refs
        # read these same files, even if the candidate ref changed its own fixtures.
        data = strict_json(scenario.read_text(encoding="utf-8"))
        require(type(data) is dict, "scenario must be an object")
        for side in ("p1", "p2"):
            require(type(data.get(side)) is dict and "team" in data[side], f"missing {side}.team")
            team = data[side]["team"]
            if type(team) is str:
                inputs.add(within_root(scenario.parent / team, root))
            else:
                require(type(team) is list and team, f"invalid {side}.team")
    return cases, sorted(inputs)


def schedule(cases, pairs):
    """One excluded warmup per case/build, followed by balanced AB / BA pairs."""
    result = []
    for case in cases:
        for variant in ("baseline", "candidate"):
            result.append({"case": case["name"], "phase": "warmup", "pair": None,
                           "order": "AB", "variant": variant})
        for pair in range(pairs):
            order = "AB" if pair % 2 == 0 else "BA"
            variants = ("baseline", "candidate") if order == "AB" else ("candidate", "baseline")
            for variant in variants:
                result.append({"case": case["name"], "phase": "measure", "pair": pair,
                               "order": order, "variant": variant})
    return result


def child_cpu():
    if sys.platform.startswith("linux"):
        import resource
        usage = resource.getrusage(resource.RUSAGE_CHILDREN)
        return usage.ru_utime + usage.ru_stime
    return None


def terminate_and_reap(process):
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    elif os.name == "nt":
        # Windows has no POSIX process group kill. taskkill /T handles descendants;
        # direct kill remains a fallback, and wait below always reaps our child.
        try:
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=10, check=False)
        except (OSError, subprocess.TimeoutExpired):
            pass
    if process.poll() is None:
        process.kill()
    process.wait()


def execute(command, *, cwd, env, timeout, stdout_path, stderr_path):
    """Capture files directly so timeout cleanup cannot block on inherited pipes."""
    process = None
    watchdog = None
    timed_out = threading.Event()
    status, error, returncode = "ok", None, None
    cpu_before = child_cpu()
    started = time.perf_counter_ns()
    with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
        try:
            process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                       stdout=stdout, stderr=stderr,
                                       start_new_session=(os.name == "posix"))

            def deadline():
                if process.poll() is None:
                    timed_out.set()
                    terminate_and_reap(process)

            # wait(timeout) polls on POSIX and can add tens of milliseconds of jitter.
            # Block on waitpid instead; the watchdog owns deadline termination.
            watchdog = threading.Timer(timeout, deadline)
            watchdog.daemon = True
            watchdog.start()
            returncode = process.wait()
            watchdog.cancel()
            watchdog.join()
            if timed_out.is_set():
                status, error = "timeout", f"process exceeded {timeout} seconds"
            elif returncode != 0:
                status, error = "process_error", f"process returned {returncode}"
        except OSError as exception:
            status, error = "spawn_error", str(exception)
            if process is not None:
                terminate_and_reap(process)
                returncode = process.returncode
        except BaseException:
            if process is not None:
                terminate_and_reap(process)
            raise
        finally:
            if watchdog is not None:
                watchdog.cancel()
                watchdog.join()
    wall_ns = time.perf_counter_ns() - started
    cpu_after = child_cpu()
    cpu_seconds = None if cpu_before is None else max(0.0, cpu_after - cpu_before)
    return {"status": status, "error": error, "returncode": returncode,
            "pid": None if process is None else process.pid, "wall_ns": wall_ns,
            "cpu_seconds": cpu_seconds}


def machine_info():
    cpu_model = platform.processor() or os.environ.get("PROCESSOR_IDENTIFIER")
    if sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/cpuinfo").read_text().splitlines():
                if line.startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                    break
        except OSError:
            pass
    affinity = None
    if hasattr(os, "sched_getaffinity"):
        affinity = sorted(os.sched_getaffinity(0))
    return {"platform": platform.platform(), "system": platform.system(),
            "release": platform.release(), "machine": platform.machine(),
            "cpu_model": cpu_model, "logical_cpus": os.cpu_count(), "affinity": affinity,
            "python": sys.version, "hostname": platform.node(),
            "cpu_time_source": "RUSAGE_CHILDREN sequential delta" if child_cpu() is not None else None}


def summary(records, cases, pairs):
    output = []
    for case in cases:
        selected = [record for record in records if record["phase"] == "measure"
                    and record["case"] == case["name"]]
        require(len(selected) == pairs * 2, f"{case['name']}: incomplete measured data")
        ratios, cpu_ratios, paired = [], [], []
        baseline_ns, candidate_ns = [], []
        for pair in range(pairs):
            entries = [record for record in selected if record["pair"] == pair]
            require(len(entries) == 2, "missing pair")
            by_variant = {record["variant"]: record for record in entries}
            require(set(by_variant) == {"baseline", "candidate"}, "invalid pair")
            a, b = by_variant["baseline"], by_variant["candidate"]
            require(a["status"] == b["status"] == "ok", "failed measurement")
            require(a["wall_ns"] > 0 and b["wall_ns"] > 0, "nonpositive timing")
            ratio = b["wall_ns"] / a["wall_ns"]
            ratios.append(ratio)
            baseline_ns.append(a["wall_ns"])
            candidate_ns.append(b["wall_ns"])
            cpu_ratio = None
            if a["cpu_seconds"] is not None and a["cpu_seconds"] > 0 and b["cpu_seconds"] > 0:
                cpu_ratio = b["cpu_seconds"] / a["cpu_seconds"]
                cpu_ratios.append(cpu_ratio)
            paired.append({"pair": pair, "order": a["order"],
                           "wall_ratio_candidate_over_baseline": ratio,
                           "cpu_ratio_candidate_over_baseline": cpu_ratio})
        output.append({"case": case["name"], "pairs": paired,
                       "median_wall_ratio_candidate_over_baseline": statistics.median(ratios),
                       "min_wall_ratio_candidate_over_baseline": min(ratios),
                       "max_wall_ratio_candidate_over_baseline": max(ratios),
                       "baseline_median_wall_seconds": statistics.median(baseline_ns) / 1e9,
                       "candidate_median_wall_seconds": statistics.median(candidate_ns) / 1e9,
                       "median_cpu_ratio_candidate_over_baseline":
                           statistics.median(cpu_ratios) if len(cpu_ratios) == pairs else None})
    require(output, "no measured data")
    return output


def write_summary(path, result):
    lines = ["## Paired engine benchmark", "",
             f"Status: **{result['status']}**. Suite: `{result['suite']}`; "
             f"threads: {result['threads']}; pairs per case: {result['pairs_per_case']}.", "",
             f"Completed process invocations (including warmups): {result['completed_invocations']}.", ""]
    if result["status"] == "ok":
        lines.extend([
            "Each case/build has one excluded warmup. Measured pairs alternate AB/BA.", "",
            "| Case | Baseline median seconds | Candidate median seconds | Median C/B ratio | Min C/B | Max C/B |",
            "|---|---:|---:|---:|---:|---:|",
        ])
        for case in result["cases"]:
            lines.append(
                f"| {case['case']} | {case['baseline_median_wall_seconds']:.6f} | "
                f"{case['candidate_median_wall_seconds']:.6f} | "
                f"{case['median_wall_ratio_candidate_over_baseline']:.6f} | "
                f"{case['min_wall_ratio_candidate_over_baseline']:.6f} | "
                f"{case['max_wall_ratio_candidate_over_baseline']:.6f} |"
            )
        lines.extend(["", "Ratios are candidate / baseline for each adjacent pair; values below 1 "
                      "mean a shorter candidate wall time in that pair. These runs do not establish "
                      "a universal speedup. Smoke is an infrastructure check, not a performance result.",
                      "", "All outputs matched exactly, including finite float bits and work counters. "
                      "Repeated equality is a workload guard, not additional independent rule proof."])
    else:
        error = result.get("error", "run incomplete").replace("\n", " ")
        lines.extend([f"Failure: {error}", "", "No timing comparison is accepted. Inspect result.json, "
                      "records.jsonl, and raw stdout/stderr in the artifact."])
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def run(args):
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "raw").mkdir()
    records = []
    result = {"schema": 1, "status": "running", "started_utc": utc_now(),
              "completed_invocations": 0, "suite": args.suite, "threads": args.threads,
              "pairs_per_case": args.pairs}
    write_json(out / "result.json", result)
    try:
        cases, fixtures = load_cases(args.suite)
        executables = {"baseline": args.baseline_exe.resolve(strict=True),
                       "candidate": args.candidate_exe.resolve(strict=True)}
        hashes = {path: sha256(path) for path in set(executables.values()) | set(fixtures)
                  | {HERE / "harness.rs", HERE / "suites.json", Path(__file__).resolve()}}
        runs = schedule(cases, args.pairs)
        env = os.environ.copy()
        removed = sorted(key for key in env if key.startswith(("LAB_ENGINE_", "LAB_SEARCH_", "RAYON_")))
        for key in removed:
            del env[key]
        fixed_env = {"LAB_ENGINE_FACTORED": "0", "RAYON_NUM_THREADS": str(args.threads),
                     "PYTHONUTF8": "1", "PYTHONHASHSEED": "0",
                     "OPENBLAS_NUM_THREADS": "1", "OMP_NUM_THREADS": "1"}
        env.update(fixed_env)
        manifest = {"schema": 1, "started_utc": result["started_utc"],
                    "shared_checkout": str(ROOT), "suite": args.suite, "cases": cases,
                    "threads": args.threads, "pairs_per_case": args.pairs,
                    "warmups_per_case_and_build": 1, "per_process_timeout_seconds": args.timeout,
                    "machine": machine_info(), "environment": fixed_env,
                    "removed_environment_variable_names": removed,
                    "binary_paths": {name: str(path) for name, path in executables.items()},
                    "binary_sha256": {name: hashes[path] for name, path in executables.items()},
                    "input_sha256": {path.relative_to(ROOT).as_posix(): hashes[path] for path in fixtures},
                    "harness_sha256": hashes[HERE / "harness.rs"],
                    "runner_sha256": hashes[Path(__file__).resolve()],
                    "suites_sha256": hashes[HERE / "suites.json"],
                    "search": {"setup_rolls": "Median", "search_rolls": "Median", "depth": 2,
                               "beam": 2, "outcomes": 2, "evaluator": "Heuristic",
                               "factored": False, "split_heavy_cells": False,
                               "scheduler": "existing scheduler from each ref"},
                    "comparison": "strict JSON equality, including finite f32 bits and all work counters",
                    "schedule": runs}
        write_json(out / "manifest.json", manifest)
        references = {}
        case_by_name = {case["name"]: case for case in cases}
        with (out / "records.jsonl").open("x", encoding="utf-8") as journal:
            for index, planned in enumerate(runs):
                record = dict(planned, invocation=index, started_utc=utc_now())
                stem = f"{index:04d}-{record['case']}-{record['phase']}-{record['variant']}"
                stdout_path, stderr_path = out / "raw" / f"{stem}.stdout", out / "raw" / f"{stem}.stderr"
                record.update(stdout=stdout_path.relative_to(out).as_posix(),
                              stderr=stderr_path.relative_to(out).as_posix())
                case = case_by_name[record["case"]]
                command = [str(executables[record["variant"]]), str(ROOT / case["scenario"]),
                           str(args.threads), case["position"]]
                record["command"] = command
                record.update(execute(command, cwd=ROOT, env=env, timeout=args.timeout,
                                      stdout_path=stdout_path, stderr_path=stderr_path))
                if record["status"] == "ok":
                    try:
                        value = validate_output(strict_json(stdout_path.read_text(encoding="utf-8")))
                        encoded = canonical(value)
                        record["semantic_sha256"] = hashlib.sha256(encoded.encode()).hexdigest()
                        reference = references.setdefault(record["case"], encoded)
                        require(encoded == reference,
                                f"exact workload mismatch for {record['case']} (includes work counters)")
                    except (BenchmarkError, UnicodeError, OSError) as error:
                        record.update(status="validation_error", error=str(error))
                journal.write(json.dumps(record, sort_keys=True, allow_nan=False) + "\n")
                journal.flush()
                os.fsync(journal.fileno())
                records.append(record)
                result["completed_invocations"] = len(records)
                write_json(out / "result.json", result)
                if record["status"] != "ok":
                    raise BenchmarkError(f"invocation {index}: {record['error']}")
        for path, digest in hashes.items():
            require(sha256(path) == digest, f"binary or shared input changed during run: {path}")
        result.update(status="ok", cases=summary(records, cases, args.pairs),
                      interpretation="Candidate/baseline ratios describe only these paired runs; "
                                     "there is no universal speedup verdict. Repeated equality is a "
                                     "workload guard, not additional independent rule proof.")
    except (Exception, KeyboardInterrupt) as error:
        # Preserve diagnostics even for an unexpected runner bug; never report a partial
        # run as successful. The exception type distinguishes infrastructure failures.
        result.update(status="failed", error=str(error) or type(error).__name__)
        result["error_type"] = type(error).__name__
    finally:
        result["finished_utc"] = utc_now()
        write_json(out / "result.json", result)
        write_summary(out / "summary.md", result)
    print(json.dumps(result, indent=2, allow_nan=False))
    return 0 if result["status"] == "ok" else 1


def bounded_pairs(value):
    try:
        pairs = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("pairs must be an even integer from 2 to 20") from error
    if not 2 <= pairs <= 20 or pairs % 2:
        raise argparse.ArgumentTypeError("pairs must be an even integer from 2 to 20")
    return pairs


def bounded_timeout(value):
    try:
        timeout = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("timeout must be 1..600 seconds") from error
    if not math.isfinite(timeout) or not 1 <= timeout <= 600:
        raise argparse.ArgumentTypeError("timeout must be 1..600 seconds")
    return timeout


def parser():
    argument_parser = argparse.ArgumentParser(description=__doc__)
    argument_parser.add_argument("--baseline-exe", required=True, type=Path)
    argument_parser.add_argument("--candidate-exe", required=True, type=Path)
    argument_parser.add_argument("--suite", required=True, choices=("smoke", "narrow"))
    argument_parser.add_argument("--threads", type=int, choices=(1, 2, 4), default=1)
    argument_parser.add_argument("--pairs", type=bounded_pairs, default=6)
    argument_parser.add_argument("--out-dir", required=True, type=Path)
    argument_parser.add_argument("--timeout", type=bounded_timeout, default=600.0)
    return argument_parser


def main():
    args = parser().parse_args()
    try:
        return run(args)
    except OSError as error:
        print(f"benchmark could not create a new output directory: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
