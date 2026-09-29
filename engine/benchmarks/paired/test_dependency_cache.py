"""Dependency targets are Cargo inputs, never verified build/regression evidence.

All subprocesses are mocked. Test products are inert files in temporary folders;
no Rust compiler, regression executable, or benchmark is invoked.
"""
import copy
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch

import build_cache
import ci
import dependency_target
import test_build_cache as fixtures


class DependencyCacheTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        fixtures.setup_workspace(self.root)
        values = fixtures.environment(
            BUILD_CACHE_ENABLED="0", BUILD_CACHE_ALLOW_SAVE="0",
            BASELINE_DEPENDENCY_CACHE_READY="1", CANDIDATE_DEPENDENCY_CACHE_READY="1",
            RUSTFLAGS="-Ctarget-cpu=x86-64",
        )
        active = patch.dict(os.environ, values, clear=True)
        active.start()
        self.addCleanup(active.stop)
        self.sources = {label: (self.root / label / "engine/original-source.txt").read_bytes()
                        for label in fixtures.LABELS}

    def dependency_target(self, label):
        target = self.root / ("target-" + label)
        fingerprint = target / "release/.fingerprint/serde-fixturehash/lib-serde.json"
        fixtures.write_json(fingerprint, {"features": "[]"})
        library = target / "release/deps/libserde-fixturehash.rlib"
        library.parent.mkdir(parents=True)
        library.write_bytes(b"inert external dependency " + label.encode())
        build_script = target / "release/build/serde-fixturehash/build-script-build"
        build_script.parent.mkdir(parents=True)
        build_script.write_bytes(b"inert external dependency build script")
        return target

    def fake_process(self, calls, *, seeded=(), fail_label=None, wrong=None):
        owner = self

        class Process:
            def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                target = Path(env["CARGO_TARGET_DIR"])
                label = target.name.removeprefix("target-")
                owner.assertIn(label, fixtures.LABELS)
                owner.assertEqual(cwd, owner.root / label / "engine")
                owner.assertEqual(env["RUSTFLAGS"], "-Ctarget-cpu=x86-64")
                if label not in [previous for previous, _ in calls]:
                    if label in seeded:
                        owner.assertTrue((target / "release/deps/libserde-fixturehash.rlib").is_file())
                    else:
                        owner.assertFalse(target.exists(), "Rejected dependency target must be quarantined")
                calls.append((label, list(argv)))
                self.returncode = 19 if label == fail_label else 0
                fixtures.make_fingerprints(owner.root, label, wrong=wrong)
                binary = target / "release/examples" / ("ci_bench.exe" if os.name == "nt" else "ci_bench")
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b"inert freshly built benchmark " + label.encode())
                binary.chmod(0o755)
                timing = target / "cargo-timings" / ("cargo-timing-fixture-" + argv[1] + ".html")
                timing.parent.mkdir(parents=True, exist_ok=True)
                timing.write_text("fixture timing " + label + " " + json.dumps(argv), encoding="utf-8")
                if argv[1] == "test":
                    stdout.write("test newly_run_regression ... ok\n")
                    stdout.write("test result: ok. 141 passed; 0 failed; 0 ignored; "
                                 "0 measured; 0 filtered out; finished in 0.01s\n")
                stdout.flush()

            def wait(self, timeout):
                owner.assertEqual(timeout, ci.COMMAND_TIMEOUT_SECONDS)
                return self.returncode

        return Process

    def assert_sources_unchanged(self):
        for label, original in self.sources.items():
            self.assertEqual((self.root / label / "engine/original-source.txt").read_bytes(), original)

    def test_helper_accepts_external_dependencies_without_treating_them_as_a_verified_build(self):
        target = self.dependency_target("baseline")
        original = {path.relative_to(target): path.read_bytes() for path in target.rglob("*") if path.is_file()}
        self.assertTrue(dependency_target.prepare_target(self.root, "baseline"))
        receipt = fixtures.read_json(self.root / "ci-results/dependency-baseline.json")
        self.assertEqual(receipt["status"], "accepted")
        self.assertEqual({path.relative_to(target): path.read_bytes()
                          for path in target.rglob("*") if path.is_file()}, original)
        self.assertTrue((target / "release/build/serde-fixturehash/build-script-build").is_file())
        self.assertFalse((self.root / "ci-results/baseline-build-receipt.json").exists())
        self.assertFalse((self.root / "dependency-cache-quarantine").exists())

    def test_existing_target_requires_exact_ready_marker_and_is_not_modified_when_refused(self):
        target = self.dependency_target("baseline")
        original = (target / "release/deps/libserde-fixturehash.rlib").read_bytes()
        for ready in ("", "0", "true", "yes"):
            with self.subTest(ready=ready), patch.dict(os.environ, {"BASELINE_DEPENDENCY_CACHE_READY": ready}):
                with self.assertRaises(ValueError):
                    dependency_target.prepare_target(self.root, "baseline")
            self.assertTrue(target.is_dir())
            self.assertEqual((target / "release/deps/libserde-fixturehash.rlib").read_bytes(), original)
        self.assert_sources_unchanged()

    def test_marker_for_other_label_cannot_authorize_an_existing_target(self):
        self.dependency_target("baseline")
        with patch.dict(os.environ, {"BASELINE_DEPENDENCY_CACHE_READY": "", "CANDIDATE_DEPENDENCY_CACHE_READY": "1"}):
            with self.assertRaises(ValueError):
                dependency_target.prepare_target(self.root, "baseline")

    def test_absent_target_is_a_normal_cold_path_even_without_ready_marker(self):
        for ready in ("", "1"):
            with self.subTest(ready=ready), patch.dict(os.environ, {"BASELINE_DEPENDENCY_CACHE_READY": ready}):
                self.assertFalse(dependency_target.prepare_target(self.root, "baseline"))
            self.assertFalse((self.root / "target-baseline").exists())
            receipt = fixtures.read_json(self.root / "ci-results/dependency-baseline.json")
            self.assertEqual(receipt["status"], "absent")

    def test_workspace_crate_products_and_benchmarks_are_quarantined_not_reused(self):
        contaminants = (
            "release/.fingerprint/lab-engine-stale/lib-lab_engine.json",
            "release/.fingerprint/lab-search-stale/test-lib-lab_search.json",
            "release/.fingerprint/lab-scenario-stale/lib-lab_scenario.json",
            "release/deps/liblab_engine-stale.rlib",
            "release/deps/liblab_search-stale.rmeta",
            "release/deps/lab_engine-stale",
            "release/deps/lab_scenario-stale.exe",
            "release/deps/abilities_x-deadbeef",
            "release/deps/abilities_x-deadbeef.exe",
            "release/examples/ci_bench",
            "release/examples/ci_bench.exe",
        )
        for relative in contaminants:
            with self.subTest(product=relative), tempfile.TemporaryDirectory() as folder:
                root = Path(folder).resolve()
                (root / "ci-results").mkdir()
                target = root / "target-baseline"
                product = target / relative
                product.parent.mkdir(parents=True)
                product.write_bytes(b"stale workspace bytes")
                self.assertFalse(dependency_target.prepare_target(root, "baseline"))
                self.assertFalse(target.exists())
                receipt = fixtures.read_json(root / "ci-results/dependency-baseline.json")
                self.assertEqual(receipt["status"], "rejected")
                quarantines = list((root / "dependency-cache-quarantine").glob("baseline-*"))
                self.assertEqual(len(quarantines), 1)
                self.assertEqual((quarantines[0] / relative).read_bytes(), b"stale workspace bytes")
                self.assertFalse(quarantines[0].is_relative_to(root / "ci-results"))

    def test_dependency_link_detection_quarantines_without_following_or_deleting_input(self):
        target = self.dependency_target("baseline")
        link = target / "release/deps/libserde-fixturehash.rlib"
        original = link.read_bytes()
        original_lstat = Path.lstat
        inspected = []

        def link_metadata(path, *args, **kwargs):
            result = original_lstat(path, *args, **kwargs)
            if path == link:
                inspected.append(path)
                return os.stat_result((stat.S_IFLNK | (result.st_mode & 0o777), *result[1:]))
            return result

        # Host-independent link classification exercises the same rejection branch
        # without requiring Windows symlink privileges or skipping the test.
        with patch.object(Path, "lstat", link_metadata):
            self.assertFalse(dependency_target.prepare_target(self.root, "baseline"))
        self.assertTrue(inspected)
        self.assertFalse(target.exists())
        quarantines = list((self.root / "dependency-cache-quarantine").glob("baseline-*"))
        self.assertEqual(len(quarantines), 1)
        self.assertEqual((quarantines[0] / "release/deps/libserde-fixturehash.rlib").read_bytes(), original)
        self.assert_sources_unchanged()

    def test_valid_dependency_targets_execute_every_cargo_command_and_fresh_regressions(self):
        originals = {}
        for label in fixtures.LABELS:
            target = self.dependency_target(label)
            originals[label] = (target / "release/deps/libserde-fixturehash.rlib").read_bytes()
            stale_timing = target / "cargo-timings/old-cached.html"
            stale_timing.parent.mkdir()
            stale_timing.write_text("old run compilation report", encoding="utf-8")
        calls = []
        with patch.object(ci.subprocess, "Popen", self.fake_process(calls, seeded=fixtures.LABELS)), \
                patch.object(ci, "preserve_fingerprints", wraps=ci.preserve_fingerprints) as features, \
                patch.object(ci, "validate_prepared_turn", return_value={"status": "ok"}) as prepared:
            ci.build(self.root)
        self.assertEqual(calls, [(label, argv) for label in fixtures.LABELS
                                 for argv in ci.build_commands("narrow", "prepared-turn", label)])
        self.assertEqual([call.args[1] for call in features.call_args_list], list(fixtures.LABELS))
        prepared.assert_called_once_with(self.root)
        for label in fixtures.LABELS:
            receipt = fixtures.read_json(self.root / "ci-results" / (label + "-build-receipt.json"))
            self.assertEqual(receipt["status"], "success")
            self.assertIs(receipt["reused"], False)
            self.assertTrue(receipt["dependency_seeded"])
            self.assertEqual(receipt["commands"], [{"argv": argv, "returncode": 0}
                for argv in ci.build_commands("narrow", "prepared-turn", label)])
            log = (self.root / "ci-results" / (label + "-build.log")).read_text()
            self.assertIn("test newly_run_regression ... ok", log)
            self.assertNotIn("REUSED_VERIFIED_BUILD", log)
            actual = fixtures.read_json(self.root / "ci-results" / (label + "-features.json"))
            self.assertEqual(len(actual["fingerprints"]), 5)
            self.assertEqual(len(receipt["cargo_timings"]), 2)
            self.assertTrue(all("fixture" in item["path"] for item in receipt["cargo_timings"]))
            self.assertFalse((self.root / "ci-results/build-timings" / label / "old-cached.html").exists())
            for item in receipt["cargo_timings"]:
                self.assertEqual(fixtures.digest(self.root / "ci-results" / item["path"]), item["sha256"])
            self.assertEqual((self.root / ("target-" + label) /
                              "release/deps/libserde-fixturehash.rlib").read_bytes(), originals[label])
        self.assert_sources_unchanged()

    def test_rejected_workspace_target_falls_back_before_the_first_cargo_command(self):
        target = self.dependency_target("baseline")
        stale = target / "release/.fingerprint/lab-engine-old/lib-lab_engine.json"
        fixtures.write_json(stale, {"features": "[]"})
        stale_bytes = stale.read_bytes()
        calls = []
        with patch.object(ci.subprocess, "Popen", self.fake_process(calls)), \
                patch.object(ci, "validate_prepared_turn", return_value={"status": "ok"}):
            ci.build(self.root)
        self.assertEqual([label for label, _ in calls], ["baseline", "baseline", "candidate", "candidate"])
        self.assertFalse(stale.exists())
        quarantines = list((self.root / "dependency-cache-quarantine").glob("baseline-*"))
        self.assertEqual(len(quarantines), 1)
        self.assertEqual((quarantines[0] / stale.relative_to(target)).read_bytes(), stale_bytes)
        receipt = fixtures.read_json(self.root / "ci-results/baseline-build-receipt.json")
        self.assertFalse(receipt["reused"])
        self.assertFalse(receipt["dependency_seeded"])
        self.assertEqual(receipt["status"], "success")
        self.assert_sources_unchanged()

    def test_verified_exact_hits_still_skip_cargo_instead_of_becoming_dependency_builds(self):
        recipes = {label: fixtures.recipe(label) for label in fixtures.LABELS}
        exact_env = {"BUILD_CACHE_ENABLED": "1", "BUILD_CACHE_ALLOW_SAVE": "1"}
        for label in fixtures.LABELS:
            exact_env[label.upper() + "_CACHE_HIT"] = "true"
            exact_env[label.upper() + "_CACHE_MATCHED_KEY"] = build_cache.recipe_key(recipes[label])
        with patch.dict(os.environ, exact_env), patch.object(
                build_cache, "make_recipe", side_effect=lambda workspace, label: copy.deepcopy(recipes[label])):
            build_cache.plan(self.root)
            for label in fixtures.LABELS:
                evidence, _ = fixtures.make_cold_success(self.root, label)
                self.assertTrue(build_cache.seal(self.root, label, evidence))
                fixtures.remove_build_products(self.root, label)
            with patch.object(ci.subprocess, "Popen", side_effect=AssertionError("Exact hit must skip Cargo")), \
                    patch.object(ci, "validate_prepared_turn", return_value={"status": "ok"}) as prepared:
                ci.build(self.root)
        prepared.assert_called_once_with(self.root)
        for label in fixtures.LABELS:
            receipt = fixtures.read_json(self.root / "ci-results" / (label + "-build-receipt.json"))
            self.assertTrue(receipt["reused"])
            self.assertFalse(receipt["dependency_seeded"])
            self.assertNotIn("cargo_timings", receipt)
        self.assert_sources_unchanged()

    def test_dependency_ready_without_restored_target_still_builds_cold(self):
        calls = []
        with patch.object(ci.subprocess, "Popen", self.fake_process(calls)), \
                patch.object(ci, "validate_prepared_turn", return_value={"status": "ok"}):
            ci.build(self.root)
        self.assertEqual([label for label, _ in calls], ["baseline", "baseline", "candidate", "candidate"])
        self.assert_sources_unchanged()

    def test_seeded_cargo_failure_is_fatal_and_never_seals_success(self):
        self.dependency_target("baseline")
        calls = []
        with patch.object(ci.subprocess, "Popen", self.fake_process(calls, seeded=("baseline",), fail_label="baseline")), \
                patch.object(build_cache, "seal", wraps=build_cache.seal) as seal, \
                patch.object(ci, "validate_prepared_turn") as prepared:
            with self.assertRaisesRegex(RuntimeError, "build/test failed"):
                ci.build(self.root)
        self.assertEqual(len(calls), 1)
        seal.assert_not_called()
        prepared.assert_not_called()
        receipt = fixtures.read_json(self.root / "ci-results/baseline-build-receipt.json")
        self.assertEqual(receipt["status"], "failed")
        self.assertFalse(receipt["reused"])
        self.assertTrue(receipt["dependency_seeded"])
        self.assertEqual(len(receipt["cargo_timings"]), 1)
        self.assertTrue((self.root / "ci-results" / receipt["cargo_timings"][0]["path"]).is_file())
        self.assertTrue((self.root / "target-baseline/release/deps/libserde-fixturehash.rlib").is_file())

    def test_seeded_actual_feature_mismatch_is_fatal_even_after_cargo_success(self):
        self.dependency_target("baseline")
        calls = []
        wrong = ("lab-engine", "test-lib-lab_engine.json", [])
        with patch.object(ci.subprocess, "Popen", self.fake_process(calls, seeded=("baseline",), wrong=wrong)), \
                patch.object(build_cache, "seal", wraps=build_cache.seal) as seal, \
                patch.object(ci, "validate_prepared_turn") as prepared:
            with self.assertRaisesRegex(ValueError, "actual compiled feature activation"):
                ci.build(self.root)
        seal.assert_not_called()
        prepared.assert_not_called()
        self.assertEqual(len(calls), 2)
        receipt = fixtures.read_json(self.root / "ci-results/baseline-build-receipt.json")
        self.assertEqual(receipt["status"], "failed")
        self.assertFalse(receipt["reused"])


class PreparedSourceGuardTests(unittest.TestCase):
    """Cache-action changes are detected using saved source identity, without Git."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        fixtures.setup_workspace(self.root)
        harness = Path(ci.__file__).with_name("harness.rs")
        provenance = {"harness_sha256": fixtures.digest(harness), "sources": {}}
        self.commits = {"baseline": "a" * 40, "candidate": "b" * 40}
        self.tracked_status = {label: "" for label in fixtures.LABELS}
        self.untracked = {label: "engine/search/examples/ci_bench.rs" for label in fixtures.LABELS}
        self.inputs = ("engine/Cargo.lock", "engine/Cargo.toml", "engine/core/Cargo.toml",
                       "engine/search/Cargo.toml", "engine/scenario/Cargo.toml", "engine/py/Cargo.toml",
                       "engine/.cargo/config.toml")
        for label in fixtures.LABELS:
            root = self.root / label
            for name in self.inputs:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("identity fixture " + name + "\n", encoding="utf-8")
            injected = root / "engine/search/examples/ci_bench.rs"
            injected.parent.mkdir(parents=True)
            injected.write_bytes(harness.read_bytes())
            provenance["sources"][label] = {
                "commit": self.commits[label], "lock_sha256": fixtures.digest(root / "engine/Cargo.lock"),
                "workspace_manifest_sha256": fixtures.digest(root / "engine/Cargo.toml"),
                "search_manifest_sha256": fixtures.digest(root / "engine/search/Cargo.toml"),
                "package_manifests": {name: fixtures.digest(root / "engine" / name / "Cargo.toml")
                                      for name in ("core", "scenario", "py")},
                "cargo_configuration": {"engine/.cargo/config.toml": fixtures.digest(root / "engine/.cargo/config.toml")},
            }
        fixtures.write_json(self.root / "ci-results/provenance.json", provenance)

        def git_output(argv, cwd=None):
            label = Path(cwd).name
            if argv == ["git", "rev-parse", "HEAD"]:
                return self.commits[label]
            if argv == ["git", "status", "--porcelain", "--untracked-files=no"]:
                return self.tracked_status[label]
            if argv == ["git", "ls-files", "--others", "--exclude-standard"]:
                return self.untracked[label]
            raise AssertionError(f"Unexpected command: {argv}")

        active = patch.object(ci, "output", side_effect=git_output)
        active.start()
        self.addCleanup(active.stop)

    def test_unchanged_prepared_sources_have_successful_guard_receipt(self):
        receipt = ci.verify_prepared(self.root)
        self.assertEqual(receipt["status"], "success")
        self.assertEqual(set(receipt["sources"]), set(fixtures.LABELS))
        self.assertEqual(fixtures.read_json(self.root / "ci-results/prepared-source-verification.json"), receipt)

    def test_changed_lock_manifests_configuration_or_harness_fails_and_preserves_receipt(self):
        for label in fixtures.LABELS:
            for name in (*self.inputs, "engine/search/examples/ci_bench.rs"):
                path = self.root / label / name
                original = path.read_bytes()
                with self.subTest(label=label, input=name):
                    try:
                        path.write_bytes(original + b"changed after cache restore\n")
                        with self.assertRaisesRegex(ValueError, "changed after prepare"):
                            ci.verify_prepared(self.root)
                        receipt = fixtures.read_json(self.root / "ci-results/prepared-source-verification.json")
                        self.assertEqual(receipt["status"], "failed")
                    finally:
                        path.write_bytes(original)

    def test_changed_head_tracked_source_or_added_untracked_source_fails(self):
        for values, changed in ((self.commits, "d" * 40),
                                (self.tracked_status, " M engine/core/src/lib.rs"),
                                (self.untracked, "engine/search/examples/ci_bench.rs\nengine/core/build.rs")):
            original = values["candidate"]
            try:
                values["candidate"] = changed
                with self.subTest(change=changed), self.assertRaises(ValueError):
                    ci.verify_prepared(self.root)
                self.assertEqual(fixtures.read_json(
                    self.root / "ci-results/prepared-source-verification.json")["status"], "failed")
            finally:
                values["candidate"] = original


if __name__ == "__main__":
    unittest.main()
