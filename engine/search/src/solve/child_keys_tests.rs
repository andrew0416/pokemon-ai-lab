use super::*;
use lab_engine::{
    eval::Heuristic,
    volatile::{Volatile, VolatileState},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "../../tests/p13_support.rs"]
mod support;

const ON: bool = cfg!(feature = "experiment-borrowed-child-keys");

fn config() -> Config {
    let mut value = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    value.threads = 1;
    value.rolls = RollMode::Median;
    value
}

fn state<const N: usize>(turn: u16) -> State<N> {
    State {
        turn,
        ..State::default()
    }
}

fn captures<const N: usize>() {
    // Each storage configuration runs six cases for both N: twelve subcases.
    for (ids, seeded, enabled, capacity, hits, misses, jobs_count) in [
        (vec![0, 1, 2, 3, 4, 5, 6, 7], None, true, 32, 0, 8, 8),
        (vec![0; 8], None, true, 32, 7, 1, 1),
        (vec![0; 8], Some(0), true, 32, 8, 0, 0),
        (vec![0, 1, 0, 2, 1, 3], Some(1), true, 32, 3, 3, 3),
        (vec![0; 8], None, false, 32, 0, 0, 8),
        (vec![0; 8], None, true, 0, 7, 1, 1),
    ] {
        let mut solver = Solver::<N, _>::new(config(), &Heuristic);
        solver.tt = TranspositionTable::new(enabled, capacity);
        if let Some(key) = seeded {
            solver.tt.insert((state(key), None), 7.0);
        }
        let (mut slots, mut jobs, mut seen) = (
            vec![Some(Err(SearchError::Budget)), Some(Ok(10000.0))],
            Vec::new(),
            ChildSeen::new(),
        );
        child_keys_observer::reset();
        for (arrival, &id) in ids.iter().enumerate() {
            let before = state::<N>(id);
            let slot = solver.queue_child(
                &before,
                None,
                Decision::Turn,
                &mut slots,
                &mut jobs,
                &mut seen,
            );
            assert!(
                slot >= 2,
                "slot indices must include preceding decision-error/terminal slots"
            );
            if let Some((stored_slot, _)) = jobs
                .iter()
                .find(|(_, job)| job.state == before)
                .filter(|_| enabled)
            {
                assert_eq!(*stored_slot, slot);
            } else if !enabled {
                assert_eq!(slot, arrival + 2);
                assert_eq!(jobs.len(), arrival + 1);
            }
        }
        let counts = child_keys_observer::counts();
        assert_eq!(counts.job_captures, jobs_count);
        assert_eq!(
            counts.key_captures,
            if ON || !enabled { 0 } else { ids.len() }
        );
        assert_eq!(
            (solver.stats.tt_hits, solver.stats.tt_misses),
            (hits, misses)
        );
        assert_eq!(jobs.len(), jobs_count);
        assert_eq!(
            counts.borrowed_queries,
            if ON && enabled { ids.len() } else { 0 }
        );
    }
}

#[test]
fn twelve_batch_capture_cases_preserve_slots_stats_and_disabled_capacity_zero() {
    captures::<1>();
    captures::<2>();
}

fn single_captures<const N: usize>() {
    for mode in ["hit", "miss", "error"] {
        let mut input = support::toy::<N>();
        let original = input.clone();
        let mut cfg = config();
        if mode == "error" {
            cfg.max_turns = Some(0);
        }
        let mut solver = Solver::new(cfg, &Heuristic);
        if mode == "hit" {
            solver.tt.insert((input.clone(), None), 17.0);
        }
        child_keys_observer::reset();
        let result = solver.nash_value(&mut input, None);
        let counts = child_keys_observer::counts();
        assert_eq!(input, original);
        assert_eq!(counts.key_captures, usize::from(!ON));
        assert_eq!(counts.job_captures, usize::from(mode != "hit"));
        assert_eq!(counts.borrowed_queries, usize::from(ON));
        match mode {
            "hit" => {
                assert_eq!(result.unwrap(), 17.0);
                assert_eq!(solver.stats.tt_hits, 1);
            }
            "miss" => {
                assert!(result.unwrap().is_finite());
                assert_eq!(solver.tt_len(), 1);
            }
            _ => {
                assert!(matches!(result, Err(SearchError::Budget)));
                assert_eq!(solver.tt_len(), 0);
            }
        }
    }
}

#[test]
fn six_actual_nash_value_hit_miss_error_capture_cases_restore_state() {
    single_captures::<1>();
    single_captures::<2>();
}

#[cfg(feature = "experiment-borrowed-child-keys")]
#[test]
fn forced_collision_chain_retains_full_state_and_suspension_across_growth() {
    let (_, rest) = support::suspended();
    let mut jobs = Vec::new();
    let mut seen = ChildSeen::<2>::new();
    for index in 0..40 {
        let mut value = state::<2>(index);
        value.sides[0].history.ate_berry = index as u8;
        value.sides[0].slots[0].volatiles.set(
            Volatile::ALL[0],
            VolatileState {
                hidden: index as u8,
                ..VolatileState::NONE
            },
        );
        let suspension = if index % 2 == 0 {
            Some(rest.clone())
        } else {
            None
        };
        jobs.push((
            index as usize * 3 + 7,
            ChildJob {
                state: value,
                suspension,
                decision: Decision::Turn,
            },
        ));
        seen.insert(42, jobs.len() - 1);
    }
    for (slot, job) in &jobs {
        assert_eq!(
            seen.find(42, &job.state, job.suspension.as_ref(), &jobs),
            Some(*slot)
        );
        let mut changed = job.state.clone();
        changed.sides[0].party[0].moves[0].pp = 1;
        assert_eq!(
            seen.find(42, &changed, job.suspension.as_ref(), &jobs),
            None
        );
        assert_eq!(
            seen.find(43, &job.state, job.suspension.as_ref(), &jobs),
            None
        );
        if job.suspension.is_some() {
            assert_eq!(seen.find(42, &job.state, None, &jobs), None);
        }
    }
    assert!(child_keys_observer::counts().seen_collisions > 0);
}

#[test]
fn nash_cells_budget_and_transition_errors_do_not_register_children() {
    use lab_engine::action::SlotAction;
    let mut input = support::toy::<1>();
    let before = input.clone();
    let invalid = Choice::Turn([SlotAction::Move {
        index: 99,
        target: 0,
        gimmick: lab_engine::gimmick::Gimmick::None,
    }]);
    for budget in [Some(0), None] {
        let mut cfg = config();
        cfg.max_turns = budget;
        let mut solver = Solver::new(cfg, &Heuristic);
        child_keys_observer::reset();
        let result = solver.nash_cells(
            &mut input,
            Decision::Turn,
            None,
            &[[invalid, invalid]],
            Some(1),
            &[],
        );
        assert!(result.is_err());
        if budget.is_some() {
            assert!(matches!(result, Err(SearchError::Budget)));
        }
        assert_eq!(
            child_keys_observer::counts(),
            child_keys_observer::Counts::default()
        );
        assert_eq!(solver.nodes, 0);
        assert_eq!(input, before);
    }
}

struct CountAlloc;
static TRACK: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        System.realloc(ptr, layout, size)
    }
}

fn allocations(f: impl FnOnce()) -> usize {
    ALLOCS.store(0, Ordering::Relaxed);
    TRACK.store(true, Ordering::Relaxed);
    f();
    TRACK.store(false, Ordering::Relaxed);
    ALLOCS.load(Ordering::Relaxed)
}

#[test]
fn allocator_subprocess_proves_owned_lookup_allocations_are_removed() {
    const CHILD: &str = "LAB_P13_ALLOC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "solve::child_keys_tests::allocator_subprocess_proves_owned_lookup_allocations_are_removed", "--test-threads=1", "--nocapture"])
            .env(CHILD, "1").output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        print!("{}", String::from_utf8_lossy(&result.stdout));
        return;
    }
    let (mut input, rest) = support::suspended();
    for &v in &Volatile::ALL[..5] {
        input.sides[0].slots[0].volatiles.set(
            v,
            VolatileState {
                hidden: 1,
                ..VolatileState::NONE
            },
        );
    }
    let mut solver = Solver::new(config(), &Heuristic);
    solver.tt.insert((input.clone(), Some(rest.clone())), 23.0);
    // This is the exact original owned-key lookup, including real Clone allocation.
    let reference = allocations(|| {
        let key = (input.clone(), Some(rest.clone()));
        assert_eq!(solver.tt.get(std::hint::black_box(&key)), Some(23.0));
    });
    let (mut slots, mut jobs, mut seen) = (Vec::with_capacity(1), Vec::new(), ChildSeen::new());
    child_keys_observer::reset();
    let actual = allocations(|| {
        solver.queue_child(
            &input,
            Some(&rest),
            Decision::MidTurn,
            &mut slots,
            &mut jobs,
            &mut seen,
        );
    });
    assert!(reference > 0, "nonempty Suspension owns a queue");
    assert_eq!(actual, if ON { 0 } else { reference });
    assert_eq!(slots, vec![Some(Ok(23.0))]);
    println!("allocator old={reference} actual={actual} candidate={ON}");
}

#[cfg(feature = "experiment-borrowed-child-keys")]
#[test]
fn collision_guard_distinguishes_owned_payloads_and_different_some_suspensions() {
    let (base, first) = support::suspended();
    let (_, second) = support::suspended_named("uturn-pause");
    assert_ne!(first, second, "different remaining queues");
    let mut variants = vec![base.clone()];
    for change in 0..8 {
        let mut changed = base.clone();
        match change {
            0 => changed.sides[0].party[0].hp -= 1,
            1 => changed.sides[0].party[0].moves[0].pp ^= 1,
            2 => changed.turn += 1,
            3 => changed.sides[0].party.swap(0, 1),
            4 => changed.sides[0].history.ate_berry ^= 1,
            7 => changed.sides[0].party_order.swap(0, 1),
            5 => {
                for &v in &Volatile::ALL[..5] {
                    changed.sides[0].slots[0].volatiles.set(
                        v,
                        VolatileState {
                            hidden: 197,
                            ..VolatileState::NONE
                        },
                    );
                }
            }
            _ => {
                changed.sides[0].party[0].transformed =
                    Some(lab_engine::state::TransformBase::default())
            }
        }
        assert_ne!(changed, base);
        variants.push(changed);
    }
    let mut jobs = Vec::new();
    let mut seen = ChildSeen::new();
    for state in variants {
        for rest in [None, Some(first.clone()), Some(second.clone())] {
            let slot = jobs.len() * 2 + 11;
            jobs.push((
                slot,
                ChildJob {
                    state: state.clone(),
                    suspension: rest,
                    decision: Decision::Turn,
                },
            ));
            seen.insert(7, jobs.len() - 1);
        }
    }
    for (slot, job) in &jobs {
        assert_eq!(
            seen.find(7, &job.state, job.suspension.as_ref(), &jobs),
            Some(*slot)
        );
    }
}

/// Private capacity and error-order records supplement the public exact-output probe.
#[test]
fn exact_private_policy_records() {
    use lab_engine::action::{Gimmick, SlotAction};
    use serde_json::json;
    use std::io::Write;
    let mut records = Vec::new();
    for capacity in [0, 1, 2] {
        let mut input = support::toy::<1>();
        let original = input.clone();
        let mut solver = Solver::new(config(), &Heuristic);
        solver.tt = TranspositionTable::new(true, capacity);
        let mut values = Vec::new();
        for turn in [0, 1, 0] {
            input.turn = turn;
            values.push(solver.nash_value(&mut input, None).unwrap().to_bits());
        }
        input.turn = original.turn;
        assert_eq!(input, original);
        let SearchStats {
            nash_seconds: _,
            enumerate_seconds: _,
            tt_hits,
            tt_misses,
            nash_solves,
            nash_iterations,
            deep_tt_hits,
            deep_tt_misses,
            split_cells,
        } = solver.stats;
        records.push(json!({"capacity":capacity,"values":values,"tt_len":solver.tt_len(),
            "stats":[tt_hits,tt_misses,nash_solves,nash_iterations,deep_tt_hits,deep_tt_misses,split_cells],
            "nodes":solver.nodes,"turns":solver.turns,"unsupported":solver.unsupported,
            "omitted":solver.omitted_pairs,"broken":solver.plan_broken,"state":format!("{input:?}")}));
    }
    for reverse in [false, true] {
        let mut input = support::toy::<1>();
        let original = input.clone();
        let invalid = |index| {
            Choice::Turn([SlotAction::Move {
                index,
                target: 0,
                gimmick: Gimmick::None,
            }])
        };
        let valid = Choice::Turn([SlotAction::Move {
            index: 0,
            target: 0,
            gimmick: Gimmick::None,
        }]);
        let mut pairs = [[invalid(98), valid], [valid, valid], [invalid(99), valid]];
        if reverse {
            pairs.reverse();
        }
        let mut solver = Solver::new(config(), &Heuristic);
        let result = solver.nash_cells(&mut input, Decision::Turn, None, &pairs, Some(1), &[]);
        let mut oracle = Solver::new(config(), &Heuristic);
        let first_error =
            oracle.nash_cells(&mut input, Decision::Turn, None, &pairs[..1], Some(1), &[]);
        assert!(result.is_err());
        assert_eq!(result, first_error);
        assert_eq!(input, original);
        records.push(
            json!({"reverse":reverse,"result":format!("{result:?}"),"nodes":solver.nodes,
            "turns":solver.turns,"unsupported":solver.unsupported,"omitted":solver.omitted_pairs,
            "broken":solver.plan_broken,"state":format!("{input:?}")}),
        );
    }
    if let Some(path) = std::env::var_os("LAB_P13_UNIT_RECORDS") {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        for row in records {
            serde_json::to_writer(&mut out, &row).unwrap();
            writeln!(out).unwrap();
        }
    }
}
