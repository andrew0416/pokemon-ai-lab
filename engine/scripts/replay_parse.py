"""Public Showdown replays as a parity source (board V13-replay-parity): parse.

Usage:
  python engine/scripts/replay_parse.py <run-dir> [--only ID]

Reads `<run-dir>/replays.json` and the sanitized logs (`replay_fetch.py`) and writes one game
file per replay, `<run-dir>/games/<id>.json`, for `lab-replay` (engine/search), plus
`<run-dir>/games/index.json` (per-game counts and why a game stops early).

A spectator log does not carry the sets. The teams are rebuilt as follows (recorded per set in
`provenance`):

- open team sheet (`|showteam|`, both players agreed): species, item, ability, moves, nature and
  gender as shown; the Stat Points are not shown and are assumed (below);
- otherwise: the species and gender from the switch-in details, the moves the Pokémon used
  (called moves `[from]` and moves after a Transform excluded), the item it showed first
  (`-enditem`, `[from] item:`, `-mega`, Frisk; none if it never showed one), the base ability it
  showed before any Mega Evolution (else the species' first ability), a neutral nature;
- brought Pokémon that never appeared are filled from the unseen team preview species (a
  placeholder set: the first ability, no item, Protect);
- Stat Points (never public): HP 32, the attacking stat its moves use most 32, Speed 2 (66);
  IVs 31; level 50 (the format adjusts it).

So the positions are real game situations with plausible, not true, sets: engine-vs-Showdown
parity on them is exact (both run the same file), while the log's observations can only be
matched approximately (Stat Points, unknown items and abilities).

Decisions: every turn (one action per active slot: the move the Pokémon used and its target as
printed, Mega Evolution, or a switch printed before the turn's first move; `unknown` when the
Pokémon did not act — asleep, flinched, fainted first) with its mid-turn switches (switches
printed after the first move, in order), and every end-of-turn replacement. Each carries the
observation after it: every revealed Pokémon's HP percentage as shown (Champions:
floor(100 hp / maxhp), at least 1), status, faint, slot, forme, boosts of the active ones, the
item when known, weather, terrain, pseudo-weathers and side conditions.

A game stops (the positions before it are kept) at: Illusion (`|replace|`), Ally Switch
(`|swap|`), a Transform, a forfeit/timer loss mid-turn, or a turn the parser cannot read.
"""
import argparse
import collections
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
DEX = json.loads((HERE.parent / "data" / "champions.json").read_text(encoding="utf-8"))


def toid(s):
    return re.sub(r"[^a-z0-9]", "", (s or "").lower())


def name_of(table, s):
    entry = DEX[table].get(toid(s))
    return entry["name"] if entry else s


HP_RE = re.compile(r"^(\d+)(?:/(\d+))?([gyr]?)(?: (\w+))?$")
MOVE_SWITCHERS = set()


def parse_hp(text):
    """`50/100y par` -> (pct, hint, status, fainted)."""
    text = text.strip()
    if text.startswith("0 fnt") or text == "0":
        return 0, "", "", True
    m = HP_RE.match(text)
    if not m:
        return None
    pct = int(m.group(1))
    return pct, m.group(3) or "", m.group(4) or "", False


def ident(s):
    """`p1a: Nick` -> ("p1", 0|1|None, "Nick")."""
    m = re.match(r"^(p[12])([ab]?): ?(.*)$", s.strip())
    if not m:
        return None
    slot = {"a": 0, "b": 1}.get(m.group(2))
    return m.group(1), slot, m.group(3)


def details_species(details):
    return details.split(",")[0].strip()


class Mon:
    def __init__(self, side, name, species, gender):
        self.side, self.name = side, name
        self.base_species = species
        self.species = species
        self.gender = gender
        self.pct, self.hint, self.status, self.fainted = 100, "", "", False
        self.slot = None
        self.boosts = {}
        self.item = None  # None unknown, "" none/gone, else id
        self.orig_item = None
        self.item_changed = False
        self.item_inferred = False
        self.ability = None
        self.mega = False
        self.moves = []
        self.transformed = False
        self.move_cats = collections.Counter()


class Game:
    def __init__(self, rid, lines):
        self.rid = rid
        self.lines = lines
        self.mons = {}  # (side, name) -> Mon
        self.active = {"p1": [None, None], "p2": [None, None]}
        self.preview = {"p1": [], "p2": []}
        self.showteam = {}
        self.weather = ""
        self.terrain = ""
        self.pseudo = set()
        self.sideconds = {"p1": set(), "p2": set()}
        self.leads = {"p1": [None, None], "p2": [None, None]}
        self.notes = []

    def mon(self, who, details=None):
        side, slot, name = who
        key = (side, name)
        if key not in self.mons:
            if details is None:
                return None
            parts = [p.strip() for p in details.split(",")]
            gender = next((p for p in parts[1:] if p in ("M", "F")), "")
            self.mons[key] = Mon(side, name, parts[0], gender)
        return self.mons[key]

    def obs(self):
        mons = []
        for (side, name), m in sorted(self.mons.items()):
            o = {"side": side, "name": name, "species": m.species, "fainted": m.fainted,
                 "pct": 0 if m.fainted else m.pct, "hint": m.hint, "status": "" if m.fainted else m.status,
                 "slot": m.slot}
            if m.slot is not None:
                o["boosts"] = {k: v for k, v in sorted(m.boosts.items()) if v}
            if m.item is not None:
                o["item"] = m.item
            mons.append(o)
        return {"mons": mons, "weather": self.weather, "terrain": self.terrain,
                "pseudoWeather": sorted(self.pseudo),
                "sideConditions": {s: sorted(c) for s, c in self.sideconds.items()}}


def of_mon(game, parts):
    for p in parts:
        if p.startswith("[of] "):
            who = ident(p[5:])
            if who:
                return game.mon(who)
    return None


def from_tag(parts):
    for p in parts:
        if p.startswith("[from]"):
            return p[6:].strip()
    return None


def reveal_item(m, item_name, how):
    if m is None or not item_name:
        return
    iid = toid(item_name)
    if m.orig_item is None and not m.item_changed:
        m.orig_item = iid
    # An item already known gone stays gone: a consumed item's effect is printed after its
    # `-enditem` (`-boost ...|[from] item: Electric Seed`).
    if m.item is None:
        m.item = iid


def reveal_ability(m, ability):
    if m is None or not ability:
        return
    if m.ability is None and not m.mega and not m.transformed:
        m.ability = toid(ability)


def apply_hp(m, text):
    r = parse_hp(text)
    if r is None or m is None:
        return
    m.pct, m.hint, status, fainted = r
    if fainted:
        m.fainted = True
    else:
        m.status = status


def parse_game(rid, text):
    lines = text.split("\n")
    g = Game(rid, lines)
    ots = False
    decisions = []
    stop = None
    start_obs = None
    turn_no = 0
    cur = None  # current decision being filled
    section = None  # "turn" | "replace"
    moved_this_turn = False
    last_user = [None]
    screens = {}

    def begin_turn(n):
        nonlocal cur, section, moved_this_turn
        cur = {"kind": "turn", "turn": n, "actions": {}, "mid": {"p1": [], "p2": []}, "mega": set(),
               "start_active": {s: list(g.active[s]) for s in ("p1", "p2")},
               "start_fainted": {s: [g.active[s][i] is None or g.mons[(s, g.active[s][i])].fainted for i in (0, 1)]
                                 for s in ("p1", "p2")}}
        section = "turn"
        moved_this_turn = False

    def finish_turn():
        nonlocal cur
        if cur is None or cur["kind"] != "turn":
            return
        d = {"kind": "turn", "turn": cur["turn"], "p1": [], "p2": [], "mid": cur["mid"]}
        for side in ("p1", "p2"):
            for slot in (0, 1):
                name = cur["start_active"][side][slot]
                if cur["start_fainted"][side][slot]:
                    d[side].append({"kind": "none"})
                    continue
                act = cur["actions"].get((side, slot))
                if act is None:
                    act = {"kind": "unknown"}
                if act.get("kind") in ("move", "unknown"):
                    act = dict(act, mega=(side, slot) in cur["mega"])
                d[side].append(act)
        d["obs"] = g.obs()
        decisions.append(d)
        cur = None

    def finish_replace():
        nonlocal cur
        if cur is None or cur["kind"] != "replace":
            return
        if any(cur["switches"][s] for s in ("p1", "p2")):
            d = {"kind": "replacement", "turn": cur["turn"], "p1": [], "p2": []}
            for side in ("p1", "p2"):
                for slot in (0, 1):
                    name = cur["switches"][side].get(slot)
                    d[side].append({"kind": "switch", "name": name} if name else {"kind": "none"})
            d["obs"] = g.obs()
            decisions.append(d)
        cur = None

    for raw in lines:
        if not raw.startswith("|"):
            continue
        parts = raw.split("|")[1:]
        cmd = parts[0] if parts else ""
        args = parts[1:]
        try:
            if cmd == "poke":
                g.preview[args[0]].append(details_species(args[1]))
            elif cmd == "showteam":
                ots = True
                g.showteam[args[0]] = "|".join(args[1:])
            elif cmd in ("switch", "drag"):
                who = ident(args[0])
                side, slot, name = who
                m = g.mon(who, args[1])
                old = g.active[side][slot]
                passed = {}
                if old is not None and old != name:
                    om = g.mons[(side, old)]
                    om.slot = None
                    # Baton Pass hands the boosts over (Showdown prints no boost lines for it).
                    if any("Baton Pass" in x for x in args[3:]):
                        passed = dict(om.boosts)
                    om.boosts = {}
                    if not om.mega:
                        om.species = om.base_species if not om.mega else om.species
                m.slot = slot
                m.boosts = passed
                m.species = details_species(args[1])
                g.active[side][slot] = name
                apply_hp(m, args[2])
                if turn_no == 0:
                    g.leads[side][slot] = name
                elif section == "turn" and cur is not None:
                    if cmd == "switch":
                        if not moved_this_turn:
                            start_name = cur["start_active"][side][slot]
                            cur["actions"][(side, slot)] = {"kind": "switch", "name": name}
                        else:
                            cur["mid"][side].append(name)
                    # drag: random, not a choice
                elif section == "replace" and cur is not None:
                    cur["switches"][side][slot] = name
            elif cmd == "replace":
                stop = stop or {"turn": turn_no, "reason": "Illusion (|replace|)"}
            elif cmd == "swap":
                stop = stop or {"turn": turn_no, "reason": "Ally Switch (|swap|)"}
            elif cmd == "detailschange":
                m = g.mon(ident(args[0]))
                if m:
                    m.species = details_species(args[1])
                    if "-Mega" in m.species:
                        m.mega = True
            elif cmd == "-formechange":
                m = g.mon(ident(args[0]))
                if m:
                    m.species = args[1].strip()
            elif cmd == "-mega":
                who = ident(args[0])
                m = g.mon(who)
                if m and len(args) > 2:
                    reveal_item(m, args[2], "mega")
                if cur is not None and section == "turn":
                    cur["mega"].add((who[0], who[1]))
            elif cmd == "-transform":
                m = g.mon(ident(args[0]))
                if m:
                    m.transformed = True
                stop = stop or {"turn": turn_no, "reason": "Transform"}
            elif cmd == "move":
                who = ident(args[0])
                m = g.mon(who)
                last_user[0] = m
                move = args[1]
                tag = from_tag(args[3:])
                if m and tag is None and not m.transformed and toid(move) not in ("struggle", "recharge"):
                    mid = toid(move)
                    if mid not in m.moves:
                        m.moves.append(mid)
                    cat = DEX["moves"].get(mid, {}).get("category")
                    if cat in ("Physical", "Special"):
                        m.move_cats[cat] += 1
                if section == "turn" and cur is not None:
                    moved_this_turn = True
                    key = (who[0], who[1])
                    starter = cur["start_active"][who[0]][who[1]]
                    if (tag is None or tag == "lockedmove") and key not in cur["actions"] and starter == who[2]:
                        target = None
                        t = ident(args[2]) if len(args) > 2 and args[2] else None
                        if t and t[1] is not None:
                            target = {"side": t[0], "slot": t[1]}
                        cur["actions"][key] = {"kind": "move", "move": toid(move), "target": target,
                                               "flags": [a for a in args[3:] if a]}
            elif cmd == "cant":
                if section == "turn":
                    moved_this_turn = True
            elif cmd in ("-damage", "-heal", "-sethp"):
                who = ident(args[0])
                m = g.mon(who)
                apply_hp(m, args[1])
                tag = from_tag(args[2:])
                if tag and tag.startswith("item:"):
                    holder = of_mon(g, args[2:]) or m
                    reveal_item(holder, tag[5:].strip(), "from")
                elif tag and tag.startswith("ability:"):
                    holder = of_mon(g, args[2:]) or m
                    reveal_ability(holder, tag[8:].strip())
            elif cmd == "faint":
                m = g.mon(ident(args[0]))
                if m:
                    m.fainted, m.pct, m.status = True, 0, ""
            elif cmd == "-status":
                m = g.mon(ident(args[0]))
                if m:
                    m.status = args[1]
                tag = from_tag(args[2:])
                if tag and tag.startswith("item:"):
                    reveal_item(m, tag[5:].strip(), "from")
                elif tag and tag.startswith("ability:"):
                    reveal_ability(of_mon(g, args[2:]) or m, tag[8:].strip())
            elif cmd == "-curestatus":
                m = g.mon(ident(args[0]))
                if m:
                    m.status = ""
            elif cmd == "-cureteam":
                side = ident(args[0])[0]
                for (s, _), m in g.mons.items():
                    if s == side:
                        m.status = ""
            elif cmd in ("-boost", "-unboost"):
                m = g.mon(ident(args[0]))
                if m:
                    n = int(args[2]) * (1 if cmd == "-boost" else -1)
                    m.boosts[args[1]] = max(-6, min(6, m.boosts.get(args[1], 0) + n))
                tag = from_tag(args[3:])
                if tag and tag.startswith("item:"):
                    reveal_item(m, tag[5:].strip(), "from")
                elif tag and tag.startswith("ability:"):
                    reveal_ability(of_mon(g, args[3:]) or m, tag[8:].strip())
            elif cmd == "-setboost":
                m = g.mon(ident(args[0]))
                if m:
                    m.boosts[args[1]] = int(args[2])
            elif cmd == "-clearboost":
                m = g.mon(ident(args[0]))
                if m:
                    m.boosts = {}
            elif cmd == "-clearallboost":
                for m in g.mons.values():
                    m.boosts = {}
            elif cmd == "-clearnegativeboost":
                m = g.mon(ident(args[0]))
                if m:
                    m.boosts = {k: v for k, v in m.boosts.items() if v > 0}
            elif cmd == "-clearpositiveboost":
                m = g.mon(ident(args[0]))
                if m:
                    m.boosts = {k: v for k, v in m.boosts.items() if v < 0}
            elif cmd == "-invertboost":
                m = g.mon(ident(args[0]))
                if m:
                    m.boosts = {k: -v for k, v in m.boosts.items()}
            elif cmd == "-copyboost":
                a, b = g.mon(ident(args[0])), g.mon(ident(args[1]))
                if a and b:
                    a.boosts = dict(b.boosts)
            elif cmd == "-swapboost":
                a, b = g.mon(ident(args[0])), g.mon(ident(args[1]))
                stats = [s.strip() for s in args[2].split(",")] if len(args) > 2 and args[2] and not args[2].startswith("[") else ["atk", "def", "spa", "spd", "spe", "accuracy", "evasion"]
                if a and b:
                    for s in stats:
                        a.boosts[s], b.boosts[s] = b.boosts.get(s, 0), a.boosts.get(s, 0)
            elif cmd == "-weather":
                w = args[0]
                if w == "none":
                    g.weather = ""
                else:
                    g.weather = toid(w)
                tag = from_tag(args[1:])
                if tag and tag.startswith("ability:"):
                    reveal_ability(of_mon(g, args[1:]), tag[8:].strip())
            elif cmd in ("-fieldstart", "-fieldend"):
                eff = toid(args[0].replace("move:", ""))
                if eff.endswith("terrain"):
                    g.terrain = eff if cmd == "-fieldstart" else ""
                else:
                    (g.pseudo.add if cmd == "-fieldstart" else g.pseudo.discard)(eff)
                tag = from_tag(args[1:])
                if tag and tag.startswith("ability:"):
                    reveal_ability(of_mon(g, args[1:]), tag[8:].strip())
            elif cmd in ("-sidestart", "-sideend"):
                side = args[0][:2]
                eff = toid(args[1].replace("move:", ""))
                (g.sideconds[side].add if cmd == "-sidestart" else g.sideconds[side].discard)(eff)
                if eff in SCREENS:
                    if cmd == "-sidestart":
                        screens[(side, eff)] = (turn_no, last_user[0])
                    else:
                        screens.pop((side, eff), None)
            elif cmd == "-enditem":
                m = g.mon(ident(args[0]))
                tag = from_tag(args[2:])
                if m:
                    if tag and ("Knock Off" in tag or "Thief" in tag or "Covet" in tag or "Trick" in tag):
                        reveal_item(m, args[1], "knocked")
                    else:
                        reveal_item(m, args[1], "end")
                    m.item = ""
            elif cmd == "-item":
                m = g.mon(ident(args[0]))
                tag = from_tag(args[2:]) or ""
                if m:
                    if "Frisk" in tag or tag == "":
                        reveal_item(m, args[1], "item")
                    else:
                        m.item_changed = True
                        m.item = toid(args[1])
            elif cmd == "-ability":
                m = g.mon(ident(args[0]))
                tag = from_tag(args[2:]) or ""
                if m:
                    if "Trace" in tag:
                        reveal_ability(m, "Trace")
                    elif tag.startswith("ability:"):
                        pass
                    elif not tag:
                        reveal_ability(m, args[1])
            elif cmd in ("-activate", "-immune", "-block", "-fail", "-start", "-end"):
                tag = None
                for a in args[1:]:
                    if a.startswith("ability: "):
                        reveal_ability(g.mon(ident(args[0])) if ident(args[0]) else None, a[9:])
                        break
                    if a.startswith("[from] ability: "):
                        holder = of_mon(g, args[1:]) or (g.mon(ident(args[0])) if ident(args[0]) else None)
                        reveal_ability(holder, a[16:])
                        break
                    if a.startswith("item: ") and cmd in ("-activate", "-block", "-end", "-start"):
                        reveal_item(g.mon(ident(args[0])) if ident(args[0]) else None, a[6:], "activate")
                        break
            elif cmd == "turn":
                if section == "turn":
                    finish_turn()
                elif section == "replace":
                    finish_replace()
                n = int(args[0])
                if n == 1:
                    start_obs = g.obs()
                turn_no = n
                # A screen still up 5 turns after it went up (5 turns without Light Clay):
                # its setter holds Light Clay unless it showed another item.
                for (_, eff), (t0, setter) in list(screens.items()):
                    if n - t0 >= 5 and setter is not None and setter.orig_item is None:
                        setter.orig_item = "lightclay"
                        setter.item_inferred = True
                if stop:
                    break
                begin_turn(n)
            elif cmd == "upkeep":
                if section == "turn":
                    finish_turn()
                    cur = {"kind": "replace", "turn": turn_no, "switches": {"p1": {}, "p2": {}}}
                    section = "replace"
            elif cmd == "win" or cmd == "tie":
                if section == "turn" and cur is not None:
                    finish_turn()
                elif section == "replace":
                    finish_replace()
                section = None
                break
            elif cmd == "-message" and ("forfeited" in args[0] or "lost due" in args[0]):
                if section == "turn" and cur is not None and cur["actions"]:
                    stop = stop or {"turn": turn_no, "reason": "forfeit/timer during a turn"}
                break
        except (IndexError, ValueError, KeyError, TypeError) as e:
            stop = stop or {"turn": turn_no, "reason": f"parse error: {e} on {raw[:80]}"}
            break

    # A charge turn prints no target (`[still]`, `-prepare`); the move's next turn
    # (`[from]lockedmove`) prints the one chosen on the charge turn.
    turns = [d for d in decisions if d["kind"] == "turn"]
    for a, b in zip(turns, turns[1:]):
        for side in ("p1", "p2"):
            for slot in (0, 1):
                x, y = a[side][slot], b[side][slot]
                if (x.get("kind") == "move" and x.get("target") is None and "[still]" in x.get("flags", [])
                        and y.get("kind") == "move" and y.get("move") == x["move"]
                        and "[from]lockedmove" in y.get("flags", []) and y.get("target")):
                    x["target"] = y["target"]
    if stop:
        # Drop the decision the stop happened in and everything after it.
        decisions = [d for d in decisions if d["turn"] < stop["turn"]]
    return g, ots, decisions, start_obs, stop


def parse_packed(packed):
    sets = []
    for chunk in packed.split("]"):
        if not chunk:
            continue
        f = chunk.split("|")
        sets.append({"name": f[0], "species": f[1] or f[0], "item": f[2], "ability": f[3],
                     "moves": [m for m in f[4].split(",") if m], "nature": f[5], "gender": f[7] if len(f) > 7 else ""})
    return sets


# Abilities that print a message when their holder enters: a Pokémon that entered and never
# showed one does not have it, so an unrevealed ability is the first of the species' others.
ANNOUNCED = {"intimidate", "drizzle", "drought", "sandstream", "snowwarning", "electricsurge", "grassysurge",
             "psychicsurge", "mistysurge", "pressure", "moldbreaker", "teravolt", "turboblaze", "unnerve",
             "frisk", "airlock", "cloudnine", "neutralizinggas", "trace", "forewarn", "anticipation",
             "download", "screencleaner", "curiousmedicine", "hospitality", "supersweetsyrup",
             "orichalcumpulse", "hadronengine", "desolateland", "primordialsea", "deltastream",
             "vesselofruin", "swordofruin", "tabletsofruin", "beadsofruin", "asoneglastrier",
             "asonespectrier", "zerotohero", "commander", "costar", "embodyaspectteal",
             "embodyaspecthearthflame", "embodyaspectwellspring", "embodyaspectcornerstone",
             "intrepidsword", "dauntlessshield", "slowstart", "comatose", "fairyaura", "darkaura",
             "aurabreak"}


def silent_ability(entry):
    abilities = [a for _, a in sorted((entry or {}).get("abilities", {}).items())]
    for a in abilities:
        if toid(a) not in ANNOUNCED:
            return a
    return abilities[0] if abilities else ""


SCREENS = {"reflect", "lightscreen", "auroraveil"}


def species_entry(species):
    return DEX["species"].get(toid(species))


def default_sp(species, cats):
    entry = species_entry(species) or {}
    base = entry.get("baseStats", {})
    if cats["Physical"] > cats["Special"]:
        attack = "atk"
    elif cats["Special"] > cats["Physical"]:
        attack = "spa"
    else:
        attack = "atk" if base.get("atk", 0) >= base.get("spa", 0) else "spa"
    sp = {s: 0 for s in ("hp", "atk", "def", "spa", "spd", "spe")}
    sp["hp"], sp[attack], sp["spe"] = 32, 32, 2
    return sp


IVS = {s: 31 for s in ("hp", "atk", "def", "spa", "spd", "spe")}


def build_team(g, side, ots_sets):
    """The side's four brought Pokémon (leads first) as sets, with provenance."""
    appeared = [m for (s, _), m in g.mons.items() if s == side]
    leads = [n for n in g.leads[side] if n]
    order_names = leads + [m.name for m in appeared if m.name not in leads]
    by_name = {m.name: m for m in appeared}
    team, prov = [], []
    used_species = {toid(by_name[n].base_species) for n in order_names}
    for n in order_names:
        m = by_name[n]
        base = m.base_species
        ots = None
        if ots_sets:
            ots = next((s for s in ots_sets if toid(s["species"]) == toid(base) or toid(s["species"]).startswith(toid(base.split("-")[0])) and toid(s["species"]) == toid(base)), None)
        entry = species_entry(base) or {}
        if ots:
            moves = [name_of("moves", x) for x in ots["moves"]]
            item = name_of("items", ots["item"]) if ots["item"] else ""
            ability = name_of("abilities", ots["ability"]) if ots["ability"] else entry.get("abilities", {}).get("0", "")
            nature = ots["nature"] or "Serious"
            source = "open team sheet (Stat Points assumed)"
        else:
            moves = [name_of("moves", x) for x in m.moves[:4]] or ["Protect"]
            # A filler for the turns it did not act (`unknown`): a priority-0 move whose
            # failure changes nothing, so its choice cannot jump ahead like Protect.
            if len(moves) < 4 and "Rest" not in moves:
                moves.append("Rest")
            item = name_of("items", m.orig_item) if m.orig_item else ""
            ability = name_of("abilities", m.ability) if m.ability else silent_ability(entry)
            nature = "Serious"
            source = "log (moves used, item/ability shown; rest assumed)"
        sp = default_sp(base, m.move_cats)
        team.append({"name": n, "species": name_of("species", base), "item": item, "ability": ability,
                     "gender": m.gender, "nature": nature, "evs": sp, "ivs": IVS, "level": 50, "moves": moves})
        prov.append({"name": n, "source": source, "item_shown": (m.orig_item is not None and not m.item_inferred) or bool(ots),
                     "item_inferred": "Light Clay (a screen lasted past 5 turns)" if m.item_inferred else None,
                     "ability_shown": m.ability is not None or bool(ots), "moves_shown": len(m.moves)})
    # Unseen brought members: from the unseen preview species.
    for sp_name in g.preview[side]:
        if len(team) >= 4:
            break
        if toid(sp_name) in used_species or "*" in sp_name and any(toid(t["species"]).startswith(toid(sp_name.replace("-*", ""))) for t in team):
            continue
        base = sp_name.replace("-*", "")
        entry = species_entry(base) or {}
        ots = next((s for s in (ots_sets or []) if toid(s["species"]) == toid(base)), None)
        name = name_of("species", base)
        if any(t["name"] == name for t in team):
            name = name + " (unseen)"
        if ots:
            moves = [name_of("moves", x) for x in ots["moves"]]
            item = name_of("items", ots["item"]) if ots["item"] else ""
            ability = name_of("abilities", ots["ability"])
            nature = ots["nature"] or "Serious"
        else:
            moves, item, ability, nature = ["Protect", "Rest"], "", entry.get("abilities", {}).get("0", ""), "Serious"
        team.append({"name": name, "species": name_of("species", base), "item": item, "ability": ability,
                     "gender": "", "nature": nature, "evs": default_sp(base, collections.Counter()), "ivs": IVS,
                     "level": 50, "moves": moves})
        prov.append({"name": name, "source": "unseen preview member (placeholder set)" if not ots else "open team sheet, never appeared"})
        used_species.add(toid(base))
    return team, prov


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--only")
    args = ap.parse_args()
    run = pathlib.Path(args.run_dir)
    index = json.loads((run / "replays.json").read_text(encoding="utf-8"))
    out = run / "games"
    out.mkdir(exist_ok=True)
    rows = []
    for r in index["replays"]:
        if args.only and r["id"] != args.only:
            continue
        text = (run / r["log"]).read_text(encoding="utf-8")
        g, ots, decisions, start_obs, stop = parse_game(r["id"], text)
        row = {"id": r["id"], "ots": ots, "decisions": len(decisions), "stop": stop}
        if start_obs is None or not all(g.leads[s][0] for s in ("p1", "p2")):
            row["skipped"] = "no turn 1 / leads not found"
            rows.append(row)
            continue
        teams = {}
        provenance = {}
        for side in ("p1", "p2"):
            ots_sets = parse_packed(g.showteam[side]) if side in g.showteam else None
            teams[side], provenance[side] = build_team(g, side, ots_sets)
        unknown = sum(1 for d in decisions if d["kind"] == "turn" for s in ("p1", "p2") for a in d[s] if a["kind"] == "unknown")
        row.update({"turns": sum(1 for d in decisions if d["kind"] == "turn"), "unknown_actions": unknown,
                    "mid_switches": sum(len(d["mid"]["p1"]) + len(d["mid"]["p2"]) for d in decisions if d["kind"] == "turn")})
        game = {"id": r["id"], "url": r["url"], "format": index["format"], "ots": ots,
                "p1": {"team": teams["p1"], "order": "1234"}, "p2": {"team": teams["p2"], "order": "1234"},
                "provenance": provenance, "startObs": start_obs, "decisions": decisions, "stop": stop}
        (out / f"{r['id']}.json").write_text(json.dumps(game, indent=1, ensure_ascii=False), encoding="utf-8")
        rows.append(row)
    (out / "index.json").write_text(json.dumps(rows, indent=1, ensure_ascii=False), encoding="utf-8")
    n = len(rows)
    print(f"{n} replays: {sum(1 for r in rows if not r.get('skipped'))} games written, "
          f"{sum(1 for r in rows if r['ots'])} with open team sheets, "
          f"{sum(r['decisions'] for r in rows)} decisions, {sum(1 for r in rows if r.get('stop'))} stopped early")


if __name__ == "__main__":
    sys.exit(main())
