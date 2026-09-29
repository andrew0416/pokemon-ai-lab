"""Small process/validation tests; no engine build or benchmark workload required."""

import argparse
import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("paired_run", Path(__file__).with_name("run.py"))
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)


def valid_output():
    shallow = {
        "decision": "Turn", "ours": "[Turn]", "theirs": "[Turn]",
        "matrix": {"rows": 1, "cols": 1, "values": [0]},
        "equilibrium": {"rows": [1065353216], "cols": [1065353216],
                        "value": 0, "exploitability": 0, "iterations": 0},
        "maximin": [0, 0], "nodes": 1, "turns": 1, "depth": 1,
        "unsupported": [], "omitted": [0, 0],
    }
    deep = copy.deepcopy(shallow)
    deep.update(depth=2, beam=2, outcome_cap=2,
                levels=[{"beam": 2, "outcomes": 2}], shallow=shallow,
                stats={field: 0 for field in bench.STATS_FIELDS})
    return {"schema": 1, "state_restored": True, "analysis": deep}


class ValidationTests(unittest.TestCase):
    def test_valid_bits_are_preserved_including_signed_zero(self):
        value = valid_output()
        self.assertEqual(bench.validate_output(bench.strict_json(json.dumps(value))), value)
        negative_zero = copy.deepcopy(value)
        negative_zero["analysis"]["matrix"]["values"][0] = 2147483648
        bench.validate_output(negative_zero)
        self.assertNotEqual(bench.canonical(value), bench.canonical(negative_zero))

    def test_rejects_missing_unknown_empty_and_nonfinite_json(self):
        for text in ("", "{}", "[]", "null", "{", "{} trailing", "NaN", "Infinity",
                     '{"schema":1,"schema":1}', '{"schema":1.0}'):
            with self.subTest(text=text), self.assertRaises(bench.BenchmarkError):
                bench.validate_output(bench.strict_json(text))

    def test_rejects_invalid_nested_output(self):
        mutations = [
            lambda v: v.update(unknown=0),
            lambda v: v.update(state_restored=False),
            lambda v: v.update(schema=True),
            lambda v: v["analysis"]["matrix"].update(unknown=0),
            lambda v: v["analysis"]["matrix"].update(values=[]),
            lambda v: v["analysis"]["matrix"].update(values=[0x7FC00000]),
            lambda v: v["analysis"]["equilibrium"].update(value=0x7F800000),
            lambda v: v["analysis"]["shallow"].update(nodes=True),
            lambda v: v["analysis"].update(unsupported=["unsupported move"]),
            lambda v: v["analysis"].update(omitted=[0, 1]),
            lambda v: v["analysis"].update(maximin=[1, 0]),
            lambda v: v["analysis"].update(turns=0),
            lambda v: v["analysis"]["stats"].update(split_cells=1),
        ]
        for mutation in mutations:
            value = valid_output()
            mutation(value)
            with self.subTest(value=value), self.assertRaises(bench.BenchmarkError):
                bench.validate_output(value)

    def test_bounds(self):
        for value in ("1", "3", "21", "22", "x", "2.0"):
            with self.subTest(pairs=value), self.assertRaises(argparse.ArgumentTypeError):
                bench.bounded_pairs(value)
        for value in ("0", "601", "nan", "inf", "-1", "x"):
            with self.subTest(timeout=value), self.assertRaises(argparse.ArgumentTypeError):
                bench.bounded_timeout(value)
        self.assertEqual(bench.bounded_pairs("20"), 20)
        self.assertEqual(bench.bounded_timeout("1"), 1.0)

    def test_balanced_order_and_warmups(self):
        runs = bench.schedule([{"name": "one"}, {"name": "two"}], 6)
        for name in ("one", "two"):
            selected = [r for r in runs if r["case"] == name]
            self.assertEqual([r["variant"] for r in selected[:2]], ["baseline", "candidate"])
            self.assertTrue(all(r["phase"] == "warmup" for r in selected[:2]))
            self.assertEqual([r["order"] for r in selected[2::2]], ["AB", "BA"] * 3)
            self.assertEqual([r["variant"] for r in selected[2:6]],
                             ["baseline", "candidate", "candidate", "baseline"])

    def test_summary_pairs_by_id_not_global_median_and_excludes_warmup(self):
        cases = [{"name": "one"}]
        records = []
        for planned, wall_ns in zip(bench.schedule(cases, 2), (999999, 1, 10, 20, 20, 100)):
            records.append(dict(planned, status="ok", wall_ns=wall_ns, cpu_seconds=None))
        result = bench.summary(records, cases, 2)[0]
        self.assertEqual(result["median_wall_ratio_candidate_over_baseline"], 1.1)
        self.assertEqual(result["min_wall_ratio_candidate_over_baseline"], 0.2)
        self.assertEqual(result["max_wall_ratio_candidate_over_baseline"], 2)
        with self.assertRaises(bench.BenchmarkError):
            bench.summary(records[:-1], cases, 2)
        with self.assertRaises(bench.BenchmarkError):
            bench.summary([], [], 2)

    def test_shared_fixtures_include_all_team_dependencies(self):
        cases, inputs = bench.load_cases("narrow")
        self.assertEqual([c["position"] for c in cases], ["max", "1"])
        self.assertEqual(len(inputs), 6)
        self.assertTrue(all(path.is_relative_to(bench.ROOT) for path in inputs))
        self.assertTrue(any(path.name == "gardevoir-braverilla.json" for path in inputs))
        smoke, smoke_inputs = bench.load_cases("smoke")
        self.assertEqual(smoke[0]["name"], "poison-heal")
        self.assertEqual(len(smoke_inputs), 1)


class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)

    def fake(self, name, *, value=None, behavior="ok"):
        path = self.directory / f"{name}.py"
        payload = json.dumps(valid_output() if value is None else value)
        path.write_text(
            "import json, os, sys, time\n"
            f"behavior = {behavior!r}\n"
            "print('captured stderr', file=sys.stderr, flush=True)\n"
            "if behavior == 'timeout':\n"
            " print('started', flush=True)\n"
            " time.sleep(30)\n"
            "if behavior == 'exit': sys.exit(7)\n"
            "if behavior == 'malformed':\n"
            " print('{invalid')\n"
            "elif behavior != 'empty':\n"
            f" print({payload!r})\n",
            encoding="utf-8",
        )
        return path

    def run_fake_pair(self, baseline, candidate, *, name="output"):
        args = argparse.Namespace(baseline_exe=baseline, candidate_exe=candidate,
                                  suite="smoke", threads=1, pairs=2,
                                  out_dir=self.directory / name, timeout=1)
        original_execute = bench.execute

        def python_executable(command, **kwargs):
            # Execute real child processes on both Windows and Linux without a shell.
            return original_execute([sys.executable, "-u", *command], **kwargs)

        with mock.patch.object(bench, "execute", side_effect=python_executable), \
                contextlib.redirect_stdout(io.StringIO()):
            exitcode = bench.run(args)
        result = json.loads((args.out_dir / "result.json").read_text(encoding="utf-8"))
        records = [json.loads(line) for line in (args.out_dir / "records.jsonl").read_text(encoding="utf-8").splitlines()]
        return exitcode, result, records, args.out_dir

    def test_success_real_processes_raw_output_and_provenance(self):
        baseline, candidate = self.fake("baseline"), self.fake("candidate")
        exitcode, result, records, out = self.run_fake_pair(baseline, candidate)
        self.assertEqual(exitcode, 0)
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["completed_invocations"], 6)
        self.assertEqual(len(result["cases"][0]["pairs"]), 2)
        self.assertEqual([r["variant"] for r in records],
                         ["baseline", "candidate", "baseline", "candidate", "candidate", "baseline"])
        self.assertEqual(len({r["pid"] for r in records}), 6)
        self.assertEqual(len({r["semantic_sha256"] for r in records}), 1)
        self.assertTrue(all((out / r["stdout"]).is_file() for r in records))
        self.assertTrue(all("captured stderr" in (out / r["stderr"]).read_text(encoding="utf-8") for r in records))
        manifest = json.loads((out / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["binary_sha256"]["baseline"], bench.sha256(baseline))
        self.assertEqual(manifest["environment"]["LAB_ENGINE_FACTORED"], "0")
        self.assertEqual(len(manifest["input_sha256"]), 1)
        self.assertIn("Median C/B ratio", (out / "summary.md").read_text(encoding="utf-8"))
        if os.name == "nt":
            self.assertTrue(all(r["cpu_seconds"] is None for r in records))

    def test_float_bit_or_work_counter_mismatch_fails_after_recording(self):
        for field in ("float_bit", "work_counter"):
            with self.subTest(field=field):
                value = valid_output()
                if field == "float_bit":
                    value["analysis"]["matrix"]["values"][0] = 1
                else:
                    value["analysis"]["stats"]["tt_hits"] = 1
                exitcode, result, records, _ = self.run_fake_pair(
                    self.fake("baseline"), self.fake("candidate", value=value), name=field)
                self.assertEqual(exitcode, 1)
                self.assertEqual(result["status"], "failed")
                self.assertEqual(len(records), 2)
                self.assertEqual(records[-1]["status"], "validation_error")
                self.assertIn("mismatch", result["error"])
                self.assertNotIn("cases", result)

    def test_failure_empty_and_malformed_preserve_incremental_data(self):
        for behavior in ("exit", "empty", "malformed"):
            with self.subTest(behavior=behavior):
                exitcode, result, records, out = self.run_fake_pair(
                    self.fake("baseline"), self.fake("candidate", behavior=behavior), name=behavior)
                self.assertEqual(exitcode, 1)
                self.assertEqual(result["completed_invocations"], 2)
                self.assertEqual(len(records), 2)
                self.assertTrue((out / records[-1]["stderr"]).is_file())
                if behavior == "exit":
                    self.assertEqual(records[-1]["returncode"], 7)
                    self.assertEqual(records[-1]["status"], "process_error")
                else:
                    self.assertEqual(records[-1]["status"], "validation_error")

    def test_timeout_kills_reaps_and_preserves_stdout(self):
        original_popen = subprocess.Popen
        spawned = []

        def capture_process(*args, **kwargs):
            process = original_popen(*args, **kwargs)
            spawned.append(process)
            return process

        with mock.patch.object(bench.subprocess, "Popen", side_effect=capture_process):
            exitcode, result, records, out = self.run_fake_pair(
                self.fake("baseline"), self.fake("candidate", behavior="timeout"))
        self.assertEqual(exitcode, 1)
        self.assertEqual(records[-1]["status"], "timeout")
        self.assertEqual(len(records), 2)
        self.assertIn("started", (out / records[-1]["stdout"]).read_text(encoding="utf-8"))
        self.assertTrue(all(process.poll() is not None for process in spawned))
        self.assertEqual(result["status"], "failed")

    def test_existing_output_is_preserved(self):
        out = self.directory / "existing"
        out.mkdir()
        sentinel = out / "sentinel.txt"
        sentinel.write_text("keep")
        with self.assertRaises(FileExistsError):
            bench.run(argparse.Namespace(out_dir=out))
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")

    def test_missing_binary_records_failure_before_any_launch(self):
        args = argparse.Namespace(out_dir=self.directory / "missing", suite="smoke",
                                  threads=1, pairs=2, timeout=1,
                                  baseline_exe=self.directory / "missing.exe",
                                  candidate_exe=self.directory / "also-missing.exe")
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(bench.run(args), 1)
        result = json.loads((args.out_dir / "result.json").read_text(encoding="utf-8"))
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["completed_invocations"], 0)
        self.assertIn("No timing comparison", (args.out_dir / "summary.md").read_text(encoding="utf-8"))

    def test_invalid_utf8_input_records_failure_instead_of_running(self):
        invalid = self.directory / "invalid.json"
        invalid.write_bytes(b"\xff")
        args = argparse.Namespace(out_dir=self.directory / "invalid-utf8", suite="smoke",
                                  threads=1, pairs=2, timeout=1)
        with mock.patch.object(bench, "load_cases", side_effect=lambda _: invalid.read_text(encoding="utf-8")), \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(bench.run(args), 1)
        result = json.loads((args.out_dir / "result.json").read_text(encoding="utf-8"))
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_type"], "UnicodeDecodeError")
        self.assertEqual(result["completed_invocations"], 0)
        self.assertIn("No timing comparison", (args.out_dir / "summary.md").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
