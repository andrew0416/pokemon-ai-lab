# lab_engine (Python)

Python bindings of the lab-engine turn engine (pyo3 0.29, abi3 for Python ≥ 3.12). The use is
the one `engine/DESIGN.md` "탐색의 용도와 정보 모델" describes: offline planning against a known
team. Load an oracle scenario, list each side's legal choices as Showdown choice strings, get
the exact outcome distribution of a pair of choices (or one sampled outcome), step through
replacements and mid-turn switches, read a position as canonical JSON, evaluate it, and solve
the one-turn matrix game.

The logic is in `engine/search/src/node.rs` (`lab_search::node`, feature `scenario`, unit-tested
with `cargo test -p lab-search`); `src/lib.rs` only converts arguments, errors and results.
Doubles only for now: every `format` argument is `"doubles"` (`"singles"` raises
`NotImplementedError` until the loader builds `State<1>`).

## Build and install

```bash
cd engine/py
../../.venv-doubles/Scripts/maturin.exe build --release -i ../../.venv-doubles/Scripts/python.exe --out D:/cargo-target/wheels
../../.venv-doubles/Scripts/python.exe -m pip install --force-reinstall --no-deps D:/cargo-target/wheels/lab_engine-0.1.0-cp312-abi3-win_amd64.whl
cd ../..
PYTHONUTF8=1 .venv-doubles/Scripts/python.exe -m pytest engine/py/tests   # against the installed wheel
```

Use a release build: a turn is enumerated in the extension, and a debug build is many times
slower. Long calls (`positions`, `enumerate`, `sample`, `nash_turn`, `nash`) release the GIL.

## Example

```python
import lab_engine

sc = lab_engine.load_scenario("engine/oracle/scenarios/hypnosis-gravity.json")
(weight, pos), *_ = sc.positions()      # [(probability, Position)]: Trace makes two starts (0.5 each)
pos.decision()                          # "turn"
pos.legal_choices(0)[:2]                # ['move hypnosis -2, move fakeout -1', 'move hypnosis 1, move fakeout -1']
p1, p2 = sc.turn                        # the scenario's own choices
for p, child in pos.enumerate(p1, p2, rolls="full"):
    print(p, child.evaluate(0), child.party(1)[0]["status"])   # 1/3 and 2/3 (sleep 3 or 2 turns), 55.0, slp
nxt = pos.sample(p1, p2, seed=1)        # one outcome, exact chance, reproducible
nxt.state_json()                        # '{"schema":1,"turn":2,"ended":false,...}' (engine/oracle/canonical.cjs)
eq = pos.nash_turn(0, rolls="median")   # lab-plan --solve nash: <NashTurn p1 turn 49x36 value +20.0 ...>
eq.our_strategy()[:1]                   # [('move focusblast 2, move fakeout 2', 0.238...)]

# Play a game out: each side's first sensible choice, chance by seed (replacements and
# mid-turn switches are decisions like turns; a side not asked answers "").
while pos.decision() != "finished":
    a, b = pos.legal_choices(0, "sensible")[0], pos.legal_choices(1, "sensible")[0]
    pos = pos.sample(a, b, seed=pos.turn)
pos.winner()                            # 'p2' after 3 turns
```

## API

### Module

| Name | Description |
|---|---|
| `load_scenario(source, format="doubles", base_dir=None) -> Scenario` | An oracle scenario (`engine/oracle/scenarios/*.json` format): a path (str or `os.PathLike`; team paths relative to the file) or the JSON text (a str starting with `{`; team paths relative to `base_dir`, default the current directory). Formats: `gen9championsdoublescustomgame` and `gen9championsvgc2026regmc` (team preview keeps 4). File errors raise `OSError`, bad scenarios `ValueError`. |
| `nash(matrix, iterations=20000, tol=0.01) -> Equilibrium` | Regret matching plus on a zero-sum payoff matrix (rows maximize), the solver behind `lab-plan --solve nash`. |
| `version() -> str`, `slots(format) -> int` | Crate version; active slots per side (`"doubles"` 2, `"singles"` 1). |
| `FEATURE_NAMES: list[str]`, `HEURISTIC_WEIGHTS: list[float]` | The evaluation features and the heuristic's weights (same order). |
| `EngineError(Exception)`, `Unsupported(EngineError)` | `Unsupported`: the engine does not implement something the step needs (a move, ability, item, switch-in handler, or a state without a canonical form). Skip the pair, as `lab-plan` does. Bad arguments, unparsable or illegal choices raise `ValueError`. |

### `Scenario`

| Member | Description |
|---|---|
| `positions(setup_rolls="full", lenient=False) -> list[tuple[float, Position]]` | The positions the scenario's decision is made in, with probabilities: every outcome of the leads' switch-ins and of the `setupTurns` (replayed with `setup_rolls`; `full` is exact, `median`/`extremes`/`quartiles` approximate), then the `patch`. `lenient=True` drops replayed branches in which a setup turn's choices are illegal (`lab-plan --setup-lenient`). |
| `description`, `format` (`"doubles"`), `showdown_format` | Metadata. |
| `turn -> (p1, p2) or None`, `mid_turn -> (list, list)`, `setup_turns -> list[(p1, p2)]` | The scenario's choice strings. |
| `names(side) -> list[str]` | Display names in team preview (party) order. |
| `from_canonical(state) -> Position` | A position built from canonical JSON (a dict or the text; `x-hidden` optional), the teams supplying what the canonical form leaves out (`lab_scenario::state_from_canonical`). Hidden state takes the defaults listed in `engine/scenario/src/from_canonical.rs`; what has none (a transformed Pokémon, a pending future move, volatiles whose source or payload is not canonical such as Leech Seed or Protosynthesis, a mid-turn switch) raises `Unsupported` unless `x-hidden` gives it. Genders left to chance stay undecided (use `Position.from_canonical`). The rebuilt state must print the given canonical state, else `ValueError`. |

### `Position`

Immutable; every step returns new positions. `side` is `0`/`1` or `"p1"`/`"p2"`.
Positions compare equal (and hash equally) when the battle is the same: state, party orders,
suspended turn.

| Member | Description |
|---|---|
| `decision() -> str` | `"turn"`, `"replace"` (fainted Pokémon to replace before the next turn), `"mid_turn_switch"` (a turn suspended by U-turn, Eject Button, ...), `"finished"`. |
| `winner() -> str or None` | `"p1"`, `"p2"`, `"tie"`, or `None` while the battle goes on. |
| `turn`, `format` | Showdown's turn counter (1 at the first decision); `"doubles"`. |
| `legal_choices(side, pruning="all") -> list[str]` | Showdown choice strings: `"move hypervoice, move protect"`, `"move 2 -1 mega"`, `"move knockoff 1, switch 3"`, `"switch 3"`. Targets: positive = foe slot, negative = ally slot. `switch N` counts `switch_order(side)`. A side the decision does not ask has the one choice `""`; a finished battle has none. `pruning="sensible"` drops damaging moves aimed at the ally (lab-plan's default). Moves the engine does not implement are left out (naming one raises `Unsupported`). |
| `enumerate(p1, p2, rolls="full") -> list[tuple[float, Position]]` | The exact outcome distribution of the two choice strings at this decision (the one the position asks for), one entry per distinct end state, probabilities summing to 1. `rolls`: `full` (exact: all 16 damage rolls), `extremes` (min/max at 1/2), `quartiles`, `median` (one roll), `pessimistic-p1` / `pessimistic-p2` (min roll for that side's attacks, max against it); everything else (accuracy, crits, secondary effects, Speed ties) stays exact. A turn with two spread moves can have millions of exact outcomes; use `extremes` or `median` there. A turn that stops for a mid-turn switch yields `"mid_turn_switch"` positions; answer them with `enumerate`/`sample` again (`"switch 3"` for the asked side, `""` for the other). |
| `sample(p1, p2, seed) -> Position` | One outcome drawn with exact chance (every damage roll; the rolls mode does not apply). The same seed gives the same position. |
| `state_json(hidden=False) -> str` | The state in the oracle's canonical JSON (schema 1): the exact string `canonicalKey` of `engine/oracle/canonical.cjs` gives for the same Showdown position. `hidden=True` appends one member, `"x-hidden"`, with the engine state the canonical form leaves out (party order, hazard order, volatile payloads, slot history, ...) where it differs from the defaults; the rest of the string is unchanged, and `from_canonical` of it gives back this exact position. |
| `from_canonical(state) -> Position` | Another position of the same battle from canonical JSON (a dict or the text; see `Scenario.from_canonical`), this position supplying the set data and genders. Edit `state_json(hidden=True)` and put it back to change a position by hand. |
| `with_patch(patch) -> Position` | This position with an oracle `patch` applied (the scenario files' `patch`: `p1`/`p2` per name `hp`, `status`, `statusTime`, `boosts`, `item`; `sides` conditions; `field` weather, terrain, `pseudoWeather`). |
| `evaluate(side=0, weights=None) -> float` | The heuristic evaluation (`lab_engine::eval::Heuristic`, `lab-plan --eval heuristic`) from `side`'s point of view in HP-bar units (100 = one full bar). `weights`: a dict by feature name (unnamed features keep the heuristic weight) or a full list in `FEATURE_NAMES` order. It scores material even after the battle ended; check `winner()`. |
| `features() -> list[float]` | The evaluation features, p1's counts minus p2's. |
| `party(side) -> list[dict]` | Per member in party order: `name`, `species` (current forme), `hp`, `max_hp`, `status` (Showdown id: `""`, `brn`, `frz`, `par`, `psn`, `tox`, `slp`, `fnt`), `item`, `ability` (ids), `moves` (`[(id, pp)]`), `slot` (active slot or `None`). |
| `hp(side) -> list[tuple[str, int, int]]` | `(name, hp, max_hp)` per member. |
| `active(side) -> list[str or None]`, `switch_order(side) -> list[str]` | Names per active slot; names in Showdown's party order (what `switch N` counts). |
| `nash_turn(side=0, rolls="median", depth=1, pruning="sensible", threads=0, weights=None) -> NashTurn` | The one-turn matrix game from `side`'s point of view (`lab-plan --solve nash`): every pair of choices valued after the chance node (`depth` turns of maximin below it) by the heuristic, solved by regret matching. Real teams take seconds per call (hypnosis-gravity 49×36: about 1 s release on 6 cores). |

### `NashTurn` and `Equilibrium`

`NashTurn`: `side`, `decision`, `ours`/`theirs` (choice strings, the matrix rows/columns),
`matrix` (payoffs from `side`), `rows`/`cols` (equilibrium strategies), `value`,
`exploitability`, `iterations`, `maximin` (`(row, value)`), `unsupported` (effects the engine
refused in some cell; those columns, then rows, were dropped), `omitted_ours`,
`omitted_theirs`, `nodes`, `turns`, `elapsed`; `our_strategy(min=0.01)` and
`their_strategy(min=0.01)` list `(choice, probability)` most likely first.
`Equilibrium`: `rows`, `cols`, `value`, `exploitability`, `iterations`, `maximin`.

## What is not exposed yet

- Singles: the loader builds doubles states only; `Position` dispatches through an enum so a
  singles variant can be added without changing the Python API.
- Building a position without a scenario file (teams + state by hand), and editing a position.
- The deeper searches of `lab-plan` (`--solve maximin|deep|deep-nash`, `--plan`, opponent models
  ② and ③) and `lab-rollout`; they are reachable through the binaries.
- Raw instruction lists (`Outcome::instructions`) and the `Suspension` internals.
