//! Diagnostic allocator only. Requested Rust heap bytes, not malloc usable size.
//! Live accounting is always on in this build, including allocations before a sample,
//! so frees crossing the sample boundary cannot underflow a reset counter.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static ALLOC: AtomicU64 = AtomicU64::new(0);
static FREE: AtomicU64 = AtomicU64::new(0);
static REALLOC: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
pub struct Tracking;
#[global_allocator]
static GLOBAL: Tracking = Tracking;
fn add(n: usize) {
    let live = LIVE.fetch_add(n as u64, SeqCst) + n as u64;
    PEAK.fetch_max(live, SeqCst);
    BYTES.fetch_add(n as u64, SeqCst);
}
// Delegation preserves System's allocation, alignment and failure contracts.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            ALLOC.fetch_add(1, SeqCst);
            add(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(l);
        if !p.is_null() {
            ALLOC.fetch_add(1, SeqCst);
            add(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        LIVE.fetch_sub(l.size() as u64, SeqCst);
        FREE.fetch_add(1, SeqCst);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = System.realloc(p, l, n);
        if !q.is_null() {
            REALLOC.fetch_add(1, SeqCst);
            if n >= l.size() {
                add(n - l.size());
            } else {
                LIVE.fetch_sub((l.size() - n) as u64, SeqCst);
            }
        }
        q
    }
}
#[derive(Clone, Copy)]
pub struct Sample {
    pub live: u64,
    pub peak: u64,
    pub allocations: u64,
    pub frees: u64,
    pub reallocations: u64,
    pub allocated_bytes: u64,
}
pub fn sample() -> Sample {
    Sample {
        live: LIVE.load(SeqCst),
        peak: PEAK.load(SeqCst),
        allocations: ALLOC.load(SeqCst),
        frees: FREE.load(SeqCst),
        reallocations: REALLOC.load(SeqCst),
        allocated_bytes: BYTES.load(SeqCst),
    }
}
pub fn begin() -> Sample {
    let s = sample();
    PEAK.store(s.live, SeqCst);
    s
}
