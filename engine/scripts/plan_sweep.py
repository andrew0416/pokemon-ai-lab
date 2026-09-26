"""Runs `lab-plan` over every `*-vs-*.json` scenario in a run directory (all start positions)
and writes `summary.<solve>.<rolls>.json` plus the raw outputs under `out/`.

Usage (from the repository root):
    python engine/scripts/plan_sweep.py runs/plan-20260926 [--rolls median] [--solve nash]
        [--exe D:/cargo-target/release/lab-plan.exe] [--side p1] [--top 8] [--extra "--eval material"]

Scenario files reference team files relative to their own directory (see the loader); the
sweep does not copy or modify teams. A scenario with several start states (Trace, Speed ties)
is run once per `--position`. The summary keeps, per run: equilibrium / pure maximin (nash) or
the best line (maximin), the matrix size, dropped pairs and the engine's unsupported reasons.
"""
import argparse
import json
import pathlib
import re
import subprocess
import time


def run(exe, args, timeout):
    t0 = time.time()
    p = subprocess.run(
        [exe] + args,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    return p.returncode, (p.stdout or "") + (p.stderr or ""), time.time() - t0


def parse(text, entry):
    eq = re.search(
        r"equilibrium value ([+-][0-9.]+) \(exploitability ([0-9.]+).*pure maximin ([+-][0-9.]+)",
        text,
    )
    if eq:
        entry["equilibrium"] = float(eq.group(1))
        entry["exploitability"] = float(eq.group(2))
        entry["pure_maximin"] = float(eq.group(3))
    best = re.search(r"best value ([+-][0-9.]+)", text)
    if best:
        entry["best_value"] = float(best.group(1))
    first = re.search(r"^\s*1\s+(<=)?([+-][0-9.]+)\s+(.*?)\s{2,}(.*)$", text, re.M)
    if first:
        entry["best_line"] = first.group(3).strip()
        entry["best_line_reply"] = first.group(4).strip()
    mat = re.search(r"matrix (\d+)x(\d+)", text)
    if mat:
        entry["matrix"] = [int(mat.group(1)), int(mat.group(2))]
    drop = re.search(r"dropped (\d+) of their replies and (\d+) of our choices", text)
    if drop:
        entry["dropped_theirs"] = int(drop.group(1))
        entry["dropped_ours"] = int(drop.group(2))
    drop2 = re.search(r"dropped (\d+) pair\(s\)", text)
    if drop2:
        entry["dropped_pairs"] = int(drop2.group(1))
    entry["unsupported"] = re.findall(r"^  - (.*)$", text, re.M)
    err = re.search(r"^lab-plan: (.*)$", text, re.M)
    if err:
        entry["error"] = err.group(1)
    ours = re.search(r"our mixed strategy \(>= 1%\):\n((?:  .*\n)+)", text)
    if ours:
        entry["our_strategy"] = [l.strip() for l in ours.group(1).strip().split("\n")]
    theirs = re.search(r"their mixed strategy \(>= 1%\):\n((?:  .*\n)+)", text)
    if theirs:
        entry["their_strategy"] = [l.strip() for l in theirs.group(1).strip().split("\n")]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--rolls", default="median")
    ap.add_argument("--solve", default="nash")
    ap.add_argument("--side", default="p1")
    ap.add_argument("--top", default="8")
    ap.add_argument("--exe", default="D:/cargo-target/release/lab-plan.exe")
    ap.add_argument("--extra", default="", help="extra lab-plan arguments, one string")
    ap.add_argument("--timeout", type=int, default=1800)
    a = ap.parse_args()
    run_dir = pathlib.Path(a.run_dir)
    out_dir = run_dir / "out"
    out_dir.mkdir(exist_ok=True)
    extra = a.extra.split() if a.extra else []
    summary = []
    for sc in sorted(run_dir.glob("*-vs-*.json")):
        base = [str(sc), "--side", a.side, "--rolls", a.rolls, "--solve", a.solve, "--top", a.top] + extra
        rc, text, _ = run(a.exe, base, a.timeout)
        positions = [None]
        listing = ""
        m = re.search(r"(\d+) initial states", text)
        if m:
            positions = list(range(int(m.group(1))))
            listing = text
        for pos in positions:
            args = base + (["--position", str(pos)] if pos is not None else [])
            rc, text, dt = run(a.exe, args, a.timeout)
            name = f"{sc.stem}{'' if pos is None else f'.pos{pos}'}.{a.solve}.{a.rolls}.txt"
            (out_dir / name).write_text(" ".join(args) + "\n\n" + text, encoding="utf-8")
            entry = {
                "scenario": sc.name,
                "position": pos,
                "rolls": a.rolls,
                "solve": a.solve,
                "rc": rc,
                "seconds": round(dt, 1),
            }
            if pos is not None:
                line = re.search(rf"^\s*{pos}: p=([0-9.]+) (.*)$", listing, re.M)
                if line:
                    entry["position_probability"] = float(line.group(1))
                    entry["position_actives"] = line.group(2).strip()
            parse(text, entry)
            summary.append(entry)
            shown = {k: v for k, v in entry.items() if k not in ("our_strategy", "their_strategy", "unsupported")}
            print(json.dumps(shown, ensure_ascii=False))
    path = run_dir / f"summary.{a.solve}.{a.rolls}.json"
    json.dump(summary, open(path, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print("wrote", path)


if __name__ == "__main__":
    main()
