"""Search-speed benchmark for `lab-plan` (board S24-baseline-timing, wave 15 L6).

Runs a fixed set of positions x search modes with `--rolls median`, records wall time, the
node / enumeration counts and the reported time `lab-plan` prints, and the values each mode
reports (so a speed change that also changes a value shows up), in
`<out-dir>/<label>.json` (label: the engine's short commit by default). `--table` prints the
Markdown comparison of every JSON in the directory (one column per label, in file order).

Usage (from the repository root):
    python engine/scripts/search_bench.py [--exe D:/cargo-target/release/lab-plan.exe]
        [--out-dir runs/search-bench-20260927] [--label <name>] [--modes nash,deep-nash,plan]
        [--only sand-owen] [--threads n] [--repeat k] [--extra "--foo"]
    python engine/scripts/search_bench.py --compare base=a.exe,new=b.exe [--repeat 2] ...
    python engine/scripts/search_bench.py --table [--out-dir ...] [--labels a,b]

The positions are fixed (the baseline): scenarios of `runs/plan-20260926` (gitignored; run
directory of the first search application, turn 1 of four library teams), referenced by
path, never copied or modified. `deep-nash` (beam 4, outcomes 4) runs on sand-owen and
psy-cona only (coaching-panda's costs about 4 minutes; `--all-deep` adds it): the whole bench
is meant to be re-run after every search change. `plan` is a one-turn plan with its children
valued by their next-turn equilibrium (`--child-nash --beam 6 --outcomes 4`). Values are
evaluator scores (not win rates); they are recorded here only to check that a speed change
keeps them. Wall times depend on the machine's load (other sessions share the cores); the node
and enumeration counts do not, and are the measure of search work.
"""
import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]

# (name, scenario relative to the plan run directory, extra position args, plan line)
CASES = [
    (
        "sand-owen",
        "gardevoir-vs-sand-owen.json",
        ["--position", "1"],
        "move protect, move grassyglide 2",
    ),
    (
        "psy-cona",
        "starmie-vs-psy-cona.json",
        [],
        "move liquidation 2 mega, move fakeout 1",
    ),
    (
        "coaching-panda",
        "gardevoir-vs-coaching-panda.json",
        ["--position", "0"],
        "move focusblast 2 mega, move fakeout 1",
    ),
]

MODES = {
    "nash": lambda plan: ["--solve", "nash"],
    # The root by double oracle (S24d 3/3; builds from 11dc2a5 on).
    "nash-lazy": lambda plan: ["--solve", "nash", "--lazy"],
    "deep-nash": lambda plan: ["--solve", "deep-nash", "--beam", "4", "--outcomes", "4"],
    "plan": lambda plan: ["--plan", plan, "--child-nash", "--beam", "6", "--outcomes", "4"],
    # Depth 3 (S24c): children are depth-2 analyses (beams 3, 2 outcomes), grandchildren
    # one-turn equilibria. Not in the default modes (minutes per position); runs on every case.
    "deep-nash3": lambda plan: ["--solve", "deep-nash", "--beam", "4,3", "--outcomes", "4,2"],
}

# deep-nash only where its cost fits the budget (fixed with the baseline: coaching-panda's
# deep-nash took 244 s at 9ff1081, sand-owen 131 s, psy-cona 57 s).
DEEP_CASES = {"sand-owen", "psy-cona"}


def git_commit():
    try:
        return subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except Exception:
        return "unknown"


def git_dirty():
    try:
        out = subprocess.run(
            ["git", "status", "--porcelain", "--", "engine"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        return bool(out.strip())
    except Exception:
        return None


def parse(text):
    entry = {}
    stats = re.search(r"([0-9]+) nodes, ([0-9]+) enumerations, ([0-9.]+) s", text)
    if stats:
        entry["nodes"] = int(stats.group(1))
        entry["enumerations"] = int(stats.group(2))
        entry["reported_s"] = float(stats.group(3))
    eq = re.search(
        r"matrix (\d+)x(\d+)[^;]*; equilibrium value ([+-][0-9.]+) \(exploitability[a-z ]* ([0-9.]+)",
        text,
    )
    if eq:
        entry["matrix"] = f"{eq.group(1)}x{eq.group(2)}"
        entry["value"] = float(eq.group(3))
        entry["exploitability"] = float(eq.group(4))
    levels = re.search(r"deep-nash depth (\d+) \(levels beam/outcomes ([^)]*)\)", text)
    if levels:
        entry["depth"] = int(levels.group(1))
        entry["levels"] = levels.group(2)
    deep = re.search(
        r"shallow matrix (\d+)x(\d+), equilibrium ([+-][0-9.]+); deep matrix (\d+)x(\d+), "
        r"equilibrium ([+-][0-9.]+) \(exploitability ([0-9.]+)",
        text,
    )
    if deep:
        entry["shallow_value"] = float(deep.group(3))
        entry["matrix"] = f"{deep.group(4)}x{deep.group(5)}"
        entry["value"] = float(deep.group(6))
        entry["exploitability"] = float(deep.group(7))
    plan = re.search(r"value ([+-][0-9.]+) against the worst replies", text)
    if plan:
        entry["plan_value"] = float(plan.group(1))
    child = re.search(r"next-turn equilibrium .*: value ([+-][0-9.]+|NaN)", text)
    if child:
        entry["value"] = float(child.group(1)) if child.group(1) != "NaN" else None
    stats_line = re.search(r"^search stats: (.*)$", text, re.M)
    if stats_line:
        entry["search_stats"] = stats_line.group(1).strip()
    return entry


def process_cpu_seconds(p):
    """User + kernel CPU time of a finished child (Windows `GetProcessTimes`; None elsewhere).
    Less sensitive to other sessions' load than wall time, blind to parallel speed-ups."""
    if os.name != "nt":
        return None
    import ctypes
    from ctypes import wintypes

    handle = getattr(p, "_handle", None)
    if handle is None:
        return None
    times = [wintypes.FILETIME() for _ in range(4)]
    ok = ctypes.windll.kernel32.GetProcessTimes(
        wintypes.HANDLE(int(handle)), *[ctypes.byref(t) for t in times]
    )
    if not ok:
        return None

    def seconds(ft):
        return ((ft.dwHighDateTime << 32) | ft.dwLowDateTime) / 1e7

    return seconds(times[2]) + seconds(times[3])


def show_path(path):
    rel = os.path.relpath(path, ROOT)
    return (rel if not rel.startswith("..") else str(pathlib.Path(path).resolve())).replace("\\", "/")


def run_case(exe, plan_dir, case, mode, extra, timeout):
    name, scenario, position, plan = case
    scenario_path = plan_dir / scenario
    args = [str(scenario_path), "--side", "p1", "--rolls", "median", "--top", "3"]
    args += position + MODES[mode](plan) + extra
    t0 = time.perf_counter()
    p = subprocess.Popen(
        [exe] + args,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    stdout, stderr = p.communicate(timeout=timeout)
    wall = time.perf_counter() - t0
    cpu = process_cpu_seconds(p)
    text = (stdout or "") + (stderr or "")
    entry = {
        "case": name,
        "mode": mode,
        "scenario": show_path(scenario_path),
        "command": [pathlib.Path(exe).name] + [
            show_path(a) if a == str(scenario_path) else a
            for a in args
        ],
        "returncode": p.returncode,
        "wall_s": round(wall, 3),
        "cpu_s": None if cpu is None else round(cpu, 3),
    }
    entry.update(parse(text))
    if p.returncode != 0:
        entry["error"] = text.strip()[-2000:]
    return entry


def bench(opts):
    """Runs every (case, mode) for each build in turn — interleaved when several builds are
    compared (`--compare label=exe,...`), so a change in the machine's load hits them alike —
    and writes one `<label>.json` per build."""
    out_dir = pathlib.Path(opts.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    plan_dir = pathlib.Path(opts.plan_dir)
    modes = [m.strip() for m in opts.modes.split(",") if m.strip()]
    extra = opts.extra.split() if opts.extra else []
    if opts.threads is not None:
        extra += ["--threads", str(opts.threads)]
    commit = git_commit()
    if opts.compare:
        builds = []
        for item in opts.compare.split(","):
            label, exe = item.split("=", 1)
            builds.append((label, exe))
    else:
        builds = [(opts.label or commit, opts.exe)]
    results = {label: [] for label, _ in builds}
    for case in CASES:
        if opts.only and case[0] not in opts.only.split(","):
            continue
        for mode in modes:
            if mode == "deep-nash" and case[0] not in DEEP_CASES and not opts.all_deep:
                continue
            runs = {label: [] for label, _ in builds}
            for _ in range(max(1, opts.repeat)):
                for label, exe in builds:
                    # Only builds that know `--stats` get it (`--stats-for label,...`).
                    args = list(extra)
                    if opts.stats_for and label in opts.stats_for.split(","):
                        args.append("--stats")
                    e = run_case(exe, plan_dir, case, mode, args, opts.timeout)
                    runs[label].append(e)
                    print(
                        f"{label:>28} {case[0]:>15} {mode:>9}: {e['wall_s']:8.2f} s wall, cpu {e['cpu_s']} s, "
                        f"{e.get('nodes', '?')} nodes, {e.get('enumerations', '?')} enumerations, "
                        f"value {e.get('value', e.get('plan_value', '?'))}"
                        + (f"  ERROR {e['error'][:200]}" if "error" in e else ""),
                        flush=True,
                    )
            for label, _ in builds:
                best = min(runs[label], key=lambda e: e["wall_s"])
                best["wall_runs_s"] = [e["wall_s"] for e in runs[label]]
                best["cpu_runs_s"] = [e["cpu_s"] for e in runs[label]]
                results[label].append(best)
    for label, exe in builds:
        doc = {
            "label": label,
            "commit": commit,
            "dirty": git_dirty(),
            "date": time.strftime("%Y-%m-%d %H:%M:%S"),
            "exe": exe,
            "cpus": os.cpu_count(),
            "extra": extra,
            "repeat": opts.repeat,
            "interleaved_with": [b[0] for b in builds if b[0] != label],
            "results": results[label],
        }
        path = out_dir / f"{label}.json"
        path.write_text(json.dumps(doc, indent=1, ensure_ascii=False), encoding="utf-8")
        print(f"wrote {path}")


def table(opts):
    out_dir = pathlib.Path(opts.out_dir)
    docs = []
    for path in sorted(out_dir.glob("*.json"), key=lambda p: p.stat().st_mtime):
        try:
            docs.append(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, ValueError):
            continue
    if opts.labels:
        wanted = opts.labels.split(",")
        docs = sorted(
            [d for d in docs if d["label"] in wanted], key=lambda d: wanted.index(d["label"])
        )
    keys = []
    for d in docs:
        for r in d["results"]:
            k = (r["case"], r["mode"])
            if k not in keys:
                keys.append(k)
    head = "| case | mode | " + " | ".join(d["label"] for d in docs) + " |"
    print(head)
    print("|" + "---|" * (2 + len(docs)))
    for case, mode in keys:
        cells = []
        base = None
        for d in docs:
            r = next((r for r in d["results"] if (r["case"], r["mode"]) == (case, mode)), None)
            if r is None or r.get("returncode") != 0:
                cells.append("-")
                continue
            if base is None:
                base = r["wall_s"]
            speed = f" ({base / r['wall_s']:.2f}x)" if base and r is not None else ""
            v = r.get("value", r.get("plan_value"))
            vtxt = f"{v:+.1f}" if isinstance(v, (int, float)) else "?"
            cpu = r.get("cpu_s")
            cputxt = f", cpu {cpu:.0f} s" if isinstance(cpu, (int, float)) else ""
            cells.append(
                f"{r['wall_s']:.1f} s{speed}{cputxt}, {r.get('enumerations', '?')} enum, v {vtxt}"
            )
        print(f"| {case} | {mode} | " + " | ".join(cells) + " |")


def main():
    date = time.strftime("%Y%m%d")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--exe", default="D:/cargo-target/release/lab-plan.exe")
    ap.add_argument("--out-dir", default=str(ROOT / "runs" / f"search-bench-{date}"))
    ap.add_argument("--plan-dir", default=str(ROOT / "runs" / "plan-20260926"))
    ap.add_argument("--label")
    ap.add_argument("--modes", default="nash,deep-nash,plan")
    ap.add_argument("--only")
    ap.add_argument("--threads", type=int)
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--extra", default="")
    ap.add_argument("--all-deep", action="store_true", help="deep-nash on every case")
    ap.add_argument("--timeout", type=int, default=3600)
    ap.add_argument(
        "--compare",
        help="label=exe,label=exe: run several builds interleaved per case (one JSON each)",
    )
    ap.add_argument("--stats-for", help="labels whose lab-plan gets --stats")
    ap.add_argument("--table", action="store_true")
    ap.add_argument("--labels", help="--table: these labels, in this order")
    opts = ap.parse_args()
    if opts.table:
        table(opts)
    else:
        bench(opts)
    return 0


if __name__ == "__main__":
    sys.exit(main())
