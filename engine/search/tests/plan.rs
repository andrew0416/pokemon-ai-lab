//! The search foundation against the turn engine: legal choices are exactly what the engine
//! runs, choice strings round-trip, and the maximin values agree with a brute-force pass over
//! every pair of choices (Expect and Worst), through a turn that suspends for mid-turn
//! switches.

use std::path::PathBuf;

use serde_json::Value;

use lab_engine::eval::Material;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{enumerate_turn, EnumerateOptions, RollMode};
use lab_engine::Doubles;
use lab_scenario::{
    canonical_json, load_scenario_file, parse_choice, scenario_positions, Position,
};
use lab_search::{
    decision, format_choice, legal_choices, transitions, Chance, Choice, Config, Decision, Pruning,
    Solver, WIN,
};

/// The brute force and the solver under test both enumerate the exact distribution.
const OPTIONS: EnumerateOptions = EnumerateOptions {
    rolls: RollMode::Full,
};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The scenario's position; with several initial states, the one matching the oracle
/// fixture's `before`.
fn position(name: &str) -> Position {
    let loaded =
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    let positions = scenario_positions(&loaded).unwrap();
    if positions.len() == 1 {
        return positions.into_iter().next().unwrap();
    }
    let fixture = engine_dir().join(format!("oracle/expected/{name}.turn.json"));
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    let before = serde_json::to_string(&fixture["before"]).unwrap();
    positions
        .into_iter()
        .find(|p| {
            let json: Value =
                serde_json::from_str(&canonical_json(&p.state, &loaded.meta).unwrap()).unwrap();
            serde_json::to_string(&json).unwrap() == before
        })
        .expect("a position matches the fixture's before")
}

fn turn_choices(
    state: &Doubles,
    side: SideId,
    pruning: Pruning,
) -> Vec<[lab_engine::action::SlotAction; 2]> {
    legal_choices(state, Ruleset::CHAMPIONS_MC, Decision::Turn, side, pruning)
        .into_iter()
        .map(|c| match c {
            Choice::Turn(a) => a,
            other => panic!("{other:?} at a turn"),
        })
        .collect()
}

/// Every legal pair of choices is a turn the engine runs (nothing the generator offers is
/// rejected), including the suspended U-turn/Eject Button turn.
#[test]
fn legal_choices_are_accepted_by_the_turn_engine() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    assert_eq!(decision(&state, None).unwrap(), Decision::Turn);
    let ours = turn_choices(&state, SideId::One, Pruning::All);
    let theirs = turn_choices(&state, SideId::Two, Pruning::All);
    // Talonflame: U-turn at either foe or the ally, or switch; Snorlax: Harden or switch; one
    // bench member so both cannot switch. Sensible pruning drops the two U-turns at Snorlax.
    assert_eq!(ours.len(), 7, "{ours:?}");
    assert_eq!(theirs.len(), 3, "{theirs:?}");
    assert_eq!(
        turn_choices(&state, SideId::One, Pruning::Sensible).len(),
        5
    );
    for a in &ours {
        for b in &theirs {
            let outcomes = enumerate_turn(&mut state, Ruleset::CHAMPIONS_MC, [*a, *b])
                .unwrap_or_else(|e| panic!("{a:?} vs {b:?}: {e}"));
            let total: f64 = outcomes.iter().map(|o| o.probability).sum();
            assert!((total - 1.0).abs() < 1e-9);
        }
    }
    assert_eq!(state, position.state);
}

/// A generated action written as a choice string parses back to itself, so `lab-plan` output
/// pastes into a scenario's `turn`.
#[test]
fn choice_strings_round_trip() {
    let position = position("hypnosis-gravity");
    for side in [SideId::One, SideId::Two] {
        let actions = turn_choices(&position.state, side, Pruning::All);
        assert!(actions.len() > 20, "{side:?}: {}", actions.len());
        for action in actions {
            let order = &position.order[side.index()];
            let text = format_choice(&position.state, side, order, &action);
            let parsed = parse_choice(&position.state, side, order, &text)
                .unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(parsed, action, "{text:?}");
        }
    }
}

/// `Pruning::Sensible` drops damaging moves at the ally and nothing else.
#[test]
fn sensible_pruning_drops_ally_attacks() {
    let position = position("hypnosis-gravity");
    let all = turn_choices(&position.state, SideId::One, Pruning::All);
    let sensible = turn_choices(&position.state, SideId::One, Pruning::Sensible);
    assert!(sensible.len() < all.len());
    let order = &position.order[0];
    for action in &sensible {
        let text = format_choice(&position.state, SideId::One, order, action);
        // Hypnosis at the ally stays (a status move), Focus Blast at the ally goes.
        assert!(!text.contains("focusblast -"), "{text}");
    }
    assert!(sensible
        .iter()
        .any(|a| format_choice(&position.state, SideId::One, order, a).contains("hypnosis -")));
    assert!(all
        .iter()
        .any(|a| format_choice(&position.state, SideId::One, order, a).contains("focusblast -")));
}

/// The replacement decision after a double knock-out offers each side its bench.
#[test]
fn replacement_choices_fill_the_empty_slot() {
    let position = position("ko-replace");
    let mut state = position.state.clone();
    let decision = decision(&state, None).unwrap();
    assert_eq!(decision, Decision::Replacement);
    let p1 = legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        decision,
        SideId::One,
        Pruning::All,
    );
    let p2 = legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        decision,
        SideId::Two,
        Pruning::All,
    );
    assert!(!p1.is_empty() && !p2.is_empty());
    for choice in p1.iter().chain(&p2) {
        let Choice::Switches(switches) = choice else {
            panic!("{choice:?}");
        };
        assert_eq!(
            switches.iter().filter(|s| s.is_some()).count(),
            1,
            "{choice:?}"
        );
    }
    let outcomes = transitions(
        &mut state,
        Ruleset::CHAMPIONS_MC,
        EnumerateOptions::default(),
        decision,
        None,
        [p1[0], p2[0]],
    )
    .unwrap();
    let total: f64 = outcomes.iter().map(|o| o.probability).sum();
    assert!((total - 1.0).abs() < 1e-9);
    assert_eq!(state, position.state);
}

/// Brute force: for each of our choices, the worst reply's chance value (average or minimum
/// over the outcomes, descending into mid-turn switch decisions the same way).
fn brute_force(
    state: &mut Doubles,
    us: SideId,
    chance: Chance,
    depth: u32,
) -> Vec<(Choice<2>, f32)> {
    let ruleset = Ruleset::CHAMPIONS_MC;
    let decision = decision(state, None).unwrap();
    let ours = legal_choices(state, ruleset, decision, us, Pruning::Sensible);
    let theirs = legal_choices(state, ruleset, decision, us.other(), Pruning::Sensible);
    ours.into_iter()
        .map(|a| {
            let worst = theirs
                .iter()
                .map(|&b| {
                    let pair = match us {
                        SideId::One => [a, b],
                        SideId::Two => [b, a],
                    };
                    chance_value(state, us, chance, decision, None, pair, depth - 1)
                })
                .fold(f32::INFINITY, f32::min);
            (a, worst)
        })
        .collect()
}

fn chance_value(
    state: &mut Doubles,
    us: SideId,
    chance: Chance,
    decision: Decision,
    suspension: Option<&lab_engine::turn::Suspension>,
    pair: [Choice<2>; 2],
    depth: u32,
) -> f32 {
    let ruleset = Ruleset::CHAMPIONS_MC;
    let outcomes = transitions(state, ruleset, OPTIONS, decision, suspension, pair).unwrap();
    let values: Vec<(f64, f32)> = outcomes
        .iter()
        .map(|o| {
            state.apply(&o.instructions);
            let v = node_value(state, us, chance, o.suspension.as_ref(), depth);
            state.reverse(&o.instructions);
            (o.probability, v)
        })
        .collect();
    match chance {
        Chance::Expect => values.iter().map(|(p, v)| p * *v as f64).sum::<f64>() as f32,
        Chance::Worst => values.iter().map(|(_, v)| *v).fold(f32::INFINITY, f32::min),
    }
}

fn node_value(
    state: &mut Doubles,
    us: SideId,
    chance: Chance,
    suspension: Option<&lab_engine::turn::Suspension>,
    depth: u32,
) -> f32 {
    use lab_engine::eval::Evaluator;
    use lab_engine::state::BattleResult;
    let ruleset = Ruleset::CHAMPIONS_MC;
    let decision = decision(state, suspension).unwrap();
    if let Decision::Over(result) = decision {
        return match result {
            BattleResult::Win(side) if side == us => WIN + depth as f32,
            BattleResult::Win(_) => -(WIN + depth as f32),
            _ => 0.0,
        };
    }
    if depth == 0 {
        let score = Material.evaluate(state);
        return if us == SideId::One { score } else { -score };
    }
    let ours = legal_choices(state, ruleset, decision, us, Pruning::Sensible);
    let theirs = legal_choices(state, ruleset, decision, us.other(), Pruning::Sensible);
    let next = if decision == Decision::Turn {
        depth - 1
    } else {
        depth
    };
    ours.into_iter()
        .map(|a| {
            theirs
                .iter()
                .map(|&b| {
                    let pair = match us {
                        SideId::One => [a, b],
                        SideId::Two => [b, a],
                    };
                    chance_value(state, us, chance, decision, suspension, pair, next)
                })
                .fold(f32::INFINITY, f32::min)
        })
        .fold(f32::NEG_INFINITY, f32::max)
}

/// The solver's exact lines equal the brute-force maximin values, for both sides, both chance
/// modes and a turn whose outcomes suspend for mid-turn switches (U-turn into Eject Button).
#[test]
fn exact_lines_match_brute_force() {
    let position = position("eject-button-uturn");
    for us in [SideId::One, SideId::Two] {
        for chance in [Chance::Expect, Chance::Worst] {
            let mut state = position.state.clone();
            let expected = brute_force(&mut state, us, chance, 1);
            let mut config = Config::new(Ruleset::CHAMPIONS_MC, us);
            config.chance = chance;
            config.rolls = RollMode::Full;
            config.exact_lines = true;
            let evaluator = Material;
            let mut solver = Solver::new(config, &evaluator);
            let analysis = solver.analyse(&mut state, None).unwrap();
            assert_eq!(
                state, position.state,
                "the solver must leave the position unchanged"
            );
            assert_eq!(analysis.decision, Decision::Turn);
            assert_eq!(analysis.lines.len(), expected.len());
            for line in &analysis.lines {
                assert!(line.exact, "{line:?}");
                let (_, value) = expected
                    .iter()
                    .find(|(a, _)| *a == line.ours)
                    .unwrap_or_else(|| panic!("{:?} not in the brute force", line.ours));
                assert!(
                    (line.value - value).abs() < 1e-3,
                    "{us:?} {chance:?} {:?}: solver {} brute force {value}",
                    line.ours,
                    line.value
                );
            }
            let best = expected
                .iter()
                .map(|(_, v)| *v)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((analysis.value - best).abs() < 1e-3);
            // Lines come best first.
            for pair in analysis.lines.windows(2) {
                assert!(pair[0].value >= pair[1].value);
            }
        }
    }
}

/// With cutoffs, the best line's value is exact and every other line is at most it.
#[test]
fn cutoff_lines_bound_the_best() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let expected = brute_force(&mut state, SideId::One, Chance::Expect, 1);
    let best = expected
        .iter()
        .map(|(_, v)| *v)
        .fold(f32::NEG_INFINITY, f32::max);
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let analysis = solver.analyse(&mut state, None).unwrap();
    assert!((analysis.value - best).abs() < 1e-3);
    assert!(analysis.lines[0].exact);
    for line in &analysis.lines[1..] {
        assert!(line.value <= analysis.value + 1e-3, "{line:?}");
    }
}

/// Two turns deep through the same position: the solver agrees with brute force.
#[test]
fn depth_two_matches_brute_force() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let expected = brute_force(&mut state, SideId::Two, Chance::Worst, 2);
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::Two);
    config.depth = 2;
    config.chance = Chance::Worst;
    config.rolls = RollMode::Full;
    config.exact_lines = true;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let analysis = solver.analyse(&mut state, None).unwrap();
    assert_eq!(state, position.state);
    assert_eq!(analysis.depth, 2);
    for line in &analysis.lines {
        let (_, value) = expected.iter().find(|(a, _)| *a == line.ours).unwrap();
        assert!((line.value - value).abs() < 1e-3, "{line:?} vs {value}");
    }
}
