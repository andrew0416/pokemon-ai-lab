"""Fresh correctness evaluation. No performance measurements or cached verdicts.

Immutable sources, corpus and shared probes are fingerprinted. Each case has a
hard subprocess timeout; failed, timed-out and skipped cases remain in totals.
Exact differential comparisons hash the entire streamed JSONL, never a score
rounded for display. Oracle comparison uses lab-check's canonical keys (no hash
keys), with its explicit 1e-9 probability tolerance.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import gzip
import hashlib
import json
import itertools
import math
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tarfile
import time

from combined_contract import validate_activation_probe, validate_precedence

HERE = Path(__file__).resolve().parent

# These are comparison contracts chosen from fixture provenance, not exceptions
# chosen from an engine verdict. Every selected case still executes and compares.
ORACLE_CONTRACTS = {
    "rr-attract-undecided-gender": "gender-mixture-v1",
    "rr-cute-charm-undecided-gender": "gender-mixture-v1",
    "rr-rivalry-undecided-gender": "gender-mixture-v1",
    "ss-redirect-tie-hidden-order": "hidden-redirect-order-v1",
}
SCOPED_TURN_FIXTURES = {
    "bb-choicelock-struggle", "red-card-drag-update", "hyper-beam-recharge",
    "u-truant-recharge", "nn-pressure-locked-outrage",
}


def validate_oracle_contract(data, contract):
    if contract not in set(ORACLE_CONTRACTS.values()):
        raise ValueError("Unknown oracle contract")
    if data.get("schema_version") != 1 or data.get("contract") != contract:
        raise ValueError("Missing or wrong oracle contract identity")
    if data.get("status") == "engine-error":
        if not isinstance(data.get("error"), str) or not data["error"]:
            raise ValueError("Oracle contract error lacks a reason")
        return
    if data.get("status") not in ("match", "mismatch"):
        raise ValueError("Invalid oracle contract status")
    branches, checks = data.get("branches"), data.get("checks", {})
    matching, generated = data.get("matching_positions"), data.get("generated_positions")
    if (not isinstance(branches, list) or not branches or matching != len(branches)
            or not isinstance(generated, int) or generated < matching
            or not all(b.get("state_restored") is True and isinstance(b.get("distribution"), dict)
                       and b["distribution"] for b in branches)
            or checks.get("all_states_restored") is not True
            or checks.get("uniform_weights") is not True):
        raise ValueError("Missing oracle branch/probability/restoration evidence")
    for branch in branches:
        probabilities = list(branch["distribution"].values())
        weight = branch.get("normalized_weight")
        setup_probability = branch.get("setup_probability")
        if (any(not isinstance(p, (int, float)) or not math.isfinite(p) or p < 0 for p in probabilities)
                or abs(sum(probabilities) - 1.0) > 1e-9
                or not isinstance(weight, (int, float)) or not math.isfinite(weight)
                or abs(weight - 1.0 / matching) > 1e-12
                or not isinstance(setup_probability, (int, float)) or not math.isfinite(setup_probability)
                or abs(setup_probability - weight) > 1e-12):
            raise ValueError("Invalid oracle branch probability mass or weight")
    if checks.get("matching_probability_mass") != 1.0:
        raise ValueError("Incomplete oracle setup probability mass")
    if data.get("comparison", {}).get("matches") is not (data["status"] == "match"):
        raise ValueError("Oracle status disagrees with actual comparison")
    if contract == "gender-mixture-v1":
        if (data.get("scope") != "complete-weighted-gender-mixture"
                or checks.get("expected_assignments") != matching
                or checks.get("distinct_assignments") != matching
                or len({b.get("gender_assignment") for b in branches}) != matching
                or not data.get("engine_distribution")):
            raise ValueError("Incomplete gender mixture contract")
    else:
        if (data.get("scope") != "both-hidden-histories-with-independent-oracles"
                or matching != 2 or checks.get("direct_oracle_histories") != 2
                or checks.get("metamorphic_histories") != 0
                or {b.get("recipient_slot") for b in branches} != {0, 1}
                or any(b.get("validation") != "direct-oracle" for b in branches)):
            raise ValueError("Incomplete independent hidden-history oracle coverage")


def read(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def write(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def sha(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def variant(name):
    return next(v for v in read(HERE / "variants.json")["variants"] if v["id"] == name)


def semantic_oracle(verdict):
    return {k: v for k, v in verdict.items() if k != "engineMs"}


def scoped_status(counts):
    if counts["errors"]:
        return "error"
    return "empty" if not counts["positions"] or not counts["outcomes"] or counts["empty_positions"] else "ok"


EXPECTED_CHOICE_REJECTIONS = {
    "bb-choicelock-struggle": ("decision", "invalid", '\"move harden\": empty slot'),
    "red-card-drag-update": ("decision", "invalid", '\"move harden\": empty slot'),
    "hyper-beam-recharge": ("enumeration", "invalid", "One slot 0: target 0 for Hyper Beam (Normal)"),
    "u-truant-recharge": ("enumeration", "invalid", "One slot 0: target 0 for Hyper Beam (Normal)"),
    "nn-pressure-locked-outrage": ("enumeration", "invalid", "One slot 0: target 1 for Outrage (RandomNormal)"),
}


def validate_scoped_turn(first, lines):
    fields = ("positions", "outcomes", "errors", "hidden_diagnostics", "enumerations_restored",
              "outcome_rollbacks", "position_completions", "empty_positions")
    scopes = {scope: dict.fromkeys(fields, 0) for scope in ("requested", "additional", "global")}
    positions, ended, observed_outcomes = {}, set(), {}
    total, last, before, loaded, rejected = 1, first, None, False, 0
    if first.get("kind") != "case" or first.get("scope_policy") != "oracle-before-with-additional-observations":
        raise ValueError("Invalid scoped turn start")
    for row in lines:
        if not isinstance(row, dict) or last.get("kind") == "complete":
            raise ValueError("Invalid or extra scoped turn record")
        last, total = row, total + 1
        kind = row.get("kind")
        if kind == "complete":
            continue
        if kind == "loaded":
            if loaded or not isinstance(row.get("before"), dict) or "recorded_turn_choices" not in row:
                raise ValueError("Missing/duplicate canonical selector or original choices")
            before, loaded = row["before"], True
            continue
        if kind not in ("position", "outcome", "error", "position-complete"):
            raise ValueError("Unknown scoped turn record")
        scope = row.get("scope")
        if scope not in scopes:
            raise ValueError("Missing or invalid turn scope")
        counts, position = scopes[scope], row.get("position")
        if kind == "position":
            if not isinstance(position, int) or position in positions:
                raise ValueError("Duplicate or invalid position identity")
            canonical = row.get("canonical_state")
            if scope == "global":
                if canonical is not None:
                    raise ValueError("Unclassified position unexpectedly has a canonical state")
            elif not loaded or not isinstance(canonical, dict) or (canonical == before) != (scope == "requested"):
                raise ValueError("Position scope does not match the oracle before selector")
            positions[position], observed_outcomes[position] = scope, 0
            counts["positions"] += 1
            continue
        if position is not None:
            if positions.get(position) != scope or position in ended:
                raise ValueError("Record has no open parent in the same scope")
        elif kind != "error" or scope != "global":
            raise ValueError("Record lacks its parent")
        if kind == "outcome":
            if row.get("input_restored") is not True or row.get("incremental_hash_checked") is not True:
                raise ValueError("Outcome restoration/hash invariant missing")
            if row.get("outcome") != observed_outcomes[position]:
                raise ValueError("Missing or duplicate outcome index")
            observed_outcomes[position] += 1
            counts["outcomes"] += 1
            counts["outcome_rollbacks"] += 1
            counts["hidden_diagnostics"] += row.get("hidden", {}).get("status") != "ok"
        elif kind == "position-complete":
            if (row.get("enumeration_restored") is not True or row.get("all_outcomes_reversed") is not True
                    or row.get("outcomes") != observed_outcomes[position]):
                raise ValueError("Incomplete scoped position restoration/counts")
            ended.add(position)
            counts["position_completions"] += 1
            counts["enumerations_restored"] += 1
            counts["empty_positions"] += observed_outcomes[position] == 0
        else:
            if position is not None:
                if row.get("input_restored") is not True:
                    raise ValueError("Errored position was not restored")
                ended.add(position)
            counts["errors"] += 1
            if row.get("stage") == "enumeration":
                counts["enumerations_restored"] += 1
            expected = row.get("expected_choice_rejection")
            if not isinstance(expected, bool):
                raise ValueError("Error lacks explicit rejection classification")
            if expected:
                signature = (row.get("stage"), row.get("category"), row.get("message"))
                if (scope != "additional" or not isinstance(first.get("scenario"), str)
                        or row.get("contract") != Path(first["scenario"]).stem
                        or signature != EXPECTED_CHOICE_REJECTIONS.get(row.get("contract"))):
                    raise ValueError("Unrecognized error cannot be an expected choice rejection")
                rejected += 1
            elif row.get("contract") is not None:
                raise ValueError("Unexpected error carries an accepted contract")
    if last.get("kind") != "complete" or last.get("schema_version") != 2 or ended != set(positions):
        raise ValueError("Missing scoped turn completion or incomplete parents")
    for name, counts in scopes.items():
        expected = dict(counts, status=scoped_status(counts))
        if last.get("scopes", {}).get(name) != expected:
            raise ValueError("Scoped terminal counts/status disagree: " + name)
    summed = {field: sum(scope[field] for scope in scopes.values()) for field in fields}
    for field in ("positions", "outcomes", "errors", "hidden_diagnostics", "enumerations_restored", "outcome_rollbacks"):
        if last.get(field) != summed[field]:
            raise ValueError("Wrong combined scoped count: " + field)
    requested, additional, global_scope = (scopes[k] for k in ("requested", "additional", "global"))
    unexpected = additional["errors"] - rejected
    status = ("error" if global_scope["errors"] or unexpected or additional["empty_positions"] else
              "no-matching-parent" if not requested["positions"] else scoped_status(requested))
    selection = {"mode": "canonical-before", "matched_parents": requested["positions"],
                 "additional_parents": additional["positions"], "unclassified_parents": global_scope["positions"]}
    if (last.get("status") != status or last.get("all_positions_status") != scoped_status(summed)
            or last.get("success_scope") != "requested" or last.get("selection") != selection
            or last.get("expected_choice_rejections") != rejected
            or last.get("unexpected_additional_errors") != unexpected):
        raise ValueError("Wrong scoped turn verdict or selection")
    return {"records": total, "last": last, "complete": True, "status": status,
            "successful": status == "ok", "success_scope": "requested",
            "coverage": {"selection": selection, "scopes": last["scopes"],
                         "all_positions_status": last["all_positions_status"],
                         "expected_choice_rejections": rejected}}


def validate_probe(kind, lines):
    rows = iter(lines)
    first = next(rows, None)
    if kind == "turn" and isinstance(first, dict) and first.get("schema_version") == 2:
        return validate_scoped_turn(first, rows)
    return validate_probe_v1(kind, itertools.chain([] if first is None else [first], rows))


def validate_probe_v1(kind, lines):
    counts, first, last, total = {}, None, None, 0
    for row in lines:
        if not isinstance(row, dict):
            raise ValueError("Probe record must be an object")
        if last and last.get("kind") == "complete":
            raise ValueError("Record after terminal complete")
        first = row if first is None else first
        last, total = row, total + 1
        key = row.get("kind")
        counts[key] = counts.get(key, 0) + 1
        if kind == "turn" and key == "outcome":
            if row.get("input_restored") is not True or row.get("incremental_hash_checked") is not True:
                raise ValueError("Outcome restoration/hash invariant missing")
        if kind == "turn" and key == "position-complete":
            if row.get("enumeration_restored") is not True or row.get("all_outcomes_reversed") is not True:
                raise ValueError("Position restoration missing")
    if not total:
        raise ValueError("No probe records")
    if kind == "turn":
        if (first.get("kind") != "case" or counts.get("case") != 1
                or last.get("kind") != "complete" or counts.get("complete") != 1
                or first.get("schema_version") != 1 or last.get("schema_version") != 1):
            raise ValueError("Missing or duplicated turn start/completion")
        for field, record_kind in (("positions", "position"), ("outcomes", "outcome"), ("errors", "error")):
            if last.get(field) != counts.get(record_kind, 0):
                raise ValueError("Wrong terminal count: " + field)
        if last.get("outcome_rollbacks") != last["outcomes"]:
            raise ValueError("Incomplete rollback count")
        expected = "error" if last["errors"] else ("ok" if last["positions"] and last["outcomes"] else "empty")
        if last.get("status") != expected:
            raise ValueError("Wrong turn status")
        if expected == "ok" and (last.get("enumerations_restored") != last["positions"]
                                 or counts.get("position-complete", 0) != last["positions"]):
            raise ValueError("Incomplete position enumeration")
    elif kind == "search":
        if total != 1 or last.get("schema_version") != 1 or last.get("status") not in ("ok", "error"):
            raise ValueError("Invalid search result envelope")
        if last.get("stage") == "search":
            if last.get("state_restored") is not True or not all(k in last for k in ("input", "result", "stats", "config")):
                raise ValueError("Search restoration/result evidence missing")
        elif last.get("status") != "error":
            raise ValueError("Search never reached evaluation")
    else:
        raise ValueError("Unknown probe kind")
    return {"records": total, "last": last, "complete": True, "status": last["status"],
            "successful": last["status"] == "ok"}


def bounded(command, stdout, stderr, timeout, cwd=None, env=None):
    """Timeout kills the entire child process group on the Linux evaluation host."""
    with Path(stdout).open("wb") as out, Path(stderr).open("wb") as err:
        p = subprocess.Popen(command, stdout=out, stderr=err, cwd=cwd, env=env,
                             start_new_session=(os.name != "nt"))
        try:
            return p.wait(timeout=timeout), False
        except subprocess.TimeoutExpired:
            if os.name != "nt":
                os.killpg(p.pid, signal.SIGKILL)
            else:
                p.kill()
            p.wait()
            return p.returncode, True


def extract_checked(archive, destination):
    destination = Path(destination).resolve()
    destination.mkdir(parents=True, exist_ok=False)
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            target = (destination / member.name).resolve()
            if not target.is_relative_to(destination) or not member.isfile():
                raise ValueError(f"unsafe corpus member: {member.name}")
            target.parent.mkdir(parents=True, exist_ok=True)
            with tar.extractfile(member) as source, target.open("xb") as out:
                shutil.copyfileobj(source, out)


def plan(args):
    manifest = read(HERE / "data/corpus-manifest.json")
    archive = HERE / "data/corpus.tar.gz"
    assert sha(archive) == manifest["archive_sha256"]
    corpus = Path("corpus")
    extract_checked(archive, corpus)
    for relative, meta in manifest["members"].items():
        assert sha(corpus / relative) == meta["sha256"], relative
    plain = Path("corpus-plain")
    plain.mkdir(exist_ok=False)
    jobs, inputs, scenario_paths, before_paths = [], {
        (corpus / relative).as_posix(): meta["sha256"] for relative, meta in manifest["members"].items()
    }, {}, {}
    for entry in manifest["turn_jobs"]:
        key = entry["id"]
        target = plain / (key + ".json")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(gzip.decompress((corpus / entry["scenario"]).read_bytes()))
        scenario_paths[key] = target.as_posix()
        inputs[target.as_posix()] = sha(target)

    def add(key, kind, input_id, argv, timeout, factored=0, **contracts):
        jobs.append({"id": key, "kind": kind, "input_id": input_id, "args": argv,
                     "timeout": timeout, "factored": factored, **contracts})

    for entry in manifest["oracle_jobs"]:
        add("oracle/corpus/" + entry["id"], "oracle", entry.get("position_id", entry["id"]),
            [(corpus / entry["scenario"]).as_posix(), (corpus / entry["report"]).as_posix(),
             "--tolerance", "1e-9"], 120)
    for entry in manifest["turn_jobs"]:
        add("turn/corpus/" + entry["id"], "turn", entry["id"],
            [scenario_paths[entry["id"]], "--rolls", "median", "--factored", "off"], 90)
    for i, entry in enumerate(manifest["search_jobs"]):
        key = entry["id"]
        report = json.loads(gzip.decompress((corpus / entry["report"]).read_bytes()))
        before_path = plain / (key + ".before.json")
        write(before_path, report["before"])
        inputs[before_path.as_posix()] = sha(before_path)
        before_paths[key] = before_path.as_posix()
        # Balanced sides and semantics, selected independently of runtime/results.
        mode = "mixed" if i % 2 else "maximin"
        side = str(1 + (i // 2) % 2)
        chance = "worst" if (i // 4) % 2 else "expect"
        common = [scenario_paths[key], mode, side, chance, "median", "1", "--before", before_path.as_posix()]
        add("search/main/" + key, "search", key, common, 90)
        if i % 8 == 0:
            add("turn/factored/" + key, "turn", key,
                [scenario_paths[key], "--rolls", "median", "--factored", "on"], 90, 1)
            extra = common.copy()
            extra[1], extra[4] = "mixed", "extremes"
            add("search/extremes/" + key, "search", key, extra, 120)
        if i % 16 == 0:
            extra = common.copy()
            extra[1], extra[3], extra[5] = "mixed", "expect", "2"
            add("search/parallel/" + key, "search", key, extra, 90)
        if i % 16 == 8:
            extra = common.copy()
            extra[1], extra[3] = "deep", "expect"
            add("search/deep/" + key, "search", key, extra, 180)

    # Broader hand-authored mechanics fixtures, using their original exact reports.
    controller = HERE.parents[2]
    fixture_root = controller / "engine/oracle"
    skipped, seen_scenarios = [], set()
    for file in sorted((fixture_root / "expected").glob("*.json")):
        report = read(file)
        if file.name == "vd-frozen-time-merge.frz-time.json":
            skipped.append({"file": file.name, "reason": "freeze statusTime extension requires existing dedicated comparator"})
            continue
        if not (report.get("mode") in ("full", "extremes", "fixed")
                and isinstance(report.get("before"), dict) and isinstance(report.get("outcomes"), list)):
            skipped.append({"file": file.name, "reason": "not an exact turn distribution report"})
            continue
        scenario = (controller / report["scenario"]).resolve()
        assert scenario.is_relative_to((fixture_root / "scenarios").resolve()) and scenario.is_file()
        # Keep paths relative to the workflow workspace for identical output/error strings.
        scen, rep = os.path.relpath(scenario).replace(os.sep, "/"), os.path.relpath(file).replace(os.sep, "/")
        inputs[scen], inputs[rep] = sha(scenario), sha(file)
        contract = ORACLE_CONTRACTS.get(scenario.stem)
        if contract:
            argv = [scen, rep, "--contract", contract, "--tolerance", "1e-9"]
            if contract == "hidden-redirect-order-v1":
                alternate = HERE / "data/contracts/ss-redirect-tie-hidden-order-rod-a.turn.json"
                alternate_path = os.path.relpath(alternate).replace(os.sep, "/")
                inputs[alternate_path] = sha(alternate)
                argv += ["--alternate-report", alternate_path]
            add("oracle/fixture/" + file.name, "oracle", "fixture/" + scenario.stem,
                argv, 120, oracle_contract=contract)
        else:
            add("oracle/fixture/" + file.name, "oracle", "fixture/" + scenario.stem,
                [scen, rep, "--tolerance", "1e-9"], 120)
        if scen not in seen_scenarios:
            seen_scenarios.add(scen)
            argv = [scen, "--rolls", "median", "--factored", "off"]
            if scenario.stem in SCOPED_TURN_FIXTURES:
                argv += ["--before", rep]
            add("turn/fixture/" + scenario.stem, "turn", "fixture/" + scenario.stem,
                argv, 90)
    assert len({j["id"] for j in jobs}) == len(jobs), "Duplicate case identity"
    write(args.out, {"schema": 1, "manifest_sha256": sha(HERE / "data/corpus-manifest.json"),
                     "archive_sha256": sha(archive), "jobs": jobs, "inputs": inputs,
                     "excluded_fixture_reports": skipped,
                     "corpus_counts": manifest["counts"], "search_selection": manifest["search_selection"]})
    print(json.dumps({"jobs": len(jobs), "by_kind": {k: sum(j["kind"] == k for j in jobs)
          for k in ("oracle", "turn", "search")}, "excluded_fixture_reports": len(skipped)}))


def expected_features(v):
    # Explicit allow-list; observer dependencies are part of the fingerprint.
    requested = v["features"]
    if len(requested) != len(set(requested)):
        raise ValueError("Duplicate requested agreement features")
    core = set()
    search = {"cli", "default", "lab-scenario", "scenario", "serde_json"}
    closures = {
        "lab-engine/experiment-hurt-readers": ({"experiment-hurt-readers"}, set()),
        "lab-engine/experiment-compact-volatiles": ({"experiment-compact-volatiles"}, set()),
        "lab-search/experiment-leaf-ending-observer": (
            {"experiment-leaf-ending-states", "experiment-leaf-ending-observer"},
            {"experiment-leaf-ending-states", "experiment-leaf-ending-observer"}),
        "lab-search/experiment-prepared-turn-observe": (
            {"experiment-prepared-turn", "experiment-prepared-turn-observe"},
            {"experiment-prepared-turn", "experiment-prepared-turn-observe"}),
    }
    for feature in requested:
        if feature not in closures:
            raise ValueError("Unrecognized agreement feature: " + feature)
        expected_core, expected_search = closures[feature]
        core |= expected_core
        search |= expected_search
    return core, search


def activation_checks(v, records, jobs, binaries, result_dir):
    core, _ = expected_features(v)
    leaf_required = "experiment-leaf-ending-observer" in core
    prepared_required = "experiment-prepared-turn-observe" in core
    activation = {"required": leaf_required or prepared_required, "passed": True,
                  "leaf_required": leaf_required, "prepared_required": prepared_required}
    observed = []
    for record in records:
        if record["kind"] != "search" or not record.get("successful"):
            continue
        for line in record["stderr"].splitlines():
            try:
                value = json.loads(line)
                if value.get("phase") == "search-complete" and value.get("requested_threads") == 1:
                    observed.append((record, value))
            except ValueError:
                continue
    if leaf_required:
        leaf = {
            "visited_cases": sum(meta.get("leaf", {}).get("visits", 0) > 0 for _, meta in observed),
            "batches": sum(meta.get("leaf", {}).get("batches", 0) for _, meta in observed),
            "visits": sum(meta.get("leaf", {}).get("visits", 0) for _, meta in observed),
            "observer_compiled": bool(observed) and all(
                meta.get("leaf_observer_compiled") is True for _, meta in observed),
        }
        leaf["passed"] = leaf["observer_compiled"] and leaf["batches"] > 0 and leaf["visits"] > 0
        activation["leaf"] = leaf
        activation["passed"] &= leaf["passed"]
    if prepared_required:
        by_id = {j["id"]: j for j in jobs}
        checks = []
        # Preserve the original predetermined eight deep controls. P9 owns their
        # turn leaves; nash_cells uses ordinary transitions for nonleaf work.
        # These controls demonstrate precedence, not prepared-path activation.
        # A separate fixed exact-depth-two probe below proves actual coexistence.
        if leaf_required:
            control_ids = [j["id"] for j in jobs if j["kind"] == "search"
                           and j["args"][1] == "deep" and j["args"][5] == "1"][:8]
            observed_by_id = {r["id"]: (r, m) for r, m in observed}
            controls = [observed_by_id[key] for key in control_ids if key in observed_by_id]
        else:
            control_ids = [r["id"] for r, _ in observed[:8]]
            controls = observed[:8]
        # The leaf path stays enabled in BOTH controls. Only prepared is toggled.
        for record, metadata in controls:
            job = dict(by_id[record["id"]])
            job["id"] = "prepared-off-control/" + job["id"]
            job["args"] = [*job["args"], "--prepared", "off"]
            off = run_case(job, binaries, result_dir)
            off_metadata = []
            for line in off["stderr"].splitlines():
                try:
                    meta = json.loads(line)
                    if isinstance(meta, dict) and meta.get("phase") == "search-complete":
                        off_metadata.append(meta)
                except ValueError:
                    pass
            off_meta = off_metadata[0] if len(off_metadata) == 1 else {}
            on_count = metadata.get("prepared", {}).get("parent_checks")
            off_count = off_meta.get("prepared", {}).get("parent_checks")
            checks.append({"id": record["id"], "equal": off["complete"] and off.get("successful", False)
                           and off["sha256"] == record["sha256"], "on_parent_checks": on_count,
                           "off_parent_checks": off_count,
                           "toggle_confirmed": metadata.get("prepared_requested") is True
                           and off_meta.get("prepared_requested") is False
                           and metadata.get("prepared_compiled") is True
                           and off_meta.get("prepared_compiled") is True
                           and metadata.get("prepared_observer_compiled") is True
                           and off_meta.get("prepared_observer_compiled") is True})
            if leaf_required:
                checks[-1].update(on_metadata=metadata, off_metadata=off_meta)
                checks[-1]["leaf_active_both"] = all(
                    meta.get("leaf_observer_compiled") is True
                    and all(type(meta.get("leaf", {}).get(key)) is int
                            and meta["leaf"][key] > 0 for key in ("batches", "visits"))
                    for meta in (metadata, off_meta))
        prepared = {"control_selection": "frozen-plan-first-eight-deep-single-thread" if leaf_required
                    else "first-eight-successful-single-thread",
                    "expected_control_ids": control_ids, "controls": checks,
                    "passed": len(control_ids) == 8 and len(checks) == 8
                    and all(c["equal"] and c["toggle_confirmed"] for c in checks) and any(
                    type(c["on_parent_checks"]) is int and type(c["off_parent_checks"]) is int
                    and 0 < c["on_parent_checks"] < c["off_parent_checks"] for c in checks)}
        if leaf_required:
            prepared["purpose"] = "leaf-precedence"
            prepared["passed"] = True  # Validator recomputes the complete contract below.
            try:
                if len(observed) != len({record["id"] for record, _ in observed}):
                    raise ValueError("Duplicate search-complete observer records")
                validate_precedence(prepared)
            except (ValueError, TypeError):
                prepared["passed"] = False
            activation["prepared_nonleaf"] = run_nonleaf_activation(binaries, result_dir)
            activation["passed"] &= activation["prepared_nonleaf"]["passed"]
        activation["prepared"] = prepared
        activation["passed"] &= prepared["passed"]
    return activation


def run_nonleaf_activation(binaries, result_dir):
    """Independent eight-control gate, deliberately excluded from the frozen 6,184 cases."""
    stdout = result_dir / "prepared-nonleaf-activation.stdout"
    stderr = result_dir / "prepared-nonleaf-activation.stderr"
    receipt = {"passed": False, "timeout": False, "returncode": None,
               "output_file": stdout.name, "stderr_file": stderr.name}
    try:
        binary = binaries["activation"]
        receipt["binary_sha256"] = sha(binary)
        code, timeout = bounded([str(binary)], stdout, stderr, 120,
                                env=dict(os.environ, LAB_ENGINE_FACTORED="0"))
        receipt.update(returncode=code, timeout=timeout, stdout_sha256=sha(stdout))
        if stdout.stat().st_size > 8 * 1024 * 1024:
            raise ValueError("Activation output exceeds fixed envelope limit")
        def unique_object(pairs):
            value = {}
            for key, item in pairs:
                if key in value:
                    raise ValueError("Duplicate activation JSON key: " + key)
                value[key] = item
            return value
        probe = json.loads(stdout.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
        receipt["probe"] = probe
        validate_activation_probe(probe)
        if code != 0 or timeout:
            raise ValueError("Activation process failed or timed out")
        receipt["passed"] = True
    except (ValueError, OSError, KeyError, TypeError) as error:
        receipt["error"] = f"{type(error).__name__}: {error}"
    write(result_dir / "prepared-nonleaf-activation.json", receipt)
    return receipt


def uses_combined_activation(v):
    core, _ = expected_features(v)
    return {"experiment-prepared-turn-observe", "experiment-leaf-ending-observer"} <= core


def prepare(args):
    v = variant(args.variant)
    root, out = args.source.resolve(), args.out.resolve()
    assert git(root, "rev-parse", "HEAD") == v["sha"]
    assert not git(root, "status", "--porcelain"), "Source checkout must be clean"
    out.mkdir(parents=True, exist_ok=False)
    injected = {}
    probes = [("turn", "scenario"), ("search", "search"), ("oracle", "scenario")]
    if uses_combined_activation(v):
        probes.append(("activation", "search"))
    for name, package in probes:
        relative = f"engine/{package}/examples/ci_agreement_{name}.rs"
        target = root / relative
        assert not target.exists()
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(HERE / f"{name}_probe.rs", target)
        injected[relative] = sha(target)
    write(out / "provenance.json", {
        "schema": 1, "variant": v, "controller_sha": git(HERE, "rev-parse", "HEAD"),
        "injected_sources": injected,
        "controller_files": {p.name: sha(p) for p in HERE.glob("*.py")},
        "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
        "environment": {k: os.environ.get(k) for k in (
            "RUSTFLAGS", "CARGO_PROFILE_RELEASE_LTO", "LAB_ENGINE_FACTORED",
            "RUST_MIN_STACK", "RAYON_NUM_THREADS")},
        "cached_results_reused": False, "performance_measured": False,
    })
    command = ["cargo", "build", "--release", "--locked", "-p", "lab-scenario",
               "--bin", "lab-check", "--example", "ci_agreement_turn",
               "--example", "ci_agreement_oracle", "-p", "lab-search",
               "--example", "ci_agreement_search"]
    if uses_combined_activation(v):
        command += ["--example", "ci_agreement_activation"]
    if v["features"]:
        command += ["--features", ",".join(v["features"])]
    code, timeout = bounded(command, out / "build.log", out / "build.err", 1200, cwd=root / "engine")
    assert code == 0 and not timeout, "Build failed; see preserved logs"
    target = root / "engine/target/release"
    actual = {}
    for package in ("lab-engine", "lab-scenario", "lab-search"):
        matches = list((target / ".fingerprint").glob(package + "-*/lib-*.json"))
        assert len(matches) == 1, (package, matches)
        fingerprint = read(matches[0])
        actual[package] = json.loads(fingerprint["features"])
    expected_core, expected_search = expected_features(v)
    assert set(actual["lab-engine"]) == expected_core, actual
    assert set(actual["lab-search"]) == expected_search, actual
    receipt = read(out / "provenance.json")
    binary_paths = {"oracle": "lab-check", "turn": "examples/ci_agreement_turn",
                    "oracle-contract": "examples/ci_agreement_oracle",
                    "search": "examples/ci_agreement_search"}
    if uses_combined_activation(v):
        binary_paths["activation"] = "examples/ci_agreement_activation"
    receipt.update(actual_features=actual, binaries={
        name: sha(target / path) for name, path in binary_paths.items()})
    write(out / "provenance.json", receipt)


def run_case(job, binaries, result_dir):
    name = hashlib.sha256(job["id"].encode()).hexdigest()[:24]
    stdout, stderr = result_dir / (name + ".stdout"), result_dir / (name + ".stderr")
    binary = "oracle-contract" if job.get("oracle_contract") else job["kind"]
    command = [str(binaries[binary]), *job["args"]]
    env = dict(os.environ, LAB_ENGINE_FACTORED=str(job.get("factored", 0)))
    code, timeout = bounded(command, stdout, stderr, job["timeout"], env=env)
    record = {"id": job["id"], "kind": job["kind"], "input_id": job["input_id"],
              "returncode": code, "timeout": timeout, "sha256": sha(stdout),
              "stdout_bytes": stdout.stat().st_size, "complete": False}
    try:
        if job["kind"] == "oracle":
            data = read(stdout)
            allowed = {"match", "mismatch", "unsupported", "engine-error", "no-position", "ambiguous", "check-timeout"}
            assert data["status"] in allowed
            if job.get("oracle_contract"):
                validate_oracle_contract(data, job["oracle_contract"])
                record["oracle_contract"] = job["oracle_contract"]
            record.update(status=data["status"], verdict=data, successful=data["status"] == "match",
                          complete=not timeout and code == (0 if data["status"] == "match" else 1))
        else:
            with stdout.open(encoding="utf-8") as f:
                record.update(validate_probe(job["kind"], (json.loads(line) for line in f)))
            record["complete"] &= not timeout and code == 0
            if code:
                record["status"] = "process-failed"
    except (ValueError, OSError, KeyError, AssertionError) as e:
        record.update(status="invalid-output", parse_error=str(e))
        record["complete"] = False
    if timeout:
        record["status"] = "timeout"
        record["complete"] = False
    # Preserve stderr (observers/diagnostics) and a compact first-output sample.
    record["stderr"] = stderr.read_text(encoding="utf-8", errors="replace")[:100000]
    with stdout.open("rb") as f:
        record["sample"] = f.read(4096).decode("utf-8", errors="replace")
    # Full streamed output retained compressed, including partial failures.
    with stdout.open("rb") as src, gzip.GzipFile(filename=str(stdout) + ".gz", mode="wb", mtime=0) as dst:
        shutil.copyfileobj(src, dst)
    stdout.unlink()
    record["output_file"] = stdout.name + ".gz"
    write(result_dir / (name + ".json"), record)
    return record


def evaluate(args):
    # Job plan is frozen before any variant is run and shared byte-for-byte.
    out = args.out.resolve()
    plan_data = read(args.plan)
    jobs = plan_data["jobs"]
    for relative, h in plan_data["inputs"].items():
        assert sha(relative) == h, relative
    ids = [j["id"] for j in jobs]
    assert len(ids) == len(set(ids)) and jobs
    root = args.source.resolve()
    binaries = {"oracle": root / "engine/target/release/lab-check",
                "oracle-contract": root / "engine/target/release/examples/ci_agreement_oracle",
                "turn": root / "engine/target/release/examples/ci_agreement_turn",
                "search": root / "engine/target/release/examples/ci_agreement_search"}
    if uses_combined_activation(variant(args.variant)):
        binaries["activation"] = root / "engine/target/release/examples/ci_agreement_activation"
    receipt = read(out / "provenance.json")
    assert receipt["variant"] == variant(args.variant)
    assert all(sha(path) == receipt["binaries"][k] for k, path in binaries.items())
    for relative, h in receipt["injected_sources"].items():
        assert sha(root / relative) == h
    assert git(root, "rev-parse", "HEAD") == receipt["variant"]["sha"]
    assert not git(root, "diff", "HEAD", "--name-only"), "Tracked source or index changed"
    result_dir = out / "cases"
    result_dir.mkdir(exist_ok=False)
    records = []
    # The append-only partial result ledger survives an interrupted job.
    partial = out / "partial.jsonl"
    with ThreadPoolExecutor(max_workers=2) as pool:
        for index, record in enumerate(pool.map(lambda j: run_case(j, binaries, result_dir), jobs), 1):
            records.append(record)
            with partial.open("a", encoding="utf-8") as f:
                f.write(json.dumps(record, ensure_ascii=False) + "\n")
            if index % 100 == 0:
                print(f"{args.variant}: completed {index}/{len(jobs)} cases", flush=True)
    assert {r["id"] for r in records} == set(ids)
    activation = activation_checks(variant(args.variant), records, jobs, binaries, result_dir)
    summary = {"schema": 1, "variant": args.variant, "plan_sha256": sha(args.plan),
               "expected": len(jobs), "observed": len(records), "activation": activation, "cases": records}
    write(out / "results.json", summary)


def compare(args):
    variants = read(HERE / "variants.json")["variants"]
    plans = list(args.results.rglob("case-plan.json"))
    errors, by_variant, results = [], {}, {}
    expected_ids, expected_kinds = set(), {}
    plan_hashes = {sha(path) for path in plans}
    if len(plan_hashes) != 1:
        errors.append("missing or conflicting frozen case plans")
    if plans:
        declared = read(plans[0])["jobs"]
        expected_ids = {j["id"] for j in declared}
        expected_kinds = {j["id"]: j["kind"] for j in declared}
        if len(expected_ids) != len(declared):
            errors.append("duplicate case IDs in plan")
    for v in variants:
        folder = args.results / v["id"]
        if not folder.is_dir():
            folder = args.results / ("agreement-" + v["id"]) / "results" / v["id"]
        full, partial = folder / "results.json", folder / "partial.jsonl"
        if full.exists():
            result = read(full)
            if result.get("variant") != v["id"]:
                errors.append(v["id"] + ": wrong variant identity")
            if plan_hashes and result.get("plan_sha256") not in plan_hashes:
                errors.append(v["id"] + ": wrong plan hash")
            records = result["cases"]
            if len(records) != result["observed"] or result["expected"] != len(expected_ids):
                errors.append(v["id"] + ": wrong declared counts")
            results[v["id"]] = result
        else:
            records = []
            if partial.exists():
                for index, line in enumerate(partial.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
                    try:
                        row = json.loads(line)
                        if not isinstance(row, dict) or "id" not in row:
                            raise ValueError("invalid partial row")
                        records.append(row)
                    except ValueError:
                        errors.append(f"{v['id']}: malformed partial line {index}")
            errors.append(v["id"] + ": missing completed result (partial records retained)")
        rows = {r["id"]: r for r in records}
        if len(rows) != len(records):
            errors.append(v["id"] + ": duplicate results")
        if set(rows) != expected_ids:
            errors.append(v["id"] + ": missing or extra cases")
        by_variant[v["id"]] = rows
    summary = {"schema": 1, "comparisons": {}, "oracle": {}, "expected_cases": len(expected_ids),
               "validation_errors": errors, "differential_complete": not errors,
               "activation": {name: r.get("activation", {"passed": False}) for name, r in results.items()}}
    for name, rows in by_variant.items():
        counts = {}
        for row in rows.values():
            if row["kind"] == "oracle":
                counts[row["status"]] = counts.get(row["status"], 0) + 1
        summary["oracle"][name] = counts
        summary.setdefault("oracle_contracts", {})[name] = [
            {"id": row["id"], "contract": row["oracle_contract"], "status": row["status"],
             "scope": row.get("verdict", {}).get("scope"),
             "matching_positions": row.get("verdict", {}).get("matching_positions"),
             "checks": row.get("verdict", {}).get("checks", {})}
            for row in rows.values() if row.get("oracle_contract")]
        summary.setdefault("scoped_turn_coverage", {})[name] = [
            {"id": row["id"], "status": row["status"], "success_scope": row["success_scope"],
             **row["coverage"]} for row in rows.values() if row.get("success_scope")]
    for v in variants:
        if not v["compare_to"]:
            continue
        counts = {"equal_success": 0, "equal_error": 0, "different": 0, "uncompared": 0}
        details, per_kind = [], {}
        base = by_variant[v["compare_to"]]
        for key in sorted(expected_ids):
            candidate, original = by_variant[v["id"]].get(key), base.get(key)
            kind = expected_kinds[key]
            if not candidate or not original or not candidate["complete"] or not original["complete"]:
                status = "uncompared"
            elif kind == "oracle":
                same = semantic_oracle(candidate["verdict"]) == semantic_oracle(original["verdict"])
                status = "equal_success" if same and candidate.get("successful") and original.get("successful") else ("equal_error" if same else "different")
            else:
                same = candidate["sha256"] == original["sha256"]
                status = "equal_success" if same and candidate.get("successful") and original.get("successful") else ("equal_error" if same else "different")
            counts[status] += 1
            pc = per_kind.setdefault(kind, {"equal_success": 0, "equal_error": 0, "different": 0, "uncompared": 0})
            pc[status] += 1
            if status != "equal_success":
                details.append({"id": key, "status": status, "baseline_status": original["status"] if original else "missing",
                                "candidate_status": candidate["status"] if candidate else "missing"})
        summary["comparisons"][v["id"]] = {"baseline": v["compare_to"], **counts,
                                               "by_kind": per_kind, "details": details}
        summary["differential_complete"] &= counts["different"] == 0 and counts["uncompared"] == 0
    summary["oracle_all_match"] = bool(expected_ids) and all(
        set(counts) == {"match"} and counts["match"] == sum(k == "oracle" for k in expected_kinds.values())
        for counts in summary["oracle"].values())
    summary["all_requested_successful"] = summary["differential_complete"] and summary["oracle_all_match"] and all(
        c["equal_error"] == 0 for c in summary["comparisons"].values())
    summary["activation_passed"] = len(results) == len(variants) and all(a.get("passed") for a in summary["activation"].values())
    summary["complete"] = summary["all_requested_successful"] and summary["activation_passed"]
    write(args.out, summary)
    print(json.dumps({k: v for k, v in summary.items() if k != "comparisons"}, indent=2))
    return 0 if summary["complete"] else 1


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="command", required=True)
    q = sub.add_parser("plan")
    q.add_argument("--out", type=Path, required=True)
    for name in ("prepare", "evaluate"):
        q = sub.add_parser(name)
        q.add_argument("--source", type=Path, required=True)
        q.add_argument("--out", type=Path, required=True)
        q.add_argument("--variant", required=True)
        if name == "evaluate":
            q.add_argument("--plan", type=Path, required=True)
    q = sub.add_parser("compare")
    q.add_argument("--results", type=Path, required=True)
    q.add_argument("--out", type=Path, required=True)
    args = p.parse_args()
    return globals()[args.command](args) or 0


if __name__ == "__main__":
    raise SystemExit(main())
