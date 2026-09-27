"""Hand-built and edited positions (board PY2): `Scenario.from_canonical`,
`Position.from_canonical`, `Position.with_patch`, `state_json(hidden=True)`.

The oracle check: a position built from Showdown's own canonical `before` (not from the
scenario loader) gives the fixture's exact outcome distribution.
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


def scenario(name):
    return lab_engine.load_scenario(SCENARIOS / f"{name}.json")


def distribution(pairs):
    out = {}
    for p, pos in pairs:
        k = key(json.loads(pos.state_json()))
        out[k] = out.get(k, 0.0) + p
    return out


@pytest.mark.parametrize("name", ["hypnosis-gravity", "ko-replace", "uturn-pause"])
def test_showdown_before_state_gives_the_oracle_distribution(name):
    fx = fixture(name)
    sc = scenario(name)
    pos = sc.from_canonical(fx["before"])
    assert key(json.loads(pos.state_json())) == key(fx["before"])
    p1, p2 = sc.turn
    engine = distribution(pos.enumerate(p1, p2, rolls="full"))
    oracle = {}
    for o in fx["outcomes"]:
        oracle[key(o["state"])] = oracle.get(key(o["state"]), 0.0) + o["p"]
    assert engine.keys() == oracle.keys()
    for k, p in oracle.items():
        assert abs(engine[k] - p) < 1e-12


def test_hidden_state_round_trip_gives_the_same_position():
    sc = scenario("single-hit")
    for _, pos in sc.positions():
        p1, p2 = sc.turn
        for _, child in pos.enumerate(p1, p2, rolls="extremes"):
            text = child.state_json(hidden=True)
            plain = child.state_json()
            if "x-hidden" in json.loads(text):
                assert text.startswith(plain[:-1])
            else:
                assert text == plain
            back = child.from_canonical(text)
            assert back == child
            assert hash(back) == hash(child)
            # A dict works as well as the text.
            assert child.from_canonical(json.loads(text)) == child


def test_edit_a_position_by_hand():
    sc = scenario("hypnosis-gravity")
    (_, pos), *_ = sc.positions()
    state = json.loads(pos.state_json(hidden=True))
    for mon in state["sides"][1]["pokemon"]:
        if mon["slot"] is not None:
            mon["hp"] = 1
    state["field"]["pseudoWeather"]["trickroom"] = {"duration": 2}
    edited = pos.from_canonical(state)
    actives = [hp for name, hp, _ in edited.hp(1) if name in edited.active(1)]
    assert actives and all(hp == 1 for hp in actives)
    pseudo = json.loads(edited.state_json())["field"]["pseudoWeather"]
    assert pseudo == state["field"]["pseudoWeather"]
    assert pseudo["trickroom"] == {"duration": 2}
    p1, p2 = sc.turn
    children = edited.enumerate(p1, p2, rolls="median")
    assert abs(sum(p for p, _ in children) - 1.0) < 1e-12


def test_with_patch_is_the_scenario_patch():
    # hypnosis-gravity's own `patch` applied by hand to the unpatched scenario's positions.
    raw = json.loads((SCENARIOS / "hypnosis-gravity.json").read_text(encoding="utf-8"))
    patch = raw.pop("patch")
    unpatched = lab_engine.load_scenario(json.dumps(raw), base_dir=str(SCENARIOS))
    patched = [pos for _, pos in scenario("hypnosis-gravity").positions()]
    by_hand = [pos.with_patch(patch) for _, pos in unpatched.positions()]
    assert by_hand == patched
    assert by_hand[0].with_patch(json.dumps({})) == by_hand[0]


def test_refusals():
    sc = scenario("hypnosis-gravity")
    (_, pos), *_ = sc.positions()
    state = json.loads(pos.state_json())
    seeded = json.loads(json.dumps(state))
    active = next(m for m in seeded["sides"][0]["pokemon"] if m["slot"] is not None)
    active["volatiles"]["leechseed"] = {}
    with pytest.raises(lab_engine.Unsupported, match="x-hidden"):
        pos.from_canonical(seeded)
    renamed = json.loads(json.dumps(state))
    renamed["sides"][0]["pokemon"][0]["name"] = "Nobody"
    with pytest.raises(ValueError):
        sc.from_canonical(renamed)
    with pytest.raises(ValueError):
        pos.from_canonical("{not json")
    with pytest.raises(ValueError):
        pos.with_patch({"p1": {"Nobody": {"hp": 1}}})
