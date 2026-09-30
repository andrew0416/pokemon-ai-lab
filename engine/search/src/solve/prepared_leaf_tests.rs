use super::*;
use lab_engine::{
    eval::Heuristic,
    hash::key_hash,
    turn::{self, FactoredScope},
};
use std::sync::Mutex;

fn toy() -> State<2> {
    let mon = |name| serde_json::json!({"species":name,"ability":"Honey Gather","nature":"Serious","evs":{},"moves":["Harden","Protect"],"level":50});
    let json = serde_json::json!({"format":lab_scenario::DOUBLES_FORMAT,
        "p1":{"team":[mon("Talonflame"),mon("Snorlax")],"order":"12"},
        "p2":{"team":[mon("Swampert"),mon("Excadrill")],"order":"12"}});
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let loaded = lab_scenario::load_scenario_str(&json.to_string(), &root).unwrap();
    lab_scenario::scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap()
    .remove(0)
    .state
}
#[derive(Default)]
struct Trace {
    states: Mutex<Vec<(String, u64, u64)>>,
    nan: bool,
}
impl Evaluator<2> for Trace {
    fn evaluate(&self, s: &State<2>) -> f32 {
        self.states
            .lock()
            .unwrap()
            .push((format!("{s:?}"), s.position_hash(), key_hash(0, s)));
        if self.nan {
            f32::from_bits(0x7fc00155)
        } else {
            Heuristic.evaluate(s)
        }
    }
}
fn config(us: SideId, chance: Chance) -> Config {
    let mut c = Config::new(Ruleset::CHAMPIONS_MC, us);
    c.threads = 1;
    c.rolls = RollMode::Median;
    c.chance = chance;
    c
}
fn run(
    state: &State<2>,
    ours: &[Choice<2>],
    theirs: &[Choice<2>],
    mut c: Config,
    enabled: bool,
    nan: bool,
    window: (f32, f32),
) -> String {
    c.prepared_turn = enabled;
    let trace = Trace {
        nan,
        ..Default::default()
    };
    let mut solver = Solver::new(c, &trace);
    let mut batch = PreparedMatrix::new(state, c, Decision::Turn, ours, theirs, &mut solver.stats);
    let mut work = state.clone();
    let mut results = Vec::new();
    for (r, &a) in ours.iter().enumerate() {
        for (col, &b) in theirs.iter().enumerate() {
            let pair = solver.pair(a, b);
            results.push(
                solver
                    .chance_prepared(
                        &mut work,
                        Decision::Turn,
                        None,
                        pair,
                        Next::Depth(0),
                        window.0,
                        window.1,
                        batch.as_mut().map(|p| (p, r, col)),
                    )
                    .map(f32::to_bits),
            );
            assert_eq!(work, *state);
            assert_eq!(format!("{work:?}"), format!("{state:?}"));
            assert_eq!(work.position_hash(), state.position_hash());
        }
    }
    let mut stats = solver.stats();
    stats.enumerate_seconds = 0.0;
    stats.nash_seconds = 0.0;
    format!(
        "{results:?}|{}|{}|{:?}|{stats:?}|{:?}",
        solver.nodes,
        solver.turns,
        solver.unsupported,
        trace.states.lock().unwrap()
    )
}
#[test]
fn asymmetric_side_mapping_nan_cutoff_and_evaluator_order_match() {
    let _flat = FactoredScope::new(false);
    let state = toy();
    for us in [SideId::One, SideId::Two] {
        let mut ours = game::legal_choices(
            &state,
            Ruleset::CHAMPIONS_MC,
            Decision::Turn,
            us,
            Pruning::All,
        );
        let mut theirs = game::legal_choices(
            &state,
            Ruleset::CHAMPIONS_MC,
            Decision::Turn,
            us.other(),
            Pruning::All,
        );
        ours.truncate(2);
        theirs.truncate(3);
        assert_eq!((ours.len(), theirs.len()), (2, 3));
        for chance in [Chance::Expect, Chance::Worst] {
            for nan in [false, true] {
                for window in [(f32::NEG_INFINITY, f32::INFINITY), (-1.0, 1.0)] {
                    assert_eq!(
                        run(
                            &state,
                            &ours,
                            &theirs,
                            config(us, chance),
                            false,
                            nan,
                            window
                        ),
                        run(
                            &state,
                            &ours,
                            &theirs,
                            config(us, chance),
                            true,
                            nan,
                            window
                        )
                    );
                }
            }
        }
    }
}
#[test]
fn serial_budget_precedes_validation_and_nonleaf_declines() {
    let _flat = FactoredScope::new(false);
    let state = toy();
    let ours = game::legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        Decision::Turn,
        SideId::One,
        Pruning::All,
    );
    let theirs = game::legal_choices(
        &state,
        Ruleset::CHAMPIONS_MC,
        Decision::Turn,
        SideId::Two,
        Pruning::All,
    );
    for max in [0, 1] {
        let mut c = config(SideId::One, Chance::Expect);
        c.max_turns = Some(max);
        let old = run(
            &state,
            &ours[..1],
            &theirs[..2],
            c,
            false,
            false,
            (-1.0, 1.0),
        );
        #[cfg(feature = "experiment-prepared-leaf-observer")]
        {
            turn::reset_validation_counts();
            turn::prepared_leaf_observer::reset();
        }
        let new = run(
            &state,
            &ours[..1],
            &theirs[..2],
            c,
            true,
            false,
            (-1.0, 1.0),
        );
        assert_eq!(old, new);
        assert!(new.contains("Budget"));
        #[cfg(feature = "experiment-prepared-leaf-observer")]
        {
            let counts = turn::prepared_leaf_observer::counts();
            assert_eq!(counts.requests, max);
            if max == 0 {
                assert_eq!(turn::validation_counts(), [0, 0, 0]);
            }
        }
    }
    let c = config(SideId::One, Chance::Expect);
    let trace = Trace::default();
    let mut solver = Solver::new(c, &trace);
    let mut batch =
        PreparedMatrix::new(&state, c, Decision::Turn, &ours, &theirs, &mut solver.stats).unwrap();
    let pair = solver.pair(ours[0], theirs[0]);
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    turn::prepared_leaf_observer::reset();
    assert!(solver
        .try_leaf_endings(
            &mut state.clone(),
            Decision::Turn,
            pair,
            Next::Depth(1),
            -1.0,
            1.0,
            Instant::now(),
            Some((&mut batch, 0, 0))
        )
        .is_none());
    assert!(solver
        .try_leaf_endings(
            &mut state.clone(),
            Decision::Replacement,
            pair,
            Next::Depth(0),
            -1.0,
            1.0,
            Instant::now(),
            Some((&mut batch, 0, 0))
        )
        .is_none());
    assert!(trace.states.lock().unwrap().is_empty());
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    assert_eq!(turn::prepared_leaf_observer::counts().requests, 0);
}
#[test]
fn execution_error_returns_no_evaluator_visit_and_restores_snapshot() {
    let _flat = FactoredScope::new(false);
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/scenarios/rr-rivalry-switch-in-undecided-gender.json");
    let loaded = lab_scenario::load_scenario_file(path).unwrap();
    let p = lab_scenario::scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap()
    .remove(0);
    let lab_scenario::Decision::Turn(pair) = lab_scenario::scenario_decision(&loaded, &p).unwrap()
    else {
        panic!("turn")
    };
    let mut state = p.state;
    state.sides[1].party[0].gender = lab_engine::dex::Gender::Random;
    let ours = [Choice::Turn(pair[0])];
    let theirs = [Choice::Turn(pair[1])];
    let old = run(
        &state,
        &ours,
        &theirs,
        config(SideId::One, Chance::Expect),
        false,
        false,
        (-1.0, 1.0),
    );
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    {
        turn::prepared_leaf_observer::reset();
        turn::final_state_observer::reset();
    }
    let new = run(
        &state,
        &ours,
        &theirs,
        config(SideId::One, Chance::Expect),
        true,
        false,
        (-1.0, 1.0),
    );
    assert_eq!(old, new);
    assert!(new.contains("Rivalry next to") && new.contains("of undecided gender"));
    assert!(new.ends_with("|[]"));
    #[cfg(feature = "experiment-prepared-leaf-observer")]
    {
        let c = turn::prepared_leaf_observer::counts();
        assert_eq!((c.requests, c.errors, c.batches), (1, 1, 0));
        assert_eq!(turn::final_state_observer::counts().visits, 0);
    }
}
