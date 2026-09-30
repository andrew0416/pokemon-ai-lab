#[cfg(feature = "experiment-prepared-leaf")]
use lab_engine::action::JointAction;
// Bounded P15 proof. Records contain no timings or intentionally changed validator counts.
use lab_engine::{
    eval::Heuristic,
    hash::key_hash,
    rules::Ruleset,
    state::{SideId, State},
    turn::{self, FactoredScope, RollMode},
};
use lab_search::{Chance, Config, Solver};
use serde_json::{json, Value};
use std::{io::Write, time::Duration};
#[path = "p13_support.rs"]
mod support;
const RULES: Ruleset = Ruleset::CHAMPIONS_MC;

fn toy<const N: usize>() -> State<N> {
    let mut state = support::toy::<N>();
    for side in &mut state.sides {
        for mon in &mut side.party {
            mon.moves[1] = lab_engine::state::MoveSlot::full(lab_engine::dex::moves::PROTECT);
        }
    }
    state
}
#[cfg(feature = "experiment-prepared-leaf")]
fn options(rolls: RollMode) -> turn::EnumerateOptions {
    turn::EnumerateOptions { rolls }
}
fn full<const N: usize>(state: &State<N>) -> Value {
    json!({"debug":format!("{state:?}"),"hash":key_hash(0,state),"position_hash":state.position_hash()})
}
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
fn eq(e: &lab_search::Equilibrium) -> Value {
    json!({"rows":e.rows.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),
        "cols":e.cols.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),
        "value":e.value.to_bits(),"exploitability":e.exploitability.to_bits(),"iterations":e.iterations})
}
fn config(
    enabled: bool,
    us: SideId,
    chance: Chance,
    depth: u32,
    threads: usize,
    rolls: RollMode,
    max: Option<u64>,
) -> Config {
    let mut c = Config::new(RULES, us);
    #[cfg(feature = "experiment-prepared-turn")]
    {
        c.prepared_turn = enabled;
    }
    #[cfg(not(feature = "experiment-prepared-turn"))]
    let _ = enabled;
    c.rolls = rolls;
    c.threads = threads;
    c.chance = chance;
    c.depth = depth;
    c.max_turns = max;
    c
}
fn search<const N: usize>(
    original: &State<N>,
    suspension: Option<&turn::Suspension>,
    c: Config,
    mode: &str,
) -> Value {
    let mut state = original.clone();
    let mut solver = Solver::new(c, &Heuristic);
    let result=match mode {
        "mixed"=>solver.analyse_mixed(&mut state,suspension).map(|mut a|{
            a.elapsed=Duration::ZERO;
            json!({"debug":format!("{a:?}"),"matrix":a.matrix.values.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),"equilibrium":eq(&a.equilibrium),"maximin":a.maximin.1.to_bits()})
        }),
        "exact"=>solver.analyse(&mut state,suspension).map(|mut a|{
            a.elapsed=Duration::ZERO;
            json!({"debug":format!("{a:?}"),"value":a.value.to_bits(),"lines":a.lines.iter().map(|l|l.value.to_bits()).collect::<Vec<_>>()})
        }),
        "deep"=>solver.analyse_deep_mixed(&mut state,suspension,1).map(|mut a|{
            a.elapsed=Duration::ZERO;a.shallow.elapsed=Duration::ZERO;
            json!({"debug":format!("{a:?}"),"matrix":a.matrix.values.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),"equilibrium":eq(&a.equilibrium),"shallow":eq(&a.shallow.equilibrium)})
        }),
        "nash"=>solver.nash_value(&mut state,suspension).map(|v|json!({"value":v.to_bits()})),
        _=>unreachable!(),
    };
    assert_eq!(full(&state), full(original));
    json!({"before":full(original),"after":full(&state),"suspension":format!("{suspension:?}"),
        "result":match result { Ok(v)=>json!({"ok":v}),Err(e)=>json!({"error":format!("{e:?}")}) },"stats":stats(solver.stats())})
}
#[allow(unused_mut)]
fn record<const N: usize>(
    rows: &mut Vec<Value>,
    id: String,
    state: &State<N>,
    susp: Option<&turn::Suspension>,
    c: Config,
    mode: &str,
) {
    let mut off = c;
    #[cfg(feature = "experiment-prepared-turn")]
    {
        off.prepared_turn = false;
    }
    let mut on = c;
    #[cfg(feature = "experiment-prepared-turn")]
    {
        on.prepared_turn = true;
    }
    let a = search(state, susp, off, mode);
    let b = search(state, susp, on, mode);
    assert_eq!(a, b, "Config.prepared_turn changed {id}");
    rows.push(json!({"id":id,"slots":N,"mode":mode,"signature":b}));
}
fn public_cases<const N: usize>(rows: &mut Vec<Value>) {
    let state = toy::<N>();
    for side in [SideId::One, SideId::Two] {
        for chance in [Chance::Expect, Chance::Worst] {
            for mode in ["exact", "mixed"] {
                for depth in [1, 2] {
                    // One-action descendants keep the depth-two cases bounded.
                    let input = if depth == 1 {
                        state.clone()
                    } else {
                        support::toy::<N>()
                    };
                    record(
                        rows,
                        format!("toy-{N}-{side:?}-{chance:?}-{mode}-{depth}"),
                        &input,
                        None,
                        config(true, side, chance, depth, 1, RollMode::Median, None),
                        mode,
                    );
                }
            }
        }
    }
    for (id, rolls, factored, threads) in [
        ("full", RollMode::Full, false, 1),
        ("factored", RollMode::Median, true, 1),
        ("parallel", RollMode::Median, false, 2),
        ("extremes", RollMode::Extremes, false, 1),
        ("pessimistic", RollMode::Pessimistic(SideId::Two), false, 1),
    ] {
        let _scope = FactoredScope::new(factored);
        record(
            rows,
            format!("fallback-{N}-{id}"),
            &state,
            None,
            config(true, SideId::Two, Chance::Expect, 1, threads, rolls, None),
            "mixed",
        );
    }
    for max in [0, 1] {
        record(
            rows,
            format!("budget-{N}-{max}"),
            &state,
            None,
            config(
                true,
                SideId::One,
                Chance::Expect,
                1,
                1,
                RollMode::Median,
                Some(max),
            ),
            "mixed",
        );
    }
    for kind in ["terminal", "unsupported"] {
        let mut input = state.clone();
        if kind == "terminal" {
            input.result = lab_engine::state::BattleResult::Tie;
        } else {
            input.field[lab_engine::field::FieldEffect::Gravity as usize] =
                lab_engine::field::Effect {
                    turns: lab_engine::field::Effect::PERMANENT,
                    value: 0,
                };
        }
        record(
            rows,
            format!("{kind}-{N}"),
            &input,
            None,
            config(
                true,
                SideId::One,
                Chance::Expect,
                1,
                1,
                RollMode::Median,
                None,
            ),
            "mixed",
        );
    }
    record(
        rows,
        format!("deep-{N}"),
        &support::toy::<N>(),
        None,
        config(
            true,
            SideId::One,
            Chance::Expect,
            1,
            1,
            RollMode::Median,
            None,
        ),
        "deep",
    );
}

#[test]
fn exact_feature_and_config_toggles_preserve_public_search_records() {
    let _flat = FactoredScope::new(false);
    let mut rows = Vec::new();
    public_cases::<1>(&mut rows);
    public_cases::<2>(&mut rows);
    let (mut resumed, susp) = support::suspended();
    let moves = support::toy::<1>().sides[0].party[0].moves;
    for side in &mut resumed.sides {
        for mon in &mut side.party {
            mon.moves = moves;
        }
    }
    record(
        &mut rows,
        "successful-resume".into(),
        &resumed,
        Some(&susp),
        config(
            true,
            SideId::One,
            Chance::Expect,
            1,
            1,
            RollMode::Median,
            Some(256),
        ),
        "nash",
    );
    let mut replacement = support::toy::<1>();
    replacement.sides[0].party[0].hp = 0;
    replacement.sides[0].slots[0] = Default::default();
    replacement.sides[0].party[1].hp = replacement.sides[0].party[1].max_hp;
    record(
        &mut rows,
        "successful-replacement".into(),
        &replacement,
        None,
        config(
            true,
            SideId::One,
            Chance::Expect,
            1,
            1,
            RollMode::Median,
            Some(256),
        ),
        "nash",
    );
    assert_eq!(rows.len(), 54);
    assert!(rows
        .iter()
        .filter(|r| r["id"].as_str().unwrap().starts_with("successful-"))
        .all(|r| r["signature"]["result"].get("ok").is_some()));
    if let Ok(path) = std::env::var("LAB_P15_RECORDS") {
        let mut f = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        for row in rows {
            serde_json::to_writer(&mut f, &row).unwrap();
            writeln!(f).unwrap();
        }
    }
    println!("P15 PUBLIC CASES 54");
}

#[cfg(feature = "experiment-prepared-leaf")]
fn endings<const N: usize>(result: Result<Option<turn::FinalStates<N>>, turn::TurnError>) -> Value {
    match result {
        Err(e) => json!({"error":format!("{e:?}")}),
        Ok(None) => json!({"declined":true}),
        Ok(Some(batch)) => {
            let mut rows = Vec::new();
            let done=batch.visit::<()>(|s,p,susp|{rows.push(json!({"state":full(s),"probability":p.to_bits(),"suspension":format!("{susp:?}")}));std::ops::ControlFlow::Continue(())});
            assert!(done.is_continue());
            json!({"ok":rows})
        }
    }
}
#[cfg(feature = "experiment-prepared-leaf")]
fn choices<const N: usize>(s: &State<N>) -> [Vec<JointAction<N>>; 2] {
    [SideId::One, SideId::Two].map(|side| turn::legal_joint_actions(s, RULES, side))
}
#[cfg(feature = "experiment-prepared-leaf")]
fn fixture(name: &str) -> (State<2>, [JointAction<2>; 2]) {
    let loaded = lab_scenario::load_scenario_file(
        support::root().join(format!("oracle/scenarios/{name}.json")),
    )
    .unwrap();
    let p = lab_scenario::scenario_positions_with(&loaded, options(RollMode::Median))
        .unwrap()
        .into_iter()
        .find(|p| {
            // A missed setup Hyper Beam does not create a legal Recharge decision.
            name != "hyper-beam-recharge"
                || turn::locked_move(
                    &p.state,
                    lab_engine::state::SlotRef {
                        side: SideId::One,
                        slot: 0,
                    },
                ) == Some(turn::Locked::Recharge)
        })
        .expect("fixture has a positive setup branch");
    let lab_scenario::Decision::Turn(pair) = lab_scenario::scenario_decision(&loaded, &p).unwrap()
    else {
        panic!("turn fixture")
    };
    (p.state, pair)
}

#[cfg(feature = "experiment-prepared-leaf")]
fn core_matrix<const N: usize>() {
    let _flat = FactoredScope::new(false);
    let state = toy::<N>();
    let mut sides = choices(&state);
    sides[0].truncate(2);
    sides[1].truncate(3);
    // Singles has only two choices; duplicate indices are still distinct cache entries.
    while sides[1].len() < 3 {
        sides[1].push(sides[1][0]);
    }
    let opts = options(RollMode::Median);
    let mut batch = turn::PreparedTurn::new(&state, RULES, sides.clone());
    let mut refs = Vec::new();
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    turn::reset_validation_counts();
    for a in &sides[0] {
        for b in &sides[1] {
            let mut work = state.clone();
            refs.push(endings(turn::try_enumerate_turn_final_states(
                &mut work,
                RULES,
                [*a, *b],
                opts,
            )));
            assert_eq!(full(&work), full(&state));
        }
    }
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    {
        assert_eq!(turn::validation_counts(), [6, 12, 6]);
        turn::reset_validation_counts();
        turn::prepared_leaf_observer::reset();
    }
    for r in 0..2 {
        for c in 0..3 {
            assert_eq!(
                refs[r * 3 + c],
                endings(batch.try_enumerate_final_states([r, c], opts))
            );
        }
    }
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    {
        assert_eq!(turn::validation_counts(), [1, 5, 1]);
        assert_eq!(turn::prepared_leaf_observer::counts().batches, 6);
    }
    // Re-enter using the original Outcome API; both paths consume the same lazy cache.
    let expected =
        turn::enumerate_turn_with(&mut state.clone(), RULES, [sides[0][0], sides[1][0]], opts)
            .unwrap();
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    turn::reset_validation_counts();
    let actual = batch.enumerate([0, 0], opts).unwrap();
    assert_eq!(format!("{expected:?}"), format!("{actual:?}"));
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    assert_eq!(turn::validation_counts(), [0, 0, 0]);
    for outcome in actual {
        let mut work = state.clone();
        work.apply(&outcome.instructions);
        work.reverse(&outcome.instructions);
        assert_eq!(full(&work), full(&state));
    }
}

#[cfg(feature = "experiment-prepared-leaf")]
#[test]
fn owned_batch_shares_validation_and_preserves_complete_endings() {
    core_matrix::<1>();
    core_matrix::<2>();
    let _flat = FactoredScope::new(false);
    for name in [
        "eject-button-uturn",
        "mega-tyranitar",
        "ee-transform-mega",
        "hyper-beam-recharge",
        "o28-struggle",
    ] {
        let (state, pair) = fixture(name);
        let opts = options(RollMode::Median);
        let reference = endings(turn::try_enumerate_turn_final_states(
            &mut state.clone(),
            RULES,
            pair,
            opts,
        ));
        assert!(reference.get("ok").is_some(), "positive fixture {name}");
        let mut external = state.clone();
        let mut batch = turn::PreparedTurn::new(&external, RULES, [vec![pair[0]], vec![pair[1]]]);
        for _ in 0..2 {
            assert_eq!(
                reference,
                endings(batch.try_enumerate_final_states([0, 0], opts)),
                "{name}"
            );
        }
        external.result = lab_engine::state::BattleResult::Tie;
        assert_eq!(
            reference,
            endings(batch.try_enumerate_final_states([0, 0], opts))
        );
        let mut fresh = turn::PreparedTurn::new(&external, RULES, [vec![pair[0]], vec![pair[1]]]);
        assert_eq!(
            endings(fresh.try_enumerate_final_states([0, 0], opts)),
            json!({"error":"BattleOver"})
        );
    }
}

#[cfg(feature = "experiment-prepared-leaf")]
#[test]
fn cached_error_priority_rules_normalization_and_declined_paths_match() {
    let _flat = FactoredScope::new(false);
    let mut state = toy::<2>();
    state.field[lab_engine::field::FieldEffect::Gravity as usize] = lab_engine::field::Effect {
        turns: lab_engine::field::Effect::PERMANENT,
        value: 0,
    };
    let s = choices(&state);
    let mut bad1 = s[0][0];
    let mut bad2 = s[1][0];
    bad1[0] = lab_engine::action::SlotAction::Pass;
    bad2[0] = lab_engine::action::SlotAction::Pass;
    let sides = [vec![s[0][0], bad1], vec![s[1][0], bad2]];
    let mut batch = turn::PreparedTurn::new(&state, RULES, sides.clone());
    for [r, c] in [[0, 0], [1, 1], [0, 1], [1, 0], [0, 0], [1, 1]] {
        let expected = turn::try_enumerate_turn_final_states(
            &mut state.clone(),
            RULES,
            [sides[0][r], sides[1][c]],
            options(RollMode::Median),
        );
        assert!(expected.is_err());
        assert_eq!(
            endings(expected),
            endings(batch.try_enumerate_final_states([r, c], options(RollMode::Median)))
        );
    }
    // Parent errors always precede the cached per-side/support errors.
    for kind in ["over", "replacement", "midturn"] {
        let mut parent = toy::<2>();
        match kind {
            "over" => parent.result = lab_engine::state::BattleResult::Tie,
            "replacement" => parent.sides[0].slots[0].party_index = None,
            _ => parent.sides[0].slots[0].switch_flag = lab_engine::state::SwitchFlag::Move,
        }
        let invalid = [[lab_engine::action::SlotAction::Pass; 2]; 2];
        let mut batch =
            turn::PreparedTurn::new(&parent, RULES, [vec![invalid[0]], vec![invalid[1]]]);
        let expected = turn::try_enumerate_turn_final_states(
            &mut parent.clone(),
            RULES,
            invalid,
            options(RollMode::Median),
        );
        assert!(expected.is_err());
        assert_eq!(
            endings(expected),
            endings(batch.try_enumerate_final_states([0, 0], options(RollMode::Median)))
        );
    }
    // A requested move/target is normalized to Recharge in the common check_side body.
    let mut locked = toy::<2>();
    locked.sides[0].slots[0].volatiles.set(
        lab_engine::volatile::Volatile::MustRecharge,
        lab_engine::volatile::VolatileState {
            active: true,
            ..lab_engine::volatile::VolatileState::NONE
        },
    );
    let legal = choices(&locked);
    let mut pair = [legal[0][0], legal[1][0]];
    pair[0][0] = lab_engine::action::SlotAction::Move {
        index: 0,
        target: 2,
        gimmick: lab_engine::gimmick::Gimmick::None,
    };
    let reference = endings(turn::try_enumerate_turn_final_states(
        &mut locked.clone(),
        RULES,
        pair,
        options(RollMode::Median),
    ));
    assert!(reference.get("ok").is_some(), "MustRecharge normalization");
    let mut batch = turn::PreparedTurn::new(&locked, RULES, [vec![pair[0]], vec![pair[1]]]);
    assert_eq!(
        reference,
        endings(batch.try_enumerate_final_states([0, 0], options(RollMode::Median)))
    );
    let (mega, pair) = fixture("mega-tyranitar");
    let mut denied =
        turn::PreparedTurn::new(&mega, Ruleset::NO_GIMMICKS, [vec![pair[0]], vec![pair[1]]]);
    let reference = turn::try_enumerate_turn_final_states(
        &mut mega.clone(),
        Ruleset::NO_GIMMICKS,
        pair,
        options(RollMode::Median),
    );
    assert!(reference.is_err());
    assert_eq!(
        endings(reference),
        endings(denied.try_enumerate_final_states([0, 0], options(RollMode::Median)))
    );
    let state = toy::<1>();
    let sides = choices(&state);
    let pair = [sides[0][0], sides[1][0]];
    for (rolls, factored) in [(RollMode::Full, false), (RollMode::Median, true)] {
        let _scope = FactoredScope::new(factored);
        let opts = options(rolls);
        let mut batch = turn::PreparedTurn::new(&state, RULES, [vec![pair[0]], vec![pair[1]]]);
        #[cfg(feature = "experiment-prepared-leaf-observer")]
        {
            turn::reset_validation_counts();
            turn::prepared_leaf_observer::reset();
        }
        assert!(batch
            .try_enumerate_final_states([0, 0], opts)
            .unwrap()
            .is_none());
        #[cfg(feature = "experiment-prepared-leaf-observer")]
        {
            assert_eq!(turn::validation_counts(), [0, 0, 0]);
            let c = turn::prepared_leaf_observer::counts();
            assert_eq!((c.requests, c.declined, c.batches, c.errors), (1, 1, 0, 0));
        }
        let old = turn::enumerate_turn_with(&mut state.clone(), RULES, pair, opts).unwrap();
        let fallback = batch.enumerate([0, 0], opts).unwrap();
        assert_eq!(format!("{old:?}"), format!("{fallback:?}"));
    }
}

#[cfg(feature = "experiment-prepared-leaf-observer")]
#[test]
fn real_serial_validation_reductions_preserve_leaf_work_and_outputs() {
    let _flat = FactoredScope::new(false);
    let state = toy::<2>();
    let run = |enabled| {
        turn::reset_validation_counts();
        turn::final_state_observer::reset();
        turn::prepared_leaf_observer::reset();
        let result = search(
            &state,
            None,
            config(
                enabled,
                SideId::Two,
                Chance::Expect,
                1,
                1,
                RollMode::Median,
                None,
            ),
            "mixed",
        );
        (
            result,
            turn::validation_counts(),
            turn::final_state_observer::counts(),
            turn::prepared_leaf_observer::counts(),
        )
    };
    let (a, ordinary, leaf_a, p15_a) = run(false);
    let (b, prepared, leaf_b, p15_b) = run(true);
    assert_eq!(a, b);
    assert_eq!(leaf_a, leaf_b);
    assert!(leaf_a.batches > 0 && leaf_a.visits > 0);
    assert_eq!(p15_a.requests, 0);
    if cfg!(feature = "experiment-prepared-leaf") {
        assert!((0..3).all(|i| 0 < prepared[i] && prepared[i] < ordinary[i]));
        assert!(p15_b.batches > 0);
        assert_eq!(p15_b.requests, p15_b.batches);
        assert_eq!(p15_b.errors, 0);
    } else {
        assert_eq!(ordinary, prepared);
        assert_eq!(p15_b.requests, 0);
    }
    println!(
        "P15_ACTIVATION {}",
        json!({"ordinary":ordinary,"prepared":prepared,
        "p15":{"requests":p15_b.requests,"declined":p15_b.declined,"batches":p15_b.batches,"errors":p15_b.errors},
        "leaf":{"batches":leaf_b.batches,"visits":leaf_b.visits,"materialized_outcomes":leaf_b.materialized_outcomes,"emitted_instructions":leaf_b.emitted_instructions}})
    );
}
