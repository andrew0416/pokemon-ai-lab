"""Public Showdown replays as a parity source (board V13-replay-parity): collect.

Usage:
  python engine/scripts/replay_fetch.py <run-dir> [--format gen9championsvgc2026regmc]
                                        [--count 200] [--delay 1.0]

Lists the format's public replays newest first through replay.pokemonshowdown.com's
`search.json?format=<f>[&page=<n>]` (51 per page, 50 new; a full page means there is
another; uploads during the run shift the pages, which only repeats replays, deduplicated by id), then downloads each replay's `<id>.json` (its `log` is the battle's spectator log).
Every request waits at least `--delay` seconds after the previous one. Private and password
replays are skipped.

No personal data is kept: the player names are replaced by `p1`/`p2` everywhere in the stored
log (and the lines that only carry names or chat — `|j|`, `|l|`, `|n|`, `|c|`, `|raw|`,
`|inactive|`, `|inactiveoff|`, `|t:|` — are dropped). The replay id and the URL stand for the game.

Writes `<run-dir>/replays/<id>.log` (the sanitized log) and `<run-dir>/replays.json`:
{"collected": <UTC ISO time>, "format", "api", "replays": [{"id", "url", "uploadtime",
"uploaded" (UTC ISO), "rating", "log": "replays/<id>.log", "fetched"}]}. Re-running skips the
replays already stored (`--count` counts them).
"""
import argparse
import datetime
import json
import pathlib
import re
import sys
import time
import urllib.request

API = "https://replay.pokemonshowdown.com"
DROP = ("|j|", "|J|", "|l|", "|L|", "|n|", "|N|", "|c|", "|c:|", "|raw|", "|inactive|", "|inactiveoff|", "|t:|", "|html|", "|uhtml|")
_last = [0.0]


def get(url, delay):
    wait = _last[0] + delay - time.monotonic()
    if wait > 0:
        time.sleep(wait)
    req = urllib.request.Request(url, headers={"User-Agent": "pokemon-ai-lab parity research (replay_fetch.py)"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.loads(r.read().decode("utf-8"))
    finally:
        _last[0] = time.monotonic()


def iso(ts):
    return datetime.datetime.fromtimestamp(ts, datetime.timezone.utc).isoformat()


def sanitize(log, players):
    names = [n for n in players if n]
    out = []
    for line in log.split("\n"):
        if line.startswith(DROP):
            continue
        if line.startswith("|player|"):
            parts = line.split("|")
            # |player|p1|name|avatar|rating -> |player|p1|p1
            line = "|".join(parts[:3] + ([parts[2]] if len(parts) > 3 else []))
        for i, name in enumerate(names):
            if name:
                line = re.sub(re.escape(name), f"p{i + 1}", line, flags=re.IGNORECASE)
                ident = re.sub(r"[^a-z0-9]", "", name.lower())
                if ident and len(ident) >= 3:
                    line = re.sub(rf"\b{re.escape(ident)}\b", f"p{i + 1}", line)
        out.append(line)
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--format", default="gen9championsvgc2026regmc")
    ap.add_argument("--count", type=int, default=200)
    ap.add_argument("--delay", type=float, default=1.0)
    args = ap.parse_args()
    run = pathlib.Path(args.run_dir)
    (run / "replays").mkdir(parents=True, exist_ok=True)
    index_path = run / "replays.json"
    index = json.loads(index_path.read_text(encoding="utf-8")) if index_path.exists() else {
        "collected": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "format": args.format, "api": f"{API}/search.json?format={args.format}[&page=<n>]",
        "replays": []}
    have = {r["id"] for r in index["replays"]}
    page_no = 1
    while len(index["replays"]) < args.count:
        url = f"{API}/search.json?format={args.format}" + (f"&page={page_no}" if page_no > 1 else "")
        page = get(url, args.delay)
        if not page:
            break
        for entry in page[:50]:
            if len(index["replays"]) >= args.count:
                break
            if entry.get("private") or entry.get("password") or entry["id"] in have:
                continue
            rid = entry["id"]
            try:
                data = get(f"{API}/{rid}.json", args.delay)
            except Exception as e:  # noqa: BLE001 - recorded, not fatal
                print(f"{rid}: {e}", file=sys.stderr)
                continue
            (run / "replays" / f"{rid}.log").write_text(sanitize(data["log"], data.get("players", [])), encoding="utf-8")
            index["replays"].append({
                "id": rid, "url": f"{API}/{rid}", "uploadtime": data.get("uploadtime", entry["uploadtime"]),
                "uploaded": iso(data.get("uploadtime", entry["uploadtime"])), "rating": entry.get("rating"),
                "log": f"replays/{rid}.log",
                "fetched": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")})
            have.add(rid)
            index_path.write_text(json.dumps(index, indent=1), encoding="utf-8")
            print(f"{len(index['replays'])}/{args.count} {rid}", flush=True)
        if len(page) < 51:
            break
        page_no += 1
    index_path.write_text(json.dumps(index, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
