//! P8d correctness/activation tests. Explicitly registered with required observer feature;
//! the timing build has no observer. This is bounded turn work, not a performance benchmark.
mod common;

use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::turn::{
    replay_action_keys_observer as observe, sample_turn, EnumerateOptions, FactoredScope, RollMode,
};
use lab_engine::Doubles;
use lab_scenario::{
    load_scenario_file, run_decision_mid_turn_with, scenario_choices, scenario_decision,
    scenario_positions, LoadedScenario, Position,
};

fn load(name: &str) -> (LoadedScenario, Vec<Position>) {
    let loaded =
        load_scenario_file(common::engine_dir().join(format!("oracle/scenarios/{name}.json")))
            .unwrap();
    let positions = scenario_positions(&loaded).unwrap();
    (loaded, positions)
}

fn assert_outcomes(before: &Doubles, cached: &[Outcome], reference: &[Outcome]) {
    assert_eq!(cached.len(), reference.len());
    for (left, right) in cached.iter().zip(reference) {
        assert_eq!(left.probability.to_bits(), right.probability.to_bits());
        assert_eq!(left.instructions, right.instructions);
        assert_eq!(left.suspension, right.suspension);
        let mut state = before.clone();
        let mut hash = state.position_hash();
        for instruction in &left.instructions {
            hash = hash.wrapping_add(state.apply_hashed(instruction));
        }
        assert_eq!(hash, state.position_hash(), "incremental ending hash");
        let mut expected = before.clone();
        expected.apply(&right.instructions);
        assert_eq!(state, expected, "exact ending State");
        state.reverse(&left.instructions);
        assert_eq!(state, *before, "full rollback");
        assert_eq!(state.position_hash(), before.position_hash());
    }
}

fn compare(name: &str, rolls: RollMode, factored: bool) -> observe::Counts {
    let (loaded, positions) = load(name);
    let _scope = FactoredScope::new(factored);
    let mut total = observe::Counts::default();
    for position in positions {
        let decision = scenario_decision(&loaded, &position).unwrap();
        let run = |state: &mut Doubles| {
            run_decision_mid_turn_with(
                state,
                &position.order,
                &decision,
                &loaded.mid_turn,
                EnumerateOptions { rolls },
            )
        };
        let mut state = position.state.clone();
        observe::reset();
        let cached = run(&mut state);
        let on = observe::counts();
        assert_eq!(state, position.state, "{name}: cached input restored");
        observe::reset();
        let reference = observe::without_reuse(|| run(&mut state));
        let off = observe::counts();
        assert_eq!(state, position.state, "{name}: reference input restored");
        assert_eq!(off.reused_keys, 0);
        assert_eq!(
            on.stage_runs, off.stage_runs,
            "{name}: replay work preserved"
        );
        assert_eq!(on.initial_priority_runs, off.initial_priority_runs);
        assert_eq!(on.multihit_resume_runs, off.multihit_resume_runs);
        assert_eq!(on.midturn_switch_runs, off.midturn_switch_runs);
        assert_eq!(
            off.action_key_calls,
            on.action_key_calls + on.reused_keys,
            "{name}: reuse accounts for every eliminated key call"
        );
        match (cached, reference) {
            (Ok(left), Ok(right)) => assert_outcomes(&position.state, &left, &right),
            (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
            other => panic!("{name}: success/error differs: {other:?}"),
        }
        total.filled_inputs += on.filled_inputs;
        total.stored_keys += on.stored_keys;
        total.action_key_calls += on.action_key_calls;
        total.rejected_at_pick += on.rejected_at_pick;
        total.reused_keys += on.reused_keys;
        total.reused_runs += on.reused_runs;
        total.stage_runs += on.stage_runs;
        total.initial_priority_runs += on.initial_priority_runs;
        total.multihit_resume_runs += on.multihit_resume_runs;
        total.midturn_switch_runs += on.midturn_switch_runs;
        if factored {
            assert_eq!(on.filled_inputs, 0, "factored cannot receive a cache");
            assert_eq!(on.reused_keys, 0);
        }
    }
    eprintln!("P8d {name} {rolls:?} factored={factored}: {total:?}");
    total
}

#[test]
fn ordinary_keys_activate_without_changing_stage_work_or_exact_results() {
    let mut reused = 0;
    for name in [
        "single-hit",
        "aa-quick-draw",
        "o88-lagging-tail-quick-claw",
        "o81-custap",
        "f-trick-room-speed-wrap",
        "f-trick-room-raw-speed",
        "f-speed-snapshot-eject-pack",
        "electro-ball-gyro-ball",
        "o44-quickfeet",
        "stance-change-raw-speed",
        "o61-gale-wings-triage",
        "o61-gale-wings-damaged",
        "after-you",
        "quash",
    ] {
        let counts = compare(name, RollMode::Median, false);
        assert!(
            counts.initial_priority_runs > 0,
            "{name}: initial stage covered"
        );
        reused += counts.reused_keys;
    }
    assert!(reused > 0, "candidate must actually reuse keys");
}

#[test]
fn resumed_hits_and_midturn_switches_keep_the_uncached_path() {
    let hits = compare("double-hit", RollMode::Median, false);
    assert!(hits.multihit_resume_runs > 0);
    let switches = compare("uturn-switch", RollMode::Median, false);
    assert!(switches.midturn_switch_runs > 0);
    compare("uturn-pause", RollMode::Median, false);
}

#[test]
fn bounded_full_and_factored_paths_preserve_bits_and_activation_scope() {
    let ordinary = compare("single-hit", RollMode::Full, false);
    assert!(ordinary.reused_keys > 0);
    compare("single-hit", RollMode::Full, true);
    compare("o61-gale-wings-damaged", RollMode::Median, true);
}

#[test]
fn in_stage_errors_and_successes_preserve_original_inputs() {
    compare("aa-power-construct-faint", RollMode::Median, false);
    let (loaded, positions) = load("aa-power-construct-faint");
    let mut errors = 0;
    let count = positions.len();
    for position in positions {
        let decision = scenario_decision(&loaded, &position).unwrap();
        let mut state = position.state.clone();
        errors += usize::from(
            run_decision_mid_turn_with(
                &mut state,
                &position.order,
                &decision,
                &loaded.mid_turn,
                EnumerateOptions {
                    rolls: RollMode::Median,
                },
            )
            .is_err(),
        );
        assert_eq!(state, position.state);
    }
    assert!(
        errors > 0 && errors < count,
        "fixture must cover real errors and successes"
    );
}

#[test]
fn sampling_remains_seed_identical_and_never_reuses() {
    let (loaded, positions) = load("single-hit");
    for position in positions {
        let choices = scenario_choices(&loaded, &position.state).unwrap();
        for seed in [0, 7, 42] {
            let mut state = position.state.clone();
            observe::reset();
            let cached = sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, 4, seed).unwrap();
            assert_eq!(observe::counts().reused_keys, 0);
            assert_eq!(observe::counts().filled_inputs, 0);
            assert_eq!(state, position.state);
            let reference = observe::without_reuse(|| {
                sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, 4, seed).unwrap()
            });
            assert_outcomes(&position.state, &cached, &reference);
            assert_eq!(state, position.state);
        }
    }
}
