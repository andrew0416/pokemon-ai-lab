"""Parity sweep (FF-parity-harness): runs the Showdown oracle (`engine/oracle/enumerate.cjs`) and
lab-engine (`lab-check`) on every position scenario of a directory and tabulates how often the
engine's outcome distribution matches Showdown's exactly.

Usage: python engine/scripts/parity_sweep.py <scenario-dir> [--mode full] [--fallback extremes]
           [--max-branches 5000] [--fallback-max-branches 60000] [--jobs N] [--out <dir>]
           [--check <lab-check.exe>] [--timeout 1800] [--keep-reports mismatch|all|none]
           [--limit N] [--pattern '*.json']

The scenarios are the ones `lab-parity` writes (`<name>.<policy>.g<game>.s<step>.json`: a game's
earlier decisions as `setupTurns` pinned by `setupStates`), but any oracle scenario works. The
positions of one game run in step order on one worker, and each oracle report's outcome traces
(`--traces`) are written into the next position's `startTrace`/`setupTraces`, so Showdown replays
a pinned setup turn as one branch instead of searching for it; games run in parallel (`--jobs`).

Per position: `enumerate.cjs --mode <mode> --collapse-secondaries --traces` with a branch cap; if
it overflows, again with `--fallback` (default `extremes`, damage rolls min/max on both sides);
then `lab-check <scenario> <report>` in the report's mode. The status of a position is lab-check's
(`match`, `mismatch`, `unsupported`, `engine-error`, `no-position`, `ambiguous`) or the oracle's
failure (`oracle-overflow`, `oracle-setup-failed`, `oracle-timeout`, `oracle-error`).

Output (`--out`, default `<scenario-dir>/../sweep`): `summary.json` (every position's row and the
totals), `summary.md` (tables), `checks/<stem>.check.json` (lab-check verdicts), and oracle
reports under `reports/` (by default only for positions that did not match).

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

HERE = pathlib.Path(__file__).resolve().parent
ENUMERATE = HERE.parent / "oracle" / "enumerate.cjs"
NAME = re.compile(r"^(?P<game>.+)\.s(?P<step>\d+)\.json$")
DESCRIPTION = re.compile(r"decision (?P<step>\d+) = turn (?P<turn>\d+) (?P<kind>\w+)")
GAME = re.compile(r"^(?P<matchup>.+)\.(?P<policy>random|nash)\.g(?P<game>\d+)$")

ORACLE_FAILURES = ("oracle-overflow", "oracle-setup-failed", "oracle-timeout", "oracle-error")
STATUSES = ("match", "mismatch", "unsupported", "engine-error", "no-position", "ambiguous") + ORACLE_FAILURES


def stable(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def read_json(path):
    return json.loads(pathlib.Path(path).read_text(encoding="utf-8-sig"))


def run_oracle(args, scenario, report, mode, cap):
    """(ok, error_status, message, elapsed_s)."""
    cmd = ["node", str(ENUMERATE), str(scenario), "--mode", mode, "--collapse-secondaries", "--traces",
           "--max-branches", str(cap), "--out", str(report)]
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
    for i, path in enumerate(paths):
        scenario = read_json(path)
        row = position_meta(path, scenario)
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
        report = None
        for mode, cap in [(args.mode, args.max_branches)] + ([(args.fallback, args.fallback_max_branches)]
                                                             if args.fallback and args.fallback != args.mode else []):
            candidate = out / "reports" / f"{stem}.{mode}.json"
            ok, failure, message, elapsed = run_oracle(args, path, candidate, mode, cap)
            row.setdefault("oracle_attempts", []).append(
                {"mode": mode, "cap": cap, "ok": ok, "status": failure, "message": message, "seconds": round(elapsed, 1)})
            if ok:
                report = candidate
                break
            if failure != "oracle-overflow":
                break
        if report is None:
            row["status"] = row["oracle_attempts"][-1]["status"]
            row["error"] = row["oracle_attempts"][-1]["message"]
            rows.append(row)
            progress(row)
            continue
        data = read_json(report)
        row.update(mode=data["mode"], branches=data["branches"], oracle_outcomes=data["distinctOutcomes"],
                   oracle_ms=data["elapsedMs"], setup_ms=data.get("setupMs"))
        if data.get("startTrace"):
            start_trace = data["startTrace"]
        for k, t in enumerate(data.get("setupTraces") or []):
            if t:
                setup_traces[k] = t
        # The next position of this game pins this decision's outcome: remember its trace.
        if i + 1 < len(paths):
            nxt = read_json(paths[i + 1])
            pins = nxt.get("setupStates") or []
            step = len(scenario.get("setupTurns") or [])
            if step < len(pins) and pins[step] is not None:
                target = stable(pins[step])
                for o in data["outcomes"]:
                    if o.get("trace") and stable(o["state"]) == target:
                        setup_traces[step] = o["trace"]
                        break
        verdict = run_check(args, path, report, out / "checks" / f"{stem}.check.json")
        row["status"] = verdict.get("status", "engine-error")
        for key in ("engineOutcomes", "onlyEngine", "onlyOracle", "maxSharedDiff", "tv", "variants", "engineMs"):
            if key in verdict:
                row[key] = verdict[key]
        if verdict.get("differences"):
            row["differences"] = verdict["differences"]
        if verdict.get("error"):
            row["error"] = verdict["error"]
        keep = args.keep_reports == "all" or (args.keep_reports == "mismatch" and row["status"] != "match")
        if not keep:
            report.unlink(missing_ok=True)
        rows.append(row)
        progress(row)
    return rows


def table(rows, key):
    groups = {}
    for r in rows:
        groups.setdefault(r.get(key, "?"), []).append(r)
    out = [f"| {key} | positions | match | mismatch | unsupported | oracle failed | other |", "|---|---|---|---|---|---|---|"]
    for k in sorted(groups, key=str):
        g = groups[k]
        c = lambda s: sum(1 for r in g if r["status"] == s)
        oracle = sum(1 for r in g if r["status"] in ORACLE_FAILURES)
        other = len(g) - c("match") - c("mismatch") - c("unsupported") - oracle
        out.append(f"| {k} | {len(g)} | {c('match')} | {c('mismatch')} | {c('unsupported')} | {oracle} | {other} |")
    return "\n".join(out)


def write_summary(args, out, rows, elapsed):
    counts = {s: sum(1 for r in rows if r["status"] == s) for s in STATUSES}
    compared = counts["match"] + counts["mismatch"]
    summary = {
        "scenario_dir": str(args.dir),
        "mode": args.mode,
        "fallback": args.fallback,
        "max_branches": args.max_branches,
        "fallback_max_branches": args.fallback_max_branches,
        "check": args.check,
        "positions": len(rows),
        "counts": counts,
        "compared": compared,
        "mismatch_rate": (counts["mismatch"] / compared) if compared else None,
        "by_mode": {m: sum(1 for r in rows if r.get("mode") == m) for m in ("full", "extremes")},
        "elapsed_s": round(elapsed, 1),
        "rows": sorted(rows, key=lambda r: (r["game_id"], r.get("step", 0))),
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False), encoding="utf-8")
    lines = [
        "# Parity sweep",
        "",
        f"`{args.dir}`: {len(rows)} positions, oracle `--mode {args.mode}` (cap {args.max_branches}) "
        f"then `{args.fallback}` (cap {args.fallback_max_branches}), `--collapse-secondaries`; {elapsed:.0f} s.",
        "",
        "| positions | exact match | mismatch | unsupported | oracle failed | engine error | no position | ambiguous |",
        "|---|---|---|---|---|---|---|---|",
        f"| {len(rows)} | {counts['match']} | {counts['mismatch']} | {counts['unsupported']} | "
        f"{sum(counts[s] for s in ORACLE_FAILURES)} | {counts['engine-error']} | {counts['no-position']} | {counts['ambiguous']} |",
        "",
        "Oracle failures: " + ", ".join(f"{s} {counts[s]}" for s in ORACLE_FAILURES) + ".",
        f"Compared in full mode: {sum(1 for r in rows if r.get('mode') == 'full')}, "
        f"in extremes mode: {sum(1 for r in rows if r.get('mode') == 'extremes')}.",
        "",
        "## By matchup", "", table(rows, "matchup"), "",
        "## By policy", "", table(rows, "policy"), "",
        "## By decision kind", "", table(rows, "kind"), "",
        "## By comparison mode", "", table(rows, "mode"), "",
    ]
    bad = [r for r in rows if r["status"] not in ("match",) and r["status"] not in ORACLE_FAILURES]
    if bad:
        lines += ["## Positions that did not match", "", "| position | status | mode | first differences / error |",
                  "|---|---|---|---|"]
        for r in sorted(bad, key=lambda r: (r["game_id"], r.get("step", 0))):
            what = "; ".join(r.get("differences", [])[:3]) or r.get("error", "")
            what = what.replace("|", "\\|")[:300]
            lines.append(f"| {r['scenario']} | {r['status']} | {r.get('mode', '')} | {what} |")
        lines.append("")
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
    args = ap.parse_args()
    if args.fallback == "none":
        args.fallback = None
    out = args.out or (args.dir.parent / "sweep")
    (out / "reports").mkdir(parents=True, exist_ok=True)
    (out / "checks").mkdir(parents=True, exist_ok=True)

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
