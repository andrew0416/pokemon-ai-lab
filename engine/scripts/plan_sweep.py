"""Runs `lab-plan` over every `*-vs-*.json` scenario in a run directory (all start positions)
and writes `summary.<solve>.<rolls>.json` plus the raw outputs under `out/`.

Usage (from the repository root):
    python engine/scripts/plan_sweep.py runs/plan-20260926 [--rolls median] [--solve nash]
        [--exe D:/cargo-target/release/lab-plan.exe] [--side p1] [--top 8] [--extra "--eval material"]
        [--leads all]

Scenario files reference team files relative to their own directory (see the loader); the
sweep does not copy or modify teams. A scenario with several start states (Trace, Speed ties)
is run once per `--position`. The summary keeps, per run: equilibrium / pure maximin (nash) or
the best line (maximin), the matrix size, dropped pairs and the engine's unsupported reasons.

`--leads all` derives, for each scenario, every lead pair of the four brought members on both
sides (6 x 6 variants, written under `leads/`, team paths adjusted), runs them all and writes a
Markdown table of the values per lead pair (`leads/<scenario>.<solve>.<rolls>.md`), rows = our
leads, columns = theirs. Values are averaged over start positions weighted by probability.
"""
import argparse
import itertools
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
    deep = re.search(r"^\s*1\s+([+-][0-9.]+)\s+([+-][0-9.]+)\s+(.*?)\s{2,}(.*)$", text, re.M)
    if deep and "deep:" in text:
        entry["deep_value"] = float(deep.group(1))
        entry["deep_shallow"] = float(deep.group(2))
        entry["deep_line"] = deep.group(3).strip()
        entry["deep_reply"] = deep.group(4).strip()
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


def run_scenario(a, sc, out_dir, extra, summary):
    """Runs one scenario file for every start position; appends entries to `summary`."""
    base = [str(sc), "--side", a.side, "--rolls", a.rolls, "--solve", a.solve, "--top", a.top] + extra
    rc, text, _ = run(a.exe, base, a.timeout)
    positions = [None]
    listing = ""
    m = re.search(r"(\d+) initial states", text)
    if m:
        positions = list(range(int(m.group(1))))
        listing = text
    entries = []
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
        entries.append(entry)
        shown = {k: v for k, v in entry.items() if k not in ("our_strategy", "their_strategy", "unsupported")}
        print(json.dumps(shown, ensure_ascii=False))
    return entries


def weighted_value(entries, key):
    """The value averaged over start positions (by probability; equal weights without one)."""
    vals = [(e.get("position_probability", 1.0), e[key]) for e in entries if key in e]
    if not vals:
        return None
    total = sum(p for p, _ in vals)
    return sum(p * v for p, v in vals) / total if total else None


def team_names(scenario_path, spec):
    team = spec["team"]
    path = (scenario_path.parent / team).resolve()
    sets = json.load(open(path, encoding="utf-8"))
    return [s["species"] for s in sets]


def lead_variants(a, sc, out_dir, extra, summary):
    """All 6 x 6 lead pairs of the four brought members on both sides."""
    scenario = json.load(open(sc, encoding="utf-8"))
    leads_dir = sc.parent / "leads"
    leads_dir.mkdir(exist_ok=True)
    names = {side: team_names(sc, scenario[side]) for side in ("p1", "p2")}
    orders = {side: scenario[side]["order"] for side in ("p1", "p2")}
    for side in ("p1", "p2"):
        if len(orders[side]) != 4:
            raise SystemExit(f"{sc.name}: {side} order {orders[side]!r} must pick exactly 4 members")

    def variants(order):
        brought = list(order)
        out = []
        for lead in itertools.combinations(brought, 2):
            back = [c for c in brought if c not in lead]
            out.append("".join(lead) + "".join(back))
        return out

    def label(side, order):
        return " + ".join(names[side][int(c) - 1] for c in order[:2])

    p1_orders = variants(orders["p1"])
    p2_orders = variants(orders["p2"])
    key = {"nash": "equilibrium", "deep": "deep_value"}.get(a.solve, "best_value")
    table = {}
    for o1 in p1_orders:
        for o2 in p2_orders:
            variant = dict(scenario)
            variant["p1"] = dict(scenario["p1"], order=o1)
            variant["p2"] = dict(scenario["p2"], order=o2)
            for side in ("p1", "p2"):
                team = variant[side]["team"]
                if not pathlib.Path(team).is_absolute():
                    variant[side]["team"] = "../" + team
            variant.pop("turn", None)
            variant["description"] = f"{scenario.get('description', '')} [leads p1 {o1[:2]} / p2 {o2[:2]}]"
            vpath = leads_dir / f"{sc.stem}.{o1}-{o2}.json"
            json.dump(variant, open(vpath, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
            entries = run_scenario(a, vpath, out_dir, extra, summary)
            table[(o1, o2)] = weighted_value(entries, key)
    lines = [f"# {sc.stem}: {key} by lead pair ({a.solve}, {a.rolls}); rows = p1 leads, columns = p2 leads", ""]
    header = "| p1 \\ p2 | " + " | ".join(label("p2", o2) for o2 in p2_orders) + " |"
    lines.append(header)
    lines.append("|---|" + "---|" * len(p2_orders))
    for o1 in p1_orders:
        cells = []
        for o2 in p2_orders:
            v = table[(o1, o2)]
            cells.append("—" if v is None else f"{v:+.1f}")
        lines.append(f"| {label('p1', o1)} | " + " | ".join(cells) + " |")
    # Row minima: the lead pair's value against its worst opposing lead (what a lead choice
    # guarantees if the opponent could see it), and the best row by that measure.
    lines.append("")
    lines.append("| p1 leads | min over p2 leads | mean |")
    lines.append("|---|---|---|")
    for o1 in p1_orders:
        vals = [table[(o1, o2)] for o2 in p2_orders if table[(o1, o2)] is not None]
        if vals:
            lines.append(f"| {label('p1', o1)} | {min(vals):+.1f} | {sum(vals) / len(vals):+.1f} |")
    md = leads_dir / f"{sc.stem}.{a.solve}.{a.rolls}.md"
    md.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("wrote", md)


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
    ap.add_argument("--leads", default="fixed", choices=["fixed", "all"])
    ap.add_argument("--only", default="", help="substring filter on scenario file names")
    a = ap.parse_args()
    run_dir = pathlib.Path(a.run_dir)
    out_dir = run_dir / "out"
    out_dir.mkdir(exist_ok=True)
    extra = a.extra.split() if a.extra else []
    summary = []
    for sc in sorted(run_dir.glob("*-vs-*.json")):
        if a.only and a.only not in sc.name:
            continue
        if a.leads == "all":
            lead_variants(a, sc, out_dir, extra, summary)
        else:
            run_scenario(a, sc, out_dir, extra, summary)
    suffix = ".leads" if a.leads == "all" else ""
    path = run_dir / f"summary{suffix}.{a.solve}.{a.rolls}.json"
    json.dump(summary, open(path, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print("wrote", path)


if __name__ == "__main__":
    main()
