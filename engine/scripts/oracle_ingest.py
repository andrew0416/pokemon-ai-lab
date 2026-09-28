"""Collects the oracle workflow's per-position verdicts from GitHub (annotations, no artifacts).

    python engine/scripts/oracle_ingest.py <run_id> [<run_id> ...] --out runs/parity-actions-20260928

Each `oracle (...)` job of the runner workflow (.github/workflows/oracle.yml) leaves one `::notice`
whose message is the row `job;position;shard;oracle;oracle s;oracle MB;runs;outcomes;frontier;
factored;flat;TV;engine outcomes;check s` (oracle_job.py `emit`). This reads those rows through
the check-runs API (`GITHUB_TOKEN` from the environment or D:/tools/gh-token.txt), so a run that
is still in progress yields its finished positions. Jobs without a notice (killed runner,
cancelled) are listed as `runner-failed` with the job conclusion.

Writes <out>/<run_id>.rows.json and <out>/<run_id>.summary.md, and prints the counts.
"""
import json
import os
import pathlib
import sys
import urllib.request

REPO = "andrew0416/pokemon-ai-lab"
FIELDS = ["job", "position", "shard", "oracle", "oracle_s", "oracle_mb", "runs", "outcomes",
          "frontier", "factored", "flat", "tv", "engine_outcomes", "check_s"]


def token() -> str:
    t = os.environ.get("GITHUB_TOKEN")
    if not t:
        p = pathlib.Path("D:/tools/gh-token.txt")
        t = p.read_text(encoding="utf-8").strip() if p.exists() else ""
    return t


def api(url: str):
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token()}",
                                               "Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def jobs(run_id: str) -> list:
    out, page = [], 1
    while True:
        d = api(f"https://api.github.com/repos/{REPO}/actions/runs/{run_id}/jobs?per_page=100&page={page}")
        out += d["jobs"]
        if len(out) >= d["total_count"] or not d["jobs"]:
            return out
        page += 1


KEYS = {"position": "position", "shard": "shard", "oracle": "oracle", "oracle s": "oracle_s",
        "oracle MB": "oracle_mb", "runs": "runs", "outcomes": "outcomes", "frontier": "frontier",
        "factored": "factored", "flat": "flat", "TV": "tv", "engine outcomes": "engine_outcomes",
        "check s": "check_s"}


def parse_row(msg: str) -> dict | None:
    """The job notice: `position=... | oracle=fit | oracle s=... | ... | check s=...` (one line;
    an unfinished run appends progress lines after it)."""
    line = msg.splitlines()[0] if msg else ""
    if "position=" not in line:
        return None
    row = {k: "" for k in FIELDS}
    for part in line.split(" | "):
        if "=" in part:
            k, v = part.split("=", 1)
            if k.strip() in KEYS:
                row[KEYS[k.strip()]] = v.strip()
    return row


def ingest(run_id: str) -> list[dict]:
    rows = []
    for j in jobs(run_id):
        if not j["name"].startswith("oracle ("):
            continue
        row = None
        if j["status"] == "completed":
            for a in api(f"https://api.github.com/repos/{REPO}/check-runs/{j['id']}/annotations?per_page=100"):
                if a.get("annotation_level") == "notice":
                    row = parse_row(a.get("message", ""))
                    if row:
                        break
        if row is None:
            name = j["name"][len("oracle ("):-1]
            parts = [p.strip() for p in name.split(",")]
            row = {k: "" for k in FIELDS}
            row.update(job=parts[0], position=parts[1] if len(parts) > 1 else "",
                       shard=parts[2] if len(parts) > 2 else "",
                       oracle=("runner-failed:" + str(j["conclusion"])) if j["status"] == "completed" else j["status"])
        if not row.get("job"):
            row["job"] = j["name"][len("oracle ("):-1].split(",")[0].strip()
        row["run_id"] = run_id
        row["job_id"] = j["id"]
        rows.append(row)
    return rows


def summary(rows: list[dict]) -> str:
    counts = {}
    for r in rows:
        key = r["oracle"] if r["oracle"] != "fit" else f"fit {r['factored'] or '?'}/{r['flat'] or '?'}"
        counts[key] = counts.get(key, 0) + 1
    lines = ["| position | shard | oracle | oracle s | MB | runs | outcomes | factored | flat | TV | engine outcomes |",
             "|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (r["position"], r["shard"])):
        lines.append(f"| {r['position']} | {r['shard']} | {r['oracle']} | {r['oracle_s']} | {r['oracle_mb']} | "
                     f"{r['runs']} | {r['outcomes']} | {r['factored']} | {r['flat']} | {r['tv']} | {r['engine_outcomes']} |")
    return "counts: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())) + "\n\n" + "\n".join(lines) + "\n"


if __name__ == "__main__":
    args = sys.argv[1:]
    out = pathlib.Path(args[args.index("--out") + 1]) if "--out" in args else pathlib.Path(".")
    run_ids = [a for a in args if a.isdigit()]
    out.mkdir(parents=True, exist_ok=True)
    for run_id in run_ids:
        rows = ingest(run_id)
        (out / f"{run_id}.rows.json").write_text(json.dumps(rows, indent=1, ensure_ascii=False), encoding="utf-8")
        text = summary(rows)
        (out / f"{run_id}.summary.md").write_text(f"# oracle run {run_id}\n\n{text}", encoding="utf-8")
        print(run_id, text.splitlines()[0])
