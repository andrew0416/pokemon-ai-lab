//! Fixed-size observation of persistent sequence cache shape; diagnostic only.
use std::cell::Cell;
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub finishes: usize,
    pub rank_slots_processed: usize,
    pub max_buckets: usize,
    pub max_empty_buckets: usize,
    pub max_leaf_slots: usize,
    pub max_entries: usize,
    pub bucket_capacity: usize,
    pub leaf_capacity: usize,
}
thread_local! {static DATA:Cell<Stats>=Cell::new(Stats::default());}
pub fn reset() {
    DATA.set(Stats::default());
}
pub fn get() -> Stats {
    DATA.get()
}
pub(crate) fn record(
    buckets: usize,
    entries: usize,
    leaves: usize,
    bucket_capacity: usize,
    leaf_capacity: usize,
) {
    let mut s = DATA.get();
    s.finishes += 1;
    s.rank_slots_processed += leaves;
    s.max_buckets = s.max_buckets.max(buckets);
    s.max_empty_buckets = s.max_empty_buckets.max(buckets - entries);
    s.max_leaf_slots = s.max_leaf_slots.max(leaves);
    s.max_entries = s.max_entries.max(entries);
    s.bucket_capacity = s.bucket_capacity.max(bucket_capacity);
    s.leaf_capacity = s.leaf_capacity.max(leaf_capacity);
    DATA.set(s);
}
