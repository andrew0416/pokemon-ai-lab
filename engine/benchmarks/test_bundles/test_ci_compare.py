"""Controller unit tests use inert fixtures and mocked subprocesses; never Cargo."""
import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("test_bundle_compare", Path(__file__).with_name("ci_compare.py"))
driver = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(driver)


def put(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


class SourceFixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.roots = {label: self.root / label for label in driver.LABELS}
        original_path = "engine/scenario/tests/first.rs"
        manifest = '[package]\nname = "lab-scenario"\nversion = "0.1.0"\n[features]\noriginal = []\n'
        for root in self.roots.values():
            put(root / original_path, '#[test]\nfn plain() {}\n')
            put(root / "engine/scenario/Cargo.toml", manifest)
            put(root / "engine/Cargo.lock", 'version = 4\n')
            put(root / driver.MARKER_PATH, 'pub fn unchanged() {}\n')
            put(root / "fixtures/team.json", '{"same":true}\n')
        candidate_manifest = manifest.replace('[package]\n', '[package]\nautotests = false\n')
        candidate_manifest += '\n[[test]]\nname = "bundle_one"\npath = "tests/bundle_one.rs"\n'
        put(self.roots["candidate"] / "engine/scenario/Cargo.toml", candidate_manifest)
        put(self.roots["candidate"] / "engine/scenario/tests/bundle_one.rs",
            '// only wrapper modules\n#[path = "first.rs"]\nmod first;\n')
        self.mapping_value = {
            "schema_version": 1, "source_sha": "a" * 40,
            "packages": {"lab-scenario": {"candidate_targets": [{"name": "bundle_one", "path": "tests/bundle_one.rs"}]}},
            "entries": [{"package": "lab-scenario", "package_directory": "scenario", "original_target": "first",
                         "original_path": original_path, "source_sha256": driver.digest(self.roots["baseline"] / original_path),
                         "candidate_target": "bundle_one", "module_prefix": "first::", "isolated": False,
                         "original_tests": ["plain"], "candidate_tests": ["first::plain"]}],
        }
        self.mapping = driver.Mapping(self.mapping_value)


class SourceTests(SourceFixture):
    def test_only_manifest_test_discovery_and_declared_wrappers_may_differ(self):
        inventories, difference = driver.validate_sources(self.roots, self.mapping)
        self.assertEqual(difference["changed"], ["engine/scenario/Cargo.toml"])
        self.assertEqual(difference["added"], ["engine/scenario/tests/bundle_one.rs"])
        self.assertEqual(inventories["baseline"]["engine/Cargo.lock"], inventories["candidate"]["engine/Cargo.lock"])
        # A checkout's Git metadata is outside the source comparison and work copy.
        put(self.roots["baseline"] / ".git/config", "baseline metadata")
        put(self.roots["candidate"] / ".git/config", "different metadata")
        self.assertEqual(driver.validate_sources(self.roots, self.mapping)[0], inventories)

    def test_engine_fixture_original_test_lock_and_feature_changes_are_rejected(self):
        paths = [driver.MARKER_PATH, "fixtures/team.json", "engine/scenario/tests/first.rs", "engine/Cargo.lock",
                 "engine/scenario/Cargo.toml"]
        for name in paths:
            path = self.roots["candidate"] / name
            original = path.read_bytes()
            try:
                if name.endswith("Cargo.toml"):
                    path.write_bytes(original.replace(b"original = []", b'original = ["other"]'))
                else:
                    path.write_bytes(original + b"changed\n")
                with self.subTest(source=name), self.assertRaises(ValueError):
                    driver.validate_sources(self.roots, self.mapping)
            finally:
                path.write_bytes(original)

    def test_wrapper_cannot_inject_an_extra_test_or_non_module_code(self):
        path = self.roots["candidate"] / "engine/scenario/tests/bundle_one.rs"
        path.write_text(path.read_text() + "#[test]\nfn synthetic() {}\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Wrapper contains more"):
            driver.validate_sources(self.roots, self.mapping)

    def test_marker_preserves_every_other_file_and_records_original_and_changed_bytes(self):
        original, _ = driver.validate_sources(self.roots, self.mapping)
        output = self.root / "marker-evidence"
        changed = driver.add_marker(self.roots, original, output)
        self.assertEqual((output / "changed-lib.rs").read_bytes(), (output / "original-lib.rs").read_bytes() + driver.MARKER)
        for label in driver.LABELS:
            self.assertEqual([name for name in original[label] if original[label][name] != changed[label][name]], [driver.MARKER_PATH])
        put(self.roots["candidate"] / "fixtures/team.json", "changed elsewhere")
        with self.assertRaisesRegex(ValueError, "Unexpected source/input"):
            driver.require_unchanged(self.roots, changed)

    def test_normalization_removes_only_declared_prefix_and_preserves_nested_test_names(self):
        self.assertEqual(self.mapping.normalize("candidate", "lab-scenario", "bundle_one", "first::nested::case"),
                         ("lab-scenario", "first", "nested::case"))
        self.assertEqual(self.mapping.normalize("baseline", "lab-scenario", "first", "nested::case"),
                         ("lab-scenario", "first", "nested::case"))
        self.assertEqual(self.mapping.normalize("candidate", "lab-engine", "lab_engine", "unit::case"),
                         ("lab-engine", "lab_engine", "unit::case"))
        with self.assertRaises(ValueError):
            self.mapping.normalize("candidate", "lab-scenario", "bundle_one", "unknown::case")
        value = copy.deepcopy(self.mapping_value)
        value["entries"].append(copy.deepcopy(value["entries"][0]))
        with self.assertRaisesRegex(ValueError, "Duplicate original"):
            driver.Mapping(value)

    def test_existing_or_nested_output_and_work_are_rejected_before_any_process(self):
        for output, work in ((self.root / "exists", self.root / "work"),
                             (self.roots["baseline"] / "results", self.root / "work"),
                             (self.root / "out", self.root / "out/work")):
            if output.name == "exists":
                output.mkdir()
            args = argparse.Namespace(baseline=self.roots["baseline"], candidate=self.roots["candidate"],
                                      mapping=self.root / "not-read.json", out_dir=output, work_dir=work)
            with self.subTest(output=output), patch.object(driver, "run_process") as execute:
                with self.assertRaises((ValueError, FileExistsError)):
                    driver.compare(args)
                execute.assert_not_called()


class LibtestTests(unittest.TestCase):
    def test_runtime_listing_not_static_annotation_scan_is_authoritative(self):
        self.assertEqual(driver.list_tests("macro_generated::case: test\nplain: test\n\n2 tests, 0 benchmarks\n"),
                         ["macro_generated::case", "plain"])
        self.assertEqual(driver.list_tests("0 tests, 0 benchmarks\n"), [])
        cases = ["same: test\nsame: test\n2 tests, 0 benchmarks\n",
                 "plain: test\n2 tests, 0 benchmarks\n", "bench: benchmark\n0 tests, 1 benchmark\n",
                 "plain: test\n", "unexpected output\n0 tests, 0 benchmarks\n"]
        for text in cases:
            with self.subTest(text=text), self.assertRaises(ValueError):
                driver.list_tests(text)

    def test_every_test_must_pass_exactly_once_and_skips_or_duplicates_fail(self):
        text = "running 2 tests\ntest first ... ok\ntest nested::second ... ok\n\n"
        text += "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.0s\n"
        self.assertEqual(driver.passed_tests(text, ["first", "nested::second"]), ["first", "nested::second"])
        for changed in (text.replace("nested::second", "first"), text.replace("0 ignored", "1 ignored"),
                        text.replace("0 filtered out", "1 filtered out"), text.replace("first ... ok", "first ... ignored"),
                        text.replace("2 passed", "1 passed"), text + "test first ... ok\n"):
            with self.subTest(text=changed), self.assertRaises(ValueError):
                driver.passed_tests(changed, ["first", "nested::second"])

    def test_doctests_are_a_separate_passing_gate_and_allow_zero_doc_tests(self):
        zero = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        self.assertEqual(driver.doc_result(zero)["passed"], 0)
        self.assertEqual(driver.doc_result(zero + zero.replace("0 passed", "3 passed"))["passed"], 3)
        for text in ("", zero.replace("0 ignored", "1 ignored"), zero + "test result: FAILED.\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                driver.doc_result(text)

    def test_commands_include_all_packages_common_p8g_and_compile_timings(self):
        command = driver.compile_command()
        self.assertEqual(command.count("--timings"), 1)
        self.assertIn("--message-format=json", command)
        self.assertIn("--no-run", command)
        self.assertEqual(command[-2:], ["--features", "lab-engine/experiment-hurt-readers"])
        self.assertEqual([command[index + 1] for index, item in enumerate(command) if item == "-p"], list(driver.PACKAGES))
        self.assertNotIn("--no-run", driver.compile_command(doc=True))
        self.assertIn("--doc", driver.compile_command(doc=True))


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.source, self.target = self.root / "source", self.root / "target"
        self.src = self.source / "engine/core/src/lib.rs"
        put(self.src, "pub fn fixture() {}")
        self.executable = self.target / "release/deps/lab_engine-fixture"
        put(self.executable, "inert executable")
        self.decl = {"name": "lab_engine", "kind": ["lib"], "src_path": str(self.src), "test": True}
        self.packages = {"core-id": {"name": "lab-engine", "cwd": str(self.source / "engine/core"), "targets": [self.decl]}}
        self.artifact = {"reason": "compiler-artifact", "package_id": "core-id", "profile": {"test": True},
                         "target": self.decl, "executable": str(self.executable), "fresh": False}

    def stream(self, *items):
        return "\n".join(json.dumps(item) for item in (*items, {"reason": "build-finished", "success": True}))

    def test_test_profile_and_executable_select_real_unit_artifact_and_capture_hash(self):
        non_test = copy.deepcopy(self.artifact)
        non_test["profile"]["test"] = False
        result = driver.collect_binaries(self.stream(non_test, self.artifact), self.packages, self.source, self.target)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["sha256"], driver.digest(self.executable))
        self.assertEqual(result[0]["cwd"], str(self.source / "engine/core"))
        self.assertEqual(result[0]["kind"], ["lib"])
        self.assertFalse(result[0]["cargo_fresh"])

    def test_missing_duplicate_wrong_package_source_or_target_artifact_is_rejected(self):
        changes = ({"package_id": "other-id"}, {"executable": str(self.src)},
                   {"target": {**self.decl, "src_path": str(self.root / "outside.rs")}},
                   {"target": {**self.decl, "name": "wrong-target"}})
        for update in changes:
            item = {**self.artifact, **update}
            with self.subTest(update=update), self.assertRaises(ValueError):
                driver.collect_binaries(self.stream(item), self.packages, self.source, self.target)
        with self.assertRaises(ValueError):
            driver.collect_binaries(self.stream(self.artifact, self.artifact), self.packages, self.source, self.target)
        with self.assertRaises(ValueError):
            driver.collect_binaries(json.dumps(self.artifact), self.packages, self.source, self.target)

    def test_unchanged_bin_or_unit_target_cannot_disappear_from_compiler_stream(self):
        missing_bin = {"name": "unchanged_tool", "kind": ["bin"], "src_path": str(self.src), "test": True}
        self.packages["core-id"]["targets"].append(missing_bin)
        with self.assertRaisesRegex(ValueError, "every enabled metadata test target"):
            driver.collect_binaries(self.stream(self.artifact), self.packages, self.source, self.target)

    def test_metadata_uses_each_selected_package_manifest_directory_as_binary_cwd(self):
        packages = []
        for name in driver.PACKAGES:
            manifest = self.source / "engine" / name.removeprefix("lab-") / "Cargo.toml"
            put(manifest, '[package]\nname = "' + name + '"\n')
            packages.append({"name": name, "id": name + "-id", "manifest_path": str(manifest), "targets": []})
        data = {"packages": packages, "workspace_members": [item["id"] for item in packages]}
        result = driver.package_metadata(json.dumps(data), self.source)
        self.assertEqual(result["lab-scenario-id"]["cwd"], str(self.source / "engine/scenario"))
        outside = self.root / "outside/Cargo.toml"
        put(outside, "outside")
        data["packages"][0]["manifest_path"] = str(outside)
        with self.assertRaisesRegex(ValueError, "outside the selected workspace"):
            driver.package_metadata(json.dumps(data), self.source)


class StageGateTests(SourceFixture):
    def test_roster_mismatch_stops_before_any_test_execution(self):
        targets = {label: self.root / ("target-" + label) for label in driver.LABELS}
        with patch.object(driver, "timed_command", return_value=({}, "unused compiler JSON")), \
                patch.object(driver, "collect_binaries", return_value=[]), \
                patch.object(driver, "binary_roster", side_effect=[[("lab-scenario", "first", "one")],
                                                                    [("lab-scenario", "first", "other")]]), \
                patch.object(driver, "run_binaries") as execute:
            with self.assertRaisesRegex(ValueError, "canonical test roster differs"):
                driver.stage("cold", self.roots, targets, {label: {} for label in driver.LABELS},
                             self.mapping, self.root / "evidence")
        execute.assert_not_called()
        self.assertTrue((self.root / "evidence/cold/rosters.json").is_file())

    def test_core_edit_requires_actual_nonfresh_core_compiler_artifact(self):
        targets = {label: self.root / ("target-" + label) for label in driver.LABELS}
        for path in targets.values():
            path.mkdir()
        artifact = {"package": "lab-engine", "kind": ["lib"], "source": driver.MARKER_PATH, "cargo_fresh": True}
        with patch.object(driver, "timed_command", return_value=({}, "unused compiler JSON")), \
                patch.object(driver, "collect_binaries", return_value=[artifact]), \
                patch.object(driver, "binary_roster") as enumerate_tests:
            with self.assertRaisesRegex(ValueError, "freshly rebuilt core"):
                driver.stage("core-edit", self.roots, targets, {label: {} for label in driver.LABELS},
                             self.mapping, self.root / "evidence")
        enumerate_tests.assert_not_called()
        self.assertTrue((self.root / "evidence/core-edit/baseline/compile/rebuild-evidence.json").is_file())


class ProcessTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.env = driver.environment(self.root / "target")

    def process(self, *, returncode=0, timeout=False, cpu="1.25 0.50\n"):
        class FakeProcess:
            pid = 12345

            def __init__(self, argv, **kwargs):
                self.returncode = None
                self.waited = False
                self.expected = returncode
                Path(argv[argv.index("-o") + 1]).write_text(cpu, encoding="utf-8")
                kwargs["stdout"].write(b"raw standard output\n")
                kwargs["stderr"].write(b"raw standard error\n")

            def wait(self, timeout=None):
                if timeout is not None and not self.waited:
                    self.waited = True
                    if self_timeout:
                        raise subprocess.TimeoutExpired("fixture", timeout)
                self.returncode = self.expected
                return self.returncode

            def poll(self):
                return self.returncode

        self_timeout = timeout
        return FakeProcess

    def test_success_preserves_raw_streams_cpu_wall_and_exact_command(self):
        with patch.object(driver.subprocess, "Popen", self.process()):
            receipt, text = driver.run_process(["inert", "argument"], self.root, self.env, self.root / "ok")
        self.assertEqual(text, "raw standard output\n")
        self.assertEqual(receipt["cpu_seconds"], 1.75)
        self.assertGreaterEqual(receipt["wall_seconds"], 0)
        self.assertEqual(receipt["argv"], ["inert", "argument"])
        self.assertEqual((self.root / "ok.stderr").read_bytes(), b"raw standard error\n")
        self.assertEqual(receipt["artifacts"]["stdout"]["sha256"], driver.digest(self.root / "ok.stdout"))
        with self.assertRaises(FileExistsError):
            driver.run_process(["inert"], self.root, self.env, self.root / "ok")

    def test_nonzero_and_malformed_cpu_fail_and_preserve_partial_evidence(self):
        for index, values in enumerate(({"returncode": 7}, {"cpu": "invalid"}, {"cpu": "NaN 0\n"})):
            prefix = self.root / f"failure-{index}"
            with self.subTest(values=values), patch.object(driver.subprocess, "Popen", self.process(**values)):
                with self.assertRaises((RuntimeError, ValueError)):
                    driver.run_process(["inert"], self.root, self.env, prefix)
            receipt = json.loads(prefix.with_suffix(".json").read_text())
            self.assertEqual(receipt["status"], "failed")
            self.assertTrue(prefix.with_suffix(".stdout").is_file())

    def test_timeout_kills_the_entire_group_and_preserves_failure_receipt(self):
        with patch.object(driver.subprocess, "Popen", self.process(timeout=True)), \
                patch.object(driver.os, "killpg", create=True) as kill, \
                patch.object(driver.signal, "SIGKILL", 9, create=True):
            with self.assertRaisesRegex(RuntimeError, "process group terminated"):
                driver.run_process(["inert"], self.root, self.env, self.root / "timeout", timeout=1)
        kill.assert_called_once_with(12345, 9)
        receipt = json.loads((self.root / "timeout.json").read_text())
        self.assertTrue(receipt["timed_out"])
        self.assertEqual(receipt["status"], "failed")


if __name__ == "__main__":
    unittest.main()
