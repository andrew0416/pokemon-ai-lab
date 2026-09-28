"""The search API (board PY3b): `Position.maximin / nash / deep / deep_nash / plan` and
`Scenario.believed / rollout` return plain dicts that `json.dumps` writes as they are (no NaN:
a value never computed is None), their choices are the position's own choice strings, and they
agree with the matrix-game object `nash_turn` and with each other where they must.

Runs against the installed wheel (see README). Values are evaluator scores, not win rates; the
scenario is the oracle's small `eject-button-uturn` (3 x 5 choices at the root).
"""

import json
import pathlib

import pytest

import lab_engine

ENGINE = pathlib.Path(__file__).resolve().parents[2]
SCENARIO = ENGINE / "oracle" / "scenarios" / "eject-button-uturn.json"
TEAMS = ENGINE / "search" / "tests" / "data"


def position():
    sc = lab_engine.load_scenario(SCENARIO)
    (_, pos), = sc.positions(setup_rolls="median")
    return sc, pos


def strict_json(report):
    """The report as JSON with NaN refused, parsed back."""
    return json.loads(json.dumps(report, allow_nan=False))


def test_search_modes_are_json_with_legal_choices():
    _, pos = position()
    ours = set(pos.legal_choices(0, "sensible"))
    theirs = set(pos.legal_choices(1, "sensible"))
    maximin = pos.maximin(threads=2)
    assert strict_json(maximin) == maximin
    assert maximin["mode"] == "maximin"
    assert {line["ours"] for line in maximin["lines"]} <= ours
    assert maximin["value"] == max(line["value"] for line in maximin["lines"])

    nash = pos.nash(threads=2)
    assert strict_json(nash) == nash
    assert set(nash["ours"]) <= ours and set(nash["theirs"]) <= theirs
    turn = pos.nash_turn(0, threads=2)
    assert nash["value"] == pytest.approx(turn.value, abs=1e-5)
    assert nash["ours"] == turn.ours
    assert len(nash["matrix"]) == len(turn.ours)
    assert sum(p for _, p in nash["our_strategy"]) == pytest.approx(1.0, abs=0.05)
    lazy = pos.nash(lazy=True, threads=2)
    assert lazy["mode"] == "nash-lazy"
    assert lazy["value"] == pytest.approx(nash["value"], abs=0.2)

    deep = pos.deep(beam=2, outcomes=2, threads=2)
    assert strict_json(deep) == deep
    assert len(deep["lines"]) <= 2
    for line in deep["lines"]:
        assert line["ours"] in ours
        assert all(reply in theirs for reply, _ in line["replies"])


def test_deep_nash_levels():
    _, pos = position()
    two = pos.deep_nash(beam=2, outcomes=2, threads=2)
    assert two["depth"] == 2
    assert two["levels"] == [{"beam": 2, "outcomes": 2}]
    three = pos.deep_nash(beam=[2, 2], outcomes=[2, 1], threads=2)
    assert strict_json(three) == three
    assert three["depth"] == 3
    assert three["levels"] == [{"beam": 2, "outcomes": 2}, {"beam": 2, "outcomes": 1}]
    assert three["stats"]["deep_tt_misses"] > 0
    # The shorter list repeats its last entry; `outcomes=None` follows every outcome.
    same = pos.deep_nash(beam=[2, 2], outcomes=[2, 1], threads=1)
    assert same["value"] == three["value"], "the thread count changes nothing"
    repeat = pos.deep_nash(beam=[2, 2], outcomes=2, threads=2)
    assert repeat["levels"] == [{"beam": 2, "outcomes": 2}, {"beam": 2, "outcomes": 2}]
    every = pos.deep_nash(beam=2, outcomes=None, threads=2)
    assert every["levels"] == [{"beam": 2, "outcomes": None}]
    # The one-turn game inside is `nash`'s.
    assert two["shallow"]["value"] == pytest.approx(pos.nash(threads=2)["value"], abs=1e-5)
    with pytest.raises(ValueError):
        pos.deep_nash(beam=[])


def test_plan_forms_and_child_nash():
    _, pos = position()
    first, second = pos.legal_choices(0, "sensible")[:2]
    one = pos.plan(first, threads=2)
    assert one["plan"] == [first]
    assert one["child"] is None
    listed = pos.plan([first], threads=2)
    assert listed["value"] == one["value"]
    two = pos.plan(f"{first} / {second}", depth=1, threads=2)
    assert len(two["plan"]) == 2
    child = pos.plan(first, child_nash=True, beam=2, threads=2)
    assert child["child"]["beam"] == 2
    assert child["child"]["value"] is not None
    # The maximin line of the same choice is the plan's value.
    line = next(l for l in pos.maximin(exact=True, threads=2)["lines"] if l["ours"] == first)
    assert one["value"] == pytest.approx(line["value"], abs=1e-5)
    with pytest.raises(ValueError):
        pos.plan("move bogus")


def test_believed_and_observed():
    sc, pos = position()
    same = TEAMS / "eject-button-uturn.p1-team.json"
    bulky = TEAMS / "eject-button-uturn.p1-team-bulky.json"
    report = sc.believed(believed_teams=[same], threads=2)
    assert strict_json(report) == report
    assert report["mode"] == "believed"
    # Believing our real team: their real equilibrium strategy, answered at the game's value.
    assert report["best_response"] == pytest.approx(report["real_equilibrium"], abs=0.1)
    assert report["real_equilibrium"] == pytest.approx(pos.nash(threads=2)["value"], abs=1e-5)
    two = sc.believed(believed_teams=[same, bulky], believed_weights=[3, 1], threads=2)
    assert [t["prior"] for t in two["teams"]] == pytest.approx([0.75, 0.25])
    assert sum(p for _, p in two["their_strategy"]) == pytest.approx(1.0, abs=0.01)
    observed = sc.believed(threads=2)
    assert observed["mode"] == "observed"
    assert observed["value"] == pytest.approx(report["real_equilibrium"], abs=1e-5)
    # The scenario has no setup turns to observe.
    with pytest.raises(ValueError):
        sc.believed(observed={1: "Talonflame:50"})
    text = lab_engine.load_scenario(SCENARIO.read_text(encoding="utf-8"), base_dir=SCENARIO.parent)
    with pytest.raises(ValueError):
        text.believed(believed_teams=[same])


def test_rollout_reports_are_deterministic():
    sc, _ = position()
    a = sc.rollout(games=3, seed=7, max_turns=3, game_threads=2, threads=1)
    b = sc.rollout(games=3, seed=7, max_turns=3, game_threads=1, threads=1)
    assert strict_json(a) == a
    tally = a["tally"]
    assert sum(tally[k] for k in ("p1", "p2", "tie", "cutoff", "aborted")) == 3
    strip = lambda r: [{k: v for k, v in g.items() if k != "elapsed_s"} for g in r["game_records"]]
    assert strip(a) == strip(b)
    assert a["teams"] == [], "inline teams: no team files"
    deep = sc.rollout(games=1, policy="deep-nash", beam=[2, 2], outcomes=[1, 1], max_turns=2)
    assert deep["policy"]["depth"] == 3
    with pytest.raises(ValueError):
        sc.rollout(policy="bogus")


def test_bad_search_arguments():
    _, pos = position()
    with pytest.raises(ValueError):
        pos.maximin(chance="bogus")
    with pytest.raises(ValueError):
        pos.nash(rolls="bogus")
    with pytest.raises(ValueError):
        pos.maximin(side=2)
