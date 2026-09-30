use super::*;
use lab_engine::action::SlotAction;
use lab_engine::eval::{Heuristic, Material};
use lab_engine::state::SwitchFlag;
use lab_engine::turn::{enumerate_turn_with, FactoredScope};
use lab_scenario::{load_scenario_file, scenario_decision, scenario_positions_with};
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Mutex;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(true) };
    static APPLIES: Cell<usize> = const { Cell::new(0) };
    static REVERSES: Cell<usize> = const { Cell::new(0) };
}
pub(in crate::solve) fn enabled() -> bool {
    ENABLED.get()
}
pub(in crate::solve) fn record_apply() {
    APPLIES.set(APPLIES.get() + 1);
}
pub(in crate::solve) fn record_reverse() {
    REVERSES.set(REVERSES.get() + 1);
}
fn path<R>(enabled: bool, f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ENABLED.set(self.0);
        }
    }
    let _restore = Restore(ENABLED.replace(enabled));
    f()
}
fn fixture(name: &str) -> (State<2>, [Choice<2>; 2]) {
    let file =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../oracle/scenarios/{name}.json"));
    let loaded = load_scenario_file(file).unwrap();
    let position = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap()
    .remove(0);
    let lab_scenario::Decision::Turn(choices) = scenario_decision(&loaded, &position).unwrap()
    else {
        panic!("turn fixture required: {name}")
    };
    (position.state, choices.map(Choice::Turn))
}
fn config(rolls: RollMode, chance: Chance, us: SideId) -> Config {
    let mut c = Config::new(Ruleset::CHAMPIONS_MC, us);
    c.rolls = rolls;
    c.chance = chance;
    c.threads = 1;
    c
}

#[derive(Default)]
struct TraceEval {
    states: Mutex<Vec<State<2>>>,
    nan: bool,
}
impl Evaluator<2> for TraceEval {
    fn evaluate(&self, state: &State<2>) -> f32 {
        self.states.lock().unwrap().push(state.clone());
        if self.nan {
            f32::NAN
        } else {
            Heuristic.evaluate(state)
        }
    }
}
#[derive(Debug, PartialEq)]
struct Trace {
    result: Result<u32, SearchError>,
    nodes: u64,
    turns: u64,
    unsupported: Vec<String>,
    omitted: usize,
    states: Vec<State<2>>,
}
#[allow(clippy::too_many_arguments)]
fn chance_trace(
    direct: bool,
    original: &State<2>,
    decision: Decision,
    suspension: Option<&Suspension>,
    pair: [Choice<2>; 2],
    next: Next<'_, 2>,
    c: Config,
    window: (f32, f32),
    nan: bool,
) -> Trace {
    path(direct, || {
        let eval = TraceEval {
            nan,
            ..Default::default()
        };
        let mut solver = Solver::new(c, &eval);
        let mut state = original.clone();
        let value = solver.chance(
            &mut state, decision, suspension, pair, next, window.0, window.1,
        );
        assert_eq!(
            state, *original,
            "input must restore on success, cutoff, NaN and error"
        );
        Trace {
            result: value.map(f32::to_bits),
            nodes: solver.nodes,
            turns: solver.turns,
            unsupported: solver.unsupported,
            omitted: solver.omitted_pairs,
            states: eval.states.into_inner().unwrap(),
        }
    })
}

#[test]
fn p9_full_states_probabilities_suspensions_and_order() {
    let _flat = FactoredScope::new(false);
    let mut compared = 0;
    for (name, rolls) in [
        ("single-hit", RollMode::Full),
        ("single-hit", RollMode::Extremes),
        ("single-hit", RollMode::Quartiles),
        ("single-hit", RollMode::Median),
        ("single-hit", RollMode::Pessimistic(SideId::One)),
        ("single-hit", RollMode::Pessimistic(SideId::Two)),
        ("single-hit", RollMode::Fixed(3)),
        ("eject-button-uturn", RollMode::Full),
        ("mega-tyranitar", RollMode::Median),
        ("ee-transform-mega", RollMode::Median),
        ("ee-imposter-present", RollMode::Median),
        ("history-ragefist-metalburst", RollMode::Median),
        ("r-spread-eject-buttons-tie", RollMode::Median),
        ("dd-emergency-exit-recoil-eject-button", RollMode::Median),
        ("ab-turn-limit-tie", RollMode::Median),
        ("ab-future-sight-absent-user-two-occupants", RollMode::Median),
    ] {
        let (original, pair) = fixture(name);
        let [Choice::Turn(a), Choice::Turn(b)] = pair else {
            unreachable!()
        };
        let mut state = original.clone();
        let options = EnumerateOptions { rolls };
        let outcomes =
            enumerate_turn_with(&mut state, Ruleset::CHAMPIONS_MC, [a, b], options).unwrap();
        assert_eq!(state, original);
        let batch =
            try_enumerate_turn_final_states(&mut state, Ruleset::CHAMPIONS_MC, [a, b], options)
                .unwrap()
                .expect("flat batch");
        assert_eq!(state, original);
        let mut i = 0;
        let done: ControlFlow<()> = batch.visit(|end, probability, suspension| {
            let outcome = &outcomes[i];
            state.apply(&outcome.instructions);
            assert_eq!(&state, end, "whole State {name} {rolls:?} outcome {i}");
            assert_eq!(probability.to_bits(), outcome.probability.to_bits());
            assert_eq!(suspension, outcome.suspension.as_ref());
            assert_eq!(
                game::decision(end, suspension),
                game::decision(&state, outcome.suspension.as_ref())
            );
            state.reverse(&outcome.instructions);
            assert_eq!(state, original);
            i += 1;
            ControlFlow::Continue(())
        });
        assert!(done.is_continue());
        assert_eq!(i, outcomes.len());
        compared += i;
    }
    println!("P9 exact State/probability bits/Suspension/order checked: {compared} endings across 16 cases");
}

#[test]
fn p9_chance_bits_counters_star1_nan_and_injected_evaluator() {
    let _flat = FactoredScope::new(false);
    let mut cases = 0;
    for name in [
        "single-hit",
        "eject-button-uturn",
        "ab-turn-limit-tie",
        "ab-future-sight-absent-user-two-occupants",
    ] {
        let (state, pair) = fixture(name);
        for rolls in [RollMode::Median, RollMode::Full] {
            for chance in [Chance::Expect, Chance::Worst] {
                for us in [SideId::One, SideId::Two] {
                    for window in [
                        (f32::NEG_INFINITY, f32::INFINITY),
                        (-1.0, 1.0),
                        (-11001.0, -11000.0),
                        (11000.0, 11001.0),
                    ] {
                        for nan in [false, true] {
                            let run = |direct| {
                                chance_trace(
                                    direct,
                                    &state,
                                    Decision::Turn,
                                    None,
                                    pair,
                                    Next::Depth(0),
                                    config(rolls, chance, us),
                                    window,
                                    nan,
                                )
                            };
                            assert_eq!(
                                run(false),
                                run(true),
                                "{name} {rolls:?} {chance:?} {us:?} {window:?} NaN={nan}"
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    println!("P9 chance result bits, evaluator State sequence and counters checked: {cases} pairs");
}

#[test]
fn p9_horizon_validation_terminal_and_no_resume() {
    let (original, pair) = fixture("eject-button-uturn");
    let mut source = original.clone();
    let outcomes = game::transitions(
        &mut source,
        Ruleset::CHAMPIONS_MC,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
        Decision::Turn,
        None,
        pair,
    )
    .unwrap();
    let suspended = outcomes.iter().find(|o| o.suspension.is_some()).unwrap();
    source.apply(&suspended.instructions);
    let suspension = suspended.suspension.as_ref();
    for (mut state, has_suspension) in [
        (source.clone(), true),
        (source.clone(), false),
        (original.clone(), true),
        (original.clone(), false),
    ] {
        for result in [
            BattleResult::Ongoing,
            BattleResult::Win(SideId::One),
            BattleResult::Tie,
        ] {
            state.result = result;
            let eval = TraceEval::default();
            let mut solver =
                Solver::new(config(RollMode::Median, Chance::Expect, SideId::One), &eval);
            let suspension = if has_suspension { suspension } else { None };
            let expected = game::decision(&state, suspension)
                .map(|decision| match decision {
                    Decision::Over(result) => solver.terminal(result, 0),
                    _ => Heuristic.evaluate(&state),
                })
                .map(f32::to_bits)
                .map_err(SearchError::from);
            let before = state.clone();
            assert_eq!(
                solver
                    .value(&mut state, suspension, 0, -1.0, 1.0)
                    .map(f32::to_bits),
                expected
            );
            assert_eq!(solver.nodes, 1);
            assert_eq!(solver.turns, 0);
            assert_eq!(state, before);
            assert_eq!(
                eval.states.lock().unwrap().len(),
                usize::from(expected.is_ok() && !result.is_over())
            );
        }
    }
    // A leaf asking for a replacement is also evaluated as-is.
    let mut replacement = original.clone();
    replacement.sides[0].slots[0].party_index = None;
    assert_eq!(
        game::decision(&replacement, None).unwrap(),
        Decision::Replacement
    );
    let mut solver = Solver::new(
        config(RollMode::Median, Chance::Expect, SideId::One),
        &Material,
    );
    let expected = Material.evaluate(&replacement);
    assert_eq!(
        solver
            .depth_zero_value(&replacement, None)
            .unwrap()
            .to_bits(),
        expected.to_bits()
    );
    assert_eq!(solver.turns, 0);
}

#[test]
fn p9_errors_budgets_and_all_or_nothing() {
    let _flat = FactoredScope::new(false);
    let (original, valid) = fixture("single-hit");
    let [Choice::Turn(mut bad), b] = valid else {
        unreachable!()
    };
    bad[0] = SlotAction::Switch { party_index: 255 };
    let invalid = [Choice::Turn(bad), b];
    let mut over = original.clone();
    over.result = BattleResult::Win(SideId::One);
    let mut pending = original.clone();
    pending.sides[0].slots[0].switch_flag = SwitchFlag::Move;
    let mut replacement = original.clone();
    replacement.sides[0].slots[0].party_index = None;
    let (mut unsupported, unsupported_pair) =
        fixture("rr-rivalry-switch-in-undecided-gender");
    // Loading resolves genders. Restore one explicitly undecided gender so the
    // benched Rivalry holder refuses during its switch-in, after turn execution
    // starts. This intentionally tests a hand-made state, not a loader output.
    unsupported.sides[1].party[0].gender = lab_engine::dex::Gender::Random;
    for (state, pair) in [
        (&original, invalid),
        (&over, invalid),
        (&pending, invalid),
        (&replacement, invalid),
        (&original, [Choice::WAIT; 2]),
        (&unsupported, unsupported_pair),
    ] {
        for max_turns in [None, Some(0), Some(1)] {
            let mut c = config(RollMode::Median, Chance::Expect, SideId::One);
            c.max_turns = max_turns;
            let run = |direct| {
                chance_trace(
                    direct,
                    state,
                    Decision::Turn,
                    None,
                    pair,
                    Next::Depth(0),
                    c,
                    (-1.0, 1.0),
                    false,
                )
            };
            let old = run(false);
            let new = run(true);
            assert_eq!(old, new);
            assert!(new.states.is_empty());
            assert_eq!(new.nodes, 0);
        }
    }
    let result = chance_trace(
        true,
        &unsupported,
        Decision::Turn,
        None,
        unsupported_pair,
        Next::Depth(0),
        config(RollMode::Median, Chance::Expect, SideId::One),
        (-1.0, 1.0),
        false,
    );
    assert!(f32::from_bits(result.result.unwrap()).is_nan());
    assert!(!result.unsupported.is_empty());
    assert_eq!(
        result.unsupported,
        vec!["Luxray: Rivalry next to a Pokémon of undecided gender".to_owned()]
    );
    assert!(result.states.is_empty());
}

#[test]
fn p9_factored_recursive_plan_resume_and_replacement_fallbacks() {
    let (state, pair) = fixture("eject-button-uturn");
    let options = EnumerateOptions {
        rolls: RollMode::Median,
    };
    for factored in [false, true] {
        let _scope = FactoredScope::new(factored);
        for next in [Next::Depth(0), Next::Depth(1), Next::Plan(&[], 0)] {
            let run = |direct| {
                chance_trace(
                    direct,
                    &state,
                    Decision::Turn,
                    None,
                    pair,
                    next,
                    config(RollMode::Median, Chance::Expect, SideId::One),
                    (-1.0, 1.0),
                    false,
                )
            };
            assert_eq!(run(false), run(true));
        }
        let mut work = state.clone();
        let [Choice::Turn(a), Choice::Turn(b)] = pair else {
            unreachable!()
        };
        if factored {
            assert!(try_enumerate_turn_final_states(
                &mut work,
                Ruleset::CHAMPIONS_MC,
                [a, b],
                options
            )
            .unwrap()
            .is_none());
            assert_eq!(work, state);
        }
        let outcomes = game::transitions(
            &mut work,
            Ruleset::CHAMPIONS_MC,
            options,
            Decision::Turn,
            None,
            pair,
        )
        .unwrap();
        let outcome = outcomes.iter().find(|o| o.suspension.is_some()).unwrap();
        work.apply(&outcome.instructions);
        let choices = [SideId::One, SideId::Two].map(|side| {
            game::legal_choices(
                &work,
                Ruleset::CHAMPIONS_MC,
                Decision::MidTurn,
                side,
                Pruning::All,
            )[0]
        });
        let run = |direct| {
            chance_trace(
                direct,
                &work,
                Decision::MidTurn,
                outcome.suspension.as_ref(),
                choices,
                Next::Depth(0),
                config(RollMode::Median, Chance::Expect, SideId::One),
                (-1.0, 1.0),
                false,
            )
        };
        assert_eq!(run(false), run(true));
        let mut replace = state.clone();
        replace.sides[0].slots[0].party_index = None;
        let choices = [SideId::One, SideId::Two].map(|side| {
            game::legal_choices(
                &replace,
                Ruleset::CHAMPIONS_MC,
                Decision::Replacement,
                side,
                Pruning::All,
            )[0]
        });
        let run = |direct| {
            chance_trace(
                direct,
                &replace,
                Decision::Replacement,
                None,
                choices,
                Next::Depth(0),
                config(RollMode::Median, Chance::Expect, SideId::One),
                (-1.0, 1.0),
                false,
            )
        };
        assert_eq!(run(false), run(true));
    }
}

#[cfg(feature = "experiment-leaf-ending-observer")]
#[test]
fn p9_observer_confirms_removed_round_trip() {
    use lab_engine::turn::final_state_observer as observer;
    let _flat = FactoredScope::new(false);
    let (state, pair) = fixture("eject-button-uturn");
    let mut measurements = Vec::new();
    for direct in [false, true] {
        observer::reset();
        APPLIES.set(0);
        REVERSES.set(0);
        let trace = chance_trace(
            direct,
            &state,
            Decision::Turn,
            None,
            pair,
            Next::Depth(0),
            config(RollMode::Full, Chance::Expect, SideId::One),
            (f32::NEG_INFINITY, f32::INFINITY),
            false,
        );
        measurements.push((trace, observer::counts(), APPLIES.get(), REVERSES.get()));
    }
    let (old, a, applies, reverses) = &measurements[0];
    let (new, b, direct_applies, direct_reverses) = &measurements[1];
    assert_eq!(old, new);
    assert!(a.materialized_outcomes > 0 && a.emitted_instructions > 0);
    assert_eq!(*applies, old.nodes as usize);
    assert_eq!(applies, reverses);
    assert_eq!(b.materialized_outcomes, 0);
    assert_eq!(b.emitted_instructions, 0);
    assert_eq!(b.batches, 1);
    assert_eq!(b.visits, old.nodes as usize);
    assert_eq!((*direct_applies, *direct_reverses), (0, 0));
    println!("P9 work observer legacy={a:?} apply={applies} reverse={reverses}; direct={b:?} apply=0 reverse=0");
}
