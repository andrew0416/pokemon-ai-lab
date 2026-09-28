"""The Python API against the Rust side and the Showdown oracle fixtures.

Runs against the wheel installed in the venv (`maturin build --release` + `pip install`), from
the repository root or anywhere: paths resolve from this file. Reference numbers from
`lab-plan` (2026-09-27, engine commit of this test) are noted where they are used.
"""

import json
import pathlib

import pytest

import lab_engine

ENGINE = pathlib.Path(__file__).resolve().parents[2]
SCENARIOS = ENGINE / "oracle" / "scenarios"
EXPECTED = ENGINE / "oracle" / "expected"


def key(state):
    """An order-independent key of a canonical state (a parsed JSON object)."""
    return json.dumps(state, sort_keys=True, separators=(",", ":"))


def fixture(name, kind="turn"):
    return json.loads((EXPECTED / f"{name}.{kind}.json").read_text(encoding="utf-8"))


def scenario(name):
    return lab_engine.load_scenario(SCENARIOS / f"{name}.json")


def start(name, fx):
    """The scenario and its position whose canonical JSON is the fixture's `before`."""
    sc = scenario(name)
    before = key(fx["before"])
    matches = [
        pos for _, pos in sc.positions() if key(json.loads(pos.state_json())) == before
    ]
    assert len(matches) == 1, f"{name}: {len(matches)} positions match the oracle's before"
    return sc, matches[0]


def distribution(pairs):
    out = {}
    for p, pos in pairs:
        k = key(json.loads(pos.state_json()))
        out[k] = out.get(k, 0.0) + p
    return out


def oracle_distribution(fx):
    out = {}
    for o in fx["outcomes"]:
        k = key(o["state"])
        out[k] = out.get(k, 0.0) + o["p"]
    return out


def assert_same_distribution(engine, oracle):
    assert len(engine) == len(oracle)
    for k, p in oracle.items():
        assert k in engine, "the engine lacks an oracle outcome"
        assert abs(engine[k] - p) < 1e-12


def test_load_and_positions():
    sc = scenario("hypnosis-gravity")
    assert sc.format == "doubles"
    assert sc.showdown_format == "gen9championsdoublescustomgame"
    assert "Hypnosis" in sc.description
    assert sc.turn == ("move hypnosis 1, move fakeout 2", "move rockslide, move protect")
    assert sc.names(0) == ["Gardevoir", "Rillaboom"]
    assert sc.names("p2") == ["Tyranitar", "Excadrill"]
    positions = sc.positions()
    # Trace copies Sand Stream or Sand Rush: two starts at 1/2.
    assert [p for p, _ in positions] == [0.5, 0.5]
    for _, pos in positions:
        assert pos.format == "doubles"
        assert pos.turn == 1
        assert pos.decision() == "turn"
        assert pos.winner() is None
        assert pos.active(0) == ["Gardevoir", "Rillaboom"]
        assert pos.switch_order(1) == ["Tyranitar", "Excadrill"]
    # The same scenario from JSON text, team paths against base_dir.
    text = (SCENARIOS / "hypnosis-gravity.json").read_text(encoding="utf-8")
    again = lab_engine.load_scenario(text, base_dir=str(SCENARIOS))
    assert [pos.state_json() for _, pos in again.positions()] == [
        pos.state_json() for _, pos in positions
    ]
    with_bom = lab_engine.load_scenario("﻿" + text, base_dir=SCENARIOS)
    assert with_bom.turn == sc.turn


@pytest.mark.parametrize(
    "name,kind",
    [
        ("single-hit", "turn"),
        ("hypnosis-gravity", "turn"),
        ("cc-lib-psy-cona-vs-sand-owen", "extremes"),
    ],
)
def test_state_json_is_the_oracle_canonical_form(name, kind):
    start(name, fixture(name, kind))


def test_legal_choice_counts_match_lab_plan():
    # lab-plan cc-lib-psy-cona-vs-sand-owen.json --rolls median: "54 choices for us" (p1),
    # "110" (p2); with --all-targets 78 and 178.
    [(_, pos)] = scenario("cc-lib-psy-cona-vs-sand-owen").positions()
    for side, sensible, everything in [(0, 54, 78), (1, 110, 178)]:
        assert len(pos.legal_choices(side, pruning="sensible")) == sensible
        choices = pos.legal_choices(side)
        assert len(choices) == everything
        assert len(set(choices)) == everything
    assert pos.legal_choices("p1") == pos.legal_choices(0)
    assert "move followme, move expandingforce 2" in pos.legal_choices(0)
    assert "move knockoff 1 mega, move highhorsepower 2" in pos.legal_choices(1)


@pytest.mark.parametrize(
    "name,decision,after",
    [
        ("hypnosis-gravity", "turn", "turn"),
        ("single-hit", "turn", "turn"),
        ("ko-replace", "replace", "turn"),
        ("uturn-pause", "turn", "mid_turn_switch"),
    ],
)
def test_exact_parity_with_the_oracle(name, decision, after):
    fx = fixture(name)
    sc, pos = start(name, fx)
    assert pos.decision() == decision
    p1, p2 = sc.turn
    children = pos.enumerate(p1, p2, rolls="full")
    assert abs(sum(p for p, _ in children) - 1.0) < 1e-12
    # One child per distinct end state, as the Rust `enumerate_turn` merges them; here each is
    # also a distinct canonical state, so the count is the oracle's.
    assert len(children) == len(fx["outcomes"])
    assert_same_distribution(distribution(children), oracle_distribution(fx))
    assert {child.decision() for _, child in children} == {after}


def test_extremes_parity_on_a_library_turn():
    name = "cc-lib-psy-cona-vs-sand-owen"
    fx = fixture(name, "extremes")
    sc, pos = start(name, fx)
    p1, p2 = sc.turn
    children = pos.enumerate(p1, p2, rolls="extremes")
    assert_same_distribution(distribution(children), oracle_distribution(fx))


def test_mid_turn_switch_resumes_like_the_oracle():
    # uturn-pause stops after U-turn; resuming with "switch 3" is the uturn-switch scenario
    # (same teams, midTurn p1 ["switch 3"]).
    sc, pos = start("uturn-pause", fixture("uturn-pause"))
    p1, p2 = sc.turn
    final = []
    for p, paused in pos.enumerate(p1, p2):
        assert paused.decision() == "mid_turn_switch"
        assert paused.legal_choices(0) == ["switch 3"]
        assert paused.legal_choices(1) == [""]
        for q, resumed in paused.enumerate("switch 3", ""):
            assert resumed.decision() == "turn"
            final.append((p * q, resumed))
        assert paused.sample("switch 3", "", seed=5).decision() == "turn"
    assert_same_distribution(distribution(final), oracle_distribution(fixture("uturn-switch")))


def test_replacement_then_turn():
    sc, pos = start("ko-replace", fixture("ko-replace"))
    assert pos.decision() == "replace"
    for side in (0, 1):
        assert "switch 3" in pos.legal_choices(side)
        assert pos.active(side).count(None) == 1
    children = pos.enumerate("switch 3", "switch 3")
    assert [p for p, _ in children] == [0.5, 0.5]
    for _, child in children:
        assert child.decision() == "turn"
        assert None not in child.active(0)
        assert child.legal_choices(0)


def test_sample_is_reproducible():
    sc, pos = start("single-hit", fixture("single-hit"))
    p1, p2 = sc.turn
    exact = distribution(pos.enumerate(p1, p2))
    for seed in (1, 2, 20260927):
        a = pos.sample(p1, p2, seed)
        b = pos.sample(p1, p2, seed=seed)
        assert a == b
        assert hash(a) == hash(b)
        assert a.state_json() == b.state_json()
        assert key(json.loads(a.state_json())) in exact
    draws = {pos.sample(p1, p2, seed).state_json() for seed in range(40)}
    assert len(draws) > 1


def test_errors():
    sc, pos = start("hypnosis-gravity", fixture("hypnosis-gravity"))
    p1, p2 = sc.turn
    for bad in ["move", "move splash, move protect", "switch 9, move protect"]:
        with pytest.raises(ValueError):
            pos.enumerate(bad, p2)
    # Parses, but Gardevoir holds no Mega Stone.
    with pytest.raises(ValueError):
        pos.enumerate("move hypnosis 1 mega, move fakeout 2", p2)
    with pytest.raises(ValueError):
        pos.enumerate(p1, p2, rolls="most")
    with pytest.raises(ValueError):
        pos.legal_choices(2)
    with pytest.raises(OSError):
        lab_engine.load_scenario(SCENARIOS / "no-such-scenario.json")
    with pytest.raises(ValueError, match="doubles format"):
        lab_engine.load_scenario(SCENARIOS / "hypnosis-gravity.json", format="singles")

    # Metronome is not implemented: left out of the legal choices, Unsupported when named.
    team = [
        {"species": "Clefable", "ability": "Magic Guard", "nature": "Serious",
         "evs": {"hp": 32}, "moves": ["Metronome", "Protect"], "level": 50},
        {"species": "Snorlax", "ability": "Thick Fat", "nature": "Serious",
         "evs": {"hp": 32}, "moves": ["Protect"], "level": 50},
    ]
    text = json.dumps({
        "format": "gen9championsdoublescustomgame",
        "p1": {"team": team, "order": "12"},
        "p2": {"team": team, "order": "12"},
    })
    [(_, metronome)] = lab_engine.load_scenario(text).positions()
    assert not any("metronome" in c for c in metronome.legal_choices(0))
    with pytest.raises(lab_engine.Unsupported) as info:
        metronome.enumerate("move metronome, move protect", "move protect, move protect")
    assert isinstance(info.value, lab_engine.EngineError)
    assert "not implemented" in str(info.value)
    with pytest.raises(lab_engine.Unsupported):
        metronome.sample("move metronome, move protect", "move protect, move protect", 1)


def test_evaluate_and_features():
    sc, pos = start("hypnosis-gravity", fixture("hypnosis-gravity"))
    assert pos.evaluate() == 0.0
    p1, p2 = sc.turn
    _, child = pos.enumerate(p1, p2)[0]
    value = child.evaluate()
    assert value != 0.0
    assert child.evaluate(1) == -value
    assert child.evaluate("p2") == -value
    assert child.evaluate(weights=list(lab_engine.HEURISTIC_WEIGHTS)) == pytest.approx(value)
    assert child.evaluate(weights={}) == value
    assert child.evaluate(weights={"sleep": -90.0}) > value
    features = child.features()
    assert len(features) == len(lab_engine.FEATURE_NAMES) == len(lab_engine.HEURISTIC_WEIGHTS)
    dot = sum(f * w for f, w in zip(features, lab_engine.HEURISTIC_WEIGHTS))
    assert dot == pytest.approx(value, abs=1e-3)
    with pytest.raises(ValueError):
        child.evaluate(weights=[1.0, 2.0])
    with pytest.raises(ValueError):
        child.evaluate(weights={"nonsense": 1.0})
    tyranitar = next(m for m in child.party(1) if m["name"] == "Tyranitar")
    assert tyranitar["status"] == "slp"
    assert tyranitar["slot"] == 0
    assert tyranitar["species"] == "Tyranitar"
    assert ("rockslide", tyranitar["moves"][0][1]) in tyranitar["moves"]
    hp = dict((name, (now, most)) for name, now, most in child.hp(0))
    assert set(hp) == {"Gardevoir", "Rillaboom"}
    assert all(0 < now <= most for now, most in hp.values())


def test_nash_matrix_solver():
    pennies = lab_engine.nash([[1.0, -1.0], [-1.0, 1.0]], iterations=4000, tol=1e-3)
    assert abs(pennies.value) < 0.02
    assert pennies.rows[0] == pytest.approx(0.5, abs=0.05)
    dominant = lab_engine.nash([[3.0, 2.0], [1.0, 0.0]])
    assert dominant.rows[0] > 0.99
    assert dominant.maximin == (0, 2.0)
    for bad in ([], [[]], [[1.0, 2.0], [3.0]]):
        with pytest.raises(ValueError):
            lab_engine.nash(bad)


def test_nash_turn_matches_lab_plan():
    # lab-plan uturn-pause.json --side p1 --rolls median --solve nash:
    # "matrix 7x3; equilibrium value +142.3".
    [(_, pos)] = scenario("uturn-pause").positions()
    small = pos.nash_turn(0, rolls="median")
    assert (len(small.ours), len(small.theirs)) == (7, 3)
    assert len(small.matrix) == 7 and all(len(row) == 3 for row in small.matrix)
    assert small.value == pytest.approx(142.3, abs=0.1)
    assert small.decision == "turn" and small.side == "p1"
    assert small.unsupported == []
    # lab-plan hypnosis-gravity.json --side p1 --position 0 --rolls median --solve nash:
    # "matrix 49x36; equilibrium value +20.0", top choice 23.8% "move focusblast 2, move
    # fakeout 2".
    _, pos = scenario("hypnosis-gravity").positions()[0]
    big = pos.nash_turn("p1", rolls="median")
    assert (len(big.ours), len(big.theirs)) == (49, 36)
    assert big.value == pytest.approx(20.0, abs=0.1)
    choice, p = big.our_strategy()[0]
    assert choice == "move focusblast 2, move fakeout 2"
    assert p == pytest.approx(0.238, abs=0.002)
    assert sum(big.rows) == pytest.approx(1.0, abs=1e-4)
    assert big.their_strategy(min=0.0)[0][1] == max(big.cols)


def test_walk_a_game_to_the_end():
    # Both sides take their first legal choice, chance by seed, until the battle ends.
    [(_, pos)] = scenario("uturn-pause").positions()
    for step in range(200):
        if pos.decision() == "finished":
            break
        p1 = pos.legal_choices(0)[0]
        p2 = pos.legal_choices(1)[0]
        pos = pos.sample(p1, p2, seed=step)
    assert pos.decision() == "finished"
    assert pos.winner() in ("p1", "p2", "tie")
    assert pos.legal_choices(0) == []
    json.loads(pos.state_json())
