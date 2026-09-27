"""Refusal audit (board R0-refusal-audit): every place the turn engine refuses a battle it cannot
simulate exactly (`TurnError::Unsupported`), extracted from the source and classified by whether
a Champions-legal battle can reach it.

Usage (from the repository root or anywhere; paths are relative to this file):

    python engine/scripts/refusals.py            # write engine/REFUSALS.md (exit 1 on problems)
    python engine/scripts/refusals.py --check    # fail if REFUSALS.md is stale or anything is
                                                 # unclassified, stale or unproven
    python engine/scripts/refusals.py facts      # print the standard-range lists the reasons cite
    LAB_ROOT=D:/pokemon-ai-lab python engine/scripts/refusals.py universe
                                                 # regenerate refusals.universe.json from the
                                                 # Champions learnsets (needs vendor/)

What is extracted (`engine/core/src/turn/**.rs`, test modules skipped):
- every `.unsupported(...)`, `TurnError::Unsupported(...)` and `map_err(TurnError::Unsupported)`
  call site;
- the messages of the *producer* functions whose strings those sites forward (the list is in the
  classification file: `check_state`, the static gate `move_unsupported`, `mega_target`, ...).

An entry is keyed by its message template (`{...}` placeholders become `{}`, whitespace runs one
space), not by line number; a forwarding site without its own text is keyed
`<function>: -> <expression>`. Every key must have an entry in `refusals.classification.json`
(`reachable`: yes | no | unknown, `kind`, `why`, and for reachable ones `repro` and `board`), and
every entry must still match a key: a new refusal cannot be added silently.

The standard range is the Champions mod at the pinned Showdown commit: species with
`isNonstandard` null (in-battle formes only if their base is), their abilities (plus Simple and
Insomnia, which Simple Beam and Worry Seed give), the moves of `learnsets.ts` for those species
(plus every move whose `isNonstandard` is null, and Struggle), and items with `isNonstandard` null.
The `evidence` checks below turn the reasons the unreachable entries cite into assertions over that
range, `engine/data/champions.json`, `engine/COVERAGE.md` and the Rust source.
"""
import argparse
import json
import os
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
ENGINE = HERE.parent
TURN = ENGINE / "core" / "src" / "turn"
OUT = ENGINE / "REFUSALS.md"
CLASSIFICATION = HERE / "refusals.classification.json"
UNIVERSE = HERE / "refusals.universe.json"
DEX = ENGINE / "data" / "champions.json"
COVERAGE = ENGINE / "COVERAGE.md"
SCENARIOS = ENGINE / "oracle" / "scenarios"
EXPECTED = ENGINE / "oracle" / "expected"

# ---------------------------------------------------------------------------------------------
# Rust lexing: blank comments, replace string literals by placeholders (same length), keep the
# literal values by position.


def lex(src):
    """Returns (code, literals): `code` has the length of `src` with comments turned into spaces
    and every string literal's text (quotes included) into `\\x01`s; `literals` maps a literal's
    start offset to (end offset, value)."""
    out = []
    literals = {}
    i, n = 0, len(src)
    char_re = re.compile(r"'(\\u\{[0-9a-fA-F]+\}|\\.|[^'\\\n])'")
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(re.sub(r"[^\n]", " ", src[i:j]))
            i = j
        elif c == "r" and re.match(r'r#*"', src[i:]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            hashes = re.match(r'r(#*)"', src[i:]).group(1)
            start = i + 2 + len(hashes)
            end = src.index('"' + hashes, start)
            j = end + 1 + len(hashes)
            literals[i] = (j, src[start:end])
            out.append("\x01" * (j - i))
            i = j
        elif c == '"' or (c == "b" and src.startswith('b"', i)):
            j = i + (2 if c == "b" else 1)
            value = []
            while src[j] != '"':
                if src[j] == "\\":
                    nxt = src[j + 1]
                    if nxt == "\n":
                        # Line continuation: the newline and the next line's leading whitespace go.
                        j += 2
                        while src[j] in " \t\r\n":
                            j += 1
                        continue
                    value.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(nxt, nxt))
                    j += 2
                    continue
                value.append(src[j])
                j += 1
            j += 1
            literals[i] = (j, "".join(value))
            out.append("\x01" * (j - i))
            i = j
        elif c == "'":
            m = char_re.match(src, i)
            if m:
                out.append(" " * (m.end() - i))
                i = m.end()
            else:
                out.append(c)  # a lifetime
                i += 1
        else:
            out.append(c)
            i += 1
    code = "".join(out)
    assert len(code) == len(src)
    return code, literals


def match_close(code, open_at):
    """Index just after the bracket that closes the one at `open_at`."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [pairs[code[open_at]]]
    j = open_at + 1
    while stack:
        ch = code[j]
        if ch in pairs:
            stack.append(pairs[ch])
        elif ch in ")]}":
            assert ch == stack[-1], f"unbalanced {ch} at {j}"
            stack.pop()
        j += 1
    return j


def strip_tests(code):
    """Blanks `#[cfg(test)]` items (test modules and test-only functions)."""
    for m in list(re.finditer(r"#\[cfg\(test\)\]\s*(?:pub(?:\([\w:]+\))?\s+)?(mod|fn|use|impl)\b", code)):
        if m.group(1) == "use":
            end = code.index(";", m.end()) + 1
        else:
            end = match_close(code, code.index("{", m.end()))
        code = code[: m.start()] + re.sub(r"[^\n]", " ", code[m.start():end]) + code[end:]
    return code


def functions(code):
    """(name, body start, body end) of every `fn`, innermost last among overlapping spans."""
    spans = []
    for m in re.finditer(r"\bfn\s+(\w+)", code):
        # The body is the first `{` outside the signature's parentheses and brackets (`[u8; 6]`
        # holds a `;`); a `;` there first means a declaration without a body.
        depth, j, brace = 0, m.end(), None
        while j < len(code):
            ch = code[j]
            if ch in "([":
                depth += 1
            elif ch in ")]":
                depth -= 1
            elif depth == 0 and ch == ";":
                break
            elif depth == 0 and ch == "{":
                brace = j
                break
            j += 1
        if brace is not None:
            spans.append((m.group(1), brace, match_close(code, brace)))
    return spans


def enclosing(spans, at):
    best = None
    for name, start, end in spans:
        if start <= at < end and (best is None or start >= best[1]):
            best = (name, start, end)
    return best[0] if best else "?"


def template(value):
    """A message template: `{...}` placeholders as `{}`, whitespace runs as one space."""
    value = value.replace("{{", "\x02").replace("}}", "\x03")
    value = re.sub(r"\{[^{}]*\}", "{}", value)
    value = value.replace("\x02", "{{").replace("\x03", "}}")
    return re.sub(r"\s+", " ", value).strip()


def literals_in(code, literals, start, end):
    return [literals[k][1] for k in sorted(literals) if start <= k < end]


def first_literal(code, literals, start, end):
    found = literals_in(code, literals, start, end)
    return found[0] if found else None


class Source:
    def __init__(self, path):
        self.path = path
        self.rel = path.relative_to(TURN).as_posix()
        self.src = path.read_text(encoding="utf-8")
        code, self.literals = lex(self.src)
        self.code = strip_tests(code)
        self.spans = functions(self.code)

    def line(self, at):
        return self.src.count("\n", 0, at) + 1

    def arg(self, open_paren):
        end = match_close(self.code, open_paren)
        return open_paren + 1, end - 1

    def text(self, start, end):
        """Source text of a code range with literals shown by value."""
        parts, i = [], start
        for k in sorted(self.literals):
            if start <= k < end:
                parts.append(self.code[i:k])
                parts.append(json.dumps(self.literals[k][1], ensure_ascii=False))
                i = self.literals[k][0]
        parts.append(self.code[i:end])
        return re.sub(r"\s+", " ", "".join(parts)).strip()


def message_of(source, start, end):
    """The template of a refusal argument, or None when it forwards a value (no own text)."""
    body = source.code[start:end].strip()
    lit = first_literal(source.code, source.literals, start, end)
    if lit is None:
        return None
    if body.startswith("format!(") or body.startswith("\x01") or "format!(" in body:
        return template(lit)
    return None


def extract(producers):
    """Every refusal key → {"sites": [...], "kind": "site"|"forward"|"producer"}."""
    entries = {}

    def add(key, site, how):
        e = entries.setdefault(key, {"sites": [], "how": set(), "calls": 0})
        if site not in e["sites"]:
            e["sites"].append(site)
        e["how"].add(how)
        if how != "producer":
            e["calls"] += 1

    files = sorted(p for p in TURN.rglob("*.rs"))
    sources = {p.relative_to(TURN).as_posix(): Source(p) for p in files}
    for rel, s in sources.items():
        code = s.code
        patterns = [
            r"\.unsupported\(",
            r"TurnError::Unsupported\(",
            r"\brefused\s*\.\s*get_or_insert_with\(",
        ]
        for pattern in patterns:
            for m in re.finditer(pattern, code):
                open_paren = m.end() - 1
                start, end = s.arg(open_paren)
                after = code[end + 1: end + 12]
                if re.match(r"\s*=>", after):
                    continue  # a match pattern, not a construction
                fn = enclosing(s.spans, m.start())
                if fn == "unsupported":
                    continue  # `Battle::unsupported` itself
                site = f"{rel}::{fn}"
                key = message_of(s, start, end)
                how = "site"
                if key is None:
                    key = f"{fn}: -> {s.text(start, end)}"
                    how = "forward"
                add(key, site, how)
        for m in re.finditer(r"([\w:]+\([^()]*\))\s*\.map_err\(\s*TurnError::Unsupported\s*\)", code):
            fn = enclosing(s.spans, m.start())
            add(f"{fn}: -> {s.text(m.start(1), m.end(1))}", f"{rel}::{fn}", "forward")
    # Producers: the messages their bodies build.
    for producer in producers:
        rel, _, name = producer.rpartition("::")
        s = sources.get(rel)
        if s is None:
            raise SystemExit(f"producer {producer}: no file {rel}")
        spans = [sp for sp in s.spans if sp[0] == name]
        if len(spans) != 1:
            raise SystemExit(f"producer {producer}: {len(spans)} functions named {name}")
        _, start, end = spans[0]
        body = s.code[start:end]
        found = []
        for m in re.finditer(r"format!\(", body):
            a, b = s.arg(start + m.end() - 1)
            lit = first_literal(s.code, s.literals, a, b)
            if lit is not None:
                found.append(lit)
        for m in re.finditer(r"\bwhy\(\s*\x01", body):
            k = start + m.end() - 1
            found.append(s.literals[k][1])
        for k in sorted(s.literals):
            if not (start <= k < end):
                continue
            e = s.literals[k][0]
            before = s.code[max(start, k - 12):k]
            after = s.code[e:e + 40]
            if re.match(r"\s*\.(into|to_owned|to_string)\(\)", after) or re.search(r"(Some|Err)\(\s*$", before):
                found.append(s.literals[k][1])
            elif re.search(r"\{\s*$", before) and re.match(r"\s*\}", after):
                found.append(s.literals[k][1])
        if not found:
            raise SystemExit(f"producer {producer}: no messages found")
        for lit in found:
            add(template(lit), f"{producer} (producer)", "producer")
    for e in entries.values():
        e["how"] = sorted(e["how"])
    return entries, sources


# ---------------------------------------------------------------------------------------------
# The standard range.


def load_dex():
    return json.loads(DEX.read_text(encoding="utf-8"))


def to_id(name):
    return re.sub(r"[^a-z0-9]", "", name.lower())


def build_universe(lab_root):
    """The Champions standard range from the dex and the mod's learnsets."""
    dex = load_dex()
    path = pathlib.Path(lab_root) / "vendor/pokemon-showdown/data/mods/champions/learnsets.ts"
    learnsets, current = {}, None
    for line in path.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^\t(\w+): \{$", line)
        if m:
            current = m.group(1)
            learnsets[current] = set()
            continue
        m = re.match(r"^\t\t\t(\w+): \[", line)
        if m and current:
            learnsets[current].add(m.group(1))
    species = {}
    for sid, s in dex["species"].items():
        if s["isNonstandard"] is not None:
            continue
        own = learnsets.get(sid) or learnsets.get(to_id(s.get("changesFrom") or "")) or learnsets.get(to_id(s["baseSpecies"]))
        base = dex["species"].get(to_id(s["baseSpecies"]))
        # An in-battle forme (no learnset of its own or of its base) is reachable only through its
        # base species, which must itself be standard.
        if own is None:
            if base is None or base["isNonstandard"] is not None:
                continue
            own = set()
        species[sid] = sorted(own)
    abilities = set()
    for sid in species:
        abilities |= set(dex["species"][sid]["abilities"].values())
    abilities |= {"Simple", "Insomnia"}
    moves = set()
    for learned in species.values():
        moves |= set(learned)
    moves |= {k for k, v in dex["moves"].items() if v["isNonstandard"] is None}
    moves.add("struggle")
    items = sorted(k for k, v in dex["items"].items() if v["isNonstandard"] is None)
    commit = dex.get("source", {}).get("commit")
    return {
        "source": {"showdown_commit": commit, "note": "generated by refusals.py universe"},
        "species": species,
        "abilities": sorted(to_id(a) for a in abilities),
        "moves": sorted(moves),
        "items": items,
    }


def load_universe():
    if not UNIVERSE.exists():
        raise SystemExit(f"{UNIVERSE} missing: run `refusals.py universe` with LAB_ROOT set")
    return json.loads(UNIVERSE.read_text(encoding="utf-8"))


# ---------------------------------------------------------------------------------------------
# Evidence: the facts the unreachable entries cite, checked.


def fn_body(sources, rel, name):
    s = sources[rel]
    spans = [sp for sp in s.spans if sp[0] == name]
    assert len(spans) == 1, f"{rel}::{name}: {len(spans)} functions"
    _, start, end = spans[0]
    return s.code[start:end]


def const_ids(code, module):
    """`module::NAME` constants in code, as dex ids (`AIR_BALLOON` → `airballoon`)."""
    return {m.group(1).replace("_", "").lower() for m in re.finditer(rf"\b{module}::([A-Z0-9_]+)\b", code)}


def const_array(sources, rel, name):
    s = sources[rel]
    m = re.search(rf"\b(?:const|static)\s+{name}\s*:[^=]*=\s*\[", s.code)
    assert m, f"{rel}: no array {name}"
    end = match_close(s.code, m.end() - 1)
    return s.code[m.end() - 1:end]


def coverage_unsupported():
    """Names COVERAGE.md lists as unsupported, per section (Korean headings of lab-coverage)."""
    text = COVERAGE.read_text(encoding="utf-8")
    out, section = {}, None
    no_switch = {}
    for line in text.splitlines():
        h = re.match(r"^## (\S+)", line)
        if h:
            section = {"기술": "moves", "특성": "abilities", "도구": "items"}.get(h.group(1))
            out.setdefault(section, set())
            continue
        m = re.match(r"^- 전체 \d+개 중 지원 \d+개, 등장 효과만 미지원 (\d+)개", line)
        if m and section:
            no_switch[section] = int(m.group(1))
        m = re.match(r"^- \*\*.*?\*\* \(\d+\): (.*)$", line)
        if m and section:
            out[section] |= {n.strip() for n in m.group(1).split(", ")}
        m = re.match(r"^\| ([^|]+) \| \d+ \| (미지원|등장 효과 미구현) \|", line)
        if m and section:
            out[section].add(m.group(1).strip())
    return out, no_switch


def evidence(universe, sources):
    """(id, claim, ok, detail) for every fact the classification relies on."""
    dex = load_dex()
    u_species = set(universe["species"])
    u_abilities = set(universe["abilities"])
    u_moves = set(universe["moves"])
    u_items = set(universe["items"])
    ability_by_id = {k: v for k, v in dex["abilities"].items()}
    results = []

    def check(eid, claim, bad, detail_ok=""):
        bad = sorted(bad)
        results.append((eid, claim, not bad, ", ".join(bad) if bad else detail_ok))

    # E1: the static gate refuses nothing in the standard range.
    unsupported, no_switch = coverage_unsupported()
    by_name = {
        "moves": {v["name"]: k for k, v in dex["moves"].items()},
        "abilities": {v["name"]: k for k, v in dex["abilities"].items()},
        "items": {v["name"]: k for k, v in dex["items"].items()},
    }
    universe_of = {"moves": u_moves, "abilities": u_abilities, "items": u_items}
    singular = {"moves": "move", "abilities": "ability", "items": "item"}
    trace_note = {"abilities": " (Trace included: its switch-in and, while it seeks, its `onUpdate` "
                  "are implemented since R1)"}
    for section in ("moves", "abilities", "items"):
        names = unsupported.get(section, set())
        ids = {by_name[section].get(n, "?" + n) for n in names}
        check(
            f"E1-{section}",
            f"COVERAGE.md: no {singular[section]} the support gate refuses is in the standard range, "
            f"and none is refused only at switch-in{trace_note.get(section, '')}",
            [i for i in ids if i in universe_of[section] or i.startswith("?")]
            + ([f"switch-in only: {no_switch.get(section)}"] if no_switch.get(section) else []),
            f"{len(ids)} refused, all outside",
        )
    # E2: no standard species has callbacks (species handlers, forme callbacks of Megas).
    check("E2", "No standard species (Megas included) has species callbacks",
          [s for s in u_species if dex["species"][s].get("handlers")], f"{len(u_species)} species")
    # E3: every standard item with Start / End moves between holders (Trick, Covet, Thief,
    # Pickpocket, Magician, Pickup, Symbiosis) and the only other `onTakeItem` are Mega Stones.
    tmi = fn_body(sources, "moves/handlers.rs", "trick_moves_item")
    movable = const_ids(tmi, "items")
    bad = []
    for i in u_items:
        d = dex["items"][i]
        h = d.get("handlers", [])
        start_end = "onStart" in h or "onEnd" in h
        seed = i.endswith("seed") and i[:-4] in ("electric", "grassy", "misty", "psychic")
        if start_end and not (d.get("isChoice") or seed or i in movable):
            bad.append(i)
        if "onTakeItem" in h and not d.get("megaStone"):
            bad.append(i + " (onTakeItem)")
    check("E3", "Every standard item with onStart / onEnd is one `trick_moves_item` moves; "
          "the only standard onTakeItem items are Mega Stones", bad)
    # E4: every standard berry's onEat is implemented or empty (resist berries).
    boe = fn_body(sources, "update.rs", "berry_on_eat")
    handled = const_ids(boe, "items")
    for arr in ("FIGY_BERRIES", "STAT_BERRIES", "STATUS_BERRIES"):
        handled |= const_ids(const_array(sources, "update.rs", arr), "items")
    resist = const_ids(const_array(sources, "items.rs", "RESIST_BERRIES"), "items")
    berries = [i for i in u_items if dex["items"][i].get("isBerry")]
    check("E4", "Every standard berry's onEat is implemented (`update::berry_on_eat`) or empty "
          "(a resist berry)", [b for b in berries if b not in handled and b not in resist],
          f"{len(berries)} berries")
    # E5: every standard ability with onEnd is handled by `switching::end_ability`.
    ea = fn_body(sources, "switching.rs", "end_ability")
    ended = const_ids(ea, "abilities")
    check("E5", "Every standard ability with onEnd is handled in `switching::end_ability`",
          [a for a in u_abilities if "onEnd" in ability_by_id[a].get("handlers", []) and a not in ended])
    # E6: every standard cantsuppress ability is also notrace (Trace never copies one).
    check("E6", "Every standard `cantsuppress` ability is also `notrace`",
          [a for a in u_abilities
           if "cantsuppress" in ability_by_id[a].get("flags", {}) and "notrace" not in ability_by_id[a].get("flags", {})])
    # E7: onWeatherChange in the standard range: Forecast only (implemented).
    wc = [a for a in u_abilities if "onWeatherChange" in ability_by_id[a].get("handlers", [])]
    wc += [i for i in u_items if "onWeatherChange" in dex["items"][i].get("handlers", [])]
    check("E7", "The only standard onWeatherChange holder is Forecast",
          [x for x in wc if x not in ("forecast",)])
    # E8: abilities, items and species that only Past (non-standard) content has.
    absent = {
        "abilities": ["neutralizinggas", "dancer", "protosynthesis", "quarkdrive", "flowergift",
                      "powerconstruct", "schooling", "shieldsdown", "commander", "noability",
                      "deltastream", "desolateland", "primordialsea", "intrepidsword",
                      "dauntlessshield", "propellertail", "iceface", "gulpmissile"],
        "items": ["mirrorherb", "utilityumbrella", "ejectpack", "roomservice", "boosterenergy",
                  "abilityshield", "heavydutyboots", "figyberry", "wikiberry", "magoberry",
                  "aguavberry", "iapapaberry", "jabocaberry", "rowapberry", "custapberry",
                  "enigmaberry", "micleberry", "starfberry", "lansatberry", "keeberry",
                  "marangaberry"],
        "moves": ["mirrormove", "naturepower", "doomdesire", "captivate", "bestow", "orderup",
                  "relicsong", "mimic", "sketch", "metronome", "assist", "magiccoat", "rollout",
                  "iceball", "shelltrap", "revelationdance"],
        "species": ["zygarde", "zygardecomplete", "zygarde10", "minior", "meloetta", "tatsugiri",
                    "dondozo", "oricorio", "weezinggalar", "greninjabond", "greninjaash",
                    "cherrim", "wishiwashi", "eiscue", "cramorant"],
    }
    pools = {"abilities": u_abilities, "items": u_items, "moves": u_moves, "species": u_species}
    check("E8", "Content only non-standard formes have is outside the range: "
          + "; ".join(f"{k}: {', '.join(v)}" for k, v in absent.items()),
          [f"{k}:{x}" for k, v in absent.items() for x in v if x in pools[k]])
    # E9: no standard multi-hit move is a status move (Magic Bounce reflects only status moves),
    # and no standard future move is multi-hit or takes Parental Bond.
    bad = [m for m in u_moves if dex["moves"][m].get("multihit") and dex["moves"][m]["category"] == "Status"]
    bad += [m for m in u_moves if "futuremove" in dex["moves"][m]["flags"] and dex["moves"][m].get("multihit")]
    check("E9", "No standard multi-hit move is a status move (a bounced move never hits twice) "
          "and no standard future move is multi-hit", bad)
    # E10: the standard moves that queue their own actions (beforeTurnCallback,
    # priorityChargeCallback) for Encore's `changeAction` and Copycat.
    own = sorted(m for m in u_moves if {"beforeTurnCallback", "priorityChargeCallback"} & set(dex["moves"][m].get("handlers", [])))
    check("E10", "No standard move with a beforeTurnCallback / priorityChargeCallback is `failencore` "
          "(Encore can lock each); Copycat can call those without `failcopycat`",
          [m for m in own if "failencore" in dex["moves"][m]["flags"]],
          ", ".join(f"{m}{' (failcopycat)' if 'failcopycat' in dex['moves'][m]['flags'] else ''}" for m in own))
    # E11: the last moves a Pokémon can have outside its move slots (Struggle; Transform, which
    # replaces the slots) are `failinstruct`.
    check("E11", "Struggle and Transform are `failinstruct` (Instruct fails on them)",
          [m for m in ("struggle", "transform") if "failinstruct" not in dex["moves"][m]["flags"]])
    # E12: a standard move whose onAfterMove is not checked for a called move is neither callable
    # by Sleep Talk nor by Copycat.
    checked = const_ids(fn_body(sources, "moves/handlers.rs", "called_after_move_checked"), "moves")
    bad = []
    for m in u_moves:
        d = dex["moves"][m]
        if "onAfterMove" in d.get("handlers", []) and m not in checked:
            if "nosleeptalk" not in d["flags"] or "failcopycat" not in d["flags"]:
                bad.append(m)
    check("E12", "Every standard move with an onAfterMove unchecked for a called move "
          "(`called_after_move_checked`) is `nosleeptalk` and `failcopycat`", bad)
    return results


# ---------------------------------------------------------------------------------------------
# Output.

KIND_TITLES = {
    "mechanic": "a mechanic the engine does not model",
    "input": "an input the scenario leaves undecided (Showdown draws it at team creation)",
    "static": "the support tables (COVERAGE.md, the evidence checks): no standard content reaches it",
    "past-only": "needs content that only non-standard (Past / CAP / LGPE / G-Max) sets have",
    "invariant": "a state the engine's own rules never produce",
    "ruleset": "excluded by the M-C ruleset (no Tera / Dynamax / Z)",
    "shape": "the battle shape (doubles only)",
    "forward": "forwards a producer's message",
}


def fixture_outcomes(name):
    for suffix, mode in ((".turn.json", "full"), (".extremes.json", "extremes"), (".initial.json", "initial")):
        path = EXPECTED / f"{name}{suffix}"
        if path.exists():
            data = json.loads(path.read_text(encoding="utf-8"))
            return f"{mode}: {len(data.get('outcomes', []))}", path.name
    return None, None


def render(entries, classification, results, problems, fixed=None, proposed=None):
    lines = []
    w = lines.append
    w("# 턴 엔진 거부 목록 (자동 생성)")
    w("")
    w("`python engine/scripts/refusals.py`가 `engine/core/src/turn/`의 거부 지점(`b.unsupported(...)`, "
      "`TurnError::Unsupported`, `check_state`와 정적 지원 검사 등 메시지를 만드는 함수)을 "
      "소스에서 뽑고 `engine/scripts/refusals.classification.json`의 분류를 붙여 만든다. 직접 편집하지 않는다. "
      "`--check`는 새 거부가 분류 없이 추가되거나 분류가 낡으면 실패한다.")
    w("")
    w("범위: Champions 모드(Showdown `9e317a6`)에서 `isNonstandard`가 null인 종(메가 포함, 배틀 중 폼은 기본 종이 표준일 때만), "
      "그 종의 특성(+심플빔·고민씨가 주는 심플·불면), `learnsets.ts`의 기술(+표준 플래그 기술·발버둥), 표준 도구. "
      "테라·다이맥스·Z는 규칙셋이 막는다. 목록은 `refusals.universe.json`, 근거 검사는 아래 '근거'.")
    w("")
    counts = {"yes": 0, "no": 0, "unknown": 0}
    for key, e in entries.items():
        c = classification.get(key)
        if c:
            counts[c["reachable"]] = counts.get(c["reachable"], 0) + 1
    sites = sorted({s for e in entries.values() for s in e["sites"] if "(producer)" not in s})
    calls = sum(e["calls"] for e in entries.values())
    forwards = sum(1 for e in entries.values() if e["how"] == ["forward"])
    produced = sum(1 for e in entries.values() if "producer" in e["how"])
    own = len(entries) - forwards - produced
    w(f"- 거부 호출 {calls}곳(함수 {len(sites)}개). 키 {len(entries)}개 = 호출에 쓰인 메시지 {own}개 + "
      f"다른 함수의 메시지를 전달하는 호출 {forwards}개 + 메시지 생산 함수(`producers`)의 메시지 {produced}개.")
    w(f"- 도달 가능 {counts.get('yes', 0)}개, 도달 불가능 {counts.get('no', 0)}개, 미확인 {counts.get('unknown', 0)}개.")
    if problems:
        w("")
        w("**문제:**")
        for p in problems:
            w(f"- {p}")
    w("")
    w("## 도달 가능 (Champions 표준 범위)")
    w("")
    w("| 키 | 지점 | 종류 | 재현 시나리오 | 오라클 결과 수 | 보드 | 이유 |")
    w("|---|---|---|---|---|---|---|")
    reach = [(k, e, classification[k]) for k, e in entries.items() if classification.get(k, {}).get("reachable") == "yes"]
    reach.sort(key=lambda x: (board_order(x[2].get("board", "")), x[0]))
    for key, e, c in reach:
        repro = c.get("repro", "")
        count, fixture = fixture_outcomes(repro) if repro else (None, None)
        w(f"| `{md(key)}` | {'<br>'.join(md(s) for s in e['sites'])} | {c['kind']} | "
          f"{'`' + repro + '`' if repro else '—'} | {count or '—'} | {c.get('board', '—')} | {md(c['why'])} |")
    w("")
    if fixed:
        w("## 고친 거부 (소스에서 사라짐)")
        w("")
        w("| 이전 키 | 보드 | 오라클 fixture | 내용 |")
        w("|---|---|---|---|")
        for key, f in fixed.items():
            count, name = fixture_outcomes(f["fixture"])
            w(f"| `{md(key)}` | {f['board']} | `{f['fixture']}` ({count or '—'}) | {md(f['what'])} |")
        w("")
    w("## 미확인")
    w("")
    unknown = [(k, e, classification[k]) for k, e in entries.items() if classification.get(k, {}).get("reachable") == "unknown"]
    if not unknown:
        w("없음.")
    for key, e, c in unknown:
        w(f"- `{md(key)}` ({', '.join(e['sites'])}): {md(c['why'])}")
    w("")
    w("## 도달 불가능 (이유별)")
    w("")
    groups = {}
    for key, e in entries.items():
        c = classification.get(key)
        if c and c["reachable"] == "no":
            groups.setdefault(c["kind"], []).append((key, e, c))
    for kind in ("past-only", "static", "invariant", "ruleset", "shape", "forward", "input", "mechanic"):
        if kind not in groups:
            continue
        w(f"### {kind}: {KIND_TITLES[kind]}")
        w("")
        w("| 키 | 지점 | 이유 | 보드 |")
        w("|---|---|---|---|")
        for key, e, c in sorted(groups[kind], key=lambda x: x[0]):
            w(f"| `{md(key)}` | {'<br>'.join(md(s) for s in e['sites'])} | {md(c['why'])} | {c.get('board', '—')} |")
        w("")
    w("## 보드 대응")
    w("")
    boards = {}
    for key, e in entries.items():
        c = classification.get(key)
        if c and c.get("board"):
            boards.setdefault(c["board"], []).append((c["reachable"], key))
    w("| 보드 | 도달 가능 | 도달 불가능 |")
    w("|---|---|---|")
    for board in sorted(boards, key=board_order):
        yes = [k for r, k in boards[board] if r == "yes"]
        no = [k for r, k in boards[board] if r != "yes"]
        w(f"| {board} | {'<br>'.join('`' + md(k) + '`' for k in yes) or '—'} | {'<br>'.join('`' + md(k) + '`' for k in no) or '—'} |")
    w("")
    if proposed:
        w("보드에 아직 없는 제안 작업(이 감사가 붙인 이름):")
        w("")
        for board, what in proposed.items():
            w(f"- **{board}**: {md(what)}")
        w("")
    w("## 근거 (자동 검사)")
    w("")
    w("| 검사 | 내용 | 결과 |")
    w("|---|---|---|")
    for eid, claim, ok, detail in results:
        w(f"| {eid} | {md(claim)} | {'통과' if ok else '**실패**'}{': ' + md(detail) if detail else ''} |")
    w("")
    return "\n".join(lines)


def board_order(board):
    m = re.match(r"R(\d+)", board)
    return (0, int(m.group(1)), board) if m else (1, 0, board)


def md(text):
    return str(text).replace("|", "\\|").replace("\n", " ")


def problems_of(entries, classification, universe):
    problems = []
    for key in entries:
        if key not in classification:
            problems.append(f"unclassified refusal `{key}` ({', '.join(entries[key]['sites'])})")
    for key in classification:
        if key not in entries:
            problems.append(f"stale classification `{key}` (no such refusal in the source)")
    for key, c in classification.items():
        if c.get("reachable") not in ("yes", "no", "unknown"):
            problems.append(f"`{key}`: reachable must be yes, no or unknown")
        if c.get("kind") not in KIND_TITLES:
            problems.append(f"`{key}`: unknown kind {c.get('kind')}")
        if not c.get("why"):
            problems.append(f"`{key}`: no reason")
        if c.get("reachable") == "yes":
            repro = c.get("repro")
            if not repro:
                problems.append(f"`{key}`: reachable without a repro scenario")
                continue
            path = SCENARIOS / f"{repro}.json"
            if not path.exists():
                problems.append(f"`{key}`: repro {repro} has no scenario")
                continue
            problems += legality(repro, path, universe)
            if not c.get("board"):
                problems.append(f"`{key}`: reachable without a board task")
    return problems


def legality(name, path, universe):
    """A repro scenario must use only standard species, abilities, items and learnable moves."""
    dex = load_dex()
    out = []
    scenario = json.loads(path.read_text(encoding="utf-8"))
    names = {v["name"]: k for k, v in dex["species"].items()}
    for side in ("p1", "p2"):
        team = scenario[side]["team"]
        if isinstance(team, str):
            team = json.loads((path.parent / team).read_text(encoding="utf-8"))
        for s in team:
            sid = names.get(s["species"], to_id(s["species"]))
            if sid not in universe["species"]:
                out.append(f"{name}: species {s['species']} is not standard")
                continue
            ability = to_id(s.get("ability", ""))
            if ability not in {to_id(a) for a in dex["species"][sid]["abilities"].values()}:
                out.append(f"{name}: {s['species']} cannot have {s.get('ability')}")
            item = to_id(s.get("item", ""))
            if item and item not in universe["items"]:
                out.append(f"{name}: item {s['item']} is not standard")
            learnable = set(universe["species"][sid])
            for m in s["moves"]:
                if to_id(m) not in learnable:
                    out.append(f"{name}: {s['species']} does not learn {m}")
            if s.get("level") != 50:
                out.append(f"{name}: {s['species']} without level 50")
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("command", nargs="?", default="write", choices=["write", "facts", "universe", "list"])
    parser.add_argument("--check", action="store_true", help="verify instead of writing")
    args = parser.parse_args()

    if args.command == "universe":
        root = os.environ.get("LAB_ROOT") or str(ENGINE.parent)
        data = build_universe(root)
        # One line per species (its learnable moves) and per list: small diffs, small file.
        parts = [f' "source": {json.dumps(data["source"])}', ' "species": {\n' + ",\n".join(
            f"  {json.dumps(k)}: {json.dumps(v)}" for k, v in data["species"].items()) + "\n }"]
        parts += [f' "{k}": {json.dumps(data[k])}' for k in ("abilities", "moves", "items")]
        UNIVERSE.write_text("{\n" + ",\n".join(parts) + "\n}\n", encoding="utf-8", newline="\n")
        print(f"{UNIVERSE}: {len(data['species'])} species, {len(data['abilities'])} abilities, "
              f"{len(data['moves'])} moves, {len(data['items'])} items")
        return 0

    classification_file = json.loads(CLASSIFICATION.read_text(encoding="utf-8"))
    classification = classification_file["entries"]
    entries, sources = extract(classification_file["producers"])
    universe = load_universe()

    if args.command == "list":
        for key, e in entries.items():
            print(f"{key}\t{', '.join(e['sites'])}\t{','.join(e['how'])}")
        return 0

    results = evidence(universe, sources)
    if args.command == "facts":
        dex = load_dex()
        print(f"standard species ({len(universe['species'])}), abilities ({len(universe['abilities'])}), "
              f"moves ({len(universe['moves'])}), items ({len(universe['items'])})")
        print("abilities:", ", ".join(dex["abilities"][a]["name"] for a in universe["abilities"]))
        print("items:", ", ".join(dex["items"][i]["name"] for i in universe["items"]))
        print("moves:", ", ".join(dex["moves"][m]["name"] for m in universe["moves"]))
        for eid, claim, ok, detail in results:
            print(f"{eid} {'ok' if ok else 'FAILED'}: {claim}{': ' + detail if detail else ''}")
        return 0

    fixed = classification_file.get("fixed", {})
    problems = problems_of(entries, classification, universe)
    problems += [f"evidence {eid} failed: {claim} ({detail})" for eid, claim, ok, detail in results if not ok]
    for key, f in fixed.items():
        if key in entries:
            problems.append(f"`{key}` is listed as fixed but is still in the source")
        if not (SCENARIOS / f"{f['fixture']}.json").exists():
            problems.append(f"fixed `{key}`: no scenario {f['fixture']}")
    text = render(entries, classification, results, problems, fixed,
                  classification_file.get("proposed_boards", {}))
    if args.check:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            problems.append(f"{OUT.name} is stale: run python engine/scripts/refusals.py")
    else:
        OUT.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {OUT} ({len(entries)} keys)")
    for p in problems:
        print("problem:", p, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
