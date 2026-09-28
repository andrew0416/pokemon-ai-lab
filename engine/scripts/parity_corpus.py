"""Parity regression corpus (V8-parity-regression-corpus): the played-game positions the parity
sweeps compared, each kept with the Showdown oracle report(s) it was compared against, so an
engine change is re-checked against all of them with `lab-check` alone (no Showdown run).

Usage:
  python engine/scripts/parity_corpus.py build <corpus-dir> [--jobs N] [--timeout S] [--resume]
                                               [--only GLOB] [--limit N] [--plan-only]
  python engine/scripts/parity_corpus.py check <corpus-dir> [--jobs N] [--check <lab-check.exe>]
                                               [--out DIR] [--only GLOB] [--timeout S] [--resume]
  python engine/scripts/parity_corpus.py stats <corpus-dir>
  python engine/scripts/parity_corpus.py extract <corpus-dir> <id> [--out DIR]

`build` reads `<corpus-dir>/sources.json`:

  {"sources": [{"set": "ff", "positions": "<dir of lab-parity scenarios>",
                "rows": "<parity_sweep.py rows/ dir>", "engine": "<commit the sweep checked>",
                "run": "<free text>"}, ...]}

(paths relative to the corpus directory). Every row of a source is one position `<set>/<stem>`; a
later source with the same set and stem replaces the earlier entry (the heavy-turn sweep's rows
replace the overflow rows of the sweep it re-ran). A position is re-enumerated by the oracle in
exactly the way its row says it was compared: `--mode full` (cap 20,000 unless the row gives
one), else `--mode extremes` (cap 60,000), else the heavy-turn fallback's reports: `--mode fixed
--roll k --staged` for each fixed roll that was compared (7, 0, 15; cap 200,000 runs) and `--mode
extremes --staged` (cap 100,000 runs) where that was compared too; always `--collapse-secondaries
--traces`. Rows whose oracle failed (overflow, setup failure) are listed as excluded with the
reason. The teams are inlined into the stored scenario (the team files' paths and SHA-256 are
kept in the index), so the corpus does not depend on `teams/` staying unchanged. Positions of one
game run in step order on one worker and every report's setup traces are written into the stored
scenario and passed to the game's later positions, as `parity_sweep.py` does.

Layout:
  corpus.json                                   index: every position (included or excluded),
                                                its source row, plan, reports, sizes
  positions/<set>/<stem>.scenario.json.gz       the scenario (teams inlined, traces written in)
  positions/<set>/<stem>.<label>.report.json.gz its oracle report(s): label full, extremes,
                                                fixed<k>-staged, extremes-staged
  build/rows/<set>/<stem>.json                  the build log of a position (`--resume` state)

`check` runs every stored report through one `lab-check --batch - --jobs <jobs>` process that reads
the gzipped files directly (board P5; `--per-report` keeps the old way: `lab-check <scenario>
<report>` per report, decompressed to a temporary directory, with `--timeout`) and writes `<out>/summary.json`, `<out>/summary.md`, `<out>/rows/` and the verdicts of
positions that did not match (`<out>/verdicts/`); `--out` defaults to `<corpus-dir>/check`. A
position with several reports is `mismatch` if any report mismatches and `match` only if all
match (parity_sweep.FIXED_PRECEDENCE). The exit code is 0 only if every checked position
matches and (unless --only or --limit restricts the run) no position of the corpus is left
without reports by a pending or failed build. Excluded positions (the sweeps' oracle failures)
are listed in the summary, not counted as failures. With both matches and mismatches the summary
adds a feature lift table (V14, `parity_lift.py`).

`build` needs LAB_ROOT (the checkout with vendor/pokemon-showdown) when run from a worktree.
"""
import argparse
import concurrent.futures
import datetime
import fnmatch
import gzip
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import threading
import time

import parity_sweep
from parity_sweep import FIXED_PRECEDENCE, ORACLE_FAILURES, read_json, stable

HERE = pathlib.Path(__file__).resolve().parent
ENUMERATE = HERE.parent / "oracle" / "enumerate.cjs"
DEFAULT_CAPS = {"full": 20000, "extremes": 60000, "fixed": 200000, "extremes-staged": 100000}
CHECK_STATUSES = ("match", "mismatch", "unsupported", "engine-error", "no-position", "ambiguous", "check-timeout")


def write_json(path, value, indent=1):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(value, indent=indent, ensure_ascii=False), encoding="utf-8")
    os.replace(tmp, path)


def gz_write(path, data: bytes):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_bytes(gzip.compress(data, compresslevel=9, mtime=0))
    os.replace(tmp, path)


def gz_read(path) -> bytes:
    return gzip.decompress(pathlib.Path(path).read_bytes())


def sha256(data: bytes):
    return hashlib.sha256(data).hexdigest()


def git(*args):
    try:
        return subprocess.run(["git", *args], cwd=HERE, capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


# ---------------------------------------------------------------------------------------------
# plan


def report_label(mode, roll, staged):
    return (f"fixed{roll}" if mode == "fixed" else mode) + ("-staged" if staged else "")


def attempt_cap(row, mode, roll, staged):
    """The cap the sweep used for the report that was compared (from its oracle attempt)."""
    for a in row.get("oracle_attempts", []):
        if a.get("ok") and a["mode"] == mode and a.get("roll") == roll and bool(a.get("staged")) == staged:
            return a.get("cap")
    return None


def plan_of(row):
    """(plan, excluded reason): the reports to regenerate, exactly as the sweep compared the
    position, or why the position is not in the corpus."""
    status = row.get("status")
    if status in ORACLE_FAILURES or status in parity_sweep.MC_STATUSES or not row.get("mode"):
        return [], f"{status}: {row.get('error', '')}".strip()
    mode = row["mode"]
    plan = []
    if mode in ("full", "extremes"):
        staged = bool(row.get("staged"))
        cap = attempt_cap(row, mode, None, staged) or DEFAULT_CAPS["extremes-staged" if staged else mode]
        plan.append({"label": report_label(mode, None, staged), "mode": mode, "roll": None, "staged": staged,
                     "cap": cap, "expect": {"branches": row.get("branches"), "outcomes": row.get("oracle_outcomes")}})
    elif mode == "fixed":
        for c in row.get("fixed", []):
            roll = c.get("roll")
            staged = bool(c.get("staged"))
            m = "fixed" if roll is not None else c["mode"]
            cap = attempt_cap(row, m, roll, staged) or DEFAULT_CAPS["fixed" if roll is not None else "extremes-staged"]
            plan.append({"label": report_label(m, roll, staged), "mode": m, "roll": roll, "staged": staged,
                         "cap": cap, "expect": {"branches": c.get("branches"), "outcomes": c.get("oracle_outcomes")}})
    else:
        return [], f"compared in mode {mode!r}, which the corpus does not keep"
    return plan, None


def collect(corpus, sources):
    """Entries (id -> entry) from the sources, later sources replacing earlier ones."""
    entries = {}
    for src in sources:
        pos_dir = (corpus / src["positions"]).resolve()
        rows_dir = (corpus / src["rows"]).resolve()
        for row_file in sorted(rows_dir.glob("*.json")):
            row = read_json(row_file)
            stem = row_file.name[:-len(".json")]
            scenario = pos_dir / f"{stem}.json"
            if not scenario.exists():
                raise SystemExit(f"{row_file}: no scenario {scenario}")
            plan, excluded = plan_of(row)
            # A source can discard positions for good (V12: a pin the old engine recorded that
            # Showdown never reaches); the reason is kept like an oracle failure's.
            if stem in src.get("discarded", {}):
                plan, excluded = [], f"discarded: {src['discarded'][stem]}"
            entry_id = f"{src['set']}/{stem}"
            m = parity_sweep.NAME.match(f"{stem}.json")
            entries[entry_id] = {
                "id": entry_id,
                "set": src["set"],
                "stem": stem,
                "game": row.get("game_id") or (m.group("game") if m else stem),
                "step": row.get("step", int(m.group("step")) if m else 0),
                "matchup": row.get("matchup"),
                "policy": row.get("policy"),
                "turn": row.get("turn"),
                "kind": row.get("kind"),
                "setup_turns": row.get("setup_turns"),
                "source": {
                    "scenario": os.path.relpath(scenario, corpus).replace(os.sep, "/"),
                    "row": os.path.relpath(row_file, corpus).replace(os.sep, "/"),
                    "engine": src.get("engine"),
                    "run": src.get("run"),
                    "status": row.get("status"),
                    "compared_by": row.get("compared_by") or row.get("mode"),
                },
                "plan": plan,
                "excluded": excluded,
            }
    return entries


# ---------------------------------------------------------------------------------------------
# build


def inline_teams(source_path, scenario):
    """The scenario with its team paths replaced by the team arrays; (scenario, team provenance)."""
    teams = {}
    for side in ("p1", "p2"):
        spec = scenario[side].get("team")
        if isinstance(spec, str):
            team_path = (source_path.parent / spec).resolve()
            data = team_path.read_bytes()
            scenario[side]["team"] = json.loads(data.decode("utf-8-sig"))
            teams[side] = {"path": spec, "resolved": str(team_path).replace(os.sep, "/"), "sha256": sha256(data)}
        else:
            teams[side] = {"path": None, "inline": True}
    return scenario, teams


def remember_traces(memory, corpus, scenario, reports, following):
    """Records the traces a position's reports used (start, setup turns) and the trace of the
    outcome the game's next position pins as its next setup turn (`following`: that entry)."""
    for data in reports:
        if data.get("startTrace"):
            memory["start"] = data["startTrace"]
        for k, t in enumerate(data.get("setupTraces") or []):
            if t:
                memory["setup"][k] = t
    if following is None:
        return
    pins = read_json(corpus / following["source"]["scenario"]).get("setupStates") or []
    step = len(scenario.get("setupTurns") or [])
    if step < len(pins) and pins[step] is not None:
        target = stable(pins[step])
        for data in reports:
            trace = next((o["trace"] for o in data["outcomes"] if o.get("trace") and stable(o["state"]) == target),
                         None)
            if trace:
                memory["setup"][step] = trace
                return


def build_game(args, corpus, entries, progress):
    """Builds one game's positions in step order. Traces found by a position's reports are given
    to the game's later positions (as parity_sweep.sweep_game does)."""
    memory = {"start": None, "setup": {}}
    for i, entry in enumerate(entries):
        following = entries[i + 1] if i + 1 < len(entries) else None
        row_file = corpus / "build" / "rows" / entry["set"] / f"{entry['stem']}.json"
        if args.resume and row_file.exists():
            row = read_json(row_file)
            if row.get("status") == "ok" and all((corpus / r["file"]).exists() for r in row["reports"]) \
                    and (corpus / row["scenario"]).exists():
                stored = json.loads(gz_read(corpus / row["scenario"]))
                remember_traces(memory, corpus, stored, [json.loads(gz_read(corpus / r["file"]))
                                                         for r in row["reports"]], following)
                progress(entry, row)
                continue
        source = corpus / entry["source"]["scenario"]
        scenario, teams = inline_teams(source, read_json(source))
        # Traces found so far in this game make the oracle replay the pinned setup turns directly.
        if scenario.get("startState") is not None and memory["start"] and not scenario.get("startTrace"):
            scenario["startTrace"] = memory["start"]
        n = len(scenario.get("setupTurns") or [])
        if n:
            traces = list(scenario.get("setupTraces") or [])
            traces += [None] * (n - len(traces))
            for k in range(n):
                if traces[k] is None and k in memory["setup"]:
                    traces[k] = memory["setup"][k]
            scenario["setupTraces"] = traces
        work = corpus / "build" / "work" / entry["set"]
        work.mkdir(parents=True, exist_ok=True)
        path = work / f"{entry['stem']}.json"
        path.write_text(json.dumps(scenario, indent=1, ensure_ascii=False), encoding="utf-8")

        row = {"id": entry["id"], "teams": teams, "attempts": [], "reports": [],
               "started": datetime.datetime.now().isoformat(timespec="seconds")}
        failed = None
        made = []
        for item in entry["plan"]:
            report = work / f"{entry['stem']}.{item['label']}.json"
            ok, failure, message, elapsed = parity_sweep.run_oracle(
                args, path, report, item["mode"], item["cap"], roll=item["roll"], staged=item["staged"])
            row["attempts"].append({"label": item["label"], "cap": item["cap"], "ok": ok, "status": failure,
                                    "message": message, "seconds": round(elapsed, 1)})
            if not ok:
                failed = f"{item['label']}: {failure}: {message}"
                break
            parity_sweep.absorb_traces(path, read_json(report))
            made.append((item, report))
        if failed:
            row["status"] = "failed"
            row["error"] = failed
            write_json(row_file, row)
            progress(entry, row)
            continue

        reports = []
        datas = [read_json(report) for _, report in made]
        remember_traces(memory, corpus, scenario, datas, following)
        for (item, report), data in zip(made, datas):
            raw = report.read_bytes()
            target = corpus / "positions" / entry["set"] / f"{entry['stem']}.{item['label']}.report.json.gz"
            gz_write(target, raw)
            meta = {"label": item["label"], "mode": data["mode"], "roll": data.get("roll"),
                    "staged": bool(data.get("staged")), "cap": item["cap"],
                    "file": os.path.relpath(target, corpus).replace(os.sep, "/"),
                    "branches": data.get("branches"), "outcomes": data.get("distinctOutcomes"),
                    "oracle_ms": data.get("elapsedMs"), "setup_ms": data.get("setupMs"),
                    "showdown": data.get("showdownCommit"), "bytes": len(raw), "gz_bytes": target.stat().st_size}
            expect = item.get("expect") or {}
            if expect.get("outcomes") is not None and expect["outcomes"] != meta["outcomes"]:
                meta["sweep_outcomes"] = expect["outcomes"]
            if expect.get("branches") is not None and expect["branches"] != meta["branches"]:
                meta["sweep_branches"] = expect["branches"]
            reports.append(meta)
        raw = path.read_bytes()
        scenario_target = corpus / "positions" / entry["set"] / f"{entry['stem']}.scenario.json.gz"
        gz_write(scenario_target, raw)
        for _, report in made:
            report.unlink(missing_ok=True)
        path.unlink(missing_ok=True)
        row.update(status="ok", scenario=os.path.relpath(scenario_target, corpus).replace(os.sep, "/"),
                   scenario_bytes=len(raw), scenario_gz_bytes=scenario_target.stat().st_size, reports=reports,
                   finished=datetime.datetime.now().isoformat(timespec="seconds"))
        write_json(row_file, row)
        progress(entry, row)


def merge_build_row(corpus, entry):
    """The entry with its build row (if any) merged in: build status, stored files, sizes."""
    e = dict(entry)
    row_file = corpus / "build" / "rows" / e["set"] / f"{e['stem']}.json"
    if e["excluded"]:
        e["build"] = "excluded"
    elif row_file.exists():
        row = read_json(row_file)
        e["build"] = row["status"]
        e["teams"] = row.get("teams")
        if row["status"] == "ok":
            e["scenario"] = row["scenario"]
            e["scenario_bytes"] = row["scenario_bytes"]
            e["scenario_gz_bytes"] = row["scenario_gz_bytes"]
            e["reports"] = row["reports"]
            e["oracle_seconds"] = round(sum(a["seconds"] for a in row["attempts"]), 1)
        else:
            e["build_error"] = row.get("error")
    else:
        e["build"] = "pending"
    return e


def write_index(corpus, entries, extra):
    """corpus.json: every entry with its build row merged in."""
    out = [merge_build_row(corpus, entry)
           for entry in sorted(entries.values(), key=lambda e: (e["set"], e["game"], e["step"]))]
    index = dict(extra)
    index["positions"] = out
    index["totals"] = totals(out)
    write_json(corpus / "corpus.json", index)
    return index


def totals(entries):
    t = {"entries": len(entries), "included": 0, "excluded": 0, "pending": 0, "failed": 0, "reports": 0,
         "by_set": {}, "by_compared": {}, "bytes": 0, "gz_bytes": 0, "oracle_seconds": 0.0,
         "differs_from_sweep": 0}
    for e in entries:
        s = t["by_set"].setdefault(e["set"], {"included": 0, "excluded": 0, "pending": 0, "failed": 0})
        state = {"ok": "included", "excluded": "excluded", "failed": "failed"}.get(e["build"], "pending")
        t[state] += 1
        s[state] += 1
        if e["build"] == "ok":
            key = " + ".join(r["label"] for r in e["reports"])
            t["by_compared"][key] = t["by_compared"].get(key, 0) + 1
            t["reports"] += len(e["reports"])
            t["bytes"] += e["scenario_bytes"] + sum(r["bytes"] for r in e["reports"])
            t["gz_bytes"] += e["scenario_gz_bytes"] + sum(r["gz_bytes"] for r in e["reports"])
            t["oracle_seconds"] += e.get("oracle_seconds", 0)
            if any("sweep_outcomes" in r or "sweep_branches" in r for r in e["reports"]):
                t["differs_from_sweep"] += 1
    t["oracle_seconds"] = round(t["oracle_seconds"], 1)
    return t


def cmd_build(args):
    corpus = args.corpus.resolve()
    sources = read_json(corpus / "sources.json")["sources"]
    if not os.environ.get("LAB_ROOT") and not (HERE.parents[1] / "vendor" / "pokemon-showdown").exists():
        raise SystemExit("set LAB_ROOT to the checkout with vendor/pokemon-showdown")
    entries = collect(corpus, sources)
    extra = {
        "schema": 1,
        "built_by": "engine/scripts/parity_corpus.py",
        "oracle": {"enumerate": str(ENUMERATE).replace(os.sep, "/"), "enumerate_sha256": sha256(ENUMERATE.read_bytes()),
                   "commit": git("rev-parse", "HEAD"),
                   "oracle_dir_clean": git("status", "--porcelain", "--", str(HERE.parent / "oracle")) == "",
                   "lab_root": os.environ.get("LAB_ROOT")},
        "sources": sources,
    }
    index = write_index(corpus, entries, extra)
    t = index["totals"]
    print(f"{t['entries']} positions: {sum(1 for e in entries.values() if not e['excluded'])} to build, "
          f"{t['excluded']} excluded", flush=True)
    if args.plan_only:
        return 0
    games = {}
    for e in entries.values():
        if e["excluded"] or (args.only and not fnmatch.fnmatch(e["id"], args.only)):
            continue
        games.setdefault((e["set"], e["game"]), []).append(e)
    for g in games.values():
        g.sort(key=lambda e: e["step"])
    order = sorted(games)
    if args.limit:
        order = order[:args.limit]
    total = sum(len(games[g]) for g in order)
    print(f"{total} positions in {len(order)} game(s), {args.jobs} job(s)", flush=True)
    lock = threading.Lock()
    done = []
    started = time.time()

    def progress(entry, row):
        with lock:
            done.append(row)
            secs = sum(a["seconds"] for a in row.get("attempts", []))
            what = row.get("error", "") if row["status"] != "ok" else " ".join(
                f"{r['label']}:{r['outcomes']}" for r in row["reports"])
            print(f"[{len(done)}/{total}] {entry['id']}: {row['status']} {secs:.0f}s {what[:200]} "
                  f"({time.time() - started:.0f}s)", flush=True)

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for _ in pool.map(lambda g: build_game(args, corpus, games[g], progress), order):
            pass
    index = write_index(corpus, entries, extra)
    t = index["totals"]
    print(json.dumps({k: t[k] for k in ("entries", "included", "excluded", "pending", "failed", "reports",
                                        "bytes", "gz_bytes", "oracle_seconds", "differs_from_sweep")}))
    print(f"written {corpus / 'corpus.json'} ({time.time() - started:.0f} s)")
    return 1 if t["failed"] else 0


# ---------------------------------------------------------------------------------------------
# check


def check_entry(args, corpus, entry, out):
    row_file = out / "rows" / entry["set"] / f"{entry['stem']}.json"
    if args.resume and row_file.exists():
        return read_json(row_file)
    started = time.time()
    results = []
    with tempfile.TemporaryDirectory(prefix="parity-corpus-") as tmp:
        tmp = pathlib.Path(tmp)
        scenario = tmp / f"{entry['stem']}.json"
        scenario.write_bytes(gz_read(corpus / entry["scenario"]))
        for rep in entry["reports"]:
            report = tmp / f"{entry['stem']}.{rep['label']}.json"
            report.write_bytes(gz_read(corpus / rep["file"]))
            try:
                proc = subprocess.run([args.check, str(scenario), str(report)], capture_output=True, text=True,
                                      encoding="utf-8", errors="replace", timeout=args.timeout)
                if proc.returncode == 2 or not proc.stdout.strip():
                    verdict = {"status": "engine-error", "error": (proc.stderr or proc.stdout).strip()[:400]}
                else:
                    verdict = json.loads(proc.stdout)
            except subprocess.TimeoutExpired:
                verdict = {"status": "check-timeout", "error": f"more than {args.timeout} s"}
            results.append(report_result(entry, rep, verdict, out))
    return finish_entry(entry, results, round(time.time() - started, 2), out, row_file)


def report_result(entry, rep, verdict, out):
    """One report's part of a position's row (the verdict of a non-match is kept whole)."""
    result = {"label": rep["label"], "mode": rep["mode"], "roll": rep.get("roll"),
              "status": verdict.get("status", "engine-error")}
    for key in ("engineOutcomes", "oracleOutcomes", "onlyEngine", "onlyOracle", "maxSharedDiff", "tv",
                "variants", "engineMs", "differences", "error"):
        if verdict.get(key) not in (None, []):
            result[key] = verdict[key]
    if result["status"] != "match":
        write_json(out / "verdicts" / entry["set"] / f"{entry['stem']}.{rep['label']}.json", verdict)
    return result


def finish_entry(entry, results, seconds, out, row_file=None):
    row_file = row_file or out / "rows" / entry["set"] / f"{entry['stem']}.json"
    statuses = [r["status"] for r in results]
    precedence = ("check-timeout",) + FIXED_PRECEDENCE
    status = next((s for s in precedence if s in statuses), statuses[0] if statuses else "engine-error")
    row = {"id": entry["id"], "set": entry["set"], "stem": entry["stem"], "status": status,
           "compared_by": " + ".join(r["label"] for r in results), "kind": entry.get("kind"),
           "matchup": entry.get("matchup"), "policy": entry.get("policy"),
           "seconds": seconds, "reports": results}
    write_json(row_file, row)
    return row


def check_batch(args, corpus, entries, out, done):
    """Board P5: every report of `entries` through one `lab-check --batch - --jobs n` process that
    reads the gzipped corpus files itself (no temporary files, no process per report). `done(row)`
    is called for each position once all its reports are in; a position's `seconds` is the sum of
    its reports' `engineMs` (lab-check's time for reading, parsing and the engine). `--timeout`
    bounds each report (`lab-check --batch --timeout`, board P5-t1: a `check-timeout` verdict)."""
    pending = {}
    jobs = []
    for n, entry in enumerate(entries):
        pending[n] = {"entry": entry, "results": [None] * len(entry["reports"])}
        for k, rep in enumerate(entry["reports"]):
            jobs.append({"id": f"{n}:{k}", "scenario": str(corpus / entry["scenario"]).replace(os.sep, "/"),
                         "report": str(corpus / rep["file"]).replace(os.sep, "/")})
    proc = subprocess.Popen([args.check, "--batch", "-", "--jobs", str(args.jobs), "--timeout", str(args.timeout)],
                            stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8",
                            errors="replace", bufsize=1)

    def feed():
        try:
            for job in jobs:
                proc.stdin.write(json.dumps(job) + "\n")
            proc.stdin.close()
        except (BrokenPipeError, OSError):
            pass

    writer = threading.Thread(target=feed, daemon=True)
    writer.start()
    for line in proc.stdout:
        if not line.strip():
            continue
        verdict = json.loads(line)
        n, k = (int(x) for x in verdict["id"].split(":"))
        slot = pending[n]
        entry = slot["entry"]
        slot["results"][k] = report_result(entry, entry["reports"][k], verdict, out)
        if all(r is not None for r in slot["results"]):
            seconds = round(sum(r.get("engineMs") or 0 for r in slot["results"]) / 1000, 2)
            done(finish_entry(entry, slot["results"], seconds, out))
            del pending[n]
    proc.wait()
    writer.join()
    stderr = proc.stderr.read().strip()
    # Whatever the process did not answer (it crashed) is an engine error of its own.
    for slot in pending.values():
        entry = slot["entry"]
        results = [r if r is not None else report_result(entry, entry["reports"][k], {
            "status": "engine-error", "error": f"lab-check --batch exited {proc.returncode}: {stderr[:300]}"}, out)
            for k, r in enumerate(slot["results"])]
        done(finish_entry(entry, results, 0.0, out))


def exe_info(path):
    p = pathlib.Path(path)
    if not p.exists():
        raise SystemExit(f"lab-check not found: {p}")
    data = p.read_bytes()
    return {"path": str(p).replace(os.sep, "/"), "sha256": sha256(data),
            "mtime": datetime.datetime.fromtimestamp(p.stat().st_mtime).isoformat(timespec="seconds")}


def cmd_check(args):
    corpus = args.corpus.resolve()
    index = read_json(corpus / "corpus.json")
    # A build still running has written its finished positions' rows but not the index yet.
    index["positions"] = [merge_build_row(corpus, e) if e.get("build") == "pending" else e
                          for e in index["positions"]]
    out = (args.out or corpus / "check").resolve()
    (out / "rows").mkdir(parents=True, exist_ok=True)
    entries = [e for e in index["positions"] if e.get("build") == "ok"
               and (not args.only or fnmatch.fnmatch(e["id"], args.only))]
    if args.limit:
        entries = entries[:args.limit]
    skipped = {s: sum(1 for e in index["positions"] if e.get("build") == s) for s in ("excluded", "pending", "failed")}
    exe = exe_info(args.check)
    print(f"{len(entries)} positions, {sum(len(e['reports']) for e in entries)} reports, {args.jobs} job(s), "
          f"lab-check {exe['path']}", flush=True)
    lock = threading.Lock()
    rows = []
    started = time.time()

    def run(entry):
        return report(check_entry(args, corpus, entry, out))

    def report(row):
        with lock:
            rows.append(row)
            if row["status"] != "match" or args.verbose:
                bad = next((r for r in row["reports"] if r["status"] != "match"), row["reports"][0])
                what = "; ".join(bad.get("differences", [])[:2]) or bad.get("error", "")
                print(f"[{len(rows)}/{len(entries)}] {row['id']}: {row['status']} {row['compared_by']} "
                      f"{what[:200]}", flush=True)
            elif len(rows) % 100 == 0:
                print(f"[{len(rows)}/{len(entries)}] {time.time() - started:.0f} s", flush=True)
        return row

    if args.per_report:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            list(pool.map(run, entries))
    else:
        todo = []
        for entry in entries:
            row_file = out / "rows" / entry["set"] / f"{entry['stem']}.json"
            if args.resume and row_file.exists():
                report(read_json(row_file))
            else:
                todo.append(entry)
        check_batch(args, corpus, todo, out, report)
    elapsed = time.time() - started
    summary = write_check_summary(args, corpus, index, out, rows, exe, skipped, elapsed)
    print(json.dumps({k: summary[k] for k in ("positions", "reports", "counts", "all_match", "elapsed_s")}))
    print(f"written {out / 'summary.json'} and {out / 'summary.md'}")
    # A partly built corpus is not "everything" unless the run was restricted on purpose.
    incomplete = skipped["pending"] + skipped["failed"]
    if incomplete and not (args.only or args.limit):
        print(f"the corpus has {incomplete} position(s) without reports (pending or failed build)")
        return 1
    return 0 if summary["all_match"] else 1


def check_table(rows, key):
    groups = {}
    for r in rows:
        groups.setdefault(r.get(key) or "?", []).append(r)
    lines = [f"| {key} | positions | match | mismatch | other |", "|---|---|---|---|---|"]
    for k in sorted(groups, key=str):
        g = groups[k]
        m = sum(1 for r in g if r["status"] == "match")
        mm = sum(1 for r in g if r["status"] == "mismatch")
        lines.append(f"| {k} | {len(g)} | {m} | {mm} | {len(g) - m - mm} |")
    return "\n".join(lines)


def write_check_summary(args, corpus, index, out, rows, exe, skipped, elapsed):
    counts = {s: sum(1 for r in rows if r["status"] == s) for s in CHECK_STATUSES}
    reports = [x for r in rows for x in r["reports"]]
    report_counts = {s: sum(1 for x in reports if x["status"] == s) for s in CHECK_STATUSES}
    engine_ms = sum(x.get("engineMs") or 0 for x in reports)
    summary = {
        "corpus": str(corpus).replace(os.sep, "/"),
        "lab_check": exe,
        "only": args.only,
        "jobs": args.jobs,
        "runner": "per-report" if args.per_report else "batch",
        "positions": len(rows),
        "reports": len(reports),
        "counts": counts,
        "report_counts": report_counts,
        "not_checked": skipped,
        "all_match": bool(rows) and counts["match"] == len(rows),
        "elapsed_s": round(elapsed, 1),
        "engine_s": round(engine_ms / 1000, 1),
        "finished": datetime.datetime.now().isoformat(timespec="seconds"),
        "rows": sorted(rows, key=lambda r: r["id"]),
    }
    # V14: which features the mismatched positions share (lift over the matched ones).
    lift = None
    if counts["mismatch"] and counts["match"]:
        import parity_lift
        lift = parity_lift.corpus_lift(corpus, rows)
        summary["lift"] = lift
    write_json(out / "summary.json", summary)
    slowest = sorted(rows, key=lambda r: -r["seconds"])[:10]
    lines = [
        "# Parity corpus check",
        "",
        f"Corpus `{summary['corpus']}`; lab-check `{exe['path']}` (sha256 `{exe['sha256'][:16]}`, built {exe['mtime']}); "
        f"{args.jobs} job(s), {summary['runner']}; {elapsed:.0f} s wall, {engine_ms / 1000:.0f} s in lab-check's engine.",
        "",
        "| positions | reports | match | mismatch | unsupported | engine error | no position | ambiguous | timeout |",
        "|---|---|---|---|---|---|---|---|---|",
        f"| {len(rows)} | {len(reports)} | {counts['match']} | {counts['mismatch']} | {counts['unsupported']} | "
        f"{counts['engine-error']} | {counts['no-position']} | {counts['ambiguous']} | {counts['check-timeout']} |",
        "",
        f"Reports: " + ", ".join(f"{k} {v}" for k, v in report_counts.items() if v) + ".",
        f"Not checked (corpus entries without reports): " + ", ".join(f"{k} {v}" for k, v in skipped.items()) + ".",
        f"**{'Every checked position matches.' if summary['all_match'] else 'Not every position matches.'}**",
        "",
        "## By set", "", check_table(rows, "set"), "",
        "## By comparison (reports)", "", check_table(rows, "compared_by"), "",
        "## By decision kind", "", check_table(rows, "kind"), "",
        "## Slowest positions", "", "| position | seconds | reports |", "|---|---|---|",
    ]
    lines += [f"| {r['id']} | {r['seconds']:.1f} | {r['compared_by']} |" for r in slowest]
    lines.append("")
    bad = [r for r in rows if r["status"] != "match"]
    if bad:
        lines += ["## Positions that did not match", "", "| position | status | report | first differences / error |",
                  "|---|---|---|---|"]
        for r in sorted(bad, key=lambda r: r["id"]):
            for x in r["reports"]:
                if x["status"] == "match":
                    continue
                what = "; ".join(x.get("differences", [])[:3]) or x.get("error", "")
                lines.append(f"| {r['id']} | {x['status']} | {x['label']} | {what.replace('|', '/')[:300]} |")
        lines.append("")
    if lift:
        import parity_lift
        lines += parity_lift.markdown(lift)
    (out / "summary.md").write_text("\n".join(lines), encoding="utf-8")
    return summary


# ---------------------------------------------------------------------------------------------
# stats, extract


def cmd_stats(args):
    corpus = args.corpus.resolve()
    index = read_json(corpus / "corpus.json")
    files = sorted((corpus / "positions").rglob("*.gz"))
    gz_total = sum(f.stat().st_size for f in files)
    largest_gz = max(files, key=lambda f: f.stat().st_size) if files else None
    entries = [e for e in index["positions"] if e.get("build") == "ok"]
    raw = [(r["bytes"], r["file"]) for e in entries for r in e["reports"]] + \
          [(e["scenario_bytes"], e["scenario"]) for e in entries]
    raw_total = sum(b for b, _ in raw)
    largest_raw = max(raw) if raw else (0, None)
    stats = {
        "positions": len(entries),
        "excluded": sum(1 for e in index["positions"] if e.get("build") == "excluded"),
        "pending_or_failed": sum(1 for e in index["positions"] if e.get("build") in ("pending", "failed")),
        "files": len(files),
        "reports": sum(len(e["reports"]) for e in entries),
        "gz_bytes": gz_total,
        "raw_bytes": raw_total,
        "ratio": round(raw_total / gz_total, 1) if gz_total else None,
        "largest_gz": {"file": str(largest_gz.relative_to(corpus)).replace(os.sep, "/"),
                       "bytes": largest_gz.stat().st_size} if largest_gz else None,
        "largest_raw": {"file": largest_raw[1], "bytes": largest_raw[0]},
        "corpus_json_bytes": (corpus / "corpus.json").stat().st_size,
    }
    print(json.dumps(stats, indent=1))
    return 0


def cmd_extract(args):
    corpus = args.corpus.resolve()
    index = read_json(corpus / "corpus.json")
    entry = next((e for e in index["positions"] if e["id"] == args.id), None)
    if entry is None or entry.get("build") != "ok":
        raise SystemExit(f"{args.id}: not a built position of the corpus")
    out = (args.out or pathlib.Path(".")).resolve()
    out.mkdir(parents=True, exist_ok=True)
    scenario = out / f"{entry['stem']}.json"
    scenario.write_bytes(gz_read(corpus / entry["scenario"]))
    print(scenario)
    for rep in entry["reports"]:
        report = out / f"{entry['stem']}.{rep['label']}.json"
        report.write_bytes(gz_read(corpus / rep["file"]))
        print(f"lab-check {scenario} {report}")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="command", required=True)
    b = sub.add_parser("build", help="regenerate the oracle reports of every compared position")
    b.add_argument("corpus", type=pathlib.Path)
    b.add_argument("--jobs", type=int, default=3)
    b.add_argument("--timeout", type=int, default=3600, help="seconds per oracle run")
    b.add_argument("--resume", action="store_true", help="keep positions already built")
    b.add_argument("--only", help="glob on position ids (<set>/<stem>)")
    b.add_argument("--limit", type=int, help="only the first N games")
    b.add_argument("--plan-only", action="store_true", help="write corpus.json's plan, run nothing")
    c = sub.add_parser("check", help="re-check the engine against every stored report (lab-check only)")
    c.add_argument("corpus", type=pathlib.Path)
    c.add_argument("--jobs", type=int, default=3)
    c.add_argument("--check", default="D:/cargo-target/release/lab-check.exe")
    c.add_argument("--out", type=pathlib.Path)
    c.add_argument("--only", help="glob on position ids (<set>/<stem>)")
    c.add_argument("--limit", type=int, help="only the first N positions")
    c.add_argument("--timeout", type=int, default=1800, help="seconds per report (a check-timeout verdict)")
    c.add_argument("--per-report", action="store_true",
                   help="one lab-check process per report on decompressed copies (the way before P5)")
    c.add_argument("--resume", action="store_true", help="reuse <out>/rows of positions already checked")
    c.add_argument("--verbose", action="store_true", help="print every position, not only non-matches")
    s = sub.add_parser("stats", help="count, total size and largest files of the corpus")
    s.add_argument("corpus", type=pathlib.Path)
    x = sub.add_parser("extract", help="write one position's scenario and reports uncompressed")
    x.add_argument("corpus", type=pathlib.Path)
    x.add_argument("id")
    x.add_argument("--out", type=pathlib.Path)
    args = ap.parse_args()
    return {"build": cmd_build, "check": cmd_check, "stats": cmd_stats, "extract": cmd_extract}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
