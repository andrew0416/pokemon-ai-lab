"""Public Showdown replays as a parity source (board V13-replay-parity): tables and oracle jobs.

Usage:
  python engine/scripts/replay_summary.py stats <run-dir>
      <run-dir>/checks/*.replay.json (lab-replay) -> <run-dir>/summary.json, summary.md:
      replay coverage, why replays stop, the observation check (model ② consistency) and its
      first structural differences by kind, the candidate list.
  python engine/scripts/replay_summary.py lists <run-dir> [--chunk 250]
      the pinned positions (<run-dir>/positions) as list files <run-dir>/pack/list-<k>.txt for
      `oracle_job.py pack` (a workflow matrix holds at most 256 runners).
  python engine/scripts/replay_summary.py ingest <run-dir> <actions-run-id>... [--repo owner/name]
      reads the oracle workflow runs' summary table annotations (a few API requests; GITHUB_TOKEN
      from the environment when set)
      and writes one parity_sweep-style row per position to <run-dir>/rows/, the totals to
      <run-dir>/oracle.json, and <run-dir>/sources.json in parity_corpus.py's format.
"""
import argparse
import collections
import json
import os
import pathlib
import re
import sys
import time
import urllib.request

NAME = re.compile(r"^(?P<game>.+)\.s(?P<step>\d+)$")


def load_checks(run):
    out = []
    for f in sorted((run / "checks").glob("*.replay.json")):
        out.append(json.loads(f.read_text(encoding="utf-8")))
    return out


def kind_of(diff):
    d = re.sub(r"\(.*?\)", "", diff)
    m = re.match(r"^(p[12]):.*? (faint|status|slot|forme|boosts|item) ", d)
    if m:
        return m.group(2)
    return d.split(" engine")[0].replace("p1 ", "").replace("p2 ", "")


def stats(run):
    checks = load_checks(run)
    games = json.loads((run / "games" / "index.json").read_text(encoding="utf-8"))
    limits = []
    log = run / "replay.log"
    if log.exists():
        limits = [line for line in log.read_text(encoding="utf-8").splitlines() if ": limit: " in line]
    decisions = sum(c["decisions"] for c in checks)
    replayed = sum(c["replayed"] for c in checks)
    consistent_prefix = sum(c["divergedAt"] if c["divergedAt"] is not None else c["replayed"] for c in checks)
    consistent_all = sum(c["consistent"] for c in checks)
    full_games = sum(1 for c in checks if c["divergedAt"] is None and c["replayed"] == c["decisions"])
    stops = collections.Counter()
    for c in checks:
        if c["stop"]:
            s = re.sub(r"^decision \d+ \(turn \d+ (\w+)\): ", r"\1: ", c["stop"])
            s = re.sub(r"actions \[.*", "actions", s)
            stops[s] += 1
    first = collections.Counter()
    after_fit = []
    for c in checks:
        k = c["divergedAt"]
        if k is None:
            continue
        chk = c["checks"][k]
        kinds = sorted({kind_of(d) for d in chk["structural"]})
        for kd in kinds:
            first[kd] += 1
        after_fit.append({"id": c["id"], "url": c["url"], "ots": c["ots"], "step": k, "turn": chk["turn"],
                          "kind": chk["kind"], "choices": chk["choices"], "tier": chk["tier"],
                          "structural": chk["structural"], "hpDistance": chk["hpDistance"],
                          "spFit": c.get("spFit", {}).get("changes", [])})
    tiers = collections.Counter(ch["tier"] for c in checks for ch in c["checks"])
    truncated = sum(1 for c in checks for ch in c["checks"] if ch.get("truncated"))
    summary = {
        "replays": len(games), "games": len(checks),
        "ots_games": sum(1 for c in checks if c["ots"]),
        "decisions": decisions, "replayed": replayed, "positions": sum(len(c["positions"]) for c in checks),
        "consistent_prefix": consistent_prefix, "consistent_all": consistent_all,
        "games_fully_consistent": full_games, "stops": stops.most_common(), "limits": limits,
        "first_divergence_kinds": first.most_common(), "tiers": dict(tiers), "truncated_decisions": truncated,
        "candidates": after_fit,
        "parse_stops": collections.Counter((g.get("stop") or {}).get("reason", "") for g in games if g.get("stop")).most_common(),
        "unknown_actions": sum(g.get("unknown_actions", 0) for g in games),
        "mid_switches": sum(g.get("mid_switches", 0) for g in games),
    }
    (run / "summary.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False), encoding="utf-8")
    lines = [
        f"- replays {summary['replays']}, games replayed {summary['games']} (open team sheets {summary['ots_games']})",
        f"- decisions {decisions}, replayed {replayed}, pinned positions {summary['positions']}",
        f"- structurally consistent: {consistent_prefix} decisions before the first divergence, "
        f"{consistent_all} in all; games consistent to the end {full_games}",
        f"- decisions needing relaxed targets (tier 2) {tiers.get(2, 0)}; choice pairs truncated {truncated}",
        "", "| replay stops at | games |", "|---|---|"]
    lines += [f"| {k} | {v} |" for k, v in stops.most_common()]
    lines += ["", "| first divergence: difference kind | games |", "|---|---|"]
    lines += [f"| {k} | {v} |" for k, v in first.most_common()]
    (run / "summary.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))


def lists(run, chunk):
    names = sorted(p for p in (run / "positions").glob("*.json"))
    out = run / "pack"
    out.mkdir(exist_ok=True)
    for k in range(0, len(names), chunk):
        part = names[k:k + chunk]
        f = out / f"list-{k // chunk + 1}.txt"
        f.write_text("\n".join(str(p).replace("\\", "/") for p in part) + "\n", encoding="utf-8")
        print(f"{f}: {len(part)} positions")


def api(url):
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "pokemon-ai-lab replay_summary.py"}
    token = os.environ.get("GITHUB_TOKEN")  # optional (5,000 requests/h instead of 60); never printed
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode("utf-8"))


def ingest(run, run_ids, repo):
    rows_dir = run / "rows"
    rows_dir.mkdir(exist_ok=True)
    table = []
    for rid in run_ids:
        jobs = []
        page = 1
        while True:
            data = api(f"https://api.github.com/repos/{repo}/actions/runs/{rid}/jobs?per_page=100&page={page}")
            jobs += data["jobs"]
            if len(data["jobs"]) < 100:
                break
            page += 1
            time.sleep(1)
        summary_job = next((j for j in jobs if j["name"].startswith("summary")), None)
        if not summary_job:
            sys.exit(f"run {rid}: no summary job yet")
        notes = api(f"https://api.github.com/repos/{repo}/check-runs/{summary_job['id']}/annotations?per_page=100")
        for n in notes:
            title = n.get("title") or ""
            if not title.startswith("oracle table"):
                continue
            cols = re.search(r"\((.*)\)", title).group(1).split(";")
            for line in n["message"].split("\n"):
                vals = line.split(";")
                if len(vals) == len(cols):
                    table.append(dict(zip(cols, vals), run=rid))
    counts = collections.Counter()
    for t in table:
        m = NAME.match(t["position"])
        status = t["factored"] or t["oracle"]
        if t["factored"] and t["flat"] and t["factored"] != t["flat"]:
            status = f"factored {t['factored']} / flat {t['flat']}"
        counts[status] += 1
        scenario = json.loads((run / "positions" / f"{t['position']}.json").read_text(encoding="utf-8"))
        desc = re.search(r"decision (\d+) = turn (\d+) (\w+)", scenario["description"])
        row = {"scenario": f"{t['position']}.json", "game_id": m.group("game") if m else t["position"],
               "matchup": "replay", "policy": "replay", "step": int(m.group("step")) if m else 0,
               "turn": int(desc.group(2)) if desc else None, "kind": desc.group(3) if desc else None,
               "setup_turns": len(scenario.get("setupTurns", [])), "mode": "full", "staged": True,
               "oracle_outcomes": int(t["outcomes"]) if t["outcomes"].isdigit() else None,
               "status": t["factored"] if t["factored"] else f"oracle-{t['oracle']}",
               "flat": t["flat"], "tv": float(t["TV"]) if t["TV"] else None,
               "engineOutcomes": int(t["engine outcomes"]) if t["engine outcomes"].isdigit() else None,
               "oracle_s": t["oracle s"], "actions_run": t["run"], "job": t.get("job")}
        (rows_dir / f"{t['position']}.json").write_text(json.dumps(row, indent=1), encoding="utf-8")
    (run / "oracle.json").write_text(json.dumps({"runs": run_ids, "positions": len(table),
                                                  "statuses": counts.most_common()}, indent=1), encoding="utf-8")
    replays = json.loads((run / "replays.json").read_text(encoding="utf-8"))
    sources = {
        "sources": [{"set": "replays-20260928", "positions": "positions", "rows": "rows",
                     "engine": "see oracle.json runs (the opus-vc commit each Actions run checked)",
                     "run": f"runs/parity-replays-20260928 (Opus VC, V13): public Showdown replays of "
                            f"{replays['format']}, teams rebuilt from the logs, positions pinned by lab-replay, "
                            f"oracle on GitHub Actions runs {', '.join(map(str, run_ids))}"}],
        "collected": replays["collected"], "api": replays["api"],
        "replays": [{k: r[k] for k in ("id", "url", "uploadtime", "uploaded", "fetched")} for r in replays["replays"]],
    }
    (run / "sources.json").write_text(json.dumps(sources, indent=1), encoding="utf-8")
    print(f"{len(table)} positions: {counts.most_common()}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["stats", "lists", "ingest"])
    ap.add_argument("run_dir")
    ap.add_argument("run_ids", nargs="*")
    ap.add_argument("--chunk", type=int, default=250)
    ap.add_argument("--repo", default="andrew0416/pokemon-ai-lab")
    a = ap.parse_args()
    run = pathlib.Path(a.run_dir)
    if a.cmd == "stats":
        stats(run)
    elif a.cmd == "lists":
        lists(run, a.chunk)
    else:
        ingest(run, a.run_ids, a.repo)


if __name__ == "__main__":
    main()
