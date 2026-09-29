"""Peak-RSS artifact/cleanup tests using tiny Python children and mocked GNU time.

No Rust build, engine execution, real memory benchmark, or network is required.
"""

import argparse
import contextlib
import copy
import hashlib
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

SPEC = importlib.util.spec_from_file_location("paired_memory", Path(__file__).with_name("memory.py"))
memory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(memory)
bench = memory.bench


def valid_output():
    shallow = {"decision": "Turn", "ours": "[Turn]", "theirs": "[Turn]",
               "matrix": {"rows": 1, "cols": 1, "values": [0]},
               "equilibrium": {"rows": [1065353216], "cols": [1065353216],
                               "value": 0, "exploitability": 0, "iterations": 0},
               "maximin": [0, 0], "nodes": 1, "turns": 1, "depth": 1,
               "unsupported": [], "omitted": [0, 0]}
    deep = copy.deepcopy(shallow)
    deep.update(depth=2, beam=2, outcome_cap=2, levels=[{"beam": 2, "outcomes": 2}],
                shallow=shallow, stats={field: 0 for field in bench.STATS_FIELDS})
    return {"schema": 1, "state_restored": True, "analysis": deep}


class ParsingTests(unittest.TestCase):
    def test_only_positive_single_integer_peak_is_accepted(self):
        for text in ("1", "123456\n"):
            self.assertEqual(memory.parse_peak_rss(text), int(text))
        for text in ("", "0\n", "-1\n", "1.5\n", "10 KiB\n", "1\n2\n", "1\n\n",
                     " 1\n", "Command exited with non-zero status 7\n1200\n", "nan"):
            with self.subTest(text=text), self.assertRaises(bench.BenchmarkError):
                memory.parse_peak_rss(text)

    def test_linux_and_gnu_time_identity_are_required(self):
        with mock.patch.object(memory.sys, "platform", "win32"), self.assertRaises(bench.BenchmarkError):
            memory.time_identity()
        with tempfile.TemporaryDirectory() as folder:
            tool = Path(folder) / "time"
            tool.write_text("test tool", encoding="utf-8")
            for text, code, accepted in (("time (GNU Time) 1.9\n", 0, True),
                                         ("BusyBox time", 0, False), ("GNU time", 1, False)):
                response = subprocess.CompletedProcess([], code, text, "")
                with self.subTest(text=text), mock.patch.object(memory.sys, "platform", "linux"), \
                        mock.patch.object(memory, "GNU_TIME", tool), \
                        mock.patch.object(memory.subprocess, "run", return_value=response) as run:
                    if accepted:
                        result = memory.time_identity()
                        self.assertEqual(result["sha256"], bench.sha256(tool))
                        self.assertEqual(result["unit"], "KiB")
                        self.assertEqual(run.call_args.kwargs["env"]["LC_ALL"], "C")
                    else:
                        with self.assertRaises(bench.BenchmarkError):
                            memory.time_identity()

    def test_schedule_is_two_pairs_without_timing_warmups(self):
        schedule = memory.memory_schedule([{"name": "one"}, {"name": "two"}])
        self.assertEqual(len(schedule), 8)
        self.assertEqual([r["variant"] for r in schedule[:4]],
                         ["baseline", "candidate", "candidate", "baseline"])
        self.assertEqual([r["order"] for r in schedule[:4]], ["AB", "AB", "BA", "BA"])
        self.assertTrue(all(r["phase"] == "memory" for r in schedule))


class MemoryPassTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tool = self.root / "mock-gnu-time"
        self.tool.write_text("GNU time test stand-in", encoding="utf-8")
        self.identity = {"path": str(self.tool.resolve()), "sha256": bench.sha256(self.tool),
                         "version": "mock GNU time", "format": "%M", "unit": "KiB"}

    def child(self, label, *, behavior="ok", value=None):
        path = self.root / (label + ".py")
        value = valid_output() if value is None else value
        path.write_text(
            "import sys, time\n"
            "print('test stderr', file=sys.stderr, flush=True)\n"
            f"behavior={behavior!r}\n"
            "if behavior == 'timeout': time.sleep(30)\n"
            "if behavior == 'exit': sys.exit(7)\n"
            f"text='{{invalid' if behavior == 'malformed' else {json.dumps(value)!r}\n"
            "sys.stdout.buffer.write((text + '\\n').encode('utf-8')); sys.stdout.buffer.flush()\n",
            encoding="utf-8")
        return path

    def fixture(self, baseline, candidate, *, suite="smoke"):
        source = self.root / "timing"
        source.mkdir()
        (source / "raw").mkdir()
        cases, fixtures = bench.load_cases(suite)
        exes = {"baseline": baseline.resolve(), "candidate": candidate.resolve()}
        raw = (json.dumps(valid_output()) + "\n").encode()
        semantic_hash = hashlib.sha256(bench.canonical(valid_output()).encode()).hexdigest()
        schedule = bench.schedule(cases, 2)
        manifest = {"schema": 1, "suite": suite, "threads": 1, "cases": cases,
                    "pairs_per_case": 2, "warmups_per_case_and_build": 1,
                    "shared_checkout": str(bench.ROOT), "schedule": schedule,
                    "environment": memory.fixed_environment(1), "machine": bench.machine_info(),
                    "binary_paths": {k: str(v) for k, v in exes.items()},
                    "binary_sha256": {k: bench.sha256(v) for k, v in exes.items()},
                    "input_sha256": {p.relative_to(bench.ROOT).as_posix(): bench.sha256(p) for p in fixtures},
                    "harness_sha256": bench.sha256(bench.HERE / "harness.rs"),
                    "suites_sha256": bench.sha256(bench.HERE / "suites.json"),
                    "runner_sha256": bench.sha256(Path(bench.__file__))}
        records = []
        by_name = {case["name"]: case for case in cases}
        for index, planned in enumerate(schedule):
            case = by_name[planned["case"]]
            stdout, stderr = f"raw/{index:04d}.stdout", f"raw/{index:04d}.stderr"
            (source / stdout).write_bytes(raw)
            (source / stderr).write_bytes(b"")
            records.append(dict(planned, invocation=index, status="ok", returncode=0,
                                stdout=stdout, stderr=stderr, semantic_sha256=semantic_hash,
                                wall_ns=100, cpu_seconds=0.00001,
                                command=[str(exes[planned["variant"]]), str(bench.ROOT / case["scenario"]),
                                         "1", case["position"]]))
        result = {"schema": 1, "status": "ok", "suite": suite, "threads": 1,
                  "pairs_per_case": 2, "completed_invocations": len(records),
                  "cases": [{"case": c["name"]} for c in cases]}
        bench.write_json(source / "manifest.json", manifest)
        bench.write_json(source / "result.json", result)
        (source / "records.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records), encoding="utf-8")
        return argparse.Namespace(baseline_exe=baseline, candidate_exe=candidate, suite=suite,
                                  threads=1, timeout=1, benchmark_dir=source, out_dir=self.root / "memory")

    def invoke(self, args, *, rss="12000\n", after_execute=None):
        execute = bench.execute

        def fake_time(command, **kwargs):
            self.assertEqual(command[:3], [str(self.tool.resolve()), "-f", "%M"])
            self.assertEqual(command[3], "-o")
            self.assertEqual(command[5], "--")
            outcome = execute([sys.executable, "-u", *command[6:]], **kwargs)
            Path(command[4]).write_text(rss if outcome["status"] == "ok" else "failed child\n",
                                       encoding="ascii")
            if after_execute is not None:
                after_execute(command, outcome)
            return outcome

        with mock.patch.object(memory, "time_identity", return_value=self.identity), \
                mock.patch.object(bench, "execute", side_effect=fake_time) as calls, \
                contextlib.redirect_stdout(io.StringIO()):
            code = memory.run(args)
        result = memory.read_json(args.out_dir / "result.json")
        journal = args.out_dir / "records.jsonl"
        records = [json.loads(line) for line in journal.read_text(encoding="utf-8").splitlines()] if journal.exists() else []
        return code, result, records, calls.call_count

    def test_success_eight_fresh_processes_provenance_and_no_timing_fields(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"), suite="narrow")
        previous = {p: bench.sha256(p) for p in args.benchmark_dir.rglob("*") if p.is_file()}
        code, result, records, calls = self.invoke(args)
        self.assertEqual((code, result["status"], calls), (0, "ok", 8))
        self.assertEqual(len(records), 8)
        self.assertEqual(len({r["pid"] for r in records}), 8)
        self.assertEqual([r["variant"] for r in records[:4]], ["baseline", "candidate", "candidate", "baseline"])
        self.assertFalse(result["timing_measurement"])
        self.assertEqual(len(result["cases"]), 2)
        self.assertEqual(result["cases"][0]["baseline_peak_rss_kib"]["samples"], [12000, 12000])
        for record in records:
            self.assertFalse({"wall_ns", "cpu_seconds"} & record.keys())
            self.assertEqual(record["peak_rss_kib"], 12000)
            self.assertEqual(set(record["raw_sha256"]), {"stdout", "stderr", "timefile"})
            for name, digest in record["raw_sha256"].items():
                self.assertEqual(bench.sha256(args.out_dir / record[name]), digest)
        manifest = memory.read_json(args.out_dir / "manifest.json")
        self.assertEqual(manifest["binary_sha256"]["candidate"], bench.sha256(args.candidate_exe))
        self.assertEqual(manifest["discarded_execute_fields"], ["wall_ns", "cpu_seconds"])
        self.assertEqual({p: bench.sha256(p) for p in previous}, previous)
        self.assertIn("never merged", (args.out_dir / "summary.md").read_text())

    def test_mismatch_retains_failed_record_and_raw_output(self):
        changed = valid_output()
        changed["analysis"]["matrix"]["values"][0] = 2147483648
        args = self.fixture(self.child("baseline"), self.child("candidate", value=changed))
        code, result, records, calls = self.invoke(args)
        self.assertEqual((code, result["status"], calls), (1, "failed", 2))
        self.assertEqual(records[-1]["status"], "validation_error")
        self.assertIn("differs from timing", records[-1]["error"])
        self.assertNotIn("cases", result)
        self.assertTrue((args.out_dir / records[-1]["timefile"]).is_file())

    def test_nonzero_exit_retains_diagnostics(self):
        args = self.fixture(self.child("baseline"), self.child("candidate", behavior="exit"))
        code, result, records, calls = self.invoke(args)
        self.assertEqual((code, result["status"], calls), (1, "failed", 2))
        self.assertEqual(records[-1]["status"], "process_error")
        self.assertEqual(records[-1]["returncode"], 7)
        self.assertIn("test stderr", (args.out_dir / records[-1]["stderr"]).read_text())

    def test_timeout_delegates_process_group_cleanup_and_preserves_files(self):
        args = self.fixture(self.child("baseline"), self.child("candidate", behavior="timeout"))
        with mock.patch.object(bench, "terminate_and_reap", wraps=bench.terminate_and_reap) as cleanup:
            code, result, records, calls = self.invoke(args)
        self.assertEqual((code, result["status"], calls), (1, "failed", 2))
        self.assertEqual(records[-1]["status"], "timeout")
        cleanup.assert_called_once()
        self.assertIsNotNone(records[-1]["returncode"])
        self.assertTrue((args.out_dir / records[-1]["stderr"]).is_file())

    def test_malformed_output_is_failure(self):
        args = self.fixture(self.child("baseline"), self.child("candidate", behavior="malformed"))
        code, result, records, _ = self.invoke(args)
        self.assertEqual((code, result["status"]), (1, "failed"))
        self.assertEqual(records[-1]["status"], "validation_error")

    def test_invalid_time_file_is_not_success(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"))
        code, result, records, calls = self.invoke(args, rss="0\n")
        self.assertEqual((code, result["status"], calls), (1, "failed", 1))
        self.assertIn("zero peak RSS", records[-1]["error"])

    def test_failed_timing_is_rejected_before_child_execution(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"))
        path = args.benchmark_dir / "result.json"
        value = memory.read_json(path)
        value["status"] = "failed"
        bench.write_json(path, value)
        code, result, records, calls = self.invoke(args)
        self.assertEqual((code, result["status"], calls, records), (1, "failed", 0, []))

    def test_wrong_executable_hash_is_rejected_before_child_execution(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"))
        with args.candidate_exe.open("a", encoding="utf-8") as stream:
            stream.write("# changed after timing\n")
        code, result, _, calls = self.invoke(args)
        self.assertEqual((code, calls), (1, 0))
        self.assertIn("executable hash differs", result["error"])

    def test_wrong_threads_schedule_and_escaped_raw_path_are_rejected(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"))
        args.threads = 2
        code, result, _, calls = self.invoke(args)
        self.assertEqual((code, calls), (1, 0))
        self.assertIn("threads differs", result["error"])
        args.threads = 1
        path = args.benchmark_dir / "records.jsonl"
        original = path.read_text(encoding="utf-8")
        records = [json.loads(line) for line in original.splitlines()]
        records[0]["stdout"] = "../baseline.py"
        path.write_text("".join(json.dumps(r) + "\n" for r in records), encoding="utf-8")
        args.out_dir = self.root / "escaped-result"
        code, result, _, calls = self.invoke(args)
        self.assertEqual((code, calls), (1, 0))
        self.assertIn("escapes", result["error"])
        path.write_text("\n".join(original.splitlines()[:-1]) + "\n", encoding="utf-8")
        args.out_dir = self.root / "short-schedule"
        code, result, _, calls = self.invoke(args)
        self.assertEqual((code, calls), (1, 0))
        self.assertIn("schedule", result["error"])

    def test_missing_time_file_and_midpass_source_change_fail_closed(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"))
        def remove_time(command, outcome):
            Path(command[4]).unlink()
        code, result, records, calls = self.invoke(args, after_execute=remove_time)
        self.assertEqual((code, calls, records[-1]["status"]), (1, 1, "validation_error"))
        args.out_dir = self.root / "changed-result"
        changed = False
        def mutate_binary(command, outcome):
            nonlocal changed
            if not changed:
                with args.baseline_exe.open("a", encoding="utf-8") as stream:
                    stream.write("# unit-test mutation\n")
                changed = True
        code, result, records, calls = self.invoke(args, after_execute=mutate_binary)
        self.assertEqual((code, calls, len(records)), (1, 4, 4))
        self.assertIn("changed during memory pass", result["error"])
        self.assertNotIn("cases", result)

    def test_existing_output_is_never_overwritten_and_timing_directory_is_protected(self):
        args = self.fixture(self.child("baseline"), self.child("candidate"))
        args.out_dir.mkdir()
        sentinel = args.out_dir / "keep.txt"
        sentinel.write_text("keep", encoding="utf-8")
        with self.assertRaises(FileExistsError):
            self.invoke(args)
        self.assertEqual(sentinel.read_text(), "keep")
        args.out_dir = args.benchmark_dir / "memory"
        with self.assertRaises(bench.BenchmarkError):
            self.invoke(args)
        self.assertFalse(args.out_dir.exists())


if __name__ == "__main__":
    unittest.main()
