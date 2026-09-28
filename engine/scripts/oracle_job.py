"""Showdown oracle runs on GitHub Actions (.github/workflows/oracle.yml): pinned parity positions
whose `enumerate.cjs` run is too long for the local machine, one runner per position (or per shard),
checked against lab-check on the runner.

Job directory (any lane can make one; pushing a changed oracle.json runs it):

    engine/jobs/<job>/oracle.json        parameters (below)
    engine/jobs/<job>/positions/*.json   pinned scenarios; team paths relative to the file
    engine/jobs/<job>/provenance.json    where the positions came from (written by `pack`)

oracle.json keys (all optional; defaults in DEFAULTS):

    mode              full | extremes | fixed            enumerate.cjs --mode
    roll              0..15 with mode fixed               enumerate.cjs --roll
    staged            true                                enumerate.cjs --staged
    collapse_secondaries true                             enumerate.cjs --collapse-secondaries
    max_branches      2000000000                          enumerate.cjs --max-branches
    oracle_minutes    310                                 kill the oracle after this (job limit 360)
    check_minutes     15                                  per lab-check run
    checks            ["factored", "flat"]                lab-check runs, in order (factored:
                                                          LAB_ENGINE_FACTORED=1)
    shards            1                                   enumerate.cjs --shard i/N runners per
                                                          position (staged only)
    shard_overrides   {"<position name>": N}              per-position shards
    only              ""                                  substring filter on position names
    node_heap_mb      14000                               node --max-old-space-size
    hash_keys         "auto"                              lab-check --hash-keys: true, false, or
                                                          "auto" (when the report has more than
                                                          200,000 outcome lines)

Commands:

    python engine/scripts/oracle_job.py pack <list-file> <job> [--set key=json ...]
        copy the positions named in <list-file> (one scenario path per line) into
        engine/jobs/<job>/positions/, rewriting their team paths, and write oracle.json
    python engine/scripts/oracle_job.py matrix <job>... > matrix.json      (workflow prepare)
    python engine/scripts/oracle_job.py run <job> <position> [--shard i/N] (workflow oracle)
    python engine/scripts/oracle_job.py merge <job> <position> <shard-dir> (workflow merge)
    python engine/scripts/oracle_job.py summary <results-dir>              (workflow summary)

Results: each run writes engine/jobs/<job>/out/<position>[.s<i>of<N>].result.json, the oracle
report (gzipped) and logs; one table row goes to $GITHUB_STEP_SUMMARY and one `::notice` annotation
per position (annotations of a public repository are readable without a token:
GET /repos/<owner>/<repo>/check-runs/<id>/annotations).
"""
import gzip
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from actions_job import rel_from  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULTS = {
    "mode": "full",
    "roll": None,
    "staged": True,
    "collapse_secondaries": True,
    "max_branches": 2_000_000_000,
    "oracle_minutes": 310,
    "check_minutes": 15,
    "checks": ["factored", "flat"],
    "shards": 1,
    "shard_overrides": {},
    "only": "",
    "node_heap_mb": 14000,
    "hash_keys": "auto",
}


def job_dir(job: str) -> pathlib.Path:
    return ROOT / "engine" / "jobs" / job


def params(job: str) -> dict:
    p = dict(DEFAULTS)
    f = job_dir(job) / "oracle.json"
    if f.exists():
        p.update(json.load(open(f, encoding="utf-8")))
    return p


def positions(job: str) -> list[str]:
    only = params(job)["only"]
    return sorted(f.stem for f in (job_dir(job) / "positions").glob("*.json") if only in f.stem)


def shards_of(p: dict, name: str) -> int:
    return int(p["shard_overrides"].get(name, p["shards"]))


# ---- pack ------------------------------------------------------------------------------------

def rebase(new_dir: pathlib.Path, old_dir: pathlib.Path, path: str) -> str:
    """A path relative to old_dir (in any checkout, e.g. the main tree beside a worktree) as a path
    relative to new_dir that reaches the same repository file in this checkout (the runner has only
    this checkout). The file must be tracked here."""
    target = (old_dir / path).resolve()
    top = subprocess.check_output(["git", "-C", str(target.parent), "rev-parse", "--show-toplevel"],
                                  text=True).strip()
    rel = target.relative_to(pathlib.Path(top).resolve())
    here = ROOT / rel
    tracked = subprocess.run(["git", "-C", str(ROOT), "ls-files", "--error-unmatch", str(rel).replace("\\", "/")],
                             capture_output=True).returncode == 0
    if not tracked:
        sys.exit(f"{rel} is not tracked in {ROOT}: the runner would not have it")
    return rel_from(new_dir, ROOT, str(here.relative_to(ROOT))).replace("\\", "/")


def pack(list_file: str, job: str, sets: list[str]) -> None:
    dst = job_dir(job)
    if dst.exists():
        sys.exit(f"{dst} exists; jobs are not overwritten")
    (dst / "positions").mkdir(parents=True)
    prov = {"list": list_file, "positions": []}
    for line in open(list_file, encoding="utf-8"):
        src = line.strip()
        if not src:
            continue
        sc = pathlib.Path(src)
        d = json.load(open(sc, encoding="utf-8-sig"))
        for side in ("p1", "p2"):
            if isinstance(d.get(side, {}).get("team"), str):
                d[side]["team"] = rebase(dst / "positions", sc.parent, d[side]["team"])
        for key in ("believedTeam", "believed_team"):
            if isinstance(d.get(key), str):
                d[key] = rebase(dst / "positions", sc.parent, d[key])
        out = dst / "positions" / sc.name
        if out.exists():
            sys.exit(f"duplicate position name {sc.name}")
        json.dump(d, open(out, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
        prov["positions"].append({"file": sc.name, "from": src.replace("\\", "/")})
    json.dump(prov, open(dst / "provenance.json", "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    # The job runs only on the branch it was packed on (the workflow checks this): a merge
    # carrying the job into another branch must not run it again.
    conf = {"branch": subprocess.check_output(["git", "rev-parse", "--abbrev-ref", "HEAD"], text=True, cwd=ROOT).strip()}
    for s in sets:
        k, v = s.split("=", 1)
        conf[k] = json.loads(v)
    json.dump(conf, open(dst / "oracle.json", "w", encoding="utf-8"), indent=1)
    print(f"packed {len(prov['positions'])} positions into {dst}")


# ---- matrix ----------------------------------------------------------------------------------

def matrix(jobs: list[str]) -> None:
    run, merge_ = [], []
    for job in jobs:
        p = params(job)
        names = positions(job)
        assert names, f"no positions in engine/jobs/{job}/positions"
        for n in names:
            k = shards_of(p, n)
            if k > 1:
                assert p["staged"], "shards need staged"
                run += [{"job": job, "position": n, "shard": f"{i}/{k}"} for i in range(k)]
                merge_.append({"job": job, "position": n, "shards": k})
            else:
                run.append({"job": job, "position": n, "shard": ""})
    assert len(run) <= 256, f"{len(run)} runners: a matrix holds at most 256"
    print("matrix=" + json.dumps({"include": run}))
    print("merge=" + json.dumps({"include": merge_ or [{"job": "", "position": "", "shards": 0}]}))
    print("has_merge=" + ("true" if merge_ else "false"))


# ---- process with timeout and peak memory ----------------------------------------------------

def run_limited(cmd: list[str], limit_s: float, stdout, stderr, env=None, tee_stderr=False) -> dict:
    """Runs cmd; kills it after limit_s. Returns exit code, seconds, peak RSS (MB, Linux)."""
    started = time.monotonic()
    proc = subprocess.Popen(cmd, stdout=stdout, stderr=subprocess.PIPE if tee_stderr else stderr,
                            env=env, start_new_session=(os.name != "nt"))
    killed = False
    peak_kb = 0
    if tee_stderr:
        import threading

        def pump():
            for raw in proc.stderr:
                line = raw.decode("utf-8", "replace")
                stderr.write(line)
                stderr.flush()
                if "staged:" in line or "branches" in line or "Error" in line:
                    print(line.rstrip(), flush=True)
        t = threading.Thread(target=pump, daemon=True)
        t.start()
    if hasattr(os, "wait4"):
        while True:
            pid, status, ru = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                peak_kb = ru.ru_maxrss
                code = os.waitstatus_to_exitcode(status)
                proc.returncode = code
                break
            if time.monotonic() - started > limit_s:
                killed = True
                os.killpg(proc.pid, signal.SIGKILL)
                pid, status, ru = os.wait4(proc.pid, 0)
                peak_kb = ru.ru_maxrss
                code = None
                proc.returncode = -9
                break
            time.sleep(1)
    else:
        try:
            code = proc.wait(timeout=limit_s)
        except subprocess.TimeoutExpired:
            killed = True
            proc.kill()
            proc.wait()
            code = None
    if tee_stderr:
        t.join(timeout=10)
    return {"code": code, "killed": killed, "s": round(time.monotonic() - started, 1),
            "peak_mb": round(peak_kb / 1024) if peak_kb else None}


def lab_check_exe() -> str:
    exe = os.environ.get("LAB_CHECK")
    if exe:
        return exe
    return str(ROOT / "engine" / "target" / "release" / ("lab-check.exe" if os.name == "nt" else "lab-check"))


def run_checks(p: dict, scenario: pathlib.Path, report: pathlib.Path, stem: pathlib.Path,
               outcome_lines: int) -> dict:
    checks = {}
    hashed = p["hash_keys"] is True or (p["hash_keys"] == "auto" and (outcome_lines or 0) > 200_000)
    for kind in p["checks"]:
        env = dict(os.environ)
        if kind == "factored":
            env["LAB_ENGINE_FACTORED"] = "1"
        else:
            env.pop("LAB_ENGINE_FACTORED", None)
        verdict = stem.with_name(stem.name + f".check-{kind}.json")
        with open(stem.with_name(stem.name + f".check-{kind}.log"), "w") as log:
            r = run_limited([lab_check_exe(), str(scenario), str(report), "--out", str(verdict)]
                            + (["--hash-keys"] if hashed else []),
                            p["check_minutes"] * 60, subprocess.DEVNULL, log, env=env)
        entry = {"s": r["s"], "peak_mb": r["peak_mb"], "hash_keys": hashed}
        if r["killed"]:
            entry["status"] = "check-timeout"
        elif verdict.exists():
            v = json.load(open(verdict, encoding="utf-8"))
            entry.update({k: v.get(k) for k in ("status", "tv", "maxSharedDiff", "engineOutcomes",
                                                 "oracleOutcomes", "onlyEngine", "onlyOracle",
                                                 "engineMs", "differences", "error")})
        else:
            entry["status"] = f"check-crash (exit {r['code']})"
        checks[kind] = entry
        print(f"check {kind}: {entry.get('status')} tv={entry.get('tv')} "
              f"engine={entry.get('engineOutcomes')} oracle={entry.get('oracleOutcomes')} {r['s']} s", flush=True)
    return checks


def report_head(path: pathlib.Path) -> dict:
    """The report's fields other than the outcomes, read line by line (reports can be GBs)."""
    head, tail, state = [], [], 0
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt", encoding="utf-8") as f:
        for line in f:
            if state == 0:
                if line.rstrip("\r\n") == ' "outcomes": [':
                    head.append(' "outcomes": [')
                    state = 1
                else:
                    head.append(line)
            elif state == 1:
                if line.startswith("]"):
                    tail.append(line)
                    state = 2
            else:
                tail.append(line)
    if state == 0:  # a report written in one piece (few outcomes)
        return json.loads("".join(head))
    return json.loads("".join(head) + "".join(tail))


def gzip_file(path: pathlib.Path) -> pathlib.Path:
    gz = path.with_name(path.name + ".gz")
    subprocess.run(["gzip", "-1", "-f", str(path)], check=True) if shutil.which("gzip") else _py_gzip(path, gz)
    return gz


def _py_gzip(path: pathlib.Path, gz: pathlib.Path) -> None:
    with open(path, "rb") as a, gzip.open(gz, "wb", compresslevel=1) as b:
        shutil.copyfileobj(a, b, 1 << 20)
    path.unlink()


# ---- summary output ----------------------------------------------------------------------------

COLUMNS = ["position", "shard", "oracle", "oracle s", "oracle MB", "runs", "outcomes", "frontier",
           "factored", "flat", "TV", "engine outcomes", "check s"]


def row_of(r: dict) -> list:
    ch = r.get("checks", {})
    fa, fl = ch.get("factored", {}), ch.get("flat", {})
    tv = next((c.get("tv") for c in (fa, fl) if c.get("tv") is not None), None)
    eo = next((c.get("engineOutcomes") for c in (fa, fl) if c.get("engineOutcomes") is not None), None)
    return [r["position"], r.get("shard") or "", r.get("oracle"), r.get("oracle_s"), r.get("oracle_peak_mb"),
            r.get("runs"), r.get("outcomes"), r.get("frontier"), fa.get("status", ""), fl.get("status", ""),
            f"{tv:.2e}" if isinstance(tv, (int, float)) else "", eo if eo is not None else "",
            "/".join(str(c.get("s")) for c in (fa, fl) if c)]


def emit(r: dict) -> None:
    row = row_of(r)
    line = " | ".join(f"{c}={v}" for c, v in zip(COLUMNS, row) if v not in ("", None))
    if r.get("oracle") != "fit" and r.get("message"):
        # the last progress lines (stage, frontier, runs) of a run that did not finish
        line += " | last=" + r["message"][-300:]
    print(f"::notice title=oracle {r['position']}{(' ' + r['shard']) if r.get('shard') else ''}::{line}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("| " + " | ".join(COLUMNS) + " |\n|" + "---|" * len(COLUMNS) + "\n")
            f.write("| " + " | ".join(str(v if v is not None else "") for v in row) + " |\n")
            if r.get("message"):
                f.write(f"\n`{r['message'][:500]}`\n")


# ---- run -------------------------------------------------------------------------------------

def run(job: str, name: str, shard: str) -> None:
    res = run_position(job, name, shard)
    emit(res)
    if any(c.get("status") != "match" for c in res.get("checks", {}).values()):
        sys.exit(1)


def run_position(job: str, name: str, shard: str = "", out: pathlib.Path | None = None,
                 overrides: dict | None = None) -> dict:
    """One position (or shard): oracle, then the lab-check runs; writes <out>/<name>.result.json
    (out defaults to the job's out/; overrides replace oracle.json keys, e.g. for local runs by
    oracle_local.py) and returns the result."""
    p = params(job)
    p.update(overrides or {})
    scenario = job_dir(job) / "positions" / f"{name}.json"
    out = out or job_dir(job) / "out"
    out.mkdir(parents=True, exist_ok=True)
    tag = ""
    if shard:
        i, k = shard.split("/")
        tag = f".s{i}of{k}"
    stem = out / f"{name}{tag}"
    report = stem.with_name(stem.name + ".oracle.json")
    cmd = ["node", f"--max-old-space-size={p['node_heap_mb']}", str(ROOT / "engine/oracle/enumerate.cjs"),
           str(scenario), "--mode", p["mode"], "--lean", "--max-branches", str(p["max_branches"]),
           "--out", str(report)]
    if p["mode"] == "fixed":
        cmd += ["--roll", str(p["roll"])]
    if p["staged"]:
        cmd.append("--staged")
    if p["collapse_secondaries"]:
        cmd.append("--collapse-secondaries")
    if shard:
        cmd += ["--shard", shard]
    env = dict(os.environ, LAB_ROOT=os.environ.get("LAB_ROOT", str(ROOT)), LAB_ORACLE_PROGRESS="1")
    print(" ".join(cmd), flush=True)
    with open(stem.with_name(stem.name + ".oracle.log"), "w", encoding="utf-8") as log:
        r = run_limited(cmd, p["oracle_minutes"] * 60, subprocess.DEVNULL, log, env=env, tee_stderr=True)
    res = {"job": job, "position": name, "shard": shard, "mode": p["mode"], "roll": p["roll"],
           "staged": p["staged"], "oracle_s": r["s"], "oracle_peak_mb": r["peak_mb"],
           "oracle_limit_s": p["oracle_minutes"] * 60}
    if r["killed"]:
        res["oracle"] = "timeout"
    elif r["code"] == 0 and report.exists():
        res["oracle"] = "fit"
    else:
        res["oracle"] = "error"
    logtext = stem.with_name(stem.name + ".oracle.log").read_text(encoding="utf-8", errors="replace")
    last = [l for l in logtext.splitlines() if l.strip()]
    res["message"] = " | ".join(last[-3:])[-600:]
    if res["oracle"] == "error" and "more than" in res["message"]:
        res["oracle"] = "overflow"
    if res["oracle"] == "fit":
        h = report_head(report)
        res.update({"runs": h.get("branches"), "outcomes": h.get("distinctOutcomes"),
                    "frontier": (h.get("staged") or {}).get("maxFrontier"),
                    "total_p": h.get("totalProbability"), "exact": h.get("exact"),
                    "elapsed_ms": h.get("elapsedMs"), "showdown": h.get("showdownCommit"),
                    "shard_info": (h.get("staged") or {}).get("shard"),
                    "report_mb": round(report.stat().st_size / 2**20, 1)})
        if not shard:
            res["checks"] = run_checks(p, scenario, report, stem, res["outcomes"])
        gzip_file(report)
    json.dump(res, open(stem.with_name(stem.name + ".result.json"), "w", encoding="utf-8"), indent=1)
    return res


# ---- merge (sharded positions) ---------------------------------------------------------------

def merge(job: str, name: str, shard_dir: str) -> None:
    p = params(job)
    out = job_dir(job) / "out"
    out.mkdir(exist_ok=True)
    files = sorted(pathlib.Path(shard_dir).rglob(f"{name}.s*of*.oracle.json.gz"))
    results = sorted(pathlib.Path(shard_dir).rglob(f"{name}.s*of*.result.json"))
    shard_results = [json.load(open(f, encoding="utf-8")) for f in results]
    k = shards_of(p, name)
    res = {"job": job, "position": name, "shard": f"merged {len(files)}/{k}", "mode": p["mode"],
           "roll": p["roll"], "staged": True, "shards": shard_results}
    res["oracle_s"] = max((s.get("oracle_s") or 0) for s in shard_results) if shard_results else None
    res["oracle_peak_mb"] = max((s.get("oracle_peak_mb") or 0) for s in shard_results) if shard_results else None
    bad = [s for s in shard_results if s.get("oracle") != "fit"]
    if len(files) != k or len(shard_results) != k or bad:
        res["oracle"] = "incomplete"
        res["message"] = f"{len(files)} reports, {len(shard_results)} results of {k}; not fit: " + \
            ", ".join(f"{s.get('shard')}={s.get('oracle')}" for s in bad)
        json.dump(res, open(out / f"{name}.merged.result.json", "w", encoding="utf-8"), indent=1)
        emit(res)
        return
    heads = [report_head(f) for f in files]
    befores = {json.dumps(h.get("before"), sort_keys=True) for h in heads}
    assert len(befores) == 1, "shards disagree on the starting state"
    merged = out / f"{name}.merged.oracle.json"
    meta = dict(heads[0])
    meta["branches"] = sum(h.get("branches", 0) for h in heads)
    meta["totalProbability"] = sum(h.get("totalProbability", 0) for h in heads)
    meta["elapsedMs"] = max(h.get("elapsedMs", 0) for h in heads)
    meta["distinctOutcomes"] = None  # counted below (equal states of different shards merge)
    meta["shards"] = [h.get("staged") for h in heads]
    total_lines = 0
    marker = '"__OUTCOMES__"'
    head_text = json.dumps({**meta, "outcomes": "__OUTCOMES__"}, indent=1, ensure_ascii=False)
    at = head_text.index(marker)
    with open(merged, "w", encoding="utf-8") as w:
        w.write(head_text[:at] + "[")
        first = True
        for f in files:
            state = 0
            with gzip.open(f, "rt", encoding="utf-8") as r:
                for line in r:
                    if state == 0:
                        if line.rstrip("\r\n") == ' "outcomes": [':
                            state = 1
                    elif state == 1:
                        if line.startswith("]"):
                            break
                        text = line.rstrip("\r\n").rstrip(",")
                        if text:
                            w.write(("\n" if first else ",\n") + text)
                            first = False
                            total_lines += 1
        w.write("\n]" + head_text[at + len(marker):] + "\n")
    res.update({"oracle": "fit", "runs": meta["branches"], "outcome_lines": total_lines,
                "total_p": meta["totalProbability"], "report_mb": round(merged.stat().st_size / 2**20, 1)})
    scenario = job_dir(job) / "positions" / f"{name}.json"
    res["checks"] = run_checks(p, scenario, merged, out / f"{name}.merged", total_lines)
    res["outcomes"] = next((c.get("oracleOutcomes") for c in res["checks"].values()
                            if c.get("oracleOutcomes") is not None), None)
    gzip_file(merged)
    json.dump(res, open(out / f"{name}.merged.result.json", "w", encoding="utf-8"), indent=1)
    emit(res)
    if any(c.get("status") != "match" for c in res["checks"].values()):
        sys.exit(1)


# ---- summary ---------------------------------------------------------------------------------

def summary(results_dir: str) -> None:
    rows = []
    for f in sorted(pathlib.Path(results_dir).rglob("*.result.json")):
        rows.append(json.load(open(f, encoding="utf-8")))
    rows.sort(key=lambda r: (r.get("job", ""), r["position"], str(r.get("shard", ""))))
    count = {}
    for r in rows:
        if r.get("shard") and not str(r["shard"]).startswith("merged"):
            continue
        ch = r.get("checks", {})
        key = r.get("oracle") if r.get("oracle") != "fit" else \
            "/".join(ch[k].get("status", "?") for k in ch) or "fit"
        count[key] = count.get(key, 0) + 1
    lines = [f"## oracle results ({len(rows)} rows)", "",
             "counts (positions; checks factored/flat): " + ", ".join(f"{k} {v}" for k, v in sorted(count.items())), "",
             "| " + " | ".join(["job"] + COLUMNS) + " |", "|" + "---|" * (len(COLUMNS) + 1)]
    for r in rows:
        lines.append("| " + " | ".join(str(v if v is not None else "") for v in [r.get("job")] + row_of(r)) + " |")
    text = "\n".join(lines) + "\n"
    print(text)
    print("::notice title=oracle summary::" + ", ".join(f"{k} {v}" for k, v in sorted(count.items())))
    # The whole table as annotations too (readable without a token; at most 10 notices a step).
    compact = [";".join(str(v if v is not None else "") for v in [r.get("job")] + row_of(r)) for r in rows]
    per = max(1, -(-len(compact) // 9))
    for i in range(0, len(compact), per):
        body = "%0A".join(compact[i:i + per])
        print(f"::notice title=oracle table {i // per + 1} ({';'.join(['job'] + COLUMNS)})::{body}")
    s = os.environ.get("GITHUB_STEP_SUMMARY")
    if s:
        with open(s, "a", encoding="utf-8") as f:
            f.write(text)
    pathlib.Path(results_dir, "summary.md").write_text(text, encoding="utf-8")
    json.dump(rows, open(pathlib.Path(results_dir, "summary.json"), "w", encoding="utf-8"), indent=1)


if __name__ == "__main__":
    cmd, *rest = sys.argv[1:] or ["help"]
    if cmd == "pack" and len(rest) >= 2:
        sets = [rest[i + 1] for i in range(2, len(rest) - 1) if rest[i] == "--set"]
        pack(rest[0], rest[1], sets)
    elif cmd == "matrix" and rest:
        matrix(rest)
    elif cmd == "run" and len(rest) >= 2:
        run(rest[0], rest[1], rest[3] if len(rest) >= 4 and rest[2] == "--shard" else "")
    elif cmd == "merge" and len(rest) == 3:
        merge(*rest)
    elif cmd == "summary" and len(rest) == 1:
        summary(rest[0])
    else:
        sys.exit(__doc__)
