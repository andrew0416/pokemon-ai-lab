"""Verified build-cache trust, invalidation, and cold-fallback tests.

All processes are mocked. Fixtures contain inert bytes, logs, and Cargo
fingerprint JSON; these tests never invoke Cargo or an engine executable.
"""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import build_cache as cache
import ci
import run


LABELS = ("baseline", "candidate")
HURT = "experiment-hurt-readers"
LEAF = "experiment-leaf-ending-states"
LEAF_OBSERVER = "experiment-leaf-ending-observer"
PREPARED = "experiment-prepared-turn"
PREPARED_OBSERVER = "experiment-prepared-turn-observe"
KINDS = {
    "lab-engine": ["lib-lab_engine.json", "test-lib-lab_engine.json"],
    "lab-search": ["lib-lab_search.json", "test-lib-lab_search.json", "example-ci_bench.json"],
}


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def command_log(commands, output):
    return "COMMAND " + json.dumps(commands[0]) + "\n" + output + "\nCOMMAND " + json.dumps(commands[1]) + "\n"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def environment(**changes):
    return {
        "BUILD_CACHE_ENABLED": "1",
        "BUILD_CACHE_ALLOW_SAVE": "1",
        "BUILD_CACHE_DEFAULT_BRANCH": "lab-engine",
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_REF": "refs/heads/lab-engine",
        "GITHUB_REPOSITORY": "andrew0416/pokemon-ai-lab",
        "GITHUB_SHA": "c" * 40,
        "GITHUB_RUN_ID": "12345",
        "GITHUB_RUN_ATTEMPT": "1",
        **changes,
    }


def recipe(label, selection="prepared-turn"):
    bridge = {
        LEAF: selection == "leaf-ending-states" and label == "candidate",
        LEAF_OBSERVER: False,
        PREPARED: selection == "prepared-turn" and label == "candidate",
        PREPARED_OBSERVER: False,
    }
    hurt = selection in ("prepared-turn", "leaf-ending-states") or (
        selection == "hurt-readers" and label == "candidate"
    )
    packages = copy.deepcopy(KINDS)
    expected = {"lab-engine": {HURT: hurt, **bridge}, "lab-search": dict(bridge)}
    if selection not in ("prepared-turn", "leaf-ending-states"):
        packages.pop("lab-search")
        expected.pop("lab-search")
    return {
        "schema_version": 1,
        "label": label,
        "suite": "narrow",
        "selection": selection,
        "source": {
            "sha": ("a" if label == "baseline" else "b") * 40,
            "tracked_files_sha256": "1" * 64,
            "tracked_file_count": 7,
            "harness_sha256": "2" * 64,
        },
        "commands": ci.build_commands("narrow", selection, label),
        "fingerprint_spec": {
            "packages": packages,
            "expected": expected,
            "hurt_active": hurt,
        },
        "identity": {
            "toolchain": {"rustc": "rustc pinned", "cargo": "cargo pinned"},
            "runner": {"os": "Linux", "arch": "X64", "image": "ubuntu24",
                       "image_version": "20260929.1", "abi": "glibc2.39"},
            "lock": "3" * 64,
            "manifests": {"core": "4" * 64, "search": "5" * 64},
            "cargo_configuration": {},
            "build_environment": {"RUSTFLAGS": "-Ctarget-cpu=x86-64",
                                  "CARGO_PROFILE_RELEASE_OPT_LEVEL": "3"},
            "controller": {"ci.py": "6" * 64, "build_cache.py": "7" * 64},
            "inputs": {"scenario.json": "8" * 64, "team.json": "9" * 64},
        },
    }


def setup_workspace(root, selection="prepared-turn"):
    result = root / "ci-results"
    result.mkdir()
    request = {
        "suite": "narrow", "candidate_feature": selection,
        "baseline_sha": "a" * 40, "candidate_sha": "b" * 40,
        "feature_args": {label: ci.feature_args(selection, label) for label in LABELS},
    }
    write_json(result / "request.json", request)
    write_json(result / "provenance.json", {})
    for label in LABELS:
        path = root / label / "engine" / "original-source.txt"
        path.parent.mkdir(parents=True)
        path.write_text(label + " source must not be modified\n", encoding="utf-8")


def make_fingerprints(root, label, selection="prepared-turn", wrong=None, omit=None):
    spec = recipe(label, selection)["fingerprint_spec"]
    for package, names in spec["packages"].items():
        folder = root / ("target-" + label) / "release/.fingerprint" / (package + "-fixturehash")
        folder.mkdir(parents=True, exist_ok=True)
        for name in names:
            if (package, name) == omit:
                continue
            features = [feature for feature, active in spec["expected"][package].items() if active]
            if wrong and (package, name) == wrong[:2]:
                features = list(wrong[2])
            # Exercise Cargo's JSON-encoded string and already-decoded list variants.
            value = features if name.startswith("test-") else json.dumps(features)
            write_json(folder / name, {"features": value})


def make_cold_success(root, label, selection="prepared-turn"):
    make_fingerprints(root, label, selection)
    target = root / ("target-" + label)
    executable = target / "release/examples" / ("ci_bench.exe" if os.name == "nt" else "ci_bench")
    executable.parent.mkdir(parents=True, exist_ok=True)
    executable.write_bytes(b"inert cached benchmark fixture\n" + label.encode())
    executable.chmod(0o755)
    evidence = ci.preserve_fingerprints(root, label, selection)
    commands = recipe(label, selection)["commands"]
    text = "COMMAND " + json.dumps(commands[0]) + "\n"
    text += "test core::example ... ok\n"
    text += "test result: ok. 90 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    text += "test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    text += "test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    text += "COMMAND " + json.dumps(commands[1]) + "\n"
    (root / "ci-results" / (label + "-build.log")).write_text(text, encoding="utf-8")
    receipt = {
        "schema_version": 1, "status": "success", "label": label, "suite": "narrow",
        "selection": selection, "commands": [{"argv": argv, "returncode": 0} for argv in commands],
        "log": label + "-build.log", "feature_evidence": label + "-features.json", "reused": False,
    }
    write_json(root / "ci-results" / (label + "-build-receipt.json"), receipt)
    return evidence, executable


def remove_build_products(root, label):
    """Only remove generated fixtures within this test's TemporaryDirectory."""
    for path in (root / ("target-" + label), root / "ci-results/fingerprints" / label):
        assert path.resolve().is_relative_to(root.resolve())
        if path.exists():
            shutil.rmtree(path)
    for suffix in ("-build.log", "-features.json", "-build-receipt.json"):
        (root / "ci-results" / (label + suffix)).unlink(missing_ok=True)


def find_bundle(root, label):
    matches = []
    for path in root.rglob("receipt.json"):
        value = read_json(path)
        if value.get("label") == label and "recipe" in value:
            matches.append(path.parent)
    if len(matches) != 1:
        raise AssertionError(f"Expected one sealed {label} bundle, got {matches}")
    return matches[0]


class CacheIdentityTests(unittest.TestCase):
    def test_recipe_key_is_canonical_and_label_isolation_is_mandatory(self):
        original = recipe("baseline")
        self.assertEqual(cache.recipe_key(original), cache.recipe_key(dict(reversed(list(original.items())))))
        changed = copy.deepcopy(original)
        changed["label"] = "candidate"
        self.assertNotEqual(cache.recipe_key(original), cache.recipe_key(changed))
        self.assertTrue(cache.recipe_key(original).startswith("verified-build-v1-"))

    def test_every_critical_identity_input_invalidates_the_exact_key(self):
        original = recipe("baseline")
        mutations = [
            ("source SHA", lambda r: r["source"].update(sha="d" * 40)),
            ("source bytes", lambda r: r["source"].update(tracked_files_sha256="a" * 64)),
            ("harness", lambda r: r["source"].update(harness_sha256="b" * 64)),
            ("feature selection", lambda r: r.update(selection="leaf-ending-states")),
            ("compiled features", lambda r: r["fingerprint_spec"]["expected"]["lab-engine"].update({PREPARED: True})),
            ("regression scope", lambda r: r.update(suite="smoke")),
            ("commands", lambda r: r["commands"][0].append("--lib")),
            ("toolchain", lambda r: r["identity"]["toolchain"].update(rustc="other rustc")),
            ("OS", lambda r: r["identity"]["runner"].update(os="Windows")),
            ("arch", lambda r: r["identity"]["runner"].update(arch="ARM64")),
            ("image", lambda r: r["identity"]["runner"].update(image="ubuntu26")),
            ("image version", lambda r: r["identity"]["runner"].update(image_version="next")),
            ("ABI", lambda r: r["identity"]["runner"].update(abi="glibc-next")),
            ("lock", lambda r: r["identity"].update(lock="a" * 64)),
            ("manifest", lambda r: r["identity"]["manifests"].update(core="b" * 64)),
            ("cargo config", lambda r: r["identity"]["cargo_configuration"].update(config="c" * 64)),
            ("rustflags", lambda r: r["identity"]["build_environment"].update(RUSTFLAGS="-Ctarget-cpu=native")),
            ("profile", lambda r: r["identity"]["build_environment"].update(CARGO_PROFILE_RELEASE_OPT_LEVEL="2")),
            ("controller", lambda r: r["identity"]["controller"].update({"ci.py": "d" * 64})),
            ("fixture", lambda r: r["identity"]["inputs"].update({"scenario.json": "e" * 64})),
            ("team", lambda r: r["identity"]["inputs"].update({"team.json": "f" * 64})),
        ]
        for title, mutate in mutations:
            with self.subTest(input=title):
                changed = copy.deepcopy(original)
                mutate(changed)
                self.assertNotEqual(cache.recipe_key(original), cache.recipe_key(changed))


class CacheRecipeInputTests(unittest.TestCase):
    """Use real file hashing and recipe construction, mocking only host/Git probes."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        setup_workspace(self.root)
        self.controller_root = self.root / "controller"
        self.controller = self.controller_root / "engine/benchmarks/paired"
        self.controller.mkdir(parents=True)
        for name in cache.CONTROLLER_FILES:
            (self.controller / name).write_text(name + " controller bytes\n", encoding="utf-8")
        self.workflow = self.controller_root / ".github/workflows/engine-benchmark.yml"
        self.workflow.parent.mkdir(parents=True)
        self.workflow.write_text("workflow fixture\n", encoding="utf-8")
        self.inputs = [self.controller_root / "fixtures/scenario.json",
                       self.controller_root / "fixtures/team.json"]
        for path in self.inputs:
            write_json(path, {"input": path.name})
        self.tracked = ["engine/Cargo.lock", "engine/Cargo.toml", "engine/core/Cargo.toml",
                        "engine/search/Cargo.toml", "engine/core/src/lib.rs"]
        self.config = self.root / "baseline/engine/.cargo/config.toml"
        self.config.parent.mkdir(parents=True)
        self.config.write_text("[build]\njobs = 1\n", encoding="utf-8")
        for label in LABELS:
            for name in self.tracked:
                path = self.root / label / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(label + " " + name + "\n", encoding="utf-8")
            harness = self.root / label / "engine/search/examples/ci_bench.rs"
            harness.parent.mkdir(parents=True)
            harness.write_bytes((self.controller / "harness.rs").read_bytes())
        self.source_sha = {"baseline": "a" * 40, "candidate": "b" * 40}
        self.dirty = ""
        self.untracked = ["engine/search/examples/ci_bench.rs"]
        self.runtime = {"versions": {"rustc": "pinned", "cargo": "pinned"},
                        "os": "Linux", "arch": "x86_64",
                        "image": {"ImageOS": "ubuntu24", "ImageVersion": "20260929.1"},
                        "system_abi": {"libc.so.6": [{"sha256": "1" * 64}]},
                        "environment": {"RUSTFLAGS": "-Ctarget-cpu=x86-64",
                                        "CARGO_PROFILE_RELEASE_OPT_LEVEL": "3"}}

        def command(argv, cwd=None):
            label = Path(cwd).name
            if argv == ["git", "rev-parse", "HEAD"]:
                return self.source_sha[label]
            if argv == ["git", "status", "--porcelain", "--untracked-files=no"]:
                return self.dirty
            raise AssertionError(f"Unexpected command: {argv}")

        def output(argv, **kwargs):
            if argv == ["git", "ls-files", "-z"]:
                return ("\0".join(self.tracked) + "\0").encode()
            if argv == ["git", "ls-files", "--others", "--exclude-standard", "-z"]:
                return ("\0".join(self.untracked) + "\0").encode()
            raise AssertionError(f"Unexpected process: {argv}")

        patches = [
            patch.dict(os.environ, environment(), clear=True),
            patch.object(cache, "__file__", str(self.controller / "build_cache.py")),
            patch.object(cache, "_command", side_effect=command),
            patch.object(cache.subprocess, "check_output", side_effect=output),
            patch.object(cache.subprocess, "Popen", side_effect=AssertionError("No processes permitted")),
            patch.object(cache, "_runtime_identity", side_effect=lambda **kwargs: copy.deepcopy(self.runtime)),
            patch.object(cache, "_cargo_configuration", side_effect=lambda root: {
                "fixture-config": {"sha256": digest(self.config)}}),
            patch.object(run, "load_cases", return_value=([], self.inputs)),
        ]
        for active in patches:
            active.start()
            self.addCleanup(active.stop)

    def test_recipe_hashes_actual_source_controller_and_shared_inputs(self):
        original = cache.make_recipe(self.root, "baseline")
        original_key = cache.recipe_key(original)
        self.assertEqual(original["source"]["tracked_file_count"], len(self.tracked))
        self.assertEqual(original["source"]["lock_sha256"],
                         digest(self.root / "baseline/engine/Cargo.lock"))
        self.assertEqual(original["identity"]["shared_input_sha256"], {
            path.relative_to(self.controller_root).as_posix(): digest(path) for path in self.inputs})
        paths = [self.root / "baseline" / name for name in self.tracked]
        paths += [self.controller / name for name in cache.CONTROLLER_FILES if name != "harness.rs"]
        paths += [self.workflow, self.config, *self.inputs]
        for path in paths:
            previous = path.read_bytes()
            with self.subTest(input=path.relative_to(self.root).as_posix()):
                try:
                    path.write_bytes(previous + b"changed\n")
                    changed = cache.make_recipe(self.root, "baseline")
                    self.assertNotEqual(original_key, cache.recipe_key(changed))
                finally:
                    path.write_bytes(previous)
        self.assertEqual(original, cache.make_recipe(self.root, "baseline"))

    def test_recipe_binds_current_runtime_and_explicit_mode(self):
        original_key = cache.recipe_key(cache.make_recipe(self.root, "baseline"))
        original_runtime = copy.deepcopy(self.runtime)
        mutations = [
            lambda v: v["versions"].update(rustc="other"),
            lambda v: v["versions"].update(cargo="other"),
            lambda v: v.update(os="other"),
            lambda v: v.update(arch="other"),
            lambda v: v["image"].update(ImageVersion="other"),
            lambda v: v["system_abi"]["libc.so.6"][0].update(sha256="9" * 64),
            lambda v: v["environment"].update(CARGO_PROFILE_RELEASE_OPT_LEVEL="2"),
        ]
        for index, mutate in enumerate(mutations):
            self.runtime = copy.deepcopy(original_runtime)
            mutate(self.runtime)
            with self.subTest(runtime_input=index):
                self.assertNotEqual(original_key, cache.recipe_key(cache.make_recipe(self.root, "baseline")))
        self.runtime = original_runtime
        request_path = self.root / "ci-results/request.json"
        request = read_json(request_path)
        request["candidate_feature"] = "leaf-ending-states"
        write_json(request_path, request)
        self.assertNotEqual(original_key, cache.recipe_key(cache.make_recipe(self.root, "baseline")))

    def test_dirty_source_wrong_sha_unexpected_untracked_and_wrong_harness_refuse(self):
        self.dirty = " M engine/core/src/lib.rs"
        with self.assertRaisesRegex(ValueError, "Tracked source changed"):
            cache.make_recipe(self.root, "baseline")
        self.dirty = ""
        self.source_sha["baseline"] = "d" * 40
        with self.assertRaisesRegex(ValueError, "requested commit"):
            cache.make_recipe(self.root, "baseline")
        self.source_sha["baseline"] = "a" * 40
        self.untracked.append("engine/core/build.rs")
        with self.assertRaisesRegex(ValueError, "unexpected untracked"):
            cache.make_recipe(self.root, "baseline")
        self.untracked.pop()
        (self.root / "baseline/engine/search/examples/ci_bench.rs").write_bytes(b"wrong harness")
        with self.assertRaisesRegex(ValueError, "Injected harness differs"):
            cache.make_recipe(self.root, "baseline")


class CacheRuntimeBoundaryTests(unittest.TestCase):
    def test_unsupported_runtime_or_compiler_overrides_disable_identity_before_tool_probes(self):
        cases = [
            ("Windows", "x86_64", {}),
            ("Linux", "aarch64", {}),
            ("Linux", "x86_64", {"RUSTFLAGS": "-Ctarget-cpu=native"}),
            ("Linux", "x86_64", {"RUSTC_WRAPPER": "wrapper"}),
            ("Linux", "x86_64", {"RUSTDOCFLAGS": "--cfg extra"}),
            ("Linux", "x86_64", {"CC": "alternate-cc"}),
            ("Linux", "x86_64", {"CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER": "alternate-ld"}),
            ("Linux", "x86_64", {"ImageVersion": ""}),
        ]
        for system, machine, changes in cases:
            values = {"RUSTFLAGS": "-Ctarget-cpu=x86-64", "ImageOS": "ubuntu24",
                      "ImageVersion": "20260929.1", **changes}
            with self.subTest(system=system, machine=machine, overrides=changes), \
                    patch.dict(os.environ, values, clear=True), \
                    patch.object(cache.platform, "system", return_value=system), \
                    patch.object(cache.platform, "machine", return_value=machine), \
                    patch.object(cache, "_command", side_effect=AssertionError("Must reject before probes")) as command:
                with self.assertRaises(ValueError):
                    cache._runtime_identity()
                command.assert_not_called()


class CacheBundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        setup_workspace(self.root)
        self.recipes = {label: recipe(label) for label in LABELS}
        self.env = patch.dict(os.environ, environment(GITHUB_OUTPUT=str(self.root / "outputs")), clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)
        self.recipe_patch = patch.object(
            cache, "make_recipe", side_effect=lambda workspace, label: copy.deepcopy(self.recipes[label])
        )
        self.recipe_patch.start()
        self.addCleanup(self.recipe_patch.stop)
        cache.plan(self.root)

    def sealed(self, label="baseline"):
        evidence, executable = make_cold_success(self.root, label)
        self.assertTrue(cache.seal(self.root, label, evidence))
        bundle = find_bundle(self.root, label)
        self.assertTrue((bundle / "receipt.json").is_file())
        executable_bytes = executable.read_bytes()
        remove_build_products(self.root, label)
        return bundle, executable_bytes

    def hit(self, label="baseline", **changes):
        values = {
            label.upper() + "_CACHE_HIT": "true",
            label.upper() + "_CACHE_MATCHED_KEY": cache.recipe_key(self.recipes[label]),
            **changes,
        }
        return patch.dict(os.environ, values)

    def cold(self, label="baseline"):
        self.assertFalse(cache.restore(self.root, label))
        self.assertFalse((self.root / ("target-" + label)).exists(),
                         "Invalid cache must be rejected before creating target")
        diagnostic = self.root / "ci-results" / ("cache-" + label + ".json")
        self.assertTrue(diagnostic.is_file())
        return read_json(diagnostic)

    def rewrite_payload(self, bundle, relative, value):
        """Tamper bytes while making the untrusted receipt's digest self-consistent."""
        path = bundle / relative
        data = value if isinstance(value, bytes) else (json.dumps(value) + "\n").encode()
        path.write_bytes(data)
        receipt = read_json(bundle / "receipt.json")
        receipt["files"][relative] = {"sha256": digest(path), "size": len(data)}
        write_json(bundle / "receipt.json", receipt)

    def test_exact_hit_restores_only_sealed_products_and_preserves_original_sources(self):
        sources = {label: (self.root / label / "engine/original-source.txt").read_bytes()
                   for label in LABELS}
        bundles = {label: self.sealed(label) for label in LABELS}
        for label in LABELS:
            with self.subTest(label=label), self.hit(label), patch.object(
                    ci.subprocess, "Popen", side_effect=AssertionError("Cargo must not run on restore")):
                self.assertTrue(cache.restore(self.root, label))
            products = list((self.root / ("target-" + label) / "release/examples").glob("ci_bench*"))
            self.assertEqual(len(products), 1)
            self.assertEqual(products[0].read_bytes(), bundles[label][1])
            evidence = ci.preserve_fingerprints(self.root, label, "prepared-turn")
            self.assertEqual(len(evidence["fingerprints"]), 5)
            cached = self.root / "ci-results/cached" / label
            self.assertTrue(cached.is_dir())
            self.assertTrue(list(cached.rglob("*.log")))
            self.assertEqual((self.root / label / "engine/original-source.txt").read_bytes(), sources[label])
            self.assertFalse((self.root / ("target-" + label) / "debug").exists())

    def test_hit_flag_and_matched_key_both_must_be_exact(self):
        self.sealed()
        key = cache.recipe_key(self.recipes["baseline"])
        cases = [
            {"BASELINE_CACHE_HIT": "", "BASELINE_CACHE_MATCHED_KEY": key},
            {"BASELINE_CACHE_HIT": "false", "BASELINE_CACHE_MATCHED_KEY": key},
            {"BASELINE_CACHE_HIT": "True", "BASELINE_CACHE_MATCHED_KEY": key},
            {"BASELINE_CACHE_HIT": "true", "BASELINE_CACHE_MATCHED_KEY": ""},
            {"BASELINE_CACHE_HIT": "true", "BASELINE_CACHE_MATCHED_KEY": key[:-8]},
            {"BASELINE_CACHE_HIT": "true", "BASELINE_CACHE_MATCHED_KEY": key + "-prefix"},
            {"BASELINE_CACHE_HIT": "true", "BASELINE_CACHE_MATCHED_KEY": cache.recipe_key(self.recipes["candidate"])},
        ]
        for change in cases:
            with self.subTest(change=change), patch.dict(os.environ, change):
                self.cold()

    def test_cache_requires_explicit_numeric_enable(self):
        self.sealed()
        for enabled in ("", "0", "true", "yes"):
            with self.subTest(enabled=enabled), self.hit(), patch.dict(
                    os.environ, {"BUILD_CACHE_ENABLED": enabled}):
                self.cold()

    def test_current_recipe_changed_after_plan_forces_cold_before_install(self):
        self.sealed()
        original = copy.deepcopy(self.recipes["baseline"])
        mutations = [
            lambda r: r["source"].update(sha="d" * 40),
            lambda r: r["source"].update(tracked_files_sha256="f" * 64),
            lambda r: r["identity"]["toolchain"].update(rustc="different"),
            lambda r: r["identity"]["inputs"].update({"team.json": "a" * 64}),
            lambda r: r["commands"][0].append("--lib"),
            lambda r: r["fingerprint_spec"]["expected"]["lab-engine"].update({HURT: False}),
        ]
        # The service reports the OLD exact key; current inputs still take precedence.
        old_key = cache.recipe_key(original)
        for mutate in mutations:
            self.recipes["baseline"] = copy.deepcopy(original)
            mutate(self.recipes["baseline"])
            with self.subTest(mutation=mutate), self.hit(
                    BASELINE_CACHE_MATCHED_KEY=old_key):
                self.cold()
        self.recipes["baseline"] = original

    def test_missing_plan_is_a_cold_miss_without_build_target(self):
        self.sealed()
        (self.root / "ci-results/cache-plan.json").unlink()
        with self.hit():
            self.cold()


    def fake_build_process(self, calls, *, selection="prepared-turn", fail_label=None, wrong=None):
        owner = self

        class Process:
            def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                label = Path(env["CARGO_TARGET_DIR"]).name.removeprefix("target-")
                owner.assertIn(label, LABELS)
                owner.assertEqual(cwd, owner.root / label / "engine")
                if label not in [old_label for old_label, _ in calls]:
                    owner.assertFalse((owner.root / ("target-" + label)).exists())
                calls.append((label, list(argv)))
                self.returncode = 23 if label == fail_label else 0
                make_fingerprints(owner.root, label, selection, wrong=wrong)
                executable = owner.root / ("target-" + label) / "release/examples" / (
                    "ci_bench.exe" if os.name == "nt" else "ci_bench"
                )
                executable.parent.mkdir(parents=True, exist_ok=True)
                executable.write_bytes(b"inert new executable\n" + label.encode())
                executable.chmod(0o755)
                if argv[1] == "test":
                    stdout.write("test example ... ok\n")
                    stdout.write("test result: ok. 141 passed; 0 failed; 0 ignored; "
                                 "0 measured; 0 filtered out; finished in 0.01s\n")
                stdout.flush()

            def wait(self, timeout):
                owner.assertEqual(timeout, ci.COMMAND_TIMEOUT_SECONDS)
                return self.returncode

        return Process

    def test_build_hit_skips_cargo_marks_reused_and_still_runs_prepared_gate(self):
        for label in LABELS:
            self.sealed(label)
        with self.hit("baseline"), self.hit("candidate"), patch.object(
                ci.subprocess, "Popen", side_effect=AssertionError("Cached build called Cargo")), \
                patch.object(ci, "preserve_fingerprints", wraps=ci.preserve_fingerprints) as verify, \
                patch.object(ci, "validate_prepared_turn", return_value={"status": "ok"}) as prepared:
            ci.build(self.root)
        self.assertEqual([call.args[1] for call in verify.call_args_list], list(LABELS))
        prepared.assert_called_once_with(self.root)
        for label in LABELS:
            receipt = read_json(self.root / "ci-results" / (label + "-build-receipt.json"))
            self.assertTrue(receipt["reused"])
        provenance = read_json(self.root / "ci-results/provenance.json")
        self.assertEqual(set(provenance["compiler_feature_evidence"]), set(LABELS))
        self.assertEqual(provenance["prepared_validation"], {"status": "ok"})
        self.assertFalse((self.root / "ci-results/benchmark/result.json").exists())
        self.assertFalse((self.root / "ci-results/memory/result.json").exists())

    def test_invalid_cache_falls_back_to_real_build_plan_before_target_creation(self):
        bundle, _ = self.sealed("baseline")
        receipt = read_json(bundle / "receipt.json")
        binary = next(name for name in receipt["files"] if name.startswith("benchmark/"))
        (bundle / binary).write_bytes(b"tampered")
        calls = []
        with self.hit("baseline"), patch.object(
                ci.subprocess, "Popen", self.fake_build_process(calls)), \
                patch.object(ci, "validate_prepared_turn", return_value={"status": "ok"}) as prepared:
            ci.build(self.root)
        self.assertEqual([label for label, _ in calls], ["baseline", "baseline", "candidate", "candidate"])
        for label in LABELS:
            self.assertEqual([argv for name, argv in calls if name == label],
                             ci.build_commands("narrow", "prepared-turn", label))
            receipt = read_json(self.root / "ci-results" / (label + "-build-receipt.json"))
            self.assertFalse(receipt["reused"])
            self.assertEqual(receipt["status"], "success")
        prepared.assert_called_once()

    def test_genuine_cold_cargo_failure_propagates_and_is_not_sealed(self):
        calls = []
        with patch.object(ci.subprocess, "Popen", self.fake_build_process(calls, fail_label="baseline")), \
                patch.object(cache, "seal", wraps=cache.seal) as seal, \
                patch.object(ci, "validate_prepared_turn") as prepared:
            with self.assertRaisesRegex(RuntimeError, "build/test failed"):
                ci.build(self.root)
        self.assertEqual(len(calls), 1)
        seal.assert_not_called()
        prepared.assert_not_called()
        self.assertFalse(list(self.root.rglob("receipt.json")))

    def test_cold_real_fingerprint_gate_rejects_feature_mismatch(self):
        calls = []
        wrong = ("lab-engine", "test-lib-lab_engine.json", [])
        with patch.object(ci.subprocess, "Popen", self.fake_build_process(calls, wrong=wrong)), \
                patch.object(cache, "seal", wraps=cache.seal) as seal, \
                patch.object(ci, "validate_prepared_turn") as prepared:
            with self.assertRaisesRegex(ValueError, "actual compiled feature activation"):
                ci.build(self.root)
        seal.assert_not_called()
        prepared.assert_not_called()
        self.assertTrue((self.root / "ci-results/baseline-features.json").is_file())

    def test_hit_is_checked_again_from_actual_installed_fingerprints(self):
        for label in LABELS:
            self.sealed(label)
        original = ci.preserve_fingerprints

        def corrupt_then_verify(workspace, label, selection):
            if label == "baseline":
                path = next((workspace / "target-baseline/release/.fingerprint").rglob("lib-lab_engine.json"))
                write_json(path, {"features": []})
            return original(workspace, label, selection)

        with self.hit("baseline"), self.hit("candidate"), patch.object(
                ci, "preserve_fingerprints", side_effect=corrupt_then_verify), \
                patch.object(ci.subprocess, "Popen") as process, \
                patch.object(ci, "validate_prepared_turn") as prepared:
            with self.assertRaisesRegex(ValueError, "actual compiled feature activation"):
                ci.build(self.root)
        process.assert_not_called()
        prepared.assert_not_called()

    def test_cache_disabled_keeps_other_mode_builds_cold(self):
        # Mode routing is not derived from the cache identity fixture.
        for selection in ("none", "hurt-readers", "leaf-ending-states"):
            with self.subTest(selection=selection), tempfile.TemporaryDirectory() as folder:
                old_root = self.root
                self.root = Path(folder)
                try:
                    setup_workspace(self.root, selection)
                    calls = []
                    with patch.dict(os.environ, {"BUILD_CACHE_ENABLED": "0"}), patch.object(
                            ci.subprocess, "Popen", self.fake_build_process(calls, selection=selection)), \
                            patch.object(ci, "validate_prepared_turn") as prepared:
                        ci.build(self.root)
                    self.assertEqual(
                        calls,
                        [(label, argv) for label in LABELS
                         for argv in ci.build_commands("narrow", selection, label)],
                    )
                    prepared.assert_not_called()
                finally:
                    self.root = old_root


    def test_untrusted_save_is_refused(self):
        evidence, _ = make_cold_success(self.root, "baseline")
        cases = [
            {"BUILD_CACHE_ALLOW_SAVE": "0"},
            {"BUILD_CACHE_ALLOW_SAVE": "true"},
            {"GITHUB_EVENT_NAME": "push"},
            {"GITHUB_EVENT_NAME": "pull_request"},
            {"GITHUB_REF": "refs/heads/experiment"},
            {"GITHUB_REF": "refs/pull/1/merge"},
            {"GITHUB_REPOSITORY": "someone/pokemon-ai-lab"},
            {"BUILD_CACHE_DEFAULT_BRANCH": "experiment", "GITHUB_REF": "refs/heads/experiment"},
        ]
        for change in cases:
            with self.subTest(change=change), patch.dict(os.environ, change):
                self.assertFalse(cache.seal(self.root, "baseline", evidence))
                self.assertFalse(list(self.root.rglob("receipt.json")))

    def test_failed_or_zero_test_build_is_never_sealed(self):
        evidence, _ = make_cold_success(self.root, "baseline")
        receipt_path = self.root / "ci-results/baseline-build-receipt.json"
        valid_receipt = read_json(receipt_path)
        log_path = self.root / "ci-results/baseline-build.log"
        valid_log = log_path.read_text()
        commands = self.recipes["baseline"]["commands"]
        cases = [
            ("status", lambda r: r.update(status="failed"), valid_log),
            ("returncode", lambda r: r["commands"][0].update(returncode=1), valid_log),
            ("wrong commands", lambda r: r["commands"][0]["argv"].append("--lib"), valid_log),
            ("no summary", lambda r: None, command_log(commands, "build succeeded")),
            ("zero tests", lambda r: None, command_log(commands,
                "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;")),
            ("ignored tests", lambda r: None, command_log(commands,
                "test result: ok. 90 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;")),
            ("filtered tests", lambda r: None, command_log(commands,
                "test result: ok. 90 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;")),
        ]
        for title, mutate, text in cases:
            current = copy.deepcopy(valid_receipt)
            mutate(current)
            write_json(receipt_path, current)
            log_path.write_text(text, encoding="utf-8")
            with self.subTest(case=title):
                self.assertFalse(cache.seal(self.root, "baseline", evidence))
                self.assertFalse(list(self.root.rglob("receipt.json")))

    def test_bundle_missing_extra_digest_and_success_tampering_are_cold(self):
        bundle, _ = self.sealed()
        original_files = {p.relative_to(bundle).as_posix(): p.read_bytes()
                          for p in bundle.rglob("*") if p.is_file()}
        binary = next(name for name in original_files if name.startswith("benchmark/"))
        cases = ("missing binary", "extra file", "extra directory", "binary digest", "failed receipt",
                 "malformed receipt", "wrong key", "wrong label", "partial file map")
        for case in cases:
            # Recreate the exact sealed fixture between independent tamper cases.
            shutil.rmtree(bundle)
            bundle.mkdir(parents=True)
            for name, data in original_files.items():
                path = bundle / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                if name.startswith("benchmark/"):
                    path.chmod(0o755)
            if case == "missing binary":
                (bundle / binary).unlink()
            elif case == "extra file":
                (bundle / "unlisted.txt").write_text("must not be restored")
            elif case == "extra directory":
                (bundle / "unlisted-directory").mkdir()
            elif case == "binary digest":
                (bundle / binary).write_bytes(b"changed executable")
            elif case == "malformed receipt":
                (bundle / "receipt.json").write_text("{not json")
            else:
                receipt = read_json(bundle / "receipt.json")
                if case == "failed receipt":
                    receipt["status"] = "failed"
                elif case == "wrong key":
                    receipt["key"] += "-other"
                elif case == "wrong label":
                    receipt["label"] = "candidate"
                elif case == "partial file map":
                    receipt["files"].pop(binary)
                write_json(bundle / "receipt.json", receipt)
            with self.subTest(case=case), self.hit():
                self.cold()

    def test_self_consistent_wrong_feature_cannot_be_trusted_by_hash_alone(self):
        bundle, _ = self.sealed("candidate")
        receipt = read_json(bundle / "receipt.json")
        evidence = read_json(bundle / "evidence/features.json")
        item = next(item for item in evidence["fingerprints"]
                    if item["package"] == "lab-engine" and item["kind"] == "lib-lab_engine.json")
        path = "fingerprints/" + "/".join(Path(item["target_path"]).parts[2:])
        item["features"] = [HURT, PREPARED, PREPARED_OBSERVER]
        self.rewrite_payload(bundle, path, {"features": json.dumps(item["features"])})
        item["sha256"] = digest(bundle / path)
        self.rewrite_payload(bundle, "evidence/features.json", evidence)
        # Raw JSON, evidence, size and both digests agree. The expected-feature gate
        # must still reject an observer that was enabled for a timing executable.
        with self.hit("candidate"):
            diagnostic = self.cold("candidate")
        self.assertIn("Actual compiled feature activation", diagnostic["reason"])

    def test_self_consistent_failed_regression_log_is_cold(self):
        bundle, _ = self.sealed()
        self.rewrite_payload(
            bundle, "logs/build.log",
            command_log(self.recipes["baseline"]["commands"],
                        "test result: FAILED. 90 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out;").encode(),
        )
        with self.hit():
            diagnostic = self.cold()
        self.assertIn("failed or unknown regression summary", diagnostic["reason"])

    def test_posix_other_execute_without_owner_execute_is_cold_before_target_creation(self):
        bundle, _ = self.sealed()
        benchmark_target = cache._benchmark_target()
        binary = bundle / "benchmark" / Path(benchmark_target).name
        original_stat = Path.stat
        inspected_modes = []

        def owner_cannot_execute(path, *args, **kwargs):
            result = original_stat(path, *args, **kwargs)
            if path == binary:
                # Real bytes, size and file type remain valid. Inject POSIX permissions
                # explicitly, since Windows chmod cannot reliably represent 0641.
                mode = (result.st_mode & ~0o777) | 0o641
                inspected_modes.append(mode & 0o777)
                return os.stat_result((mode, *result[1:]))
            return result

        # Replace only this module's OS view; changing global os.name would change
        # pathlib's host path class and test a Windows/PosixPath mismatch instead.
        posix_view = SimpleNamespace(name="posix", environ=os.environ, walk=os.walk)
        with self.hit(), patch.object(cache, "os", posix_view), \
                patch.object(cache, "_benchmark_target", return_value=benchmark_target), \
                patch.object(Path, "stat", owner_cannot_execute):
            diagnostic = self.cold()
        self.assertTrue(inspected_modes, "The actual cached benchmark mode must be inspected")
        self.assertEqual(set(inspected_modes), {0o641})
        self.assertIn("not executable by its owner", diagnostic["reason"])

    def test_each_real_compiled_package_and_kind_is_required_even_with_consistent_file_table(self):
        bundle, _ = self.sealed("candidate")
        original_receipt = read_json(bundle / "receipt.json")
        original_evidence = read_json(bundle / "evidence/features.json")
        evidence_bytes = (bundle / "evidence/features.json").read_bytes()
        for removed in original_evidence["fingerprints"]:
            with self.subTest(package=removed["package"], kind=removed["kind"]):
                name = "fingerprints/" + "/".join(Path(removed["target_path"]).parts[2:])
                fingerprint_bytes = (bundle / name).read_bytes()
                evidence = copy.deepcopy(original_evidence)
                evidence["fingerprints"].remove(removed)
                receipt = copy.deepcopy(original_receipt)
                receipt["files"].pop(name)
                receipt["artifact_mapping"].pop(name)
                write_json(bundle / "receipt.json", receipt)
                (bundle / name).unlink()
                self.rewrite_payload(bundle, "evidence/features.json", evidence)
                try:
                    with self.hit("candidate"):
                        diagnostic = self.cold("candidate")
                    self.assertIn("Missing compiled package/kind", diagnostic["reason"])
                finally:
                    (bundle / name).write_bytes(fingerprint_bytes)
                    (bundle / "evidence/features.json").write_bytes(evidence_bytes)
                    write_json(bundle / "receipt.json", original_receipt)

    def test_untrusted_bundle_origin_is_cold_even_with_exact_current_key(self):
        bundle, _ = self.sealed()
        receipt_path = bundle / "receipt.json"
        original = read_json(receipt_path)
        changes = ({"event": "pull_request"}, {"repository": "someone/other"},
                   {"ref": "refs/heads/experiment"}, {"workflow_sha": "not-a-sha"},
                   {"run_id": "0"}, {"run_attempt": ""})
        for change in changes:
            current = copy.deepcopy(original)
            current["origin"].update(change)
            write_json(receipt_path, current)
            with self.subTest(origin=change), self.hit():
                diagnostic = self.cold()
                self.assertIn("Bundle origin", diagnostic["reason"])

    def test_restore_copy_failure_preserves_partial_evidence_without_exposing_target(self):
        bundle, original_binary = self.sealed()
        original_copy = cache.shutil.copy2
        copied = []

        def interrupted_copy(source, destination, *args, **kwargs):
            copied.append(str(destination))
            if len(copied) == 2:
                raise OSError("fixture restore interrupted")
            return original_copy(source, destination, *args, **kwargs)

        with self.hit(), patch.object(cache.shutil, "copy2", side_effect=interrupted_copy):
            diagnostic = self.cold()
        self.assertIn("fixture restore interrupted", diagnostic["reason"])
        stages = list(self.root.glob("cache-restore-stage-baseline-*"))
        self.assertEqual(len(stages), 1)
        self.assertTrue(any(path.is_file() for path in stages[0].rglob("*")))
        self.assertTrue((bundle / "receipt.json").is_file())
        binary = next((bundle / "benchmark").iterdir())
        self.assertEqual(binary.read_bytes(), original_binary)

    def test_path_traversal_in_file_table_or_target_mapping_is_cold(self):
        bundle, _ = self.sealed()
        receipt_path = bundle / "receipt.json"
        original = read_json(receipt_path)
        for bad in ("../outside", "/absolute", "C:/outside", r"..\outside"):
            for field in ("files", "artifact_mapping"):
                with self.subTest(path=bad, field=field):
                    changed = copy.deepcopy(original)
                    if field == "files":
                        changed["files"][bad] = {"sha256": "0" * 64, "size": 0}
                    else:
                        name = next(n for n, item in changed[field].items() if "target_path" in item)
                        changed[field][name]["target_path"] = bad
                    write_json(receipt_path, changed)
                    with self.hit():
                        self.cold()
                    self.assertFalse((self.root / "outside").exists())

    def test_symlink_payload_is_cold_even_when_it_points_to_expected_bytes(self):
        bundle, _ = self.sealed()
        receipt = read_json(bundle / "receipt.json")
        relative = next(name for name in receipt["files"] if name.startswith("benchmark/"))
        path = bundle / relative
        outside = self.root / "outside-binary"
        outside.write_bytes(path.read_bytes())
        path.unlink()
        try:
            path.symlink_to(outside)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f"Host does not permit test symlinks: {error}")
        with self.hit():
            self.cold()


if __name__ == "__main__":
    unittest.main()
