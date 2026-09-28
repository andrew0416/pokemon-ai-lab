"""Mismatch attribution by feature lift (V14-diverge-lift): which effects the positions that did not
match have in common, as lift = P(feature | mismatch) / P(feature | match).

Usage:
  python engine/scripts/parity_lift.py <corpus-dir> [--statuses <summary.json> ...] [--source-status]
                                       [--min-mismatch 1] [--top 40] [--out <dir>]

The features of a position are the ones `parity_features.py` counts, per position: every
status, volatile, side and slot condition, weather, terrain, pseudo-weather, ability and item of
an active Pokémon, stat stage, fainted member and other state in the decision state or any
outcome state of its oracle report(s), plus the chosen moves, decision kinds and Mega Evolution of
the compared decision (`parity_features.owner_features` / `choice_features`). A feature's lift is
its share among mismatched positions over its share among matched ones, with 0.5 added to both
counts (a feature on no matched position gets a finite lift, and one mismatch does not give an
infinite one). A high lift with a few mismatches is a lead, not a cause: every feature of a
mismatched position gets its count, and effects that always come together get the same lift.

The positions' statuses come from `parity_corpus.py check` or `parity_sweep.py` summaries
(`--statuses`, several allowed: a later file's row replaces an earlier one; corpus rows by `id`,
sweep rows by scenario stem, matched against the corpus's stems), or with `--source-status` from
the corpus index itself (each position's status in the sweep it was taken from: the verdicts of
the engine as it was then, before the fixes — the "old reports" check of this tool). Only `match`
and `mismatch` count.

`parity_corpus.py check` and `parity_sweep.py` call `lift()` / `markdown()` themselves and append
a "Mismatch attribution (lift)" section to their summaries when there are both matches and
mismatches.

Output: `<out>/lift.md` and `<out>/lift.json` (`--out` defaults to the corpus directory).
"""
import argparse
import collections
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))


def feature_name(key):
    category, feature = key
    return f"{category}: {feature}"


def position_features(scenario, reports):
    """The feature names of one position: its decision state and every outcome state of `reports`
    (oracle reports as dicts), and the decision's choices (`scenario` as a dict)."""
    import parity_features  # imports parity_corpus: loaded lazily (parity_corpus imports this module)

    keys = set(parity_features.choice_features(scenario))
    for report in reports:
        keys |= set(parity_features.owner_features(report["before"]))
        for outcome in report.get("outcomes") or []:
            keys |= set(parity_features.owner_features(outcome["state"]))
    return sorted(feature_name(k) for k in keys)


def lift(rows, min_mismatch=1):
    """`rows`: dicts with `status` and `features` (names). Returns the lift table (features seen
    on at least `min_mismatch` mismatched positions, highest lift first) and the totals."""
    compared = [r for r in rows if r.get("status") in ("match", "mismatch") and r.get("features") is not None]
    mismatched = [r for r in compared if r["status"] == "mismatch"]
    matched = [r for r in compared if r["status"] == "match"]
    in_mm = collections.Counter(f for r in mismatched for f in set(r["features"]))
    in_m = collections.Counter(f for r in matched for f in set(r["features"]))
    table = []
    for feature, a in in_mm.items():
        if a < min_mismatch:
            continue
        b = in_m.get(feature, 0)
        p_mm = (a + 0.5) / (len(mismatched) + 1)
        p_m = (b + 0.5) / (len(matched) + 1)
        table.append({"feature": feature, "mismatch": a, "match": b,
                      "p_mismatch": round(a / len(mismatched), 4),
                      "p_match": round(b / len(matched), 4) if matched else None,
                      "lift": round(p_mm / p_m, 3),
                      "positions": sorted(r.get("id") or r.get("scenario", "?") for r in mismatched
                                         if feature in r["features"])[:5]})
    table.sort(key=lambda t: (-t["lift"], -t["mismatch"], t["feature"]))
    return {"mismatched": len(mismatched), "matched": len(matched), "table": table}


def markdown(result, top=40):
    """Lines of a markdown section for `lift()`'s result."""
    lines = ["## Mismatch attribution (lift)", "",
             f"lift = P(feature | mismatch) / P(feature | match) over {result['mismatched']} mismatched and "
             f"{result['matched']} matched positions (counts +0.5; `engine/scripts/parity_lift.py`). "
             "Features that always come together share a lift; a lead, not a cause.", ""]
    if not result["mismatched"] or not result["matched"]:
        lines += ["(needs both matched and mismatched positions)", ""]
        return lines
    lines += ["| feature | mismatched | matched | P(f\\|mismatch) | P(f\\|match) | lift | mismatched positions |",
              "|---|---|---|---|---|---|---|"]
    for t in result["table"][:top]:
        lines.append(f"| {t['feature'].replace('|', '/')} | {t['mismatch']} | {t['match']} | {t['p_mismatch']:.3f} | "
                     f"{t['p_match']:.3f} | {t['lift']:.2f} | {', '.join(t['positions'])} |")
    if len(result["table"]) > top:
        lines.append(f"| ({len(result['table']) - top} more in lift.json) | | | | | | |")
    lines.append("")
    return lines


def corpus_features(corpus, entries):
    """{id: feature names} of built corpus positions, from their stored scenario and reports."""
    import parity_corpus

    out = {}
    for e in entries:
        if e.get("build") != "ok":
            continue
        scenario = json.loads(parity_corpus.gz_read(corpus / e["scenario"]))
        reports = [json.loads(parity_corpus.gz_read(corpus / r["file"])) for r in e["reports"]]
        out[e["id"]] = position_features(scenario, reports)
    return out


def corpus_lift(corpus, rows, min_mismatch=1):
    """`lift()` of `parity_corpus.py check` rows (`id`, `status`) over the corpus's positions."""
    index = json.loads((corpus / "corpus.json").read_text(encoding="utf-8"))
    ids = {r["id"] for r in rows}
    features = corpus_features(corpus, [e for e in index["positions"] if e["id"] in ids])
    return lift([dict(r, features=features.get(r["id"])) for r in rows], min_mismatch)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("corpus", type=pathlib.Path)
    ap.add_argument("--statuses", type=pathlib.Path, action="append", default=[])
    ap.add_argument("--source-status", action="store_true",
                    help="each position's status in the sweep it came from (corpus.json)")
    ap.add_argument("--min-mismatch", type=int, default=1)
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--out", type=pathlib.Path)
    args = ap.parse_args()
    index = json.loads((args.corpus / "corpus.json").read_text(encoding="utf-8"))
    entries = [e for e in index["positions"] if e.get("build") == "ok"]
    status = {}
    if args.source_status:
        for e in entries:
            status[e["id"]] = (e.get("source") or {}).get("status")
    by_stem = collections.defaultdict(list)
    for e in entries:
        by_stem[e["stem"]].append(e["id"])
    for path in args.statuses:
        summary = json.loads(path.read_text(encoding="utf-8"))
        for r in summary["rows"]:
            if "id" in r:
                status[r["id"]] = r["status"]
            else:
                for i in by_stem.get(r["scenario"][:-5], []):
                    status[i] = r["status"]
    if not status:
        ap.error("no statuses: give --statuses and/or --source-status")
    rows = [{"id": i, "status": s} for i, s in status.items() if s in ("match", "mismatch")]
    features = corpus_features(args.corpus, [e for e in entries if e["id"] in {r["id"] for r in rows}])
    for r in rows:
        r["features"] = features.get(r["id"])
    result = lift(rows, args.min_mismatch)
    result.update(corpus=str(args.corpus).replace("\\", "/"), statuses=[str(p).replace("\\", "/") for p in args.statuses],
                  source_status=args.source_status,
                  mismatched_positions=sorted(r["id"] for r in rows if r["status"] == "mismatch"))
    out = args.out or args.corpus
    out.mkdir(parents=True, exist_ok=True)
    (out / "lift.json").write_text(json.dumps(result, indent=1, ensure_ascii=False), encoding="utf-8")
    lines = ["# Parity mismatch attribution", "",
             f"Corpus `{result['corpus']}`; statuses: "
             + ", ".join(([f"`{s}`" for s in result["statuses"]]) + (["the sources' sweeps"] if args.source_status else []))
             + ".", "", "Mismatched: " + ", ".join(result["mismatched_positions"]) + ".", ""]
    lines += markdown(result, args.top)
    (out / "lift.md").write_text("\n".join(lines), encoding="utf-8")
    print(json.dumps({k: result[k] for k in ("mismatched", "matched")}))
    for t in result["table"][:10]:
        print(f"{t['lift']:8.2f}  {t['mismatch']:3d}/{result['mismatched']}  {t['match']:5d}/{result['matched']}  {t['feature']}")
    print(f"written {out / 'lift.md'} and {out / 'lift.json'}")


if __name__ == "__main__":
    main()
