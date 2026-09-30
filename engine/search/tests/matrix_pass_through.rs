//! Exact external OFF/ON records. No elapsed times or observer counters enter the stream.
use lab_engine::{
    eval::Heuristic,
    hash::key_hash,
    rules::Ruleset,
    state::{SideId, State},
    turn::{FactoredScope, RollMode},
};
use lab_search::{Chance, Config, Solver};
use serde_json::{json, Value};
use std::{io::Write, time::Duration};
#[path = "p14_support.rs"]
mod support;

fn stats(s: lab_search::solve::SearchStats) -> Value {
    let lab_search::solve::SearchStats {
        tt_hits,
        tt_misses,
        nash_solves,
        nash_iterations,
        nash_seconds: _,
        enumerate_seconds: _,
        deep_tt_hits,
        deep_tt_misses,
        split_cells,
    } = s;
    json!([
        tt_hits,
        tt_misses,
        nash_solves,
        nash_iterations,
        deep_tt_hits,
        deep_tt_misses,
        split_cells
    ])
}

fn full_state<const N: usize>(state: &State<N>) -> Value {
    json!({"debug":format!("{state:?}"),"position_hash":state.position_hash(),"full_hash":key_hash(0,state)})
}

fn equilibrium(eq: &lab_search::Equilibrium) -> Value {
    json!({"rows":eq.rows.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "cols":eq.cols.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "value":eq.value.to_bits(),"exploitability":eq.exploitability.to_bits(),"iterations":eq.iterations})
}

fn supplemental<const N: usize>(rows: &mut Vec<Value>) {
    let mut input = support::toy::<N>();
    let before = input.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 1;
    config.max_turns = Some(128);
    let mut solver = Solver::new(config, &Heuristic);
    let levels = [lab_search::DeepLevel {
        beam: 1,
        outcomes: Some(1),
    }; 2];
    let mut analysis = solver
        .analyse_deep_mixed_levels(&mut input, None, &levels)
        .unwrap();
    analysis.elapsed = Duration::ZERO;
    analysis.shallow.elapsed = Duration::ZERO;
    assert!(solver.stats().deep_tt_misses > 0);
    assert_eq!(input, before);
    rows.push(
        json!({"kind":"depth3","slots":N,"before":full_state(&before),"after":full_state(&input),
        "analysis":format!("{analysis:?}"),"value":equilibrium(&analysis.equilibrium),
        "matrix":analysis.matrix.values.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "stats":stats(solver.stats())}),
    );
    for kind in ["terminal", "unsupported"] {
        let mut input = before.clone();
        if kind == "terminal" {
            input.result = lab_engine::state::BattleResult::Tie;
        } else {
            input.field[lab_engine::field::FieldEffect::Gravity as usize] =
                lab_engine::field::Effect {
                    turns: lab_engine::field::Effect::PERMANENT,
                    value: 0,
                };
        }
        let before = input.clone();
        let mut solver = Solver::new(config, &Heuristic);
        let result = solver.nash_value(&mut input, None);
        if kind == "terminal" {
            assert!(result.is_ok());
        } else {
            assert!(
                result.as_ref().unwrap().is_nan(),
                "unsupported child is the existing NaN sentinel"
            );
        }
        assert_eq!(input, before);
        let refusal = if kind == "unsupported" {
            let error = solver.analyse_mixed(&mut input, None).unwrap_err();
            assert!(matches!(error, lab_search::SearchError::Unsupported(_)));
            Some(format!("{error:?}"))
        } else {
            None
        };
        rows.push(json!({"kind":kind,"refusal":refusal,"slots":N,"before":full_state(&before),"after":full_state(&input),
            "result":format!("{result:?}"),"value_bits":result.as_ref().ok().map(|x|x.to_bits()),"stats":stats(solver.stats())}));
    }
}

fn successful_switches(rows: &mut Vec<Value>) {
    let (mut resumed, rest) = support::suspended();
    // Preserve the real remaining action queue; limit the following legal turn to Harden.
    let harden = support::toy::<1>().sides[0].party[0].moves;
    for side in &mut resumed.sides {
        for mon in &mut side.party {
            mon.moves = harden;
        }
    }
    assert_eq!(
        lab_search::decision(&resumed, Some(&rest)).unwrap(),
        lab_search::Decision::MidTurn
    );
    let before = resumed.clone();
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 1;
    config.max_turns = Some(256);
    let mut solver = Solver::new(config, &Heuristic);
    let value = solver.nash_value(&mut resumed, Some(&rest)).unwrap();
    assert_eq!(resumed, before);
    assert!(solver.stats().nash_solves > 0);
    rows.push(json!({"kind":"successful-resume","before":full_state(&before),"after":full_state(&resumed),
        "suspension":format!("{rest:?}"),"value_bits":value.to_bits(),"stats":stats(solver.stats())}));
    let mut replacement = support::toy::<1>();
    replacement.sides[0].party[0].hp = 0;
    replacement.sides[0].slots[0] = lab_engine::state::Slot::default();
    replacement.sides[0].party[1].hp = replacement.sides[0].party[1].max_hp;
    assert_eq!(
        lab_search::decision(&replacement, None).unwrap(),
        lab_search::Decision::Replacement
    );
    let before = replacement.clone();
    let mut solver = Solver::new(config, &Heuristic);
    let value = solver.nash_value(&mut replacement, None).unwrap();
    assert_eq!(replacement, before);
    rows.push(json!({"kind":"successful-replacement","before":full_state(&before),"after":full_state(&replacement),
        "value_bits":value.to_bits(),"stats":stats(solver.stats())}));
}

fn toy_records<const N: usize>(rows: &mut Vec<Value>) {
    for chance in [Chance::Expect, Chance::Worst] {
        for threads in [1, 2] {
            for dominance in [false, true] {
                for lazy in [false, true] {
                    let _scope = FactoredScope::new(false);
                    let mut state = support::toy::<N>();
                    let before = state.clone();
                    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
                    config.rolls = RollMode::Full;
                    config.threads = threads;
                    config.chance = chance;
                    config.transposition = false;
                    config.dominance = dominance;
                    config.double_oracle = lazy;
                    config.outcome_cap = Some(1);
                    let mut solver = Solver::new(config, &Heuristic);
                    let first = solver.nash_value(&mut state, None).unwrap();
                    let second = solver.nash_value(&mut state, None).unwrap();
                    assert_eq!(first.to_bits(), second.to_bits());
                    let cache_stats = stats(solver.stats());
                    let mut analysis = solver.analyse_deep_mixed(&mut state, None, 1).unwrap();
                    analysis.elapsed = Duration::ZERO;
                    analysis.shallow.elapsed = Duration::ZERO;
                    assert_eq!(state, before);
                    rows.push(json!({"kind":"toy", "slots":N,"chance":format!("{chance:?}"),
                        "threads":threads,"dominance":dominance,"lazy":lazy,
                        "before":full_state(&before),"after":full_state(&state),
                        "nash_value_bits":[first.to_bits(),second.to_bits()],"cache_stats":cache_stats,
                        "analysis":format!("{analysis:?}"),
                        "matrix_bits":analysis.matrix.values.iter().map(|value|value.to_bits()).collect::<Vec<_>>(),
                        "equilibrium_bits":equilibrium(&analysis.equilibrium),"shallow_equilibrium_bits":equilibrium(&analysis.shallow.equilibrium),
                        "shallow_matrix_bits":analysis.shallow.matrix.values.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),"stats":stats(solver.stats())}));
                }
            }
        }
    }
}

struct NanEvaluator;
impl<const N: usize> lab_engine::eval::Evaluator<N> for NanEvaluator {
    fn evaluate(&self, _state: &State<N>) -> f32 {
        f32::from_bits(0x7fc00035)
    }
}
fn nan_records<const N: usize>(rows: &mut Vec<Value>) {
    for lazy in [false, true] {
        let mut state = support::toy::<N>();
        let before = state.clone();
        let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
        config.rolls = RollMode::Median;
        config.threads = 1;
        config.double_oracle = lazy;
        config.transposition = false;
        config.max_turns = Some(32);
        let mut solver = Solver::new(config, &NanEvaluator);
        let value = solver.nash_value(&mut state, None).unwrap();
        assert!(value.is_nan());
        assert_eq!(state, before);
        rows.push(json!({"kind":"nan-evaluator","slots":N,"lazy":lazy,
            "before":full_state(&before),"after":full_state(&state),"value_bits":value.to_bits(),"stats":stats(solver.stats())}));
    }
}

#[test]
fn exact_off_on_matrix_search_error_and_resume_records() {
    #[cfg(feature = "experiment-matrix-pass-through-observer")]
    lab_search::solve::matrix_pass_through_observer::reset();
    let mut rows = Vec::new();
    toy_records::<1>(&mut rows);
    toy_records::<2>(&mut rows);
    supplemental::<1>(&mut rows);
    supplemental::<2>(&mut rows);
    successful_switches(&mut rows);
    nan_records::<1>(&mut rows);
    nan_records::<2>(&mut rows);
    for name in [
        "aa-power-construct",
        "ability-change-fails",
        "eject-button-uturn",
    ] {
        let loaded = lab_scenario::load_scenario_file(
            support::root().join(format!("oracle/scenarios/{name}.json")),
        )
        .unwrap();
        let position = lab_scenario::scenario_positions_with(
            &loaded,
            lab_engine::turn::EnumerateOptions {
                rolls: RollMode::Median,
            },
        )
        .unwrap()
        .remove(0);
        let before = position.state.clone();
        let lab_scenario::Decision::Turn(pair) =
            lab_scenario::scenario_decision(&loaded, &position).unwrap()
        else {
            panic!("fixture turn")
        };
        let mut state = before.clone();
        let outcomes = lab_engine::turn::enumerate_turn_with(
            &mut state,
            Ruleset::CHAMPIONS_MC,
            pair,
            lab_engine::turn::EnumerateOptions {
                rolls: RollMode::Median,
            },
        )
        .unwrap();
        assert_eq!(state, before);
        for (index, outcome) in outcomes.iter().enumerate() {
            let mut ended = state.clone();
            let initial = ended.position_hash();
            let mut hash = initial;
            for instruction in &outcome.instructions {
                hash = hash.wrapping_add(ended.apply_hashed(instruction));
            }
            assert_eq!(hash, ended.position_hash());
            let ending = full_state(&ended);
            let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
            config.rolls = RollMode::Median;
            config.threads = 1;
            config.max_turns = Some(0);
            let mut solver = Solver::new(config, &Heuristic);
            let value = solver.nash_value(&mut ended, outcome.suspension.as_ref());
            assert_eq!(full_state(&ended), ending);
            ended.reverse(&outcome.instructions);
            assert_eq!(ended, before);
            rows.push(json!({"kind":"fixture","name":name,"index":index,
                "before":full_state(&before),"ending":ending,"reversed":full_state(&ended),
                "probability_bits":outcome.probability.to_bits(),"instructions":format!("{:?}",outcome.instructions),
                "suspension":format!("{:?}",outcome.suspension),"result":format!("{value:?}"),"value_bits":value.as_ref().ok().map(|x|x.to_bits()),"stats":stats(solver.stats())}));
        }
    }
    assert_eq!(rows.len(), 51);
    #[cfg(feature = "experiment-matrix-pass-through-observer")]
    {
        let c = lab_search::solve::matrix_pass_through_observer::counts();
        assert!(c.a_calls > 0 && c.b_calls > 0);
        assert_eq!(c.a_calls, c.a_passthrough + c.a_fallback);
        assert_eq!(c.b_calls, c.b_passthrough + c.b_fallback);
        if cfg!(feature = "experiment-matrix-pass-through") {
            assert!(c.a_passthrough > 0 && c.b_passthrough > 0);
            assert!(c.a_fallback > 0 && c.b_fallback > 0);
        } else {
            assert_eq!((c.a_passthrough, c.b_passthrough), (0, 0));
        }
        println!(
            "P14_PUBLIC_ACTIVATION {}",
            json!({"a_calls":c.a_calls,"a_passthrough":c.a_passthrough,"a_fallback":c.a_fallback,
            "b_calls":c.b_calls,"b_passthrough":c.b_passthrough,"b_fallback":c.b_fallback})
        );
    }
    if let Some(path) = std::env::var_os("LAB_P14_RECORDS") {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        for row in rows {
            serde_json::to_writer(&mut output, &row).unwrap();
            writeln!(output).unwrap();
        }
    }
}
