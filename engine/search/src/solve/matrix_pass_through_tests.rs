//! Independent scalar reference copied from immutable source048. No production selector.
use super::*;
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};

const ON: bool = cfg!(feature = "experiment-matrix-pass-through");

fn choices<const N: usize>(len: usize) -> Vec<Choice<N>> {
    (0..len)
        .map(|i| Choice::Switches([Some(i as u8); N]))
        .collect()
}
fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}
fn panic_text(error: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = error.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = error.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "non-string panic".to_owned()
    }
}
fn caught(f: impl FnOnce() -> Value) -> Value {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => json!({"ok":value}),
        Err(error) => json!({"panic":panic_text(error)}),
    }
}
fn filtered<const N: usize>(
    result: (Vec<Choice<N>>, Vec<Choice<N>>, Vec<f32>, usize, usize),
) -> Value {
    json!({"ours":format!("{:?}",result.0),"theirs":format!("{:?}",result.1),
           "values":bits(&result.2),"omitted_ours":result.3,"omitted_theirs":result.4})
}
fn eq(eq: &Equilibrium) -> Value {
    json!({"rows":bits(&eq.rows),"cols":bits(&eq.cols),"value":eq.value.to_bits(),
        "exploitability":eq.exploitability.to_bits(),"iterations":eq.iterations})
}
fn step_value(step: Step) -> Value {
    let outcome = match step.outcome {
        StepOutcome::Value(v) => json!({"value":v.to_bits()}),
        StepOutcome::Grow { row, col } => json!({"row":row,"col":col}),
        StepOutcome::Full => json!("Full"),
    };
    json!({"outcome":outcome,"solved":step.solved.as_ref().map(eq),"exploitability":step.exploitability.to_bits()})
}
fn game<const N: usize>(n: usize, m: usize, known: Vec<Option<f32>>, full: bool) -> LazyGame<N> {
    LazyGame {
        ours: choices(n),
        theirs: choices(m),
        next_depth: 0,
        known,
        requested: vec![],
        pending: vec![],
        rows: vec![0],
        cols: vec![0],
        full,
        rounds: 0,
        asked: 0,
        last: None,
    }
}
fn record_drop<const N: usize>(n: usize, m: usize, values: Vec<f32>) -> Value {
    let expected = caught(|| {
        filtered(reference_drop(
            choices::<N>(n),
            choices::<N>(m),
            values.clone(),
        ))
    });
    let actual = caught(|| {
        filtered(drop_unevaluable(
            choices::<N>(n),
            choices::<N>(m),
            values.clone(),
        ))
    });
    assert_eq!(expected, actual, "shape {n}x{m}, bits {:?}", bits(&values));
    json!({"slots":N,"rows":n,"cols":m,"input":bits(&values),"result":actual})
}

#[test]
fn exhaustive_small_matrices_preserve_choices_bits_and_omitted_counts() {
    let mut rows = Vec::new();
    for n in 0..=3 {
        for m in 0..=3 {
            for mask in 0..1usize << (n * m) {
                let pool = [
                    0, 0x80000000, 0x7f800000, 0xff800000, 1, 0x80000001, 0x3f800000, 0xbf800000,
                ];
                let nan = [0x7fc00001, 0x7fa00023, 0xffc00456];
                let values = (0..n * m)
                    .map(|i| {
                        f32::from_bits(if mask & (1 << i) == 0 {
                            pool[i % pool.len()]
                        } else {
                            nan[i % nan.len()]
                        })
                    })
                    .collect::<Vec<_>>();
                rows.push(record_drop::<1>(n, m, values.clone()));
                rows.push(record_drop::<2>(n, m, values));
            }
        }
    }
    assert_eq!(rows.len(), 1378);
    if let Some(path) = std::env::var_os("LAB_P14_UNIT_RECORDS") {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        for row in &rows {
            serde_json::to_writer(&mut out, row).unwrap();
            writeln!(out).unwrap();
        }
    }
    println!("P14 MATRIX CASES {}", rows.len());
}

#[test]
fn malformed_dimensions_keep_legacy_short_circuit_panics_and_trailing_values() {
    let nan = f32::from_bits(0x7fa00023);
    let cases = [
        (2, 2, vec![nan, nan]),
        (2, 2, vec![1., nan]),
        (2, 2, vec![]),
        (2, 2, vec![1., 2., 3., 4., nan]),
        (0, 3, vec![nan, 1.]),
        (3, 0, vec![nan]),
        (0, 0, vec![1.]),
    ];
    for (n, m, values) in cases {
        record_drop::<2>(n, m, values);
    }
    assert!(!complete_no_nan(usize::MAX, 2, &[]));
    assert!(!complete_no_nan(2, 2, &[1., 2., 3., 4., 5.]));
    assert!(complete_no_nan(0, 3, &[]));
    assert!(complete_no_nan(3, 0, &[]));
    let short = record_drop::<2>(2, 2, vec![nan, nan]);
    assert_eq!(short["result"]["ok"]["omitted_theirs"], 2);
    assert!(record_drop::<2>(2, 2, vec![1., nan])["result"]["panic"].is_string());
}

#[test]
fn lazy_full_and_restricted_steps_preserve_equilibria_and_error_order() {
    let nan = f32::from_bits(0xffc00456);
    let cases = [
        (2, 2, vec![Some(1.), Some(-1.), Some(-1.), Some(1.)], true),
        (
            2,
            3,
            vec![Some(0.), Some(-0.), Some(1.), Some(2.), Some(3.), Some(4.)],
            true,
        ),
        (
            1,
            2,
            vec![Some(f32::INFINITY), Some(f32::NEG_INFINITY)],
            true,
        ),
        (2, 2, vec![Some(nan); 4], true),
        (2, 2, vec![Some(nan), Some(nan)], true),
        (2, 2, vec![Some(1.), Some(nan)], true),
        (0, 2, vec![], true),
        (2, 0, vec![], true),
        (0, 2, vec![None], true),
        (0, 0, vec![None], true),
        (2, 2, vec![Some(nan), None, None, None], false),
        (2, 2, vec![Some(1.), Some(2.), Some(3.), Some(4.)], false),
    ];
    for (n, m, known, full) in cases {
        for dominance in [false, true] {
            let game = game::<2>(n, m, known.clone(), full);
            let expected = caught(|| step_value(reference_step(&game, dominance)));
            let actual = caught(|| step_value(game.step(dominance)));
            assert_eq!(
                expected, actual,
                "{n}x{m} full={full} dominance={dominance}"
            );
        }
    }
    let empty = game::<2>(0, 2, vec![None], true);
    assert_eq!(
        caught(|| step_value(empty.step(false)))["panic"],
        "a full game knows every cell"
    );
    let restricted = game::<2>(2, 2, vec![Some(nan), None, None, None], false);
    assert_eq!(step_value(restricted.step(false))["outcome"], "Full");
}

#[test]
fn eligible_owned_vectors_keep_all_three_buffers_and_capacity() {
    let (mut ours, mut theirs, mut values) = (
        choices::<2>(2),
        choices::<2>(3),
        vec![0., -0., f32::INFINITY, f32::NEG_INFINITY, 1., -1.],
    );
    ours.reserve(19);
    theirs.reserve(21);
    values.reserve(23);
    let pointer = (ours.as_ptr(), theirs.as_ptr(), values.as_ptr());
    let capacity = (ours.capacity(), theirs.capacity(), values.capacity());
    let expected = bits(&values);
    let (ours, theirs, values, a, b) = drop_unevaluable(ours, theirs, values);
    assert_eq!((a, b), (0, 0));
    assert_eq!(bits(&values), expected);
    if ON {
        assert_eq!((ours.as_ptr(), theirs.as_ptr(), values.as_ptr()), pointer);
        assert_eq!(
            (ours.capacity(), theirs.capacity(), values.capacity()),
            capacity
        );
    }
}

#[cfg(feature = "experiment-matrix-pass-through-observer")]
#[test]
fn observer_separates_a_and_b_shortcuts_and_fallbacks() {
    use matrix_pass_through_observer as observe;
    observe::reset();
    drop_unevaluable(choices::<2>(2), choices::<2>(2), vec![1.; 4]);
    drop_unevaluable(choices::<2>(1), choices::<2>(1), vec![f32::NAN]);
    let game = game::<2>(2, 2, vec![], true);
    game.full_matrix_parts(vec![1.; 4]);
    game.full_matrix_parts(vec![f32::NAN; 4]);
    let c = observe::counts();
    assert_eq!(
        (c.b_calls, c.b_passthrough, c.b_fallback),
        if ON { (2, 1, 1) } else { (2, 0, 2) }
    );
    assert_eq!(
        (c.a_calls, c.a_passthrough, c.a_fallback),
        if ON { (3, 1, 2) } else { (4, 0, 4) }
    );
}

struct Tracking;
thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const {Cell::new(None)}; }
fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|c| {
        if let Some(n) = c.get() {
            c.set(Some(n + 1));
        }
    });
}
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        System.realloc(ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}
#[global_allocator]
static ALLOCATOR: Tracking = Tracking;
fn allocations<T>(f: impl FnOnce() -> T) -> (usize, T) {
    ALLOCATIONS.set(Some(0));
    let result = std::hint::black_box(f());
    let count = ALLOCATIONS.replace(None).unwrap();
    (count, result)
}
#[test]
fn isolated_allocator_proves_a_reconstruction_and_b_choice_clone_removal() {
    const CHILD: &str = "LAB_P14_ALLOC_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","solve::matrix_pass_through_tests::isolated_allocator_proves_a_reconstruction_and_b_choice_clone_removal","--nocapture","--test-threads=1"])
            .env(CHILD,"1").output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        print!("{}", String::from_utf8_lossy(&result.stdout));
        return;
    }
    #[cfg(feature = "experiment-matrix-pass-through-observer")]
    matrix_pass_through_observer::reset();
    let make = || (choices::<2>(3), choices::<2>(4), vec![1.; 12]);
    let (a, b, v) = make();
    let (reference_a, _) = allocations(|| reference_drop(a, b, v));
    let (a, b, v) = make();
    let (actual_a, _) = allocations(|| drop_unevaluable(a, b, v));
    assert!(reference_a > 0);
    assert_eq!(actual_a, if ON { 0 } else { reference_a });
    let game = game::<2>(3, 4, vec![], true);
    // B's reference uses the ACTUAL A implementation, isolating the two choice clones.
    // The already materialized values Vec is setup; values collection and Nash still allocate.
    let values = vec![1.; 12];
    let (reference_b, _) = allocations(|| {
        let (a, b, v, _, _) = drop_unevaluable(game.ours.clone(), game.theirs.clone(), values);
        (a.len(), b.len(), v)
    });
    let values = vec![1.; 12];
    let (actual_b, _) = allocations(|| game.full_matrix_parts(values));
    assert!(reference_b >= 2);
    assert_eq!(actual_b, if ON { 0 } else { reference_b });
    if ON {
        assert_eq!(reference_b, 2);
    }
    println!("P14 ALLOC candidate={ON} a_reference={reference_a} a_actual={actual_a} b_reference={reference_b} b_actual={actual_b}");
}

// Original source048 filter, unchanged apart from its name.
fn reference_drop<const N: usize>(
    ours: Vec<Choice<N>>,
    theirs: Vec<Choice<N>>,
    values: Vec<f32>,
) -> (Vec<Choice<N>>, Vec<Choice<N>>, Vec<f32>, usize, usize) {
    let (n, m) = (ours.len(), theirs.len());
    let keep_col: Vec<bool> = (0..m)
        .map(|c| (0..n).all(|r| !values[r * m + c].is_nan()))
        .collect();
    let keep_row: Vec<bool> = (0..n)
        .map(|r| (0..m).all(|c| !keep_col[c] || !values[r * m + c].is_nan()))
        .collect();
    let mut dense = Vec::new();
    for r in 0..n {
        if !keep_row[r] {
            continue;
        }
        for c in 0..m {
            if keep_col[c] {
                dense.push(values[r * m + c]);
            }
        }
    }
    let omitted_theirs = keep_col.iter().filter(|k| !**k).count();
    let omitted_ours = keep_row.iter().filter(|k| !**k).count();
    let ours: Vec<Choice<N>> = ours
        .into_iter()
        .zip(&keep_row)
        .filter(|(_, k)| **k)
        .map(|(c, _)| c)
        .collect();
    let theirs: Vec<Choice<N>> = theirs
        .into_iter()
        .zip(&keep_col)
        .filter(|(_, k)| **k)
        .map(|(c, _)| c)
        .collect();
    (ours, theirs, dense, omitted_ours, omitted_theirs)
}

// Original source048 LazyGame::step, unchanged apart from receiver/reference names.
fn reference_step<const N: usize>(game: &LazyGame<N>, dominance: bool) -> Step {
    let m = game.theirs.len();
    if game.full || game.known.iter().flatten().any(|v| v.is_nan()) {
        if !game.full {
            // An unevaluable pair: `drop_unevaluable` needs the whole matrix.
            return Step {
                outcome: StepOutcome::Full,
                solved: None,
                exploitability: f32::NAN,
            };
        }
        let values: Vec<f32> = game
            .known
            .iter()
            .map(|v| v.expect("a full game knows every cell"))
            .collect();
        let (ours, theirs, values, _, _) =
            reference_drop(game.ours.clone(), game.theirs.clone(), values);
        if ours.is_empty() || theirs.is_empty() {
            return Step {
                outcome: StepOutcome::Value(f32::NAN),
                solved: None,
                exploitability: f32::NAN,
            };
        }
        let matrix = Matrix::new(ours.len(), theirs.len(), values);
        let eq = if dominance {
            nash::solve_reduced(&matrix, 20_000, 0.01)
        } else {
            nash::solve(&matrix, 20_000, 0.01)
        };
        let exploitability = eq.exploitability;
        return Step {
            outcome: StepOutcome::Value(eq.value),
            solved: Some(eq),
            exploitability,
        };
    }
    let at = |r: usize, c: usize| game.known[r * m + c].expect("a restricted row or column");
    let mut sub = Vec::with_capacity(game.rows.len() * game.cols.len());
    for &r in &game.rows {
        for &c in &game.cols {
            sub.push(at(r, c));
        }
    }
    let eq = nash::solve(
        &Matrix::new(game.rows.len(), game.cols.len(), sub),
        20_000,
        0.001,
    );
    // Best responses in the full game: every row against their restricted strategy (the
    // restricted columns are known in full), every column against ours.
    let mut best_row = (0, f32::NEG_INFINITY);
    for r in 0..game.ours.len() {
        let v: f32 = game
            .cols
            .iter()
            .zip(&eq.cols)
            .map(|(&c, &p)| p * at(r, c))
            .sum();
        if v > best_row.1 {
            best_row = (r, v);
        }
    }
    let mut best_col = (0, f32::INFINITY);
    for c in 0..m {
        let v: f32 = game
            .rows
            .iter()
            .zip(&eq.rows)
            .map(|(&r, &p)| p * at(r, c))
            .sum();
        if v < best_col.1 {
            best_col = (c, v);
        }
    }
    let exploitability = best_row.1 - best_col.1;
    let row = (!game.rows.contains(&best_row.0) && best_row.1 > eq.value).then_some(best_row.0);
    let col = (!game.cols.contains(&best_col.0) && best_col.1 < eq.value).then_some(best_col.0);
    let outcome = if exploitability <= LAZY_TOLERANCE || (row.is_none() && col.is_none()) {
        StepOutcome::Value(eq.value)
    } else {
        StepOutcome::Grow { row, col }
    };
    Step {
        outcome,
        solved: Some(eq),
        exploitability,
    }
}
