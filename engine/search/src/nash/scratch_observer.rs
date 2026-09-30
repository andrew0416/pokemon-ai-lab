//! P16 diagnostic work counters. Absent from timing builds, independent of runtime feature.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub solve_calls: usize,
    pub checkpoints: usize,
    pub iterations: usize,
    pub normalization_allocations: usize,
    pub evaluation_scratch_allocations: usize,
    /// One owned Equilibrium, containing two output strategy Vecs.
    pub output_materializations: usize,
}
thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        solve_calls: 0, checkpoints: 0, iterations: 0,
        normalization_allocations: 0, evaluation_scratch_allocations: 0,
        output_materializations: 0,
    }) };
}
pub fn counts() -> Counts { COUNTS.get() }
pub fn reset() { COUNTS.set(Counts::default()); }
pub(super) fn update(f: impl FnOnce(&mut Counts)) {
    let mut c = counts();
    f(&mut c);
    COUNTS.set(c);
}
pub(super) fn checkpoint(reused: bool) {
    update(|c| {
        c.checkpoints += 1;
        if !reused {
            c.normalization_allocations += 2;
            c.evaluation_scratch_allocations += 1;
            c.output_materializations += 1;
        }
    });
}
