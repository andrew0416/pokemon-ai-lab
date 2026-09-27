"""Feature coverage of the parity corpus (V2c-feature-coverage-report): which statuses, volatiles,
side and slot conditions, weathers, terrains, pseudo-weathers, abilities and items of the active
Pokémon, chosen moves, decision kinds, Mega Evolution, fainted members and stat stages the
compared positions actually contained, and which of the Champions standard range never appeared.

Usage: python engine/scripts/parity_features.py <corpus-dir> [--rare 5] [--out <dir>]

Reads `<corpus-dir>/corpus.json` (engine/scripts/parity_corpus.py) and, per built position, its
scenario (the decision's choices) and every oracle report: the decision state (`before`) and
every outcome state. A feature is counted once per position, in these columns:
- `positions`: it is in the decision state or in at least one outcome state;
- `before`: it is in the decision state (the engine had to carry it into the compared turn);
- `new`: an outcome has it where the decision state did not (for the same Pokémon, side or slot;
  the compared turn created it);
- `full`: `positions` restricted to positions compared under the exact 16-roll distribution
  (the others were compared with min/max rolls or fixed rolls only).
Chosen moves, decision kinds and Mega Evolution come from the scenario's `turn` (and `midTurn`)
choices; moves chosen in pinned setup turns that are not compared positions themselves (the lead
turns; the engine had to reach the recorded outcome there, a weaker check) are counted apart. Effects that
start and end inside a turn reach a canonical state only when a turn pauses, so never-seen
conditions are listed with the counts of the move of the same id. The universes the never-seen lists are drawn from: `engine/data/champions.json` entries
with `isNonstandard` null (moves, abilities, items); the engine's volatile ids (`Volatile::id`
in `engine/core/src/volatile.rs`; the kinds `Volatile::showdown_state` leaves out of canonical
states are listed apart, as no position can show them); side conditions, pseudo-weathers,
weathers and terrains as `engine/scenario/src/canonical.rs` writes them; slot conditions from
`engine/core/src/field.rs`.

A feature seen in compared positions is evidence that the engine handles it there, not proof: the
positions are what these self-play games produced, one pinned history each.

Output: `<out>/features.md` and `<out>/features.json` (`--out` defaults to the corpus directory).
"""
import argparse
import collections
import gzip
import json
import pathlib
import re
import sys

import parity_corpus

HERE = pathlib.Path(__file__).resolve().parent
ENGINE = HERE.parent
STATUSES = ("brn", "par", "slp", "frz", "psn", "tox")
STATS = ("atk", "def", "spa", "spd", "spe", "accuracy", "evasion")
CATEGORIES = [
    ("status", "Non-volatile status (any member)"),
    ("volatile", "Volatiles of active Pokémon"),
    ("side_condition", "Side conditions"),
    ("slot_condition", "Slot conditions"),
    ("weather", "Weather"),
    ("terrain", "Terrain"),
    ("pseudo_weather", "Pseudo-weathers"),
    ("ability", "Abilities of active Pokémon"),
    ("item", "Held items of active Pokémon"),
    ("last_item", "Consumed / lost items (`lastItem`) of any member"),
    ("move", "Moves chosen in the compared decision"),
    ("decision", "Decision kinds"),
    ("mega", "Mega Evolution"),
    ("fainted", "Fainted members"),
    ("boost", "Stat stages away from 0 (active Pokémon)"),
    ("state", "Other state"),
    ("species", "Species of active Pokémon"),
]


def read_gz_json(path):
    return json.loads(gzip.decompress(pathlib.Path(path).read_bytes()))


# ---------------------------------------------------------------------------------------------
# universes


def rust_pairs(path, pattern):
    return re.findall(pattern, pathlib.Path(path).read_text(encoding="utf-8"))


def universes():
    dex = json.loads((ENGINE / "data" / "champions.json").read_text(encoding="utf-8"))
    standard = {k: sorted(v["id"] for v in dex[k].values() if v.get("isNonstandard") is None)
                for k in ("moves", "abilities", "items")}
    names = {v["id"]: v["name"] for k in ("moves", "abilities", "items") for v in dex[k].values()}
    volatile_rs = (ENGINE / "core" / "src" / "volatile.rs").read_text(encoding="utf-8")
    body = volatile_rs[volatile_rs.index("pub fn id(self) -> &'static str"):]
    body = body[:body.index("\n    }\n")]
    volatile_ids = dict(re.findall(r"Volatile::(\w+) => \"(\w+)\"", body))
    hidden_src = volatile_rs[volatile_rs.index("pub fn showdown_state"):]
    hidden_src = hidden_src[:hidden_src.index("=> None")]
    hidden = sorted(volatile_ids[v] for v in re.findall(r"Volatile::(\w+)", hidden_src))
    canonical_rs = ENGINE / "scenario" / "src" / "canonical.rs"
    side = sorted(i for _, i in rust_pairs(canonical_rs, r"\(SideEffect::(\w+), \"(\w+)\"\)"))
    pseudo = sorted(i for _, i in rust_pairs(canonical_rs, r"\(FieldEffect::(\w+), \"(\w+)\"\)"))
    weather = sorted(i for _, i in rust_pairs(canonical_rs, r"Weather::(\w+) as u8 => \"(\w+)\""))
    terrain = sorted(i for _, i in rust_pairs(canonical_rs, r"Terrain::(\w+) as u8 => \"(\w+)\""))
    slot = sorted(i for _, i in rust_pairs(ENGINE / "core" / "src" / "field.rs", r"SlotCondition::(\w+) => \"(\w+)\""))
    for what, ids in (("volatiles", volatile_ids), ("side conditions", side), ("pseudo-weathers", pseudo),
                      ("weathers", weather), ("terrains", terrain), ("slot conditions", slot)):
        if not ids:
            raise SystemExit(f"parity_features: found no {what} in the engine sources (the patterns are stale)")
    return {
        "move": {"ids": standard["moves"], "source": "engine/data/champions.json moves, isNonstandard null"},
        "ability": {"ids": standard["abilities"], "source": "engine/data/champions.json abilities, isNonstandard null"},
        "item": {"ids": standard["items"], "source": "engine/data/champions.json items, isNonstandard null"},
        "volatile": {"ids": sorted(i for i in volatile_ids.values() if i not in hidden), "hidden": hidden,
                     "source": "Volatile::id in engine/core/src/volatile.rs (without the kinds "
                               "Volatile::showdown_state leaves out of canonical states)"},
        "side_condition": {"ids": side, "source": "SIDE_CONDITIONS in engine/scenario/src/canonical.rs"},
        "slot_condition": {"ids": slot, "source": "SlotCondition::id in engine/core/src/field.rs"},
        "pseudo_weather": {"ids": pseudo, "source": "PSEUDO in engine/scenario/src/canonical.rs"},
        "weather": {"ids": weather, "source": "weather_id in engine/scenario/src/canonical.rs (primal weathers "
                                              "are not representable in canonical states)"},
        "terrain": {"ids": terrain, "source": "terrain_id in engine/scenario/src/canonical.rs"},
        "status": {"ids": list(STATUSES), "source": "Showdown non-volatile statuses"},
        "boost": {"ids": [f"{s}{sign}" for s in STATS for sign in "+-"], "source": "stat stages, both signs"},
    }, names


# ---------------------------------------------------------------------------------------------
# features of one state


def owner_features(state):
    """{(category, feature): set of owners} of a canonical state; owners tell 'new' apart (the
    same Pokémon, side or slot)."""
    out = collections.defaultdict(set)
    f = state["field"]
    if f.get("weather"):
        out[("weather", f["weather"])].add("field")
    if f.get("terrain"):
        out[("terrain", f["terrain"])].add("field")
    for k in f.get("pseudoWeather") or {}:
        out[("pseudo_weather", k)].add("field")
    if state.get("ended"):
        out[("state", "game ended")].add("field")
    for s, side in enumerate(state["sides"]):
        for k in side.get("conditions") or {}:
            out[("side_condition", k)].add(f"p{s + 1}")
        for i, slot in enumerate(side.get("slotConditions") or []):
            for k in slot:
                out[("slot_condition", k)].add(f"p{s + 1}:{i}")
        if side.get("request") and side["request"] != "move":
            out[("state", f"request {side['request']}")].add(f"p{s + 1}")
        fainted = 0
        for mon in side["pokemon"]:
            who = f"p{s + 1}:{mon['name']}"
            status = mon.get("status") or ""
            # A fainted member has 0 HP; `fnt` stays only while it still holds its slot (the
            # replacement not yet chosen); once replaced, Showdown's status is empty.
            if mon.get("hp") == 0:
                fainted += 1
                out[("fainted", "fainted member")].add(who)
            if status == "fnt":
                out[("fainted", "fainted member still in its slot (fnt)")].add(who)
            elif status:
                out[("status", status)].add(who)
            if mon.get("lastItem"):
                out[("last_item", mon["lastItem"])].add(who)
            if mon.get("slot") is None:
                continue
            out[("species", mon["species"])].add(who)
            if mon.get("ability"):
                out[("ability", mon["ability"])].add(who)
            if mon.get("item"):
                out[("item", mon["item"])].add(who)
            if "-Mega" in mon["species"]:
                out[("mega", "mega-evolved Pokémon active")].add(who)
            if mon.get("canMega"):
                out[("mega", "active Pokémon can Mega Evolve")].add(who)
            for k in mon.get("volatiles") or {}:
                out[("volatile", k)].add(who)
            for stat, v in (mon.get("boosts") or {}).items():
                if v:
                    out[("boost", f"{stat}{'+' if v > 0 else '-'}")].add(who)
            if mon.get("types"):
                out[("state", "types changed")].add(who)
            if status == "slp" and "statusTime" in mon:
                out[("state", f"sleep turns left {mon['statusTime']}")].add(who)
            if status == "tox" and "statusStage" in mon:
                out[("state", "toxic counter")].add(who)
        if fainted:
            out[("fainted", f"{fainted} fainted on one side")].add(f"p{s + 1}")
    return out


def choice_features(scenario):
    """Features of the decision's choices: chosen moves, decision kinds, Mega Evolution."""
    out = set()
    turn = scenario.get("turn") or {}
    sides_choosing = 0
    for side in ("p1", "p2"):
        text = (turn.get(side) or "").strip()
        if not text:
            continue
        sides_choosing += 1
        for part in text.split(","):
            tokens = part.split()
            if not tokens:
                continue
            kind = tokens[0]
            out.add(("decision", f"choice: {kind}"))
            if kind == "move" and len(tokens) > 1:
                out.add(("move", tokens[1]))
                target = next((t for t in tokens[2:] if t.lstrip("-").isdigit()), None)
                if target is None:
                    out.add(("decision", "move without a target (spread, self or side)"))
                elif target.startswith("-"):
                    out.add(("decision", "move aimed at an ally slot"))
                else:
                    out.add(("decision", "move aimed at a foe slot"))
                for flag in tokens[2:]:
                    if flag == "mega":
                        out.add(("mega", "Mega Evolution chosen"))
                    elif not flag.lstrip("-").isdigit():
                        out.add(("decision", f"choice flag: {flag}"))
    out.add(("decision", f"sides choosing: {sides_choosing}"))
    mid = scenario.get("midTurn") or {}
    if any(mid.get(side) for side in ("p1", "p2")):
        out.add(("decision", "mid-turn switch choice (U-turn, Eject Button, ...)"))
    return out


# ---------------------------------------------------------------------------------------------


def to_id(name):
    return re.sub(r"[^a-z0-9]", "", (name or "").lower())


def team_features(scenario, on_teams):
    """Moves, abilities and items on the scenario's (inlined) teams: what the games could have
    shown, to tell 'never chosen' from 'on no team'."""
    for side in ("p1", "p2"):
        team = scenario[side].get("team")
        if not isinstance(team, list):
            continue
        for member in team:
            for mv in member.get("moves") or []:
                on_teams["move"].add(to_id(mv))
            if member.get("ability"):
                on_teams["ability"].add(to_id(member["ability"]))
            if member.get("item"):
                on_teams["item"].add(to_id(member["item"]))


def collect(corpus):
    index = json.loads((corpus / "corpus.json").read_text(encoding="utf-8"))
    # A build still running has written its finished positions' rows but not the index yet.
    index["positions"] = [parity_corpus.merge_build_row(corpus, e) if e.get("build") == "pending" else e
                          for e in index["positions"]]
    entries = [e for e in index["positions"] if e.get("build") == "ok"]
    counts = collections.defaultdict(lambda: collections.Counter())
    on_teams = collections.defaultdict(set)
    # Moves chosen in pinned setup turns, per distinct game turn: the engine had to reach the
    # recorded outcome there, a weaker check than a compared decision.
    setup_turns = {}
    positions = 0
    for e in entries:
        positions += 1
        scenario = read_gz_json(corpus / e["scenario"])
        team_features(scenario, on_teams)
        for k, turn in enumerate(scenario.get("setupTurns") or []):
            chosen = set()
            for text in turn[:2]:
                for part in (text or "").split(","):
                    tokens = part.split()
                    if len(tokens) > 1 and tokens[0] == "move":
                        chosen.add(tokens[1])
            setup_turns[(e["set"], e["game"], k)] = chosen
        reports = [read_gz_json(corpus / r["file"]) for r in e["reports"]]
        before = owner_features(reports[0]["before"])
        outcome = collections.defaultdict(set)
        new = set()
        for report in reports:
            for o in report["outcomes"]:
                for key, owners in owner_features(o["state"]).items():
                    outcome[key] |= owners
                    if owners - before.get(key, set()):
                        new.add(key)
        choices = choice_features(scenario)
        modes = {r["mode"] for r in e["reports"]}
        exact_full = modes == {"full"}
        keys = set(before) | set(outcome) | choices
        kind = e.get("kind") or "?"
        keys.add(("decision", f"decision: {kind}"))
        for key in keys:
            c = counts[key]
            c["positions"] += 1
            if key in before:
                c["before"] += 1
            if key in new:
                c["new"] += 1
            if exact_full:
                c["full"] += 1
            c[f"set:{e['set']}"] += 1
    # Setup turn k of a game is decision k: compared as a position unless it is the lead turn
    # (k = 0) or a position the corpus lacks. Count only those, so decisions are not counted twice.
    compared = {(e["set"], e["game"], e["step"]) for e in entries}
    uncompared = {k: v for k, v in setup_turns.items() if k not in compared}
    setup_moves = collections.Counter(m for chosen in uncompared.values() for m in chosen)
    return index, positions, counts, on_teams, setup_moves, len(uncompared)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("corpus", type=pathlib.Path)
    ap.add_argument("--rare", type=int, default=5, help="'rare' threshold: seen in fewer positions than this")
    ap.add_argument("--out", type=pathlib.Path)
    args = ap.parse_args()
    corpus = args.corpus.resolve()
    out = (args.out or corpus).resolve()
    universe, names = universes()
    index, positions, counts, on_teams, setup_moves, setup_turn_count = collect(corpus)

    by_category = collections.defaultdict(dict)
    for (cat, feature), c in counts.items():
        by_category[cat][feature] = dict(c)
    chosen = by_category.get("move", {})
    report = {
        "corpus": str(corpus).replace("\\", "/"),
        "positions": positions,
        "not_built": sum(1 for e in index["positions"] if e.get("build") != "ok"),
        "rare_threshold": args.rare,
        "setup_turns": setup_turn_count,
        "columns": {"positions": "in the decision state or an outcome", "before": "in the decision state",
                    "new": "created in the compared turn (an outcome has it, the decision state did not, "
                           "for the same Pokémon/side/slot)",
                    "full": "positions compared under the exact 16-roll distribution",
                    "setup_turns": "moves only: pinned setup turns that are not compared positions themselves "
                                   "(the lead turns) and chose it"},
        "features": {cat: dict(sorted(by_category.get(cat, {}).items(), key=lambda kv: (-kv[1]["positions"], kv[0])))
                     for cat, _ in CATEGORIES},
        "gaps": {},
    }
    for m, n in setup_moves.items():
        report["features"]["move"].setdefault(m, {"positions": 0})["setup_turns"] = n
    for cat, u in universe.items():
        seen = by_category.get(cat, {})
        never = [i for i in u["ids"] if i not in seen]
        rare = sorted(((i, seen[i]["positions"]) for i in u["ids"] if i in seen and seen[i]["positions"] < args.rare),
                      key=lambda x: (x[1], x[0]))
        outside = sorted(i for i in seen if i not in u["ids"] and i not in u.get("hidden", []))
        report["gaps"][cat] = {"universe": len(u["ids"]), "source": u["source"], "seen": len(u["ids"]) - len(never),
                               "never": never, "rare": [{"id": i, "positions": n} for i, n in rare],
                               "seen_outside_universe": outside}
        if cat == "move":
            # Never chosen in a compared decision but chosen in a pinned setup turn.
            report["gaps"][cat]["never_but_in_setup_turns"] = {i: setup_moves[i] for i in never if setup_moves[i]}
        if cat in on_teams:
            # Never seen although a corpus team has it (and, for moves, no setup turn chose it):
            # the games could have shown it.
            in_setup = report["gaps"][cat].get("never_but_in_setup_turns", {})
            report["gaps"][cat]["never_but_on_a_team"] = [i for i in never if i in on_teams[cat] and i not in in_setup]
        if cat in ("volatile", "side_condition", "slot_condition", "pseudo_weather"):
            # Effects that start and end inside a turn (Protect-like volatiles, Wide Guard, Helping
            # Hand, ...) reach a canonical state only when a turn pauses: the move of the same id
            # tells whether the effect was exercised all the same.
            report["gaps"][cat]["same_id_move"] = {
                i: {"decisions": chosen.get(i, {}).get("positions", 0), "setup_turns": setup_moves.get(i, 0)}
                for i in never + [r[0] for r in rare] if chosen.get(i) or setup_moves.get(i)}
        if "hidden" in u:
            report["gaps"][cat]["not_in_canonical_states"] = u["hidden"]
    (out / "features.json").write_text(json.dumps(report, indent=1, ensure_ascii=False), encoding="utf-8")
    (out / "features.md").write_text(markdown(report, names), encoding="utf-8")
    print(json.dumps({cat: {k: (len(v) if isinstance(v, (list, dict)) else v) for k, v in g.items() if k != "source"}
                      for cat, g in report["gaps"].items()}))
    print(f"written {out / 'features.md'} and {out / 'features.json'}")
    return 0


NAMED = ("move", "ability", "item", "last_item")


def label(i, names, cat):
    """Dex name and id for moves, abilities and items; the bare id otherwise (a volatile shares its
    id with a move but is not the move)."""
    return f"{names[i]} (`{i}`)" if cat in NAMED and i in names else f"`{i}`"


def markdown(report, names):
    n = report["positions"]
    lines = [
        "# Parity corpus feature coverage",
        "",
        f"Corpus `{report['corpus']}`: {n} compared positions ({report['not_built']} corpus entries without "
        f"reports are not counted). Generated by `engine/scripts/parity_features.py`.",
        "",
        "**A feature seen in compared positions is evidence that the engine reproduces Showdown there, not a "
        "proof that it always does.** Each position is one pinned history of a self-play game; a feature seen "
        "in few positions was checked in few situations, and a feature seen only in the decision state "
        "(`before`) but never created in a compared turn (`new` 0) was only carried, not produced.",
        "",
        "Columns: `positions` = in the decision state or in at least one outcome; `before` = in the decision "
        "state; `new` = created in the compared turn (an outcome has it where the decision state did not, for "
        "the same Pokémon, side or slot); `full` = positions compared under the exact 16-roll distribution "
        "(the others: min/max rolls or fixed rolls). Moves also have `setup` = pinned setup turns that are not "
        f"themselves compared positions ({report['setup_turns']}: the games' lead turns) and chose the move: the "
        "engine had to reach the recorded outcome of such a turn, a weaker check than a compared decision.",
        "",
        "States are compared at turn boundaries, so effects that start and end inside one turn (Protect-like "
        "volatiles, Wide Guard, Quick Guard, Crafty Shield, Mat Block, Helping Hand, Follow Me, Rage Powder, "
        "Endure, Roost, flinch, ...) appear only in outcomes of a turn paused for a mid-turn switch; where a "
        "move has the same id, its counts are given next to the never-seen and rarely-seen conditions. "
        "Abilities are the active Pokémon's current ones (after Mega Evolution, Trace, ...).",
        "",
        "## Gaps against the Champions standard range",
        "",
        f"| universe | size | seen | never seen | seen in fewer than {report['rare_threshold']} positions | source |",
        "|---|---|---|---|---|---|",
    ]
    for cat, g in report["gaps"].items():
        lines.append(f"| {cat} | {g['universe']} | {g['seen']} | {len(g['never'])} | {len(g['rare'])} | {g['source']} |")
    lines.append("")
    for cat, g in report["gaps"].items():
        same = g.get("same_id_move", {})

        def show(i):
            text = label(i, names, cat)
            if i in same:
                text += f" (move chosen in {same[i]['decisions']} decisions, {same[i]['setup_turns']} setup turns)"
            return text

        lines += [f"### {cat}: never seen ({len(g['never'])} of {g['universe']})", ""]
        if "never_but_on_a_team" in g:
            in_setup = g.get("never_but_in_setup_turns", {})
            on_team = g["never_but_on_a_team"]
            if "never_but_in_setup_turns" in g:
                lines += [f"Chosen only in uncompared pinned setup turns, the lead turns ({len(in_setup)}): "
                          + (", ".join(f"{label(i, names, cat)} {n}" for i, n in in_setup.items()) or "(none)"), ""]
            lines += [f"On a corpus team but never seen ({len(on_team)}; the games could have shown these): "
                      + (", ".join(label(i, names, cat) for i in on_team) or "(none)"), ""]
            rest = [i for i in g["never"] if i not in on_team and i not in in_setup]
            lines += [f"On no corpus team ({len(rest)}): "
                      + (", ".join(label(i, names, cat) for i in rest) or "(none)"), ""]
        else:
            lines.append(", ".join(show(i) for i in g["never"]) or "(none)")
            lines.append("")
        if g["rare"]:
            if cat == "move":
                moves = report["features"]["move"]
                rare = [f"{label(r['id'], names, cat)} {r['positions']}"
                        + (f" (+{moves[r['id']]['setup_turns']} setup turns)" if moves[r["id"]].get("setup_turns") else "")
                        for r in g["rare"]]
            else:
                rare = [f"{show(r['id'])} {r['positions']}" for r in g["rare"]]
            lines += [f"Seen in fewer than {report['rare_threshold']} positions: " + ", ".join(rare), ""]
        if g.get("not_in_canonical_states"):
            lines += ["Engine-only kinds that canonical states never show (not observable by the comparison): "
                      + ", ".join(f"`{i}`" for i in g["not_in_canonical_states"]), ""]
        if g["seen_outside_universe"]:
            lines += ["Seen but outside this universe: " + ", ".join(f"`{i}`" for i in g["seen_outside_universe"]), ""]
    lines += ["## Counts per feature", ""]
    titles = dict(CATEGORIES)
    for cat, _ in CATEGORIES:
        feats = report["features"].get(cat, {})
        lines += [f"### {titles[cat]} ({len(feats)})", ""]
        if not feats:
            lines += ["(none)", ""]
            continue
        if cat == "move":
            lines += ["| feature | positions | full | setup |", "|---|---|---|---|"]
            for f, c in feats.items():
                lines.append(f"| {label(f, names, cat)} | {c.get('positions', 0)} | {c.get('full', 0)} | "
                             f"{c.get('setup_turns', 0)} |")
            lines.append("")
            continue
        lines += ["| feature | positions | before | new | full |", "|---|---|---|---|---|"]
        for f, c in feats.items():
            lines.append(f"| {label(f, names, cat) if cat in NAMED else f} | "
                         f"{c.get('positions', 0)} | {c.get('before', 0)} | {c.get('new', 0)} | {c.get('full', 0)} |")
        lines.append("")
    return "\n".join(lines)


if __name__ == "__main__":
    sys.exit(main())
