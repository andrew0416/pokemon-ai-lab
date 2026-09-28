"""Parity sweep (FF-parity-harness): runs the Showdown oracle (`engine/oracle/enumerate.cjs`) and
lab-engine (`lab-check`) on every position scenario of a directory and tabulates how often the
engine's outcome distribution matches Showdown's exactly.

Usage: python engine/scripts/parity_sweep.py <scenario-dir> [--strategy auto|full-first|fallback]
           [--mode full] [--fallback extremes] [--max-branches 5000] [--fallback-max-branches 60000]
           [--fixed-rolls 7,0,15] [--fixed-max-branches N] [--[no-]staged-fallback]
           [--staged-extremes-max-branches N] [--full-staged-max-branches N]
           [--mc-samples N] [--mc-seed S] [--turn <lab-turn.exe>]
           [--jobs N] [--out <dir>] [--check <lab-check.exe>] [--timeout 1800]
           [--keep-reports mismatch|all|none] [--limit N] [--pattern '*.json'] [--resume]

The scenarios are the ones `lab-parity` writes (`<name>.<policy>.g<game>.s<step>.json`: a game's
earlier decisions as `setupTurns` pinned by `setupStates`), but any oracle scenario works. The
positions of one game run in step order on one worker, and each oracle report's outcome traces
(`--traces`) are written into the next position's `startTrace`/`setupTraces`, so Showdown replays
a pinned setup turn as one branch instead of searching for it; games run in parallel (`--jobs`).

Per position (`--strategy auto`, the default): `enumerate.cjs --mode extremes --collapse-secondaries
--traces` (damage rolls min/max on both sides) capped at --fallback-max-branches; when its
`fullBranchEstimate` is at most --max-branches, `--mode full` as well (capped at 4x that) and the
exact report is the one compared. `--strategy full-first`: `--mode <mode>` capped at
--max-branches, on overflow `--fallback`. Then `lab-check <scenario> <report>` in the report's
mode. The status of a position is lab-check's
(`match`, `mismatch`, `unsupported`, `engine-error`, `no-position`, `ambiguous`) or the oracle's
failure (`oracle-overflow`, `oracle-setup-failed`, `oracle-timeout`, `oracle-error`).

Heavy turns (JJ-heavy-turn-parity), the overflow fallback: when the last of those modes overflows,
(1) `--mode fixed --roll k` for every k of --fixed-rolls (default 7, 0, 15: the engine's median,
all-minimum and all-maximum roll index), each capped at --fixed-max-branches: every damage roll
takes that one index on both sides and everything else stays enumerated, so each report that fits
is compared exactly by `lab-check` (`RollMode::Fixed(k)`). These runs use `enumerate.cjs --staged`
unless --no-staged-fallback: the staged enumeration merges identical states between actions (the
same distribution in far fewer runs; see enumerate.cjs and oracle/check-staged.cjs). (2) If a
fixed-roll report fit and --staged-extremes-max-branches N is set, `--mode extremes --staged`
capped at N runs as well (both roll ends mixed); if --full-staged-max-branches M is set,
`--mode full --staged` capped at M runs too (every roll: the exact distribution; V9). The position
counts as compared when at least
one of these reports fit; it is a `mismatch` if any of them mismatches and `match` if all of them
match (row fields `mode: fixed`, `rolls`, `compared_by: "fixed 7,0,15 staged + extremes staged"`,
`fixed`: one verdict per report). `--strategy fallback` runs only the fallback (positions known to
overflow in plain extremes). (3) With --mc-samples N, a position on which every fixed roll
overflows is sampled N times on both sides (`enumerate.cjs --mode mc`, `lab-turn --mc N`) and
compared feature by feature (`marginals.cjs --json`): `mc-consistent` or `mc-flagged`
(statistical, not a proof). Every successful oracle run's pinned-setup traces are written into
the scenario, so the next run on the same position replays them directly.

Output (`--out`, default `<scenario-dir>/../sweep`): `summary.json` (every position's row and the
totals), `summary.md` (tables), `rows/<stem>.json` (one row per position; `--resume` reuses them),
`checks/<stem>.check.json` (lab-check verdicts), and oracle reports under `reports/` (by default
only for positions that did not match). Each row keeps its position's features
(`parity_lift.position_features`), and with both matches and mismatches the summary adds a feature
lift table (V14).

Needs LAB_ROOT (the checkout with vendor/pokemon-showdown) when run from a worktree.
"""
import argparse
import concurrent.futures
import fnmatch
import json
import os
import pathlib
import re
import subprocess
import sys
import threading
import time

import parity_lift

HERE = pathlib.Path(__file__).resolve().parent
ENUMERATE = HERE.parent / "oracle" / "enumerate.cjs"
MARGINALS = HERE.parent / "oracle" / "marginals.cjs"
NAME = re.compile(r"^(?P<game>.+)\.s(?P<step>\d+)\.json$")
DESCRIPTION = re.compile(r"decision (?P<step>\d+) = turn (?P<turn>\d+) (?P<kind>\w+)")
GAME = re.compile(r"^(?P<matchup>.+)\.(?P<policy>random|nash)\.g(?P<game>\d+)$")

ORACLE_FAILURES = ("oracle-overflow", "oracle-setup-failed", "oracle-timeout", "oracle-error")
MC_STATUSES = ("mc-consistent", "mc-flagged")
STATUSES = ("match", "mismatch", "unsupported", "engine-error", "no-position", "ambiguous") + MC_STATUSES + ORACLE_FAILURES
# Precedence when several fixed-roll reports of one position are checked: any mismatch makes the
# position a mismatch; it matches only when every fitted report matched.
FIXED_PRECEDENCE = ("mismatch", "engine-error", "no-position", "ambiguous", "unsupported", "match")


def stable(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def read_json(path):
    return json.loads(pathlib.Path(path).read_text(encoding="utf-8-sig"))


def run_oracle(args, scenario, report, mode, cap, roll=None, samples=None, staged=False):
    """(ok, error_status, message, elapsed_s). `roll`: fixed mode's roll index; `samples`: mc mode's;
    `staged`: enumerate.cjs --staged (merge identical states between actions; same distribution)."""
    cmd = ["node", str(ENUMERATE), str(scenario), "--mode", mode, "--collapse-secondaries"]
    if staged:
        cmd.append("--staged")
    if mode == "mc":
        # The turn itself runs on Showdown's own PRNG; --collapse-secondaries only keeps the recorded
        # setup traces (made with it) replayable.
        cmd += ["--samples", str(samples)]
    else:
        cmd += ["--traces", "--max-branches", str(cap)]
    if roll is not None:
        cmd += ["--roll", str(roll)]
    cmd += ["--out", str(report)]
    started = time.time()
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                              timeout=args.timeout)
    except subprocess.TimeoutExpired:
        return False, "oracle-timeout", f"more than {args.timeout} s", time.time() - started
    elapsed = time.time() - started
    if proc.returncode == 0:
        return True, None, proc.stderr.strip().splitlines()[-1] if proc.stderr.strip() else "", elapsed
    err = proc.stderr.strip()
    lines = [l for l in err.splitlines() if l.strip()]
    message = next((l for l in lines if l.startswith("Error:")), lines[-1] if lines else "")
    if "branches; use --mode" in err:
        return False, "oracle-overflow", message, elapsed
    if "pinned state" in err:
        return False, "oracle-setup-failed", message, elapsed
    return False, "oracle-error", message[:400], elapsed


def run_check(args, scenario, report, verdict):
    proc = subprocess.run([args.check, str(scenario), str(report), "--out", str(verdict)],
                          capture_output=True, text=True, encoding="utf-8", errors="replace")
    if proc.returncode == 2 or not proc.stdout.strip():
        return {"status": "engine-error", "error": (proc.stderr or proc.stdout).strip()[:400]}
    return json.loads(proc.stdout)


def run_mc_check(args, scenario, report, engine_report, verdict):
    """Monte Carlo comparison: lab-turn samples the decision as many times as the oracle's mc report
    and `marginals.cjs --json` compares the two per feature against the sampling noise. The verdict's
    status is `mc-consistent` or `mc-flagged` (statistical, not a proof), or `engine-error`."""
    samples = read_json(report)["branches"]
    proc = subprocess.run([args.turn, str(scenario), "--before", str(report), "--mc", str(samples),
                           "--seed", str(args.mc_seed), "--out", str(engine_report)],
                          capture_output=True, text=True, encoding="utf-8", errors="replace",
                          timeout=args.timeout)
    if proc.returncode != 0:
        error = (proc.stderr or proc.stdout).strip()
        status = "unsupported" if "not implemented" in error else "engine-error"
        return {"status": status, "error": f"lab-turn --mc: {error[:400]}"}
    proc = subprocess.run(["node", str(MARGINALS), str(report), str(engine_report), "--json"],
                          capture_output=True, text=True, encoding="utf-8", errors="replace")
    if proc.returncode not in (0, 1) or not proc.stdout.strip():
        return {"status": "engine-error", "error": f"marginals: {(proc.stderr or proc.stdout).strip()[:400]}"}
    result = json.loads(proc.stdout)
    engine = read_json(engine_report)
    out = {
        "status": "mc-flagged" if result["flagged"] else "mc-consistent",
        "mode": "mc",
        "samples": samples,
        "features": result["features"],
        "flaggedFeatures": result["flagged"],
        "worstRatio": result["worstRatio"],
        "engineOutcomes": engine.get("distinctOutcomes"),
        "engineMs": engine.get("elapsedMs"),
        "differences": [f"{r['path']}: TV {r['tv']:.5f} vs noise {r['noise']:.5f}" for r in result["rows"] if r["bad"]][:8],
    }
    pathlib.Path(verdict).write_text(json.dumps(out, indent=1, ensure_ascii=False), encoding="utf-8")
    return out


def position_meta(path, scenario):
    row = {"scenario": path.name}
    m = NAME.match(path.name)
    game = m.group("game") if m else path.stem
    row["game_id"] = game
    g = GAME.match(game)
    if g:
        row.update(matchup=g.group("matchup"), policy=g.group("policy"), game=int(g.group("game")))
    d = DESCRIPTION.search(scenario.get("description", ""))
    if d:
        row.update(step=int(d.group("step")), turn=int(d.group("turn")), kind=d.group("kind"))
    row["setup_turns"] = len(scenario.get("setupTurns") or [])
    return row


def sweep_game(args, paths, out, progress):
    """Runs one game's positions in step order; returns their rows."""
    rows = []
    start_trace = None
    setup_traces = {}
    unreachable = None  # (setup turn, message) the oracle could not reach in this game
    for i, path in enumerate(paths):
        scenario = read_json(path)
        row = position_meta(path, scenario)
        # Later positions replay the same setup turn: they cannot be reached either.
        if unreachable and not (args.resume and (out / "rows" / f"{path.name[:-5]}.json").exists()):
            row["status"] = "oracle-setup-failed"
            row["error"] = f"{unreachable[1]} (not retried: an earlier position of this game failed there)"
            (out / "rows" / f"{path.name[:-5]}.json").write_text(json.dumps(row, ensure_ascii=False),
                                                                encoding="utf-8")
            rows.append(row)
            progress(row)
            continue
        # Traces found so far make the oracle replay the pinned setup turns directly.
        changed = False
        if scenario.get("startState") is not None and start_trace and scenario.get("startTrace") != start_trace:
            scenario["startTrace"] = start_trace
            changed = True
        n = len(scenario.get("setupTurns") or [])
        if n:
            traces = list(scenario.get("setupTraces") or [])
            traces += [None] * (n - len(traces))
            for k in range(n):
                if traces[k] is None and k in setup_traces:
                    traces[k] = setup_traces[k]
                    changed = True
            if changed:
                scenario["setupTraces"] = traces
        if changed:
            path.write_text(json.dumps(scenario, indent=1, ensure_ascii=False), encoding="utf-8")

        stem = path.name[:-len(".json")]
        row_file = out / "rows" / f"{stem}.json"
        if args.resume and row_file.exists():
            rows.append(read_json(row_file))
            if rows[-1]["status"] == "oracle-setup-failed" and not unreachable:
                unreachable = (0, rows[-1].get("error", ""))
            progress(rows[-1])
            continue
        attempts = row.setdefault("oracle_attempts", [])

        def attempt(mode, cap, roll=None, samples=None, staged=False):
            label = (f"fixed{roll}" if mode == "fixed" else mode) + ("-staged" if staged else "")
            candidate = out / "reports" / f"{stem}.{label}.json"
            ok, failure, message, elapsed = run_oracle(args, path, candidate, mode, cap, roll=roll, samples=samples,
                                                       staged=staged)
            entry = {"mode": mode, "cap": cap, "ok": ok, "status": failure, "message": message,
                     "seconds": round(elapsed, 1)}
            if staged:
                entry["staged"] = True
            if roll is not None:
                entry["roll"] = roll
            if samples is not None:
                entry["samples"] = samples
            attempts.append(entry)
            if ok:
                absorb_traces(path, read_json(candidate))
            return candidate if ok else None

        def fixed_chain():
            """Fixed-roll reports (roll, path) that fit, in --fixed-rolls order (JJ-heavy-turn-parity)."""
            fitted = []
            for k in args.fixed_rolls:
                candidate = attempt("fixed", args.fixed_max_branches, roll=k, staged=args.staged_fallback)
                if candidate is not None:
                    fitted.append((k, candidate))
                elif attempts[-1]["status"] != "oracle-overflow":
                    break  # a setup failure or an error: the other rolls fail the same way
            return fitted

        def fallback_chain():
            """Reports (label, roll, path) once the plain modes overflowed: the fixed-roll chain, then (if
            one of those fit and --staged-extremes-max-branches is set) staged extremes as well: both
            roll ends mixed, a superset of the fixed runs' branching, so tried only when fixed fits."""
            fitted = [("fixed", k, r) for k, r in fixed_chain()]
            if fitted and args.staged_fallback and args.staged_extremes_max_branches:
                candidate = attempt("extremes", args.staged_extremes_max_branches, staged=True)
                if candidate is not None:
                    fitted.append(("extremes", None, candidate))
            # V9: every roll, staged (the exact distribution), when --full-staged-max-branches is set.
            if fitted and args.staged_fallback and args.full_staged_max_branches:
                candidate = attempt("full", args.full_staged_max_branches, staged=True)
                if candidate is not None:
                    fitted.append(("full", None, candidate))
            return fitted

        # (label, roll, report path) of every report compared, in order.
        reports = []
        if args.strategy == "auto":
            # Extremes first; full as well when the extremes run says it fits (no wasted full runs);
            # the fixed-roll chain when extremes overflows.
            report = attempt("extremes", args.fallback_max_branches)
            if report is not None:
                estimate = read_json(report).get("fullBranchEstimate")
                if estimate is not None and estimate <= args.max_branches:
                    full = attempt("full", 4 * args.max_branches)
                    if full is not None:
                        report.unlink(missing_ok=True)
                        report = full
                reports.append((read_json(report)["mode"], None, report))
            elif attempts[-1]["status"] == "oracle-overflow":
                reports += fallback_chain()
        elif args.strategy == "fallback":
            # Only the overflow fallback: positions already known to overflow in plain extremes.
            reports += fallback_chain()
        else:
            report = None
            for mode, cap in [(args.mode, args.max_branches)] + ([(args.fallback, args.fallback_max_branches)]
                                                                 if args.fallback and args.fallback != args.mode else []):
                report = attempt(mode, cap)
                if report is not None or attempts[-1]["status"] != "oracle-overflow":
                    break
            if report is not None:
                reports.append((mode, None, report))
            elif attempts[-1]["status"] == "oracle-overflow":
                reports += fallback_chain()
        mc_report = None
        if not reports and args.mc_samples and attempts[-1]["status"] == "oracle-overflow":
            mc_report = attempt("mc", None, samples=args.mc_samples)
        if not reports and mc_report is None:
            row["status"] = attempts[-1]["status"]
            row["error"] = attempts[-1]["message"]
            m = re.search(r"(startState|setup turn (\d+))", row["error"])
            if row["status"] == "oracle-setup-failed" and m:
                unreachable = (int(m.group(2) or 0), row["error"])
            row_file.write_text(json.dumps(row, ensure_ascii=False), encoding="utf-8")
            rows.append(row)
            progress(row)
            continue
        for _, _, report in reports + ([(None, None, mc_report)] if mc_report else []):
            data = read_json(report)
            if data.get("startTrace"):
                start_trace = data["startTrace"]
            for k, t in enumerate(data.get("setupTraces") or []):
                if t:
                    setup_traces[k] = t
        # The next position of this game pins this decision's outcome: remember its trace (a fixed-roll
        # report has it only if every roll on the recorded path was that roll's value).
        if i + 1 < len(paths) and reports:
            nxt = read_json(paths[i + 1])
            pins = nxt.get("setupStates") or []
            step = len(scenario.get("setupTurns") or [])
            if step < len(pins) and pins[step] is not None:
                target = stable(pins[step])
                for _, _, report in reports:
                    trace = next((o["trace"] for o in read_json(report)["outcomes"]
                                  if o.get("trace") and stable(o["state"]) == target), None)
                    if trace:
                        setup_traces[step] = trace
                        break

        if mc_report is not None:
            verdict = run_mc_check(args, path, mc_report, out / "reports" / f"{stem}.engine-mc.json",
                                   out / "checks" / f"{stem}.mc.check.json")
            data = read_json(mc_report)
            row.update(mode="mc", compared_by="mc", samples=data["branches"], oracle_outcomes=data["distinctOutcomes"],
                       oracle_ms=data["elapsedMs"], setup_ms=data.get("setupMs"))
            row["status"] = verdict.get("status", "engine-error")
            for key in ("features", "flaggedFeatures", "worstRatio", "engineOutcomes", "engineMs", "differences", "error"):
                if verdict.get(key) is not None:
                    row[key] = verdict[key]
            keep = args.keep_reports == "all" or (args.keep_reports == "mismatch" and row["status"] != "mc-consistent")
            if not keep:
                mc_report.unlink(missing_ok=True)
                (out / "reports" / f"{stem}.engine-mc.json").unlink(missing_ok=True)
            row_file.write_text(json.dumps(row, ensure_ascii=False), encoding="utf-8")
            rows.append(row)
            progress(row)
            continue

        checked = []
        oracle_reports = []
        for label, roll, report in reports:
            data = read_json(report)
            oracle_reports.append(data)
            suffix = f".fixed{roll}" if roll is not None else ""
            verdict = run_check(args, path, report, out / "checks" / f"{stem}{suffix}.check.json")
            result = {"mode": data["mode"], "branches": data["branches"], "oracle_outcomes": data["distinctOutcomes"],
                      "oracle_ms": data["elapsedMs"], "setup_ms": data.get("setupMs"),
                      "status": verdict.get("status", "engine-error")}
            if data.get("staged"):
                result["staged"] = data["staged"]
            if roll is not None:
                result["roll"] = roll
            for key in ("engineOutcomes", "onlyEngine", "onlyOracle", "maxSharedDiff", "tv", "variants", "engineMs"):
                if key in verdict:
                    result[key] = verdict[key]
            if verdict.get("differences"):
                result["differences"] = verdict["differences"]
            if verdict.get("error"):
                result["error"] = verdict["error"]
            keep = args.keep_reports == "all" or (args.keep_reports == "mismatch" and result["status"] != "match")
            if not keep:
                report.unlink(missing_ok=True)
            checked.append(result)
        if len(checked) == 1 and checked[0].get("roll") is None:
            # full or extremes: one report, the row carries its verdict as before.
            only = dict(checked[0])
            row["compared_by"] = only["mode"] + (" staged" if only.get("staged") else "")
            row.update(only)
        else:
            # The overflow fallback: fixed-roll reports, possibly a staged extremes one as well.
            statuses = [c["status"] for c in checked]
            row["status"] = next((s for s in FIXED_PRECEDENCE if s in statuses), statuses[0])
            first = next((c for c in checked if c["status"] == row["status"]), checked[0])
            fixed = [c for c in checked if c.get("roll") is not None]
            parts = []
            if fixed:
                parts.append("fixed " + ",".join(str(c["roll"]) for c in fixed)
                             + (" staged" if any(c.get("staged") for c in fixed) else ""))
            parts += [c["mode"] + (" staged" if c.get("staged") else "") for c in checked if c.get("roll") is None]
            row.update(mode="fixed" if fixed else checked[0]["mode"], rolls=[c["roll"] for c in fixed],
                       compared_by=" + ".join(parts),
                       branches=checked[0]["branches"], oracle_outcomes=checked[0]["oracle_outcomes"],
                       oracle_ms=sum(c["oracle_ms"] for c in checked), setup_ms=checked[0].get("setup_ms"),
                       fixed=checked)
            for key in ("engineOutcomes", "onlyEngine", "onlyOracle", "maxSharedDiff", "tv", "variants",
                        "differences", "error"):
                if key in first:
                    row[key] = first[key]
            row["engineMs"] = sum(c.get("engineMs") or 0 for c in checked)
        # V14: the position's features (the reports may be deleted), for the summary's lift table.
        row["features"] = parity_lift.position_features(scenario, oracle_reports)
        row_file.write_text(json.dumps(row, ensure_ascii=False), encoding="utf-8")
        rows.append(row)
        progress(row)
    return rows


def absorb_traces(path, report):
    """Writes the report's start and setup traces into the scenario where it has none, so the next
    oracle run on this position (another roll mode) replays the pinned setup directly."""
    scenario = read_json(path)
    changed = False
    if scenario.get("startState") is not None and report.get("startTrace") and not scenario.get("startTrace"):
        scenario["startTrace"] = report["startTrace"]
        changed = True
    found = report.get("setupTraces") or []
    if found:
        traces = list(scenario.get("setupTraces") or [])
        traces += [None] * (len(found) - len(traces))
        for k, t in enumerate(found):
            if t and traces[k] is None:
                traces[k] = t
                changed = True
        if changed:
            scenario["setupTraces"] = traces
    if changed:
        path.write_text(json.dumps(scenario, indent=1, ensure_ascii=False), encoding="utf-8")




def table(rows, key):
    groups = {}
    for r in rows:
        groups.setdefault(r.get(key, "?"), []).append(r)
    out = [f"| {key} | positions | match | mismatch | mc consistent | mc flagged | unsupported | oracle failed | other |",
           "|---|---|---|---|---|---|---|---|---|"]
    for k in sorted(groups, key=str):
        g = groups[k]
        c = lambda s: sum(1 for r in g if r["status"] == s)
        oracle = sum(1 for r in g if r["status"] in ORACLE_FAILURES)
        other = len(g) - c("match") - c("mismatch") - c("mc-consistent") - c("mc-flagged") - c("unsupported") - oracle
        out.append(f"| {k} | {len(g)} | {c('match')} | {c('mismatch')} | {c('mc-consistent')} | {c('mc-flagged')} | "
                   f"{c('unsupported')} | {oracle} | {other} |")
    return "\n".join(out)


def fixed_roll_cells(args, row):
    """Per --fixed-rolls roll (and staged extremes when enabled): the check status of its report,
    `overflow`, or `-` (not run)."""
    done = {c.get("roll"): c["status"] for c in row.get("fixed", []) if c["mode"] != "full"}
    tried = {a["roll"] if a["mode"] == "fixed" else None: a["status"] for a in row.get("oracle_attempts", [])
             if a["mode"] == "fixed" or (a.get("staged") and a["mode"] != "full")}
    cells = []
    for k in args.fixed_rolls + ([None] if args.staged_extremes_max_branches else []):
        if k in done:
            cells.append(done[k])
        elif k in tried:
            cells.append((tried[k] or "?").replace("oracle-", ""))
        else:
            cells.append("-")
    return cells


def write_summary(args, out, rows, elapsed):
    counts = {s: sum(1 for r in rows if r["status"] == s) for s in STATUSES}
    compared = counts["match"] + counts["mismatch"]
    fixed_rows = [r for r in rows if r.get("mode") == "fixed"]
    by_roll = {k: sum(1 for r in fixed_rows if k in r.get("rolls", [])) for k in args.fixed_rolls}
    summary = {
        "scenario_dir": str(args.dir),
        "strategy": args.strategy,
        "mode": args.mode,
        "fallback": args.fallback,
        "max_branches": args.max_branches,
        "fallback_max_branches": args.fallback_max_branches,
        "fixed_rolls": args.fixed_rolls,
        "fixed_max_branches": args.fixed_max_branches,
        "mc_samples": args.mc_samples,
        "check": args.check,
        "positions": len(rows),
        "counts": counts,
        "compared": compared,
        "mismatch_rate": (counts["mismatch"] / compared) if compared else None,
        "by_mode": {m: sum(1 for r in rows if r.get("mode") == m) for m in ("full", "extremes", "fixed", "mc")},
        "fixed_by_roll": by_roll,
        "by_comparison": {k: sum(1 for r in rows if r.get("compared_by") == k)
                          for k in sorted({r["compared_by"] for r in rows if r.get("compared_by")})},
        "elapsed_s": round(elapsed, 1),
        "rows": sorted(rows, key=lambda r: (r["game_id"], r.get("step", 0))),
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False), encoding="utf-8")
    if args.strategy == "auto":
        how = (f"(extremes capped at {args.fallback_max_branches} branches, full as well when its estimate is at most "
               f"{args.max_branches}; when extremes overflows, fixed rolls {args.fixed_rolls} capped at "
               f"{args.fixed_max_branches})")
    elif args.strategy == "fallback":
        how = (f"(the overflow fallback only: fixed rolls {args.fixed_rolls} capped at {args.fixed_max_branches}"
               + (" runs, staged" if args.staged_fallback else " branches"))
        if args.staged_fallback and args.staged_extremes_max_branches:
            how += f"; then staged extremes capped at {args.staged_extremes_max_branches} runs"
        how += ")"
    else:
        how = (f"(`--mode {args.mode}` capped at {args.max_branches}, then `{args.fallback}` capped at "
               f"{args.fallback_max_branches}, then fixed rolls {args.fixed_rolls} capped at {args.fixed_max_branches})")
    if args.mc_samples:
        how += f"; Monte Carlo with {args.mc_samples} samples on both sides where every fixed roll overflows"
    lines = [
        "# Parity sweep",
        "",
        f"`{args.dir}`: {len(rows)} positions, strategy `{args.strategy}` {how}, `--collapse-secondaries`; "
        f"{elapsed:.0f} s.",
        "",
        "| positions | exact match | mismatch | mc consistent | mc flagged | unsupported | oracle failed | engine error | "
        "no position | ambiguous |",
        "|---|---|---|---|---|---|---|---|---|---|",
        f"| {len(rows)} | {counts['match']} | {counts['mismatch']} | {counts['mc-consistent']} | {counts['mc-flagged']} | "
        f"{counts['unsupported']} | {sum(counts[s] for s in ORACLE_FAILURES)} | {counts['engine-error']} | "
        f"{counts['no-position']} | {counts['ambiguous']} |",
        "",
        "Oracle failures: " + ", ".join(f"{s} {counts[s]}" for s in ORACLE_FAILURES) + ".",
        f"Compared exactly in full mode: {sum(1 for r in rows if r.get('mode') == 'full')}, "
        f"in extremes mode: {sum(1 for r in rows if r.get('mode') == 'extremes')}, "
        f"with fixed damage rolls: {len(fixed_rows)} (a report fit for roll "
        + ", ".join(f"{k}: {n}" for k, n in by_roll.items()) + ").",
        "A fixed-roll comparison is exact (every other random decision enumerated on both sides) but covers only "
        "the turns in which every damage roll takes that one value.",
        f"Staged extremes compared as well on {sum(1 for r in fixed_rows if 'extremes' in r.get('compared_by', ''))} "
        "of the fixed-roll positions.",
    ]
    if args.full_staged_max_branches:
        lines.append(f"Staged full (every roll, exact) compared as well on "
                     f"{sum(1 for r in fixed_rows if 'full' in r.get('compared_by', ''))} of the fixed-roll positions "
                     f"(cap {args.full_staged_max_branches} runs).")
    if args.mc_samples:
        lines.append(f"Monte Carlo (statistical, not a proof: per-feature TV distance within 4x the two-sample noise): "
                     f"consistent {counts['mc-consistent']}, flagged {counts['mc-flagged']}.")
    lines += [
        "",
        "## By matchup", "", table(rows, "matchup"), "",
        "## By policy", "", table(rows, "policy"), "",
        "## By decision kind", "", table(rows, "kind"), "",
        "## By comparison mode", "", table(rows, "mode"), "",
        "## By comparison (the fixed rolls whose report fit)", "", table(rows, "compared_by"), "",
    ]
    if fixed_rows:
        lines += ["## Fixed-roll positions", "",
                  "| position | " + " | ".join(f"roll {k}" for k in args.fixed_rolls)
                  + (" | extremes staged" if args.staged_extremes_max_branches else "")
                  + " | runs (first roll) | status |",
                  "|---|" + "---|" * (len(args.fixed_rolls) + bool(args.staged_extremes_max_branches)) + "---|---|"]
        for r in sorted(fixed_rows, key=lambda r: (r["game_id"], r.get("step", 0))):
            lines.append(f"| {r['scenario']} | " + " | ".join(fixed_roll_cells(args, r))
                         + f" | {r.get('branches', '')} | {r['status']} |")
        lines.append("")
    bad = [r for r in rows if r["status"] not in ("match", "mc-consistent") and r["status"] not in ORACLE_FAILURES]
    if bad:
        lines += ["## Positions that did not match", "", "| position | status | compared by | first differences / error |",
                  "|---|---|---|---|"]
        for r in sorted(bad, key=lambda r: (r["game_id"], r.get("step", 0))):
            what = "; ".join(r.get("differences", [])[:3]) or r.get("error", "")
            what = what.replace("|", "\\|")[:300]
            lines.append(f"| {r['scenario']} | {r['status']} | {r.get('compared_by', r.get('mode', ''))} | {what} |")
        lines.append("")
    if counts["match"] and counts["mismatch"]:
        result = parity_lift.lift(rows)
        summary["lift"] = result
        (out / "summary.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False), encoding="utf-8")
        lines += parity_lift.markdown(result)
    failed = [r for r in rows if r["status"] in ORACLE_FAILURES]
    if failed:
        lines += ["## Oracle failures", "", "| position | status | message |", "|---|---|---|"]
        for r in sorted(failed, key=lambda r: (r["game_id"], r.get("step", 0))):
            lines.append(f"| {r['scenario']} | {r['status']} | {r.get('error', '')[:200].replace('|', '/')} |")
        lines.append("")
    (out / "summary.md").write_text("\n".join(lines), encoding="utf-8")
    return summary


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dir", type=pathlib.Path)
    ap.add_argument("--mode", default="full", choices=["full", "extremes"])
    ap.add_argument("--fallback", default="extremes", choices=["extremes", "full", "none"])
    ap.add_argument("--max-branches", type=int, default=5000)
    ap.add_argument("--fallback-max-branches", type=int, default=60000)
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) - 1))
    ap.add_argument("--out", type=pathlib.Path)
    ap.add_argument("--check", default="D:/cargo-target/release/lab-check.exe")
    ap.add_argument("--timeout", type=int, default=1800)
    ap.add_argument("--keep-reports", default="mismatch", choices=["mismatch", "all", "none"])
    ap.add_argument("--limit", type=int, help="only the first N games")
    ap.add_argument("--pattern", default="*.json")
    ap.add_argument("--strategy", default="auto", choices=["auto", "full-first", "fallback"])
    ap.add_argument("--fixed-rolls", default="7,0,15",
                    help="roll indices (0 = 85%%, 15 = 100%%) of the fixed-roll fallback, in order; '' disables it")
    ap.add_argument("--fixed-max-branches", type=int, help="cap of each fixed-roll run (default: --fallback-max-branches)")
    ap.add_argument("--staged-fallback", default=True, action=argparse.BooleanOptionalAction,
                    help="the overflow fallback runs enumerate.cjs --staged (default: yes)")
    ap.add_argument("--staged-extremes-max-branches", type=int, default=0,
                    help="cap of a staged extremes run before the fixed-roll chain (0: skip it)")
    ap.add_argument("--full-staged-max-branches", type=int, default=0,
                    help="cap of a staged full run (every roll, exact) after the fixed-roll chain (0: skip it)")
    ap.add_argument("--mc-samples", type=int, default=0,
                    help="Monte Carlo samples on both sides when every fixed roll overflows (0: off)")
    ap.add_argument("--mc-seed", type=int, default=1, help="lab-turn --seed of the engine's samples")
    ap.add_argument("--turn", help="lab-turn executable for --mc-samples (default: next to --check)")
    ap.add_argument("--resume", action="store_true", help="reuse rows/<stem>.json of positions already swept")
    args = ap.parse_args()
    if args.fallback == "none":
        args.fallback = None
    args.fixed_rolls = [int(k) for k in args.fixed_rolls.split(",") if k.strip()]
    if any(not 0 <= k <= 15 for k in args.fixed_rolls):
        ap.error("--fixed-rolls: indices 0..15")
    if args.strategy == "fallback" and not args.fixed_rolls and not args.staged_extremes_max_branches:
        ap.error("--strategy fallback needs --fixed-rolls or --staged-extremes-max-branches")
    if args.fixed_max_branches is None:
        args.fixed_max_branches = args.fallback_max_branches
    if args.turn is None:
        check = pathlib.Path(args.check)
        args.turn = str(check.with_name("lab-turn" + check.suffix))
    out = args.out or (args.dir.parent / "sweep")
    for sub in ("reports", "checks", "rows"):
        (out / sub).mkdir(parents=True, exist_ok=True)

    games = {}
    for path in sorted(args.dir.iterdir()):
        if not path.is_file() or path.name.endswith(".games.json") or not fnmatch.fnmatch(path.name, args.pattern):
            continue
        m = NAME.match(path.name)
        games.setdefault(m.group("game") if m else path.stem, []).append(path)
    for paths in games.values():
        paths.sort(key=lambda p: int(NAME.match(p.name).group("step")) if NAME.match(p.name) else 0)
    order = sorted(games)
    if args.limit:
        order = order[:args.limit]
    total = sum(len(games[g]) for g in order)
    print(f"{total} positions in {len(order)} game(s), {args.jobs} job(s)", flush=True)

    lock = threading.Lock()
    done = []
    started = time.time()

    def progress(row):
        with lock:
            done.append(row)
            what = "; ".join(row.get("differences", [])[:1]) or row.get("error", "")
            print(f"[{len(done)}/{total}] {row['scenario']}: {row['status']} {row.get('mode', '')} "
                  f"{row.get('branches', '')} {what[:160]}", flush=True)

    rows = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for result in pool.map(lambda g: sweep_game(args, games[g], out, progress), order):
            rows.extend(result)
    summary = write_summary(args, out, rows, time.time() - started)
    print(json.dumps({k: summary[k] for k in ("positions", "counts", "compared", "mismatch_rate", "by_mode")}))
    print(f"written {out / 'summary.json'} and {out / 'summary.md'}")


if __name__ == "__main__":
    sys.exit(main())
