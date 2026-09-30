#![cfg(feature = "experiment-prepared-turn-observe")]

use lab_engine::action::{JointAction, SlotAction};
use lab_engine::eval::Heuristic;
use lab_engine::field::{Effect, FieldEffect};
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{self, EnumerateOptions, FactoredScope, PreparedTurn, RollMode, TurnError};
use lab_engine::volatile::{Volatile, VolatileState};
use lab_scenario::{
    load_scenario_file, load_scenario_str, scenario_decision, scenario_positions_with,
};
use lab_search::{Chance, Config, Solver};
use std::path::PathBuf;

const RULES: Ruleset = Ruleset::CHAMPIONS_MC;
fn options() -> EnumerateOptions {
    EnumerateOptions {
        rolls: RollMode::Median,
    }
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture(name: &str) -> (State<2>, [JointAction<2>; 2]) {
    let loaded = load_scenario_file(root().join(format!("oracle/scenarios/{name}.json"))).unwrap();
    let position = scenario_positions_with(&loaded, options())
        .unwrap()
        .remove(0);
    let lab_scenario::Decision::Turn(pair) = scenario_decision(&loaded, &position).unwrap() else {
        panic!("turn expected")
    };
    (position.state, pair)
}

fn toy() -> State<2> {
    let mon = |species: &str| serde_json::json!({"species":species, "ability":"Honey Gather", "nature":"Serious", "evs":{}, "moves":["Harden","Protect"], "level":50});
    let text=serde_json::json!({"format":"gen9championsdoublescustomgame", "p1":{"team":[mon("Talonflame"),mon("Snorlax")],"order":"12"},"p2":{"team":[mon("Swampert"),mon("Excadrill")],"order":"12"}}).to_string();
    let loaded = load_scenario_str(&text, &root()).unwrap();
    scenario_positions_with(&loaded, options())
        .unwrap()
        .remove(0)
        .state
}

fn choices(state: &State<2>) -> [Vec<JointAction<2>>; 2] {
    [SideId::One, SideId::Two].map(|side| turn::legal_joint_actions(state, RULES, side))
}

fn same(
    state: &State<2>,
    expected: Result<Vec<Outcome>, TurnError>,
    actual: Result<Vec<Outcome>, TurnError>,
) {
    match (expected, actual) {
        (Err(a), Err(b)) => assert_eq!(a, b),
        (Ok(a), Ok(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(&b) {
                assert_eq!(a.probability.to_bits(), b.probability.to_bits());
                assert_eq!(a.instructions, b.instructions);
                assert_eq!(a.suspension, b.suspension);
                let (mut x, mut y) = (state.clone(), state.clone());
                x.apply(&a.instructions);
                y.apply(&b.instructions);
                assert_eq!(x, y);
                x.reverse(&a.instructions);
                y.reverse(&b.instructions);
                assert_eq!(&x, state);
                assert_eq!(&y, state);
            }
        }
        (a, b) => panic!("different outcome/error kinds: {a:?} / {b:?}"),
    }
}

#[test]
fn matrix_reuses_real_validators_and_preserves_all_outcome_bits() {
    let _flat = FactoredScope::new(false);
    let state = toy();
    let mut sides = choices(&state);
    sides[0].truncate(2);
    sides[1].truncate(3);
    let mut ordinary = state.clone();
    turn::reset_validation_counts();
    let mut reference = Vec::new();
    for a in &sides[0] {
        for b in &sides[1] {
            reference.push(turn::enumerate_turn_with(
                &mut ordinary,
                RULES,
                [*a, *b],
                options(),
            ));
            assert_eq!(ordinary, state);
        }
    }
    assert_eq!(turn::validation_counts(), [6, 12, 6]);
    let mut batch = PreparedTurn::new(&state, RULES, sides);
    turn::reset_validation_counts();
    let mut reference = reference.into_iter();
    for r in 0..2 {
        for c in 0..3 {
            same(
                &state,
                reference.next().unwrap(),
                batch.enumerate([r, c], options()),
            );
        }
    }
    assert_eq!(turn::validation_counts(), [1, 5, 1]);
    // The same batch can be re-entered after every returned outcome was applied/reversed.
    batch.enumerate([0, 0], options()).unwrap();
    assert_eq!(turn::validation_counts(), [1, 5, 1]);
    println!("P8C_VALIDATIONS 2x3 ordinary=[6,12,6] prepared=[1,5,1]");
}

#[test]
fn invalid_pair_error_order_and_deferred_support_are_identical() {
    let _flat = FactoredScope::new(false);
    let mut state = toy();
    // Existing support validator rejects permanent Gravity; both side choices remain legal.
    state.field[FieldEffect::Gravity as usize] = Effect {
        turns: Effect::PERMANENT,
        value: 0,
    };
    let sides = choices(&state);
    let mut invalid1 = sides[0][0];
    invalid1[0] = SlotAction::Pass;
    let mut invalid2 = sides[1][0];
    invalid2[0] = SlotAction::Pass;
    let sides = [vec![sides[0][0], invalid1], vec![sides[1][0], invalid2]];
    let mut batch = PreparedTurn::new(&state, RULES, sides.clone());
    // Cache support error first, then ensure side errors still win on later pairs.
    for [r, c] in [[0, 0], [1, 1], [0, 1], [1, 0], [0, 0], [1, 1]] {
        let expected = turn::enumerate_turn_with(
            &mut state.clone(),
            RULES,
            [sides[0][r], sides[1][c]],
            options(),
        );
        let error = expected.as_ref().unwrap_err();
        match (r, c) {
            (0, 0) => assert!(matches!(error, TurnError::Unsupported(_))),
            (1, _) => assert!(matches!(
                error,
                TurnError::InvalidChoice {
                    side: SideId::One,
                    ..
                }
            )),
            _ => assert!(matches!(
                error,
                TurnError::InvalidChoice {
                    side: SideId::Two,
                    ..
                }
            )),
        }
        same(&state, expected, batch.enumerate([r, c], options()));
    }
}

#[test]
fn snapshots_cannot_accept_a_stale_parent_or_ruleset() {
    let _flat = FactoredScope::new(false);
    let original = toy();
    let sides = choices(&original);
    let pair = [sides[0][0], sides[1][0]];
    let mut external = original.clone();
    let mut batch = PreparedTurn::new(&external, RULES, sides.clone());
    batch.enumerate([0, 0], options()).unwrap();
    external.result = BattleResult::Tie;
    // There is no enumerate(state, ruleset, token) API. Old batch stays bound to original.
    same(
        &original,
        turn::enumerate_turn_with(&mut original.clone(), RULES, pair, options()),
        batch.enumerate([0, 0], options()),
    );
    let mut fresh = PreparedTurn::new(&external, Ruleset::NO_GIMMICKS, sides);
    assert_eq!(
        fresh.enumerate([0, 0], options()).unwrap_err(),
        TurnError::BattleOver
    );
    assert_eq!(external.result, BattleResult::Tie);
    let (mega, pair) = fixture("mega-tyranitar");
    let sides = [vec![pair[0]], vec![pair[1]]];
    let mut allowed = PreparedTurn::new(&mega, RULES, sides.clone());
    let mut disallowed = PreparedTurn::new(&mega, Ruleset::NO_GIMMICKS, sides);
    same(
        &mega,
        turn::enumerate_turn_with(&mut mega.clone(), RULES, pair, options()),
        allowed.enumerate([0, 0], options()),
    );
    let expected =
        turn::enumerate_turn_with(&mut mega.clone(), Ruleset::NO_GIMMICKS, pair, options());
    assert!(expected.is_err());
    same(&mega, expected, disallowed.enumerate([0, 0], options()));
}

#[test]
fn normalization_suspension_mega_and_transform_match() {
    let _flat = FactoredScope::new(false);
    for name in [
        "eject-button-uturn",
        "mega-tyranitar",
        "ee-transform-mega",
        "hyper-beam-recharge",
        "o28-struggle",
    ] {
        let (state, pair) = fixture(name);
        let mut batch = PreparedTurn::new(&state, RULES, [vec![pair[0]], vec![pair[1]]]);
        for _ in 0..2 {
            same(
                &state,
                turn::enumerate_turn_with(&mut state.clone(), RULES, pair, options()),
                batch.enumerate([0, 0], options()),
            );
        }
    }
    // Explicitly force the existing lock normalization to ignore the selected move/target.
    let mut state = toy();
    state.sides[0].slots[0].volatiles.set(
        Volatile::MustRecharge,
        VolatileState {
            active: true,
            ..VolatileState::NONE
        },
    );
    let sides = choices(&state);
    let mut pair = [sides[0][0], sides[1][0]];
    pair[0][0] = SlotAction::Move {
        index: 0,
        target: 2,
        gimmick: lab_engine::gimmick::Gimmick::None,
    };
    let mut batch = PreparedTurn::new(&state, RULES, [vec![pair[0]], vec![pair[1]]]);
    same(
        &state,
        turn::enumerate_turn_with(&mut state.clone(), RULES, pair, options()),
        batch.enumerate([0, 0], options()),
    );
}

#[test]
fn full_and_factored_paths_keep_single_turn_validation() {
    let (state, pair) = fixture("eject-button-uturn");
    for (rolls, factored) in [(RollMode::Full, false), (RollMode::Median, true)] {
        let _scope = FactoredScope::new(factored);
        let options = EnumerateOptions { rolls };
        assert!(!PreparedTurn::<2>::eligible(options));
        let mut batch = PreparedTurn::new(&state, RULES, [vec![pair[0]], vec![pair[1]]]);
        let expected = turn::enumerate_turn_with(&mut state.clone(), RULES, pair, options);
        turn::reset_validation_counts();
        same(&state, expected, batch.enumerate([0, 0], options));
        batch.enumerate([0, 0], options).unwrap();
        assert_eq!(turn::validation_counts(), [2, 4, 2]);
    }
}

fn signature(
    state: &State<2>,
    suspension: Option<&turn::Suspension>,
    enabled: bool,
    side: SideId,
    chance: Chance,
    depth: u32,
    mixed: bool,
    max: Option<u64>,
) -> String {
    let mut config = Config::new(RULES, side);
    config.threads = 1;
    config.rolls = RollMode::Median;
    config.chance = chance;
    config.depth = depth;
    config.prepared_turn = enabled;
    config.max_turns = max;
    let mut solver = Solver::new(config, &Heuristic);
    let mut work = state.clone();
    let result = if mixed {
        solver.analyse_mixed(&mut work, suspension).map(|a| {
            format!(
                "{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{}",
                a.decision,
                a.ours,
                a.theirs,
                a.matrix
                    .values
                    .iter()
                    .map(|x| x.to_bits())
                    .collect::<Vec<_>>(),
                a.equilibrium,
                a.nodes,
                a.turns,
                a.omitted_ours,
                a.omitted_theirs,
                a.maximin.1.to_bits()
            )
        })
    } else {
        solver.analyse(&mut work, suspension).map(|a| {
            format!(
                "{:?}|{:?}|{}|{}|{}|{}|{:?}",
                a.decision,
                a.lines,
                a.value.to_bits(),
                a.nodes,
                a.turns,
                a.omitted_pairs,
                a.unsupported
            )
        })
    };
    assert_eq!(&work, state);
    let mut stats = solver.stats();
    stats.nash_seconds = 0.0;
    stats.enumerate_seconds = 0.0;
    format!("{result:?}|{stats:?}")
}

#[test]
fn search_values_strategies_counters_budgets_and_input_restoration_match() {
    let _flat = FactoredScope::new(false);
    let state = toy();
    for side in [SideId::One, SideId::Two] {
        for chance in [Chance::Expect, Chance::Worst] {
            for depth in [1, 2] {
                for mixed in [false, true] {
                    assert_eq!(
                        signature(&state, None, false, side, chance, depth, mixed, None),
                        signature(&state, None, true, side, chance, depth, mixed, None)
                    );
                }
            }
        }
    }
    for limit in [0, 1, 5] {
        for mixed in [false, true] {
            assert_eq!(
                signature(
                    &state,
                    None,
                    false,
                    SideId::One,
                    Chance::Expect,
                    2,
                    mixed,
                    Some(limit)
                ),
                signature(
                    &state,
                    None,
                    true,
                    SideId::One,
                    Chance::Expect,
                    2,
                    mixed,
                    Some(limit)
                )
            );
        }
    }
    let mut unsupported = state.clone();
    unsupported.field[FieldEffect::Gravity as usize] = Effect {
        turns: Effect::PERMANENT,
        value: 0,
    };
    for mixed in [false, true] {
        assert_eq!(
            signature(
                &unsupported,
                None,
                false,
                SideId::One,
                Chance::Expect,
                1,
                mixed,
                None
            ),
            signature(
                &unsupported,
                None,
                true,
                SideId::One,
                Chance::Expect,
                1,
                mixed,
                None
            )
        );
    }
}

#[test]
fn parent_error_priority_terminal_replacement_and_midturn_fallback_match() {
    let _flat = FactoredScope::new(false);
    let (original, pair) = fixture("eject-button-uturn");
    let mut pending = original.clone();
    let outcomes = turn::enumerate_turn_with(&mut pending, RULES, pair, options()).unwrap();
    let suspended = outcomes.iter().find(|o| o.suspension.is_some()).unwrap();
    pending.apply(&suspended.instructions);
    let mut terminal = pending.clone();
    terminal.result = BattleResult::Tie;
    let mut replacement = original.clone();
    replacement.sides[0].party[0].hp = 0;
    replacement.sides[0].slots[0].party_index = None;
    for state in [&pending, &terminal, &replacement] {
        let invalid = [[SlotAction::Pass; 2]; 2];
        let mut batch = PreparedTurn::new(state, RULES, [vec![invalid[0]], vec![invalid[1]]]);
        same(
            state,
            turn::enumerate_turn_with(&mut state.clone(), RULES, invalid, options()),
            batch.enumerate([0, 0], options()),
        );
    }
    for (state, suspension) in [
        (&pending, suspended.suspension.as_ref()),
        (&terminal, suspended.suspension.as_ref()),
        (&replacement, None),
    ] {
        for mixed in [false, true] {
            assert_eq!(
                signature(
                    state,
                    suspension,
                    false,
                    SideId::One,
                    Chance::Expect,
                    1,
                    mixed,
                    None
                ),
                signature(
                    state,
                    suspension,
                    true,
                    SideId::One,
                    Chance::Expect,
                    1,
                    mixed,
                    None
                )
            );
        }
    }
}

#[test]
fn one_thread_exact_deep_and_deep_nash_reuse_validation() {
    let _flat = FactoredScope::new(false);
    let state = toy();
    for mode in ["exact", "deep", "deep-nash"] {
        let run = |enabled| {
            let mut config = Config::new(RULES, SideId::One);
            config.threads = 1;
            config.rolls = RollMode::Median;
            config.prepared_turn = enabled;
            config.outcome_cap = Some(1);
            config.exact_lines = true;
            let mut work = state.clone();
            let mut solver = Solver::new(config, &Heuristic);
            turn::reset_validation_counts();
            #[cfg(feature = "experiment-leaf-ending-observer")]
            turn::final_state_observer::reset();
            let result = match mode {
                "exact" => {
                    let a = solver.analyse(&mut work, None).unwrap();
                    format!(
                        "{:?}|{}|{}|{:?}|{}",
                        a.lines, a.nodes, a.turns, a.unsupported, a.omitted_pairs
                    )
                }
                "deep" => {
                    let a = solver.analyse_deep(&mut work, None, 2).unwrap();
                    format!("{:?}|{}|{}|{:?}", a.lines, a.nodes, a.turns, a.unsupported)
                }
                _ => {
                    let a = solver.analyse_deep_mixed(&mut work, None, 2).unwrap();
                    format!(
                        "{:?}|{:?}|{}|{}|{:?}|{:?}|{:?}",
                        a.matrix.values,
                        a.equilibrium,
                        a.nodes,
                        a.turns,
                        a.unsupported,
                        a.shallow.matrix.values,
                        a.shallow.equilibrium
                    )
                }
            };
            let counts = turn::validation_counts();
            assert_eq!(work, state);
            let mut stats = solver.stats();
            stats.enumerate_seconds = 0.0;
            stats.nash_seconds = 0.0;
            #[cfg(feature = "experiment-leaf-ending-observer")]
            let leaf = {
                let leaf = turn::final_state_observer::counts();
                [leaf.materialized_outcomes, leaf.batches, leaf.visits]
            };
            #[cfg(not(feature = "experiment-leaf-ending-observer"))]
            let leaf = [0usize; 3];
            (format!("{result}|{stats:?}"), counts, leaf)
        };
        let (a, ordinary, ordinary_leaf) = run(false);
        let (b, prepared, prepared_leaf) = run(true);
        assert_eq!(a, b, "{mode}");
        if cfg!(feature = "experiment-leaf-ending-states") {
            // The shallow turn edges use P9. Deep/deep-nash enumerate their beam's
            // non-leaf pairs through nash_cells, which has no PreparedMatrix, and
            // their child turn edges use P9 again. This toy has no replacement or
            // mid-turn decision, so none of these three modes consumes P8c's batch.
            assert_eq!(prepared, ordinary, "P9 bypasses P8c in {mode}");
            assert!(ordinary.into_iter().all(|count| count > 0));
        } else {
            // Without P9, each mode's shallow matrix consumes P8c's batch.
            assert!(
                prepared[0] < ordinary[0] && prepared[1] < ordinary[1] && prepared[2] < ordinary[2],
                "{mode}: ordinary={ordinary:?}, prepared={prepared:?}"
            );
        }
        #[cfg(feature = "experiment-leaf-ending-observer")]
        for leaf in [ordinary_leaf, prepared_leaf] {
            assert!(leaf[1] > 0 && leaf[2] > 0, "P9 must execute in {mode}");
            if mode == "exact" {
                assert_eq!(
                    leaf[0], 0,
                    "depth-one leaves must skip Outcome materialization"
                );
            }
        }
        println!("P8C_SEARCH_VALIDATIONS {mode} ordinary={ordinary:?} prepared={prepared:?} ordinary_leaf={ordinary_leaf:?} prepared_leaf={prepared_leaf:?}");
    }

    // Predetermined non-leaf coverage, independent of the corpus and its results.
    // Unlike deep/deep-nash's beam enumeration, pure depth-two matrix searches
    // consume PreparedMatrix at depth one and P9 at the depth-zero leaves.
    for mode in ["exact", "mixed"] {
        for side in [SideId::One, SideId::Two] {
            for chance in [Chance::Expect, Chance::Worst] {
                let run = |enabled| {
                    let mut config = Config::new(RULES, side);
                    config.threads = 1;
                    config.rolls = RollMode::Median;
                    config.depth = 2;
                    config.exact_lines = true;
                    config.chance = chance;
                    config.prepared_turn = enabled;
                    let mut work = state.clone();
                    let original_debug = format!("{state:?}");
                    let mut solver = Solver::new(config, &Heuristic);
                    turn::reset_validation_counts();
                    #[cfg(feature = "experiment-leaf-ending-observer")]
                    turn::final_state_observer::reset();
                    let result = if mode == "exact" {
                        let mut value = solver.analyse(&mut work, None).unwrap();
                        value.elapsed = std::time::Duration::ZERO;
                        format!("{value:?}")
                    } else {
                        let mut value = solver.analyse_mixed(&mut work, None).unwrap();
                        value.elapsed = std::time::Duration::ZERO;
                        format!("{value:?}")
                    };
                    let counts = turn::validation_counts();
                    assert_eq!(work, state);
                    assert_eq!(format!("{work:?}"), original_debug);
                    let mut stats = solver.stats();
                    stats.enumerate_seconds = 0.0;
                    stats.nash_seconds = 0.0;
                    #[cfg(feature = "experiment-leaf-ending-observer")]
                    let leaf = {
                        let value = turn::final_state_observer::counts();
                        [
                            value.batches,
                            value.visits,
                            value.materialized_outcomes,
                            value.emitted_instructions,
                        ]
                    };
                    #[cfg(not(feature = "experiment-leaf-ending-observer"))]
                    let leaf = [0usize; 4];
                    (format!("{result}|{stats:?}"), counts, leaf)
                };
                let (a, ordinary, ordinary_leaf) = run(false);
                let (b, prepared, prepared_leaf) = run(true);
                assert_eq!(a, b, "{mode} {side:?} {chance:?} depth two");
                assert!(
                    (0..3).all(|i| 0 < prepared[i] && prepared[i] < ordinary[i]),
                    "{mode} {side:?} {chance:?}: ordinary={ordinary:?}, prepared={prepared:?}"
                );
                #[cfg(feature = "experiment-leaf-ending-observer")]
                {
                    assert_eq!(ordinary_leaf, prepared_leaf);
                    assert!(
                        ordinary_leaf[..3].iter().all(|count| *count > 0),
                        "P9 leaves and materialized non-leaf outcomes must both execute"
                    );
                }
                println!("P8C_NONLEAF_VALIDATIONS {mode} {side:?} {chance:?} ordinary={ordinary:?} prepared={prepared:?} ordinary_leaf={ordinary_leaf:?} prepared_leaf={prepared_leaf:?}");
            }
        }
    }
}

#[test]
fn solver_full_factored_and_parallel_fallback_remains_identical() {
    let state = toy();
    for (rolls, factored, threads) in [
        (RollMode::Full, false, 1),
        (RollMode::Median, true, 1),
        (RollMode::Median, false, 2),
    ] {
        let _scope = FactoredScope::new(factored);
        let run = |enabled| {
            let mut config = Config::new(RULES, SideId::One);
            config.rolls = rolls;
            config.threads = threads;
            config.prepared_turn = enabled;
            let mut solver = Solver::new(config, &Heuristic);
            let mut work = state.clone();
            turn::reset_validation_counts();
            let a = solver.analyse_mixed(&mut work, None).unwrap();
            let counts = turn::validation_counts();
            assert_eq!(work, state);
            (
                format!(
                    "{:?}|{:?}|{}|{}|{:?}",
                    a.matrix.values, a.equilibrium, a.nodes, a.turns, a.unsupported
                ),
                counts,
            )
        };
        assert_eq!(run(false), run(true));
    }
}
