//! P12 mechanism-only counts. This module and every call site are absent without the
//! separate observer feature. There is no runtime policy switch or state cache.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub location_queries: usize,
    pub rank_queries: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        location_queries: 0,
        rank_queries: 0,
    }) };
}

pub fn reset() {
    COUNTS.with(|counts| counts.set(Counts::default()));
}

pub fn counts() -> Counts {
    COUNTS.with(Cell::get)
}

pub(super) fn location() {
    COUNTS.with(|counts| {
        let mut next = counts.get();
        next.location_queries += 1;
        counts.set(next);
    });
}

#[cfg(feature = "experiment-compact-volatiles")]
pub(super) fn rank() {
    COUNTS.with(|counts| {
        let mut next = counts.get();
        next.rank_queries += 1;
        counts.set(next);
    });
}
