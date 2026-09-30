//! P14 work counters only. This module and all calls are absent from timing builds.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub a_calls: usize,
    pub a_passthrough: usize,
    pub a_fallback: usize,
    pub b_calls: usize,
    pub b_passthrough: usize,
    pub b_fallback: usize,
}
thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        a_calls: 0, a_passthrough: 0, a_fallback: 0,
        b_calls: 0, b_passthrough: 0, b_fallback: 0,
    }) };
}
pub fn counts() -> Counts {
    COUNTS.get()
}
pub fn reset() {
    COUNTS.set(Counts::default());
}
pub(super) fn update(f: impl FnOnce(&mut Counts)) {
    let mut counts = counts();
    f(&mut counts);
    COUNTS.set(counts);
}
