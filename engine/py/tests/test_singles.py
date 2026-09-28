"""Singles through the Python API (board PY1): `load_scenario(..., format="singles")` gives
`State<1>` positions; the steps are the doubles ones. Checked against the oracle fixtures of
II-singles-loader.
"""

import json
import pathlib

import pytest

import lab_engine

ENGINE = pathlib.Path(__file__).resolve().parents[2]
SCENARIOS = ENGINE / "oracle" / "scenarios"
EXPECTED = ENGINE / "oracle" / "expected"


def key(state):
    return json.dumps(state, sort_keys=True, separators=(",", ":"))


def fixture(name):
    return json.loads((EXPECTED / f"{name}.turn.json").read_text(encoding="utf-8"))


def singles(name):
    return lab_engine.load_scenario(SCENARIOS / f"{name}.json", format="singles")


@pytest.mark.parametrize("name", ["ae-singles-hit", "ae-singles-switch"])
def test_singles_turn_matches_the_oracle(name):
    fx = fixture(name)
    sc = singles(name)
    assert sc.format == "singles"
    (p, pos), = sc.positions()
    assert p == 1.0
    assert pos.format == "singles"
    assert key(json.loads(pos.state_json())) == key(fx["before"])
    assert len(pos.active(0)) == 1
    p1, p2 = sc.turn
    engine = {}
    for q, child in pos.enumerate(p1, p2, rolls="full"):
        k = key(json.loads(child.state_json()))
        engine[k] = engine.get(k, 0.0) + q
    oracle = {}
    for o in fx["outcomes"]:
        oracle[key(o["state"])] = oracle.get(key(o["state"]), 0.0) + o["p"]
    assert engine.keys() == oracle.keys()
    assert all(abs(engine[k] - v) < 1e-12 for k, v in oracle.items())


def test_singles_choices_steps_and_search():
    sc = singles("ae-singles-hit")
    assert sc.names(0) == ["Garchomp", "Machamp", "Chansey"]
    (_, pos), = sc.positions()
    choices = pos.legal_choices(0)
    assert "switch 2" in choices and "switch 3" in choices
    assert any(c.startswith("move dragonclaw") for c in choices)
    assert all("," not in c for c in choices)
    assert pos.sample(*sc.turn, seed=3) == pos.sample(*sc.turn, seed=3)
    eq = pos.nash_turn(0, rolls="median")
    assert eq.decision == "turn"
    assert len(eq.ours) == len(choices) or eq.omitted_ours
    # Hand-built positions work in singles too.
    assert pos.from_canonical(pos.state_json(hidden=True)) == pos
    # A walk to the end.
    while pos.decision() != "finished":
        a, b = pos.legal_choices(0, "sensible")[0], pos.legal_choices(1, "sensible")[0]
        pos = pos.sample(a, b, seed=pos.turn)
    assert pos.winner() in ("p1", "p2", "tie")


def test_the_format_must_match_the_file():
    with pytest.raises(ValueError, match="singles format"):
        lab_engine.load_scenario(SCENARIOS / "ae-singles-hit.json")
    with pytest.raises(ValueError, match="doubles format"):
        lab_engine.load_scenario(SCENARIOS / "single-hit.json", format="singles")
    with pytest.raises(ValueError):
        lab_engine.load_scenario(SCENARIOS / "single-hit.json", format="triples")
    assert lab_engine.slots("singles") == 1
