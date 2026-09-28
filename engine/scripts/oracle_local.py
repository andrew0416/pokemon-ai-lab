"""Runs an oracle job (engine/jobs/<job>/: oracle.json + positions/*.json, see oracle_job.py) on the
local machine instead of GitHub Actions: the same per-position procedure as the runner
(`oracle_job.run_position`: enumerate.cjs --lean [--staged] [--collapse-secondaries], then the
lab-check runs of oracle.json "checks"), several positions at a time, with local limits.

    LAB_ROOT=D:/pokemon-ai-lab PYTHONUTF8=1 python engine/scripts/oracle_local.py <job> \\
        --jobs 4 --out runs/parity-local-<date>/<job> [--minutes 20] [--heap-mb 3000] \\
        [--check <lab-check.exe>] [--only <substring>] [--retry timeout,error]

Each position writes <out>/<name>.result.json (same format as the runner's), the oracle report
(gzipped) and logs. A position whose result.json exists is skipped (the run can be restarted);
--retry lists oracle states (timeout, error, overflow) to run again. A position whose oracle runs
past --minutes is recorded as `timeout` (left for the runners). At the end <out>/summary.md and
summary.json are written (oracle_job.summary plus totals).

Sharded positions (oracle.json shards > 1) are run unsharded here: one local process per position.
Peak memory is not measured on Windows (no wait4).
"""
import argparse
import json
import os
import pathlib
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import oracle_job  # noqa: E402

_print_lock = threading.Lock()


def log(msg: str) -> None:
    with _print_lock:
        print(time.strftime("%H:%M:%S ") + msg, flush=True)


def state_of(res: dict) -> str:
    if res.get("oracle") != "fit":
        return res.get("oracle", "?")
    ch = res.get("checks", {})
    return "/".join(ch[k].get("status", "?") for k in ch) or "fit"


def write_summary(job: str, out: pathlib.Path, wall_s: float | None) -> dict:
    import contextlib
    import io
    with contextlib.redirect_stdout(io.StringIO()):  # the runner's ::notice lines
        oracle_job.summary(str(out))
    rows = [json.load(open(f, encoding="utf-8")) for f in sorted(out.glob("*.result.json"))]
    counts: dict[str, int] = {}
    for r in rows:
        counts[state_of(r)] = counts.get(state_of(r), 0) + 1
    total = len(oracle_job.positions(job))
    fit = [r for r in rows if r.get("oracle") == "fit"]
    osec = sorted(r.get("oracle_s") or 0 for r in fit)
    csec = sum(sum((c.get("s") or 0) for c in r.get("checks", {}).values()) for r in rows)
    bad = [r for r in fit if any(c.get("status") != "match" for c in r.get("checks", {}).values())]
    lines = [f"# local oracle run: {job}", "",
             f"- positions: {len(rows)} of {total} done; states (checks factored/flat): "
             + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())),
             f"- oracle time (fit positions): total {sum(osec):.0f} s, median "
             f"{osec[len(osec) // 2] if osec else 0:.1f} s, max {osec[-1] if osec else 0:.1f} s;"
             f" lab-check total {csec:.0f} s" + (f"; wall {wall_s:.0f} s this invocation" if wall_s else ""),
             f"- not matched (fit, some check != match): {len(bad)}"
             + ("".join(f"\n  - {r['position']}: {state_of(r)}" for r in bad)),
             f"- timeout (left for runners): "
             + (", ".join(r["position"] for r in rows if r.get("oracle") == "timeout") or "none"),
             ""]
    body = (out / "summary.md").read_text(encoding="utf-8")
    (out / "summary.md").write_text("\n".join(lines) + "\n" + body, encoding="utf-8")
    return counts


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("job")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--out", required=True)
    ap.add_argument("--minutes", type=float, default=20, help="oracle limit per position")
    ap.add_argument("--heap-mb", type=int, default=3000, help="node --max-old-space-size")
    ap.add_argument("--check", help="lab-check executable (else $LAB_CHECK, else engine/target/release)")
    ap.add_argument("--only", default="", help="substring filter on position names")
    ap.add_argument("--retry", default="", help="comma list of oracle states to run again")
    a = ap.parse_args()

    if a.check:
        os.environ["LAB_CHECK"] = str(pathlib.Path(a.check).resolve())
    exe = oracle_job.lab_check_exe()
    if not pathlib.Path(exe).exists():
        sys.exit(f"lab-check not found: {exe}")
    out = pathlib.Path(a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    retry = {s for s in a.retry.split(",") if s}
    overrides = {"oracle_minutes": a.minutes, "node_heap_mb": a.heap_mb}

    todo = []
    for name in oracle_job.positions(a.job):
        if a.only not in name:
            continue
        rf = out / f"{name}.result.json"
        if rf.exists():
            prev = json.load(open(rf, encoding="utf-8"))
            if prev.get("oracle") not in retry:
                continue
        todo.append(name)
    log(f"{a.job}: {len(todo)} positions to run, {a.jobs} at a time, oracle limit {a.minutes} min, "
        f"heap {a.heap_mb} MB, lab-check {exe}, out {out}")
    json.dump({"job": a.job, "overrides": overrides, "jobs": a.jobs, "lab_check": exe,
               "lab_root": os.environ.get("LAB_ROOT"), "started": time.strftime("%Y-%m-%d %H:%M:%S")},
              open(out / "local-run.json", "w", encoding="utf-8"), indent=1)

    started = time.monotonic()
    done = 0

    def one(name: str) -> dict:
        try:
            return oracle_job.run_position(a.job, name, "", out, overrides)
        except Exception as e:  # keep the pool going; record the failure
            res = {"job": a.job, "position": name, "shard": "", "oracle": "error",
                   "message": f"oracle_local: {type(e).__name__}: {e}"[:600]}
            json.dump(res, open(out / f"{name}.result.json", "w", encoding="utf-8"), indent=1)
            return res

    with ThreadPoolExecutor(max_workers=a.jobs) as pool:
        futs = {pool.submit(one, n): n for n in todo}
        for f in as_completed(futs):
            res = f.result()
            done += 1
            ch = res.get("checks", {})
            tv = next((c.get("tv") for c in ch.values() if c.get("tv") is not None), None)
            log(f"[{done}/{len(todo)}] {res['position']}: {state_of(res)} oracle {res.get('oracle_s')} s"
                + (f" tv={tv:.2e}" if isinstance(tv, (int, float)) else ""))
    counts = write_summary(a.job, out, time.monotonic() - started)
    log(f"{a.job} finished: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())))


if __name__ == "__main__":
    main()
