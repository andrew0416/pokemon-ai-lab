"""One cold and one core-edit comparison of identical tests in different binaries.

Run on Linux CI, never against production sources. Work copies and Cargo targets
live outside the evidence directory. A dependency fetch precedes timed builds.
"""
import argparse
from collections import Counter
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import signal
import subprocess
import time
import tomllib


LABELS = ("baseline", "candidate")
PACKAGES = ("lab-engine", "lab-scenario", "lab-search")
FEATURES = "lab-engine/experiment-hurt-readers"
BUILD_ENV = {
    "CARGO_TERM_COLOR": "never", "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": "2",
    "RUSTFLAGS": "-Ctarget-cpu=x86-64", "CARGO_PROFILE_RELEASE_OPT_LEVEL": "3",
    "CARGO_PROFILE_RELEASE_DEBUG": "1", "CARGO_PROFILE_RELEASE_LTO": "off",
    "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": "16", "LAB_ENGINE_FACTORED": "0",
    "PYTHONHASHSEED": "0", "OPENBLAS_NUM_THREADS": "1", "OMP_NUM_THREADS": "1",
}
MARKER_PATH = "engine/core/src/lib.rs"
MARKER = (b"\n// CI-only unused constant: force a core source rebuild.\n"
          b"#[doc(hidden)]\npub const CI_TEST_BUNDLE_REBUILD_MARKER: u8 = 1;\n")
SUMMARY = re.compile(r"^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; "
                     r"(\d+) measured; (\d+) filtered out;", re.MULTILINE)
LIMITATIONS = [
    "One sequential trial per variant and stage; no repeated performance estimate or speedup claim.",
    "A shared cargo fetch precedes cold compilation, but OS/compiler/disk warming can still differ by order.",
    "Warm means the same per-variant target after the identical unused core constant edit; it is not a second cold build.",
    "Binary execution wall/CPU excludes compilation and --list; doctest commands include their own compilation and execution.",
    "Static mapping test names are descriptive only; actual libtest --list output defines the equality gate.",
]


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def relative(value):
    path = PurePosixPath(value)
    if (not isinstance(value, str) or not value or path.is_absolute() or "\\" in value
            or path.as_posix() != value or any(part in (".", "..") or ":" in part for part in path.parts)):
        raise ValueError(f"Unsafe relative path: {value!r}")
    return path


def inventory(root):
    result = {}
    for folder, directories, files in os.walk(root, followlinks=False):
        if Path(folder) == root:
            directories[:] = [name for name in directories if name != ".git"]
            files = [name for name in files if name != ".git"]
        for name in directories + files:
            path = Path(folder) / name
            if path.is_symlink() or (hasattr(path, "is_junction") and path.is_junction()):
                raise ValueError(f"Source links are outside this experiment: {path}")
        for name in files:
            path = Path(folder) / name
            if not path.is_file():
                raise ValueError(f"Non-file source input: {path}")
            result[path.relative_to(root).as_posix()] = {"sha256": digest(path), "bytes": path.stat().st_size}
    return result


class Mapping:
    def __init__(self, value):
        if value.get("schema_version") != 1 or not re.fullmatch("[0-9a-f]{40}", value.get("source_sha", "")):
            raise ValueError("Mapping requires schema_version=1 and an immutable source_sha")
        self.value = value
        self.original = {}
        self.candidate = {}
        self.target_sources = {}
        self.directories = {}
        for item in value["entries"]:
            package, target = item["package"], item["original_target"]
            if package not in ("lab-scenario", "lab-search") or item["package_directory"] != package[4:]:
                raise ValueError("Unexpected mapped package")
            if (package, target) in self.original:
                raise ValueError("Duplicate original test target")
            expected_path = f"engine/{item['package_directory']}/tests/{target}.rs"
            if str(relative(item["original_path"])) != expected_path:
                raise ValueError("Original test source does not match its target")
            prefix = item["module_prefix"]
            if prefix != ("" if item["isolated"] else target + "::"):
                raise ValueError("Mapping must remove exactly the declared original module prefix")
            if item["isolated"] and item["candidate_target"] != target:
                raise ValueError("An isolated target must retain its name")
            self.original[(package, target)] = item
            self.candidate.setdefault((package, item["candidate_target"]), []).append(item)
            self.directories[package] = item["package_directory"]
        if not self.original:
            raise ValueError("Empty test mapping")
        for package, data in value["packages"].items():
            if package not in self.directories:
                raise ValueError("Package declaration lacks mapping entries")
            for item in data["candidate_targets"]:
                key = (package, item["name"])
                if key in self.target_sources or key not in self.candidate:
                    raise ValueError("Duplicate or unused candidate target")
                self.target_sources[key] = f"engine/{self.directories[package]}/{relative(item['path'])}"
        if set(self.target_sources) != set(self.candidate):
            raise ValueError("Candidate target declaration is incomplete")
        for entries in self.candidate.values():
            prefixes = [item["module_prefix"] for item in entries]
            if len(prefixes) != len(set(prefixes)) or ("" in prefixes and len(prefixes) != 1):
                raise ValueError("Ambiguous candidate module mapping")

    def normalize(self, label, package, target, name):
        entries = self.candidate.get((package, target)) if label == "candidate" else None
        if entries is None:
            return (package, target, name)
        matches = [item for item in entries if name.startswith(item["module_prefix"])]
        if len(matches) != 1:
            raise ValueError(f"Unknown or ambiguous bundled test: {package}/{target}/{name}")
        item = matches[0]
        suffix = name[len(item["module_prefix"]):]
        if not suffix:
            raise ValueError("Empty test name after module normalization")
        return (package, item["original_target"], suffix)


def validate_sources(roots, mapping):
    inventories = {label: inventory(root) for label, root in roots.items()}
    before, after = (inventories[label] for label in LABELS)
    manifests = {f"engine/{directory}/Cargo.toml" for directory in mapping.directories.values()}
    wrappers = {mapping.target_sources[key] for key, entries in mapping.candidate.items()
                if not entries[0]["isolated"]}
    changed = {name for name in before.keys() & after.keys() if before[name] != after[name]}
    if set(before) - set(after) or set(after) - set(before) != wrappers or changed != manifests:
        raise ValueError("Source difference exceeds the exact test manifests and new wrappers")
    for item in mapping.original.values():
        name = item["original_path"]
        if before[name]["sha256"] != item["source_sha256"] or before[name] != after[name]:
            raise ValueError(f"Original test raw SHA-256 differs from mapping or between variants: {name}; "
                             f"mapping={item['source_sha256']}, baseline={before[name]['sha256']}, "
                             f"candidate={after[name]['sha256']}")
    for key, entries in mapping.candidate.items():
        if entries[0]["isolated"]:
            continue
        lines = [line.strip() for line in (roots["candidate"] / mapping.target_sources[key]).read_text(encoding="utf-8").splitlines()
                 if line.strip() and not line.strip().startswith("//")]
        expected = []
        for item in sorted(entries, key=lambda item: item["original_target"]):
            expected += [f'#[path = "{item["original_target"]}.rs"]', f'mod {item["original_target"]};']
        if lines != expected:
            raise ValueError("Wrapper contains more than the declared original test modules")
    for package, directory in mapping.directories.items():
        name = f"engine/{directory}/Cargo.toml"
        base = tomllib.loads((roots["baseline"] / name).read_text(encoding="utf-8"))
        candidate = tomllib.loads((roots["candidate"] / name).read_text(encoding="utf-8"))
        if base.get("test") or base["package"].get("autotests", True) is not True:
            raise ValueError("Baseline must use automatic integration test discovery")
        if candidate["package"].pop("autotests", None) is not False:
            raise ValueError("Candidate must disable automatic integration test discovery")
        if candidate.pop("test", None) != mapping.value["packages"][package]["candidate_targets"] or candidate != base:
            raise ValueError("Candidate manifest changed more than explicit test discovery")
    return inventories, {"changed": sorted(changed), "added": sorted(wrappers), "removed": []}


def environment(target):
    env = os.environ.copy()
    for name in ("CARGO_ENCODED_RUSTFLAGS", "RUST_TEST_NOCAPTURE", "RUST_TEST_THREADS"):
        env.pop(name, None)
    env.update(BUILD_ENV, CARGO_TARGET_DIR=str(target))
    return env


def run_process(argv, cwd, env, prefix, timeout=2700):
    """GNU time measures this one command's process tree; raw streams are separate."""
    paths = {name: prefix.with_name(prefix.name + suffix) for name, suffix in
             (("stdout", ".stdout"), ("stderr", ".stderr"), ("cpu", ".cpu"), ("receipt", ".json"))}
    prefix.parent.mkdir(parents=True, exist_ok=True)
    if any(path.exists() for path in paths.values()):
        raise FileExistsError("Refusing to overwrite command evidence")
    wrapped = ["/usr/bin/time", "-f", "%U %S", "-o", str(paths["cpu"]), "--", *map(str, argv)]
    receipt = {"argv": list(map(str, argv)), "executed_argv": wrapped, "cwd": str(cwd),
               "environment": {key: env[key] for key in (*BUILD_ENV, "CARGO_TARGET_DIR")},
               "timeout_seconds": timeout, "status": "running"}
    save(paths["receipt"], receipt)
    start = time.perf_counter()
    proc = None
    try:
        with paths["stdout"].open("wb") as stdout, paths["stderr"].open("wb") as stderr:
            proc = subprocess.Popen(wrapped, cwd=cwd, env=env, stdout=stdout, stderr=stderr,
                                    start_new_session=True)
            try:
                proc.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
                receipt["timed_out"] = True
                raise RuntimeError(f"Command timed out; process group terminated: {argv}")
        receipt["returncode"] = proc.returncode
        fields = paths["cpu"].read_text(encoding="utf-8").splitlines()[-1].split()
        if len(fields) != 2:
            raise ValueError("Invalid GNU time CPU output")
        user, system = map(float, fields)
        if not all(math.isfinite(value) and value >= 0 for value in (user, system)):
            raise ValueError("Invalid GNU time CPU values")
        receipt.update(user_seconds=user, system_seconds=system, cpu_seconds=user + system)
        if proc.returncode:
            raise RuntimeError(f"Command failed ({proc.returncode}): {argv}")
        receipt["status"] = "success"
    except BaseException as error:
        if proc is not None and proc.poll() is None:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
        receipt.update(status="failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        receipt["wall_seconds"] = time.perf_counter() - start
        receipt["artifacts"] = {name: {"path": str(path), "sha256": digest(path), "bytes": path.stat().st_size}
                                for name, path in paths.items() if name != "receipt" and path.is_file()}
        save(paths["receipt"], receipt)
    return receipt, paths["stdout"].read_text(encoding="utf-8")


def package_metadata(text, source):
    value = json.loads(text)
    result = {}
    for item in value["packages"]:
        if item["name"] not in PACKAGES:
            continue
        manifest = Path(item["manifest_path"]).resolve(strict=True)
        if not manifest.is_relative_to(source.resolve()) or item["id"] not in value["workspace_members"]:
            raise ValueError("Package metadata points outside the selected workspace")
        if item["id"] in result or any(old["name"] == item["name"] for old in result.values()):
            raise ValueError("Duplicate selected package metadata")
        result[item["id"]] = {"name": item["name"], "cwd": str(manifest.parent), "targets": item["targets"]}
    if {item["name"] for item in result.values()} != set(PACKAGES):
        raise ValueError("Selected package metadata is incomplete")
    return result


def collect_binaries(text, packages, source, target):
    result, seen = [], set()
    finished = []
    for line in text.splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        if item.get("reason") == "build-finished":
            finished.append(item.get("success"))
        if item.get("reason") != "compiler-artifact" or item.get("profile", {}).get("test") is not True or not item.get("executable"):
            continue
        if item["package_id"] not in packages:
            raise ValueError("Unexpected test executable package")
        if not isinstance(item.get("fresh"), bool):
            raise ValueError("Compiler artifact must report whether Cargo rebuilt it")
        package = packages[item["package_id"]]
        executable = Path(item["executable"])
        source_path = Path(item["target"]["src_path"])
        if (executable.is_symlink() or not executable.is_file()
                or not executable.resolve().is_relative_to(target.resolve())
                or source_path.is_symlink() or not source_path.resolve().is_relative_to(source.resolve())):
            raise ValueError("Compiler artifact is outside its source/target scope")
        match = [decl for decl in package["targets"] if decl["name"] == item["target"]["name"]
                 and decl["kind"] == item["target"]["kind"] and Path(decl["src_path"]).resolve() == source_path.resolve()]
        if len(match) != 1:
            raise ValueError("Compiler artifact target differs from cargo metadata")
        key = (package["name"], item["target"]["name"], tuple(item["target"]["kind"]))
        if key in seen or any(old["executable"] == str(executable) for old in result):
            raise ValueError("Duplicate test executable artifact")
        seen.add(key)
        result.append({"package": package["name"], "target": item["target"]["name"],
                       "kind": item["target"]["kind"], "source": source_path.relative_to(source).as_posix(),
                       "cwd": package["cwd"], "executable": str(executable),
                       "sha256": digest(executable), "bytes": executable.stat().st_size,
                       "cargo_fresh": item["fresh"]})
    if finished != [True] or not result:
        raise ValueError("No successful complete compiler-artifact stream")
    expected = {(package["name"], decl["name"], tuple(decl["kind"]))
                for package in packages.values() for decl in package["targets"]
                if decl.get("test") is True and decl["kind"] != ["bench"]}
    if seen != expected:
        raise ValueError("Compiler artifacts do not include every enabled metadata test target, including unit/bin tests")
    return sorted(result, key=lambda item: (item["package"], item["target"], item["kind"]))


def list_tests(text):
    names = []
    summary = []
    for line in text.splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"(.+): test", line)
        if match:
            names.append(match.group(1))
            continue
        match = re.fullmatch(r"(\d+) tests?, (\d+) benchmarks?", line)
        if match:
            summary.append(tuple(map(int, match.groups())))
            continue
        raise ValueError(f"Unknown libtest listing line: {line!r}")
    if summary != [(len(names), 0)] or len(names) != len(set(names)):
        raise ValueError("Test listing count, duplicate name, or benchmark mismatch")
    return names


def passed_tests(text, expected):
    rows = re.findall(r"^test (.+) \.\.\. (.+)$", text, re.MULTILINE)
    summaries = [tuple(map(int, match)) for match in SUMMARY.findall(text)]
    if (any(status != "ok" for _, status in rows) or Counter(name for name, _ in rows) != Counter(expected)
            or summaries != [(len(expected), 0, 0, 0, 0)]):
        raise ValueError("Every listed test must pass exactly once, with zero skipped/failed/benched/filtered tests")
    return [name for name, _ in rows]


def doc_result(text):
    summaries = [tuple(map(int, match)) for match in SUMMARY.findall(text)]
    if not summaries or any(any(values[1:]) for values in summaries) or text.count("test result:") != len(summaries):
        raise ValueError("Doctest command did not complete with all tests passing and none skipped")
    return {"passed": sum(values[0] for values in summaries), "summaries": summaries}


def compile_command(doc=False):
    argv = ["cargo", "test", "--doc" if doc else "--no-run", "--locked", "--release", "--timings"]
    if not doc:
        argv += ["--message-format=json"]
    for package in PACKAGES:
        argv += ["-p", package]
    return argv + ["--features", FEATURES]


def preserve_timings(target, output, before):
    result = []
    for path in sorted((target / "cargo-timings").glob("*.html")):
        if path.is_symlink():
            raise ValueError("Linked Cargo timing HTML")
        if before.get(path.name) == digest(path):
            continue
        destination = output / path.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        result.append({"path": str(destination), "sha256": digest(destination)})
    return result


def timed_command(argv, source, target, output):
    before = {path.name: digest(path) for path in (target / "cargo-timings").glob("*.html")}
    try:
        return run_process(argv, source / "engine", environment(target), output / "command")
    finally:
        save(output / "timings.json", preserve_timings(target, output / "timings", before))


def binary_roster(label, binaries, mapping, source, target, output):
    keys = []
    mapped_seen = set()
    save(output / "binaries.json", binaries)
    for index, binary in enumerate(binaries):
        pair = (binary["package"], binary["target"])
        expected_source = (mapping.original[pair]["original_path"] if label == "baseline" and pair in mapping.original
                           else mapping.target_sources.get(pair) if label == "candidate" else None)
        if expected_source is not None:
            if binary["kind"] != ["test"] or binary["source"] != expected_source:
                raise ValueError("Mapped target is not its declared integration source")
            mapped_seen.add(pair)
        elif binary["kind"] == ["test"] and binary["package"] in mapping.directories:
            raise ValueError("Unmapped integration executable")
        _, text = run_process([binary["executable"], "--list", "--color", "never"],
                              Path(binary["cwd"]), environment(target), output / f"binary-{index:03d}-list", 600)
        binary["tests"] = list_tests(text)
        binary["canonical"] = [mapping.normalize(label, *pair, name) for name in binary["tests"]]
        keys.extend(binary["canonical"])
        save(output / "binaries.json", binaries)
    expected = set(mapping.original if label == "baseline" else mapping.candidate)
    if mapped_seen != expected or not keys or len(keys) != len(set(keys)):
        raise ValueError("Missing mapped target, empty roster, or duplicate canonical test")
    save(output / "binaries.json", binaries)
    return sorted(keys)


def run_binaries(binaries, target, output):
    wall = cpu = 0.0
    count = 0
    for index, binary in enumerate(binaries):
        executable = Path(binary["executable"])
        if digest(executable) != binary["sha256"]:
            raise ValueError("Test executable changed after enumeration")
        receipt, text = run_process([str(executable), "--test-threads=1", "--color", "never", "--format", "pretty"],
                                    Path(binary["cwd"]), environment(target), output / f"binary-{index:03d}-run", 1200)
        passed_tests(text, binary["tests"])
        if digest(executable) != binary["sha256"]:
            raise ValueError("Test executable changed during execution")
        wall += receipt["wall_seconds"]
        cpu += receipt["cpu_seconds"]
        count += len(binary["tests"])
    return {"wall_seconds": wall, "cpu_seconds": cpu, "passed": count, "failed": 0,
            "ignored": 0, "benched": 0, "filtered": 0, "executables": len(binaries)}


def stage(name, sources, targets, packages, mapping, output, previous_roster=None):
    result, rosters, artifacts = {}, {}, {}
    for label in LABELS:
        if name == "cold" and targets[label].exists():
            raise ValueError("Cold compilation target must be new")
        if name == "core-edit" and not targets[label].is_dir():
            raise ValueError("Core-edit compilation must retain its own cold target")
        where = output / name / label
        compilation, text = timed_command(compile_command(), sources[label], targets[label], where / "compile")
        artifacts[label] = collect_binaries(text, packages[label], sources[label], targets[label])
        if name == "core-edit":
            core = [item for item in artifacts[label] if item["package"] == "lab-engine"
                    and item["kind"] == ["lib"] and item["source"] == MARKER_PATH]
            save(where / "compile/rebuild-evidence.json", core)
            if len(core) != 1 or core[0]["cargo_fresh"] is not False:
                raise ValueError("Core edit did not produce a freshly rebuilt core test artifact")
        rosters[label] = binary_roster(label, artifacts[label], mapping, sources[label], targets[label], where / "tests")
        result[label] = {"compile": compilation}
    save(output / name / "rosters.json", rosters)
    if rosters["baseline"] != rosters["candidate"] or (previous_roster is not None and rosters["baseline"] != previous_roster):
        raise ValueError("Actual canonical test roster differs between variants or stages")
    for label in LABELS:
        where = output / name / label
        result[label]["test_execution"] = run_binaries(artifacts[label], targets[label], where / "tests")
        doc_receipt, text = timed_command(compile_command(doc=True), sources[label], targets[label], where / "doctests")
        result[label]["doctests"] = {"command": doc_receipt, **doc_result(text)}
    if result["baseline"]["doctests"]["passed"] != result["candidate"]["doctests"]["passed"]:
        raise ValueError("Doctest pass count differs between variants")
    save(output / name / "result.json", result)
    return result, rosters["baseline"]


def require_unchanged(roots, expected):
    for label in LABELS:
        if inventory(roots[label]) != expected[label]:
            raise ValueError(f"Unexpected source/input modification: {label}")


def add_marker(sources, expected, output):
    originals = [(sources[label] / MARKER_PATH).read_bytes() for label in LABELS]
    if originals[0] != originals[1] or b"CI_TEST_BUNDLE_REBUILD_MARKER" in originals[0]:
        raise ValueError("Core-edit marker requires identical unmodified source")
    changed = originals[0] + MARKER
    output.mkdir(parents=True)
    (output / "original-lib.rs").write_bytes(originals[0])
    (output / "changed-lib.rs").write_bytes(changed)
    receipt = {"relative_path": MARKER_PATH, "appended_utf8": MARKER.decode(), "versions": {}}
    updated = {label: dict(files) for label, files in expected.items()}
    for label in LABELS:
        path = sources[label] / MARKER_PATH
        path.write_bytes(changed)
        updated[label][MARKER_PATH] = {"sha256": digest(path), "bytes": len(changed)}
        receipt["versions"][label] = {"before": expected[label][MARKER_PATH], "after": updated[label][MARKER_PATH]}
    require_unchanged(sources, updated)
    save(output / "change.json", receipt)
    return updated


def compare(args):
    roots = {label: getattr(args, label).resolve(strict=True) for label in LABELS}
    output, work = args.out_dir.resolve(), args.work_dir.resolve()
    locations = [*roots.values(), output, work]
    if any(a == b or a.is_relative_to(b) or b.is_relative_to(a)
           for index, a in enumerate(locations) for b in locations[index + 1:]):
        raise ValueError("Source, output and work directories must be distinct non-nested paths")
    if output.exists() or work.exists():
        raise FileExistsError("Output and work directories must both be new")
    output.mkdir(parents=True)
    work.mkdir(parents=True)
    result = {"schema_version": 1, "status": "running", "limitations": LIMITATIONS,
              "build_environment": BUILD_ENV, "labels_in_execution_order": list(LABELS)}
    sources = {label: work / label for label in LABELS}
    targets = {label: work / ("target-" + label) for label in LABELS}
    try:
        if platform.system() != "Linux" or not Path("/usr/bin/time").is_file():
            raise ValueError("This measurement controller requires Linux and GNU /usr/bin/time")
        mapping_bytes = args.mapping.read_bytes()
        (output / "mapping.json").write_bytes(mapping_bytes)
        mapping = Mapping(json.loads(mapping_bytes))
        original, difference = validate_sources(roots, mapping)
        save(output / "source-inventories.json", original)
        save(output / "source-differences.json", difference)
        result.update(source_sha=mapping.value["source_sha"], mapping_sha256=digest(output / "mapping.json"),
                      controller_sha256=digest(Path(__file__)), input_roots={label: str(root) for label, root in roots.items()},
                      work_dir=str(work))
        for label in LABELS:
            shutil.copytree(roots[label], sources[label], ignore=lambda path, names: [".git"] if Path(path) == roots[label] else [])
        require_unchanged(sources, original)
        env = environment(work / "setup-target")
        for name, argv in (("rustc", ["rustc", "-Vv"]), ("cargo", ["cargo", "-V"]),
                           ("gnu-time", ["/usr/bin/time", "--version"]), ("fetch", ["cargo", "fetch", "--locked"])):
            run_process(argv, sources["baseline"] / "engine", env, output / "setup" / name)
        packages = {}
        for label in LABELS:
            _, text = run_process(["cargo", "metadata", "--locked", "--no-deps", "--format-version=1"],
                                  sources[label] / "engine", env, output / "setup" / (label + "-metadata"))
            packages[label] = package_metadata(text, sources[label])
        require_unchanged(sources, original)
        result["cold"], roster = stage("cold", sources, targets, packages, mapping, output)
        require_unchanged(sources, original)
        edited = add_marker(sources, original, output / "core-edit-marker")
        result["core-edit"], _ = stage("core-edit", sources, targets, packages, mapping, output, roster)
        require_unchanged(sources, edited)
        require_unchanged(roots, original)
        result["status"] = "success"
        lines = ["# Test executable bundling: one sequential trial", "",
                 "Compile and test execution are measured separately; these observations are not a repeated speedup estimate.", "",
                 "| Stage | Variant | Compile wall s | Compile CPU s | Test wall s | Test CPU s | Binaries | Passed |",
                 "|---|---|---:|---:|---:|---:|---:|---:|"]
        for name in ("cold", "core-edit"):
            for label in LABELS:
                row = result[name][label]
                compile_stats, tests = row["compile"], row["test_execution"]
                lines.append(f"| {name} | {label} | {compile_stats['wall_seconds']:.3f} | {compile_stats['cpu_seconds']:.3f} | "
                             f"{tests['wall_seconds']:.3f} | {tests['cpu_seconds']:.3f} | {tests['executables']} | {tests['passed']} |")
        lines += ["", *["- " + item for item in LIMITATIONS], ""]
        (output / "summary.md").write_text("\n".join(lines), encoding="utf-8")
    except BaseException as error:
        result.update(status="failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        save(output / "result.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline", "candidate", "mapping", "out-dir", "work-dir"):
        parser.add_argument("--" + name, type=Path, required=True)
    compare(parser.parse_args())


if __name__ == "__main__":
    main()
