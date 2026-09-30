//! P16 exact arithmetic and independent allocator proof; no timing assertions.
use lab_search::nash::{self, Matrix};
use serde_json::{json, Value};
use std::io::Write;
#[path = "p16_reference.rs"]
mod reference;

fn bits(eq: &nash::Equilibrium) -> Value {
    json!({"rows":eq.rows.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "cols":eq.cols.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "value":eq.value.to_bits(),"exploitability":eq.exploitability.to_bits(),"iterations":eq.iterations})
}
fn reference_bits(eq: &reference::Equilibrium) -> Value {
    json!({"rows":eq.rows.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "cols":eq.cols.iter().map(|x|x.to_bits()).collect::<Vec<_>>(),
        "value":eq.value.to_bits(),"exploitability":eq.exploitability.to_bits(),"iterations":eq.iterations})
}
fn record(matrix: &Matrix, limit: usize, tolerance: f32, reduced: bool) -> Value {
    let rm = reference::Matrix { rows:matrix.rows, cols:matrix.cols, values:matrix.values.clone() };
    let want = if reduced { reference::solve_reduced(&rm,limit,tolerance) } else { reference::solve(&rm,limit,tolerance) };
    let got = if reduced { nash::solve_reduced(matrix,limit,tolerance) } else { nash::solve(matrix,limit,tolerance) };
    assert_eq!(bits(&got),reference_bits(&want),"{}x{}, limit={limit}, tolerance={:08x}, reduced={reduced}",matrix.rows,matrix.cols,tolerance.to_bits());
    json!({"rows":matrix.rows,"cols":matrix.cols,"values":matrix.values.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),
        "limit":limit,"tolerance_bits":tolerance.to_bits(),"reduced":reduced,"output":bits(&got)})
}
fn write_records(env: &str, rows: &[Value]) {
    if let Some(path)=std::env::var_os(env) {
        let mut f=std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
        for row in rows { serde_json::to_writer(&mut f,row).unwrap(); writeln!(f).unwrap(); }
    }
}
const LIMITS: [usize;9] = [0,1,15,16,17,31,32,33,65];

#[test]
fn exact_reference_matrix_records_preserve_bits_and_stopping() {
    let mut rows=Vec::new();
    let mut seed=0x243f6a8885a308d3u64;
    for case in 0..64 {
        let (n,m)=(1+case%9,1+(case*5)%8);
        let values=(0..n*m).map(|_| {
            seed^=seed>>12; seed^=seed<<25; seed^=seed>>27;
            let raw=seed.wrapping_mul(0x2545f4914f6cdd1d);
            ((raw>>40) as f32 / (1u64<<24) as f32)*400.0-200.0
        }).collect();
        let matrix=Matrix::new(n,m,values);
        for limit in LIMITS { for tol in [-1.0,0.0,0.01,f32::INFINITY] {
            rows.push(record(&matrix,limit,tol,false));
        }}
        let first=nash::solve(&matrix,16,-1.0).exploitability;
        // Adjacent f32 thresholds detect accidentally comparing the f64 intermediate.
        for tol in [first,f32::from_bits(first.to_bits().saturating_sub(1)),f32::from_bits(first.to_bits()+1)] {
            rows.push(record(&matrix,33,tol,false));
        }
        rows.push(record(&matrix,33,0.01,true));
    }
    for values in [vec![0.0,-0.0,-0.0,0.0], vec![f32::INFINITY,1.0,-2.0,3.0],
        vec![f32::NEG_INFINITY,1.0,-2.0,3.0],vec![f32::INFINITY,f32::NEG_INFINITY,0.0,-0.0],
        vec![f32::from_bits(0x7fc01234),1.0,2.0,3.0],vec![f32::from_bits(0xffc05678),1.0,2.0,3.0],
        vec![f32::from_bits(1),f32::from_bits(0x80000001),f32::MAX,f32::MIN],vec![3.0,2.0,1.0,0.0]] {
        let matrix=Matrix::new(2,2,values);
        for limit in LIMITS { for tol in [-1.0,-0.0,0.0,0.01,f32::NAN,f32::INFINITY,f32::NEG_INFINITY] {
            rows.push(record(&matrix,limit,tol,false));
        }}
    }
    let dominated=Matrix::new(3,3,vec![3.0,-1.0,5.0,2.0,-1.0,4.0,3.0,-1.0,5.0]);
    for limit in LIMITS { rows.push(record(&dominated,limit,0.01,true)); }
    assert_eq!(rows.len(),3073);
    write_records("LAB_P16_MATRIX_RECORDS",&rows);
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(v)=payload.downcast_ref::<String>() { v.clone() }
    else if let Some(v)=payload.downcast_ref::<&str>() { v.to_string() }
    else { format!("other:{:?}",(*payload).type_id()) }
}
#[test]
fn malformed_public_matrices_preserve_panics_and_trailing_value_behavior() {
    // Silence only expected caught panics; restore the process hook before assertions.
    let hook=std::panic::take_hook(); std::panic::set_hook(Box::new(|_|{}));
    let mut rows=Vec::new();
    for (n,m) in [(0,0),(0,2),(2,0),(1,1),(2,3),(3,2)] {
        for len in 0..=n*m+3 { for limit in [0,1,16,17] {
            let values:Vec<f32>=(0..len).map(|i|i as f32-3.0).collect();
            let matrix=Matrix{rows:n,cols:m,values:values.clone()};
            let rm=reference::Matrix{rows:n,cols:m,values};
            for reduced in [false,true] {
                let want=std::panic::catch_unwind(|| if reduced {reference::solve_reduced(&rm,limit,-1.0)} else {reference::solve(&rm,limit,-1.0)})
                    .map(|x|reference_bits(&x)).map_err(panic_text);
                let got=std::panic::catch_unwind(|| if reduced {nash::solve_reduced(&matrix,limit,-1.0)} else {nash::solve(&matrix,limit,-1.0)})
                    .map(|x|bits(&x)).map_err(panic_text);
                rows.push(json!({"rows":n,"cols":m,"len":len,"limit":limit,"reduced":reduced,"want":want,"got":got}));
            }
        }}
    }
    std::panic::set_hook(hook);
    for row in &rows { assert_eq!(row["got"],row["want"],"{row}"); }
    assert_eq!(rows.len(),296);
    write_records("LAB_P16_MALFORMED_RECORDS",&rows);
}

// Separate integration binary: cannot collide with P13's library-test global allocator.
struct CountingAllocator;
thread_local! {
    static TRACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ALLOCS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn note_alloc() { if TRACK.try_with(|x|x.get()).unwrap_or(false) { let _=ALLOCS.try_with(|x|x.set(x.get()+1)); } }
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout:std::alloc::Layout)->*mut u8 { note_alloc(); std::alloc::System.alloc(layout) }
    unsafe fn alloc_zeroed(&self, layout:std::alloc::Layout)->*mut u8 { note_alloc(); std::alloc::System.alloc_zeroed(layout) }
    unsafe fn realloc(&self,p:*mut u8,l:std::alloc::Layout,n:usize)->*mut u8 { note_alloc(); std::alloc::System.realloc(p,l,n) }
    unsafe fn dealloc(&self,p:*mut u8,l:std::alloc::Layout) { std::alloc::System.dealloc(p,l) }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator=CountingAllocator;
fn measured<T>(f:impl FnOnce()->T)->(T,usize) {
    ALLOCS.set(0); TRACK.set(true); let out=f(); TRACK.set(false); (out,ALLOCS.get())
}
#[test]
fn isolated_allocator_proves_checkpoint_allocations_removed() {
    if std::env::var_os("LAB_P16_ALLOC_CHILD").is_none() {
        let output=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","isolated_allocator_proves_checkpoint_allocations_removed","--nocapture","--test-threads=1"])
            .env("LAB_P16_ALLOC_CHILD","1").output().unwrap();
        assert!(output.status.success(),"{}\n{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
        print!("{}",String::from_utf8_lossy(&output.stdout));
        return;
    }
    let matrix=Matrix::new(3,3,vec![1.0,-1.0,0.0,-1.0,0.0,1.0,0.0,1.0,-1.0]);
    let rm=reference::Matrix{rows:3,cols:3,values:matrix.values.clone()};
    let mut records=Vec::new();
    for limit in LIMITS {
        let (want,a)=measured(||std::hint::black_box(reference::solve(std::hint::black_box(&rm),limit,-1.0)));
        let (got,b)=measured(||std::hint::black_box(nash::solve(std::hint::black_box(&matrix),limit,-1.0)));
        assert_eq!(bits(&got),reference_bits(&want));
        let q=if limit==0 {1} else {limit.div_ceil(16)};
        assert_eq!(a,9+5*q,"baseline limit={limit}");
        assert_eq!(b,if cfg!(feature="experiment-nash-scratch"){11}else{a},"candidate limit={limit}");
        records.push(json!({"limit":limit,"iterations":got.iterations,"checkpoints":q,"reference":a,"actual":b}));
    }
    println!("P16_ALLOC {}",json!({"candidate":cfg!(feature="experiment-nash-scratch"),"records":records}));
}

#[cfg(feature="experiment-nash-scratch-observer")]
#[test]
fn observer_counts_checkpoint_work_and_owned_materializations() {
    use nash::scratch_observer::{reset,counts};
    let matrix=Matrix::new(2,2,vec![1.0,-1.0,-1.0,1.0]);
    reset();
    let mut q=0; let mut iterations=0;
    for limit in LIMITS { let out=nash::solve(&matrix,limit,-1.0); iterations+=out.iterations; q+=if limit==0{1}else{limit.div_ceil(16)}; }
    let c=counts();
    assert_eq!((c.solve_calls,c.checkpoints,c.iterations),(LIMITS.len(),q,iterations));
    if cfg!(feature="experiment-nash-scratch") {
        assert_eq!((c.normalization_allocations,c.evaluation_scratch_allocations,c.output_materializations),(0,0,LIMITS.len()));
    } else { assert_eq!((c.normalization_allocations,c.evaluation_scratch_allocations,c.output_materializations),(2*q,q,q)); }
    println!("P16_ACTIVATION {}",json!({"solve_calls":c.solve_calls,"checkpoints":c.checkpoints,"iterations":c.iterations,
        "normalization_allocations":c.normalization_allocations,"evaluation_scratch_allocations":c.evaluation_scratch_allocations,"output_materializations":c.output_materializations}));
}
