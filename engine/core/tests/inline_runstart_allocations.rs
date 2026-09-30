//! Actual allocation evidence in a dedicated test executable/subprocess. The measurement
//! covers only the genuine RunStart capture/clone or its original Vec clone field operation.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::process::Command;

use lab_engine::state::{PokemonRef, SideId};
use lab_engine::turn::inline_runstart_observer::{self as observer, CaptureProbe};

struct Allocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn tracked<T>(run: impl FnOnce() -> T) -> (T, usize) {
    struct Stop;
    impl Drop for Stop {
        fn drop(&mut self) {
            TRACK.with(|track| track.set(false));
        }
    }
    ALLOCATIONS.with(|count| count.set(0));
    TRACK.with(|track| track.set(true));
    let stop = Stop;
    let value = std::hint::black_box(run());
    drop(stop);
    (value, ALLOCATIONS.with(Cell::get))
}

#[test]
fn runstart_capture_and_clone_remove_only_inline_allocations() {
    const CHILD: &str = "LAB_P11_ALLOCATION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runstart_capture_and_clone_remove_only_inline_allocations",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let output = String::from_utf8(result.stdout).unwrap();
        assert_eq!(output.matches("P11 allocation proof:").count(), 7);
        print!("{output}");
        return;
    }
    for count in [0, 1, 2, 4, 5, 12, 31] {
        let entries: Vec<_> = (0..count)
            .map(|index| {
                (
                    PokemonRef {
                        side: SideId::One,
                        party: (index % 6) as u8,
                    },
                    index as i32 * 19 - 88,
                )
            })
            .collect();
        let expected = entries.clone();
        let mut probe = CaptureProbe::new(entries);
        observer::reset();
        let (reference, original_allocs) = tracked(|| probe.reference_snapshot_clone());
        let (captured, capture_allocs) = tracked(|| probe.capture());
        let (cloned, clone_allocs) = tracked(|| captured.clone());
        assert_eq!(reference, expected);
        assert_eq!(captured.as_slice(), expected.as_slice());
        assert_eq!(cloned.as_slice(), expected.as_slice());
        assert_eq!(captured.spilled(), count > 4);
        assert_eq!(cloned.spilled(), count > 4);
        assert_eq!(original_allocs, usize::from(count > 0));
        assert_eq!(capture_allocs, usize::from(count > 4));
        assert_eq!(clone_allocs, usize::from(count > 4));
        let counts = observer::counts();
        assert_eq!(counts.captures, 1);
        assert_eq!(counts.inline_snapshots, usize::from(count <= 4));
        assert_eq!(counts.spilled_snapshots, usize::from(count > 4));
        assert_eq!(counts.empty_snapshots, usize::from(count == 0));
        assert_eq!(counts.snapshot_entries, count);
        println!("P11 allocation proof: len={count} original={original_allocs} capture={capture_allocs} clone={clone_allocs}");
    }
}
