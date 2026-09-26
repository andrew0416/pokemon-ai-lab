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

/// The matrix game's payoffs are the exact chance values (brute force), its equilibrium value
/// is at least the pure maximin, and the strategies are distributions.
#[test]
fn mixed_analysis_matches_brute_force_payoffs() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let mixed = solver.analyse_mixed(&mut state, None).unwrap();
    assert_eq!(state, position.state);
    let decision = decision(&state, None).unwrap();
    for (r, &a) in mixed.ours.iter().enumerate() {
        for (c, &b) in mixed.theirs.iter().enumerate() {
            let expected = chance_value(
                &mut state,
                SideId::One,
                Chance::Expect,
                decision,
                None,
                [a, b],
                0,
            );
            assert!(
                (mixed.matrix.at(r, c) - expected).abs() < 1e-3,
                "{a:?} vs {b:?}: {} vs {expected}",
                mixed.matrix.at(r, c)
            );
        }
    }
    let brute = brute_force(&mut state, SideId::One, Chance::Expect, 1);
    let pure = brute
        .iter()
        .map(|(_, v)| *v)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!((mixed.maximin.1 - pure).abs() < 1e-3);
    assert!(mixed.equilibrium.value >= pure - 1e-2, "{mixed:?}");
    let sum: f32 = mixed.equilibrium.rows.iter().sum();
    assert!((sum - 1.0).abs() < 1e-4);
    let sum: f32 = mixed.equilibrium.cols.iter().sum();
    assert!((sum - 1.0).abs() < 1e-4);
    assert!(!mixed.our_support(0.01).is_empty());
}

/// A one-turn plan is worth exactly its maximin line; a plan whose entry is illegal falls
/// back to maximin and is reported broken.
#[test]
fn plan_evaluation_matches_the_lines() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    config.exact_lines = true;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let analysis = solver.analyse(&mut state, None).unwrap();
    for line in &analysis.lines {
        let report = solver
            .evaluate_plan(&mut state, None, &[line.ours])
            .unwrap();
        assert_eq!(state, position.state);
        assert_eq!(report.broken, 0);
        assert!(
            (report.value - line.value).abs() < 1e-3,
            "{:?}: plan {} line {}",
            line.ours,
            report.value,
            line.value
        );
        // The worst reply comes first and carries the plan's value.
        let (reply, worst) = report.replies[0];
        assert!((worst - report.value).abs() < 1e-3);
        assert_eq!(Some(reply), line.reply.or(Some(reply)));
    }
    let report = solver
        .evaluate_plan(&mut state, None, &[Choice::WAIT])
        .unwrap();
    assert_eq!(report.broken, 1);
    assert!((report.value - analysis.value).abs() < 1e-3, "{report:?}");
}

/// `nash_value` at the root equals the mixed analysis' equilibrium value, and a one-turn plan's
/// child valuation (no cap, every reply) is the worst reply's outcome-weighted child equilibrium.
#[test]
fn child_equilibrium_matches_the_mixed_analysis() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    config.child_nash = true;
    config.reply_beam = None;
    config.outcome_cap = None;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let mixed = solver.analyse_mixed(&mut state, None).unwrap();
    let root = solver.nash_value(&mut state, None).unwrap();
    assert!(
        (root - mixed.equilibrium.value).abs() < 0.05,
        "{root} vs {mixed:?}"
    );
    let decision = decision(&state, None).unwrap();
    let plan = mixed.ours[0];
    let report = solver.evaluate_plan(&mut state, None, &[plan]).unwrap();
    assert_eq!(state, position.state);
    let child = report.child.expect("child values");
    assert_eq!(child.replies.len(), mixed.theirs.len());
    // Brute force: for each reply, sum over outcomes of p * equilibrium(child).
    for &(reply, value) in &child.replies {
        let outcomes = transitions(
            &mut state,
            Ruleset::CHAMPIONS_MC,
            OPTIONS,
            decision,
            None,
            [plan, reply],
        )
        .unwrap();
        let mut expected = 0.0f64;
        for o in &outcomes {
            state.apply(&o.instructions);
            let v = solver
                .nash_value(&mut state, o.suspension.as_ref())
                .unwrap();
            state.reverse(&o.instructions);
            expected += o.probability * f64::from(v);
        }
        assert!(
            (value - expected as f32).abs() < 0.1,
            "{reply:?}: {value} vs {expected}"
        );
    }
    assert!((child.value - child.replies[0].1).abs() < 1e-6);
}

/// The deep analysis with a beam covering everything: shallow values are the exact lines,
/// deep values are the plan child values, and identical children hit the cache.
#[test]
fn deep_analysis_agrees_with_lines_and_plans() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    config.exact_lines = true;
    config.child_nash = true;
    config.reply_beam = None;
    config.outcome_cap = None;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let analysis = solver.analyse(&mut state, None).unwrap();
    let deep = solver.analyse_deep(&mut state, None, 100).unwrap();
    assert_eq!(state, position.state);
    assert_eq!(deep.lines.len(), analysis.lines.len());
    assert!(deep.shallow_rest.is_empty());
    for line in &deep.lines {
        let shallow = analysis.lines.iter().find(|l| l.ours == line.ours).unwrap();
        assert!((line.shallow - shallow.value).abs() < 1e-3, "{line:?}");
        let report = solver
            .evaluate_plan(&mut state, None, &[line.ours])
            .unwrap();
        let child = report.child.unwrap();
        assert!(
            (line.deep - child.value).abs() < 1e-3,
            "{:?}: deep {} plan child {}",
            line.ours,
            line.deep,
            child.value
        );
    }
    for pair in deep.lines.windows(2) {
        assert!(pair[0].deep >= pair[1].deep || pair[1].deep.is_nan());
    }
}

/// Answering the opponent's own equilibrium strategy on the same position is worth at least
/// the equilibrium value (a best response cannot do worse than the equilibrium), and answering
/// a pure strategy equals the corresponding matrix column's maximum.
#[test]
fn best_response_is_at_least_the_equilibrium() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let mixed = solver.analyse_mixed(&mut state, None).unwrap();
    let strategy: Vec<(Choice<2>, f32)> = mixed
        .theirs
        .iter()
        .zip(&mixed.equilibrium.cols)
        .map(|(c, &p)| (*c, p))
        .collect();
    let response = solver.best_response(&mut state, None, &strategy).unwrap();
    assert_eq!(state, position.state);
    assert!(
        response.lines[0].1 >= mixed.equilibrium.value - 0.05,
        "{} vs {}",
        response.lines[0].1,
        mixed.equilibrium.value
    );
    for c in 0..mixed.theirs.len() {
        let pure = vec![(mixed.theirs[c], 1.0f32)];
        let response = solver.best_response(&mut state, None, &pure).unwrap();
        let column_max = (0..mixed.ours.len())
            .map(|r| mixed.matrix.at(r, c))
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((response.lines[0].1 - column_max).abs() < 1e-3);
    }
}

/// The depth-2 mixed analysis: the beams hold every choice of the shallow support, its matrix
/// is over the kept beams, the equilibrium is a probability distribution with a value inside
/// the matrix's range, and with a full beam every cell equals the plan's child value.
#[test]
fn deep_mixed_beams_and_cells() {
    let position = position("eject-button-uturn");
    let mut state = position.state.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Full;
    config.child_nash = true;
    config.reply_beam = None;
    config.outcome_cap = None;
    let evaluator = Material;
    let mut solver = Solver::new(config, &evaluator);
    let deep = solver.analyse_deep_mixed(&mut state, None, 100).unwrap();
    assert_eq!(state, position.state);
    assert_eq!(deep.matrix.rows, deep.ours.len());
    assert_eq!(deep.matrix.cols, deep.theirs.len());
    for (choice, _) in deep.shallow.our_support(lab_search::MIXED_SUPPORT) {
        assert!(
            deep.ours.contains(&choice) || deep.omitted_ours > 0,
            "{choice:?}"
        );
    }
    for (choice, _) in deep.shallow.their_support(lab_search::MIXED_SUPPORT) {
        assert!(
            deep.theirs.contains(&choice) || deep.omitted_theirs > 0,
            "{choice:?}"
        );
    }
    let sum_rows: f32 = deep.equilibrium.rows.iter().sum();
    let sum_cols: f32 = deep.equilibrium.cols.iter().sum();
    assert!((sum_rows - 1.0).abs() < 1e-3 && (sum_cols - 1.0).abs() < 1e-3);
    let lo = deep
        .matrix
        .values
        .iter()
        .cloned()
        .fold(f32::INFINITY, f32::min);
    let hi = deep
        .matrix
        .values
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(deep.equilibrium.value >= lo - 1e-3 && deep.equilibrium.value <= hi + 1e-3);
    assert!(deep.equilibrium.value >= deep.maximin.1 - 1e-3);
    // With every choice in both beams, a cell is the plan's child value for that reply.
    for (r, a) in deep.ours.iter().enumerate() {
        let report = solver.evaluate_plan(&mut state, None, &[*a]).unwrap();
        let child = report.child.unwrap();
        for (c, b) in deep.theirs.iter().enumerate() {
            if let Some((_, v)) = child.replies.iter().find(|(x, _)| x == b) {
                assert!(
                    (deep.matrix.at(r, c) - v).abs() < 1e-3,
                    "{a:?} vs {b:?}: deep {} plan child {v}",
                    deep.matrix.at(r, c)
                );
            }
        }
    }
    // A narrow beam is a subset of the full one and never wider than beam + support.
    let narrow = solver.analyse_deep_mixed(&mut state, None, 1).unwrap();
    assert!(narrow.ours.iter().all(|c| deep.ours.contains(c)));
    assert!(narrow.ours.len() <= 1 + deep.shallow.our_support(lab_search::MIXED_SUPPORT).len());
}
