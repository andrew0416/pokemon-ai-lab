//! P13: batch keys borrow the immutable jobs already needed by the solver.
#[cfg(feature = "experiment-borrowed-child-keys")]
use super::ChildJob;
#[cfg(feature = "experiment-borrowed-child-keys")]
use lab_engine::{hash::BuildKeyHasher, state::State, turn::Suspension};
#[cfg(feature = "experiment-borrowed-child-keys")]
use std::{collections::HashMap, marker::PhantomData};

#[cfg(feature = "experiment-borrowed-child-keys")]
struct Link {
    job: usize,
    next: Option<usize>,
}

/// Indices survive Vec relocation. Neither references nor State copies are stored.
#[cfg(feature = "experiment-borrowed-child-keys")]
pub(super) struct ChildSeen<const N: usize> {
    heads: HashMap<u64, usize, BuildKeyHasher>,
    links: Vec<Link>,
    marker: PhantomData<fn() -> State<N>>,
}

#[cfg(feature = "experiment-borrowed-child-keys")]
impl<const N: usize> ChildSeen<N> {
    pub(super) fn new() -> Self {
        Self {
            heads: HashMap::default(),
            links: Vec::new(),
            marker: PhantomData,
        }
    }

    pub(super) fn find(
        &self,
        hash: u64,
        state: &State<N>,
        suspension: Option<&Suspension>,
        jobs: &[(usize, ChildJob<N>)],
    ) -> Option<usize> {
        let mut link = self.heads.get(&hash).copied();
        while let Some(index) = link {
            let entry = &self.links[index];
            let (slot, job) = &jobs[entry.job];
            if &job.state == state && job.suspension.as_ref() == suspension {
                #[cfg(feature = "experiment-borrowed-child-keys-observer")]
                observer::update(|counts| counts.seen_hits += 1);
                return Some(*slot);
            }
            #[cfg(feature = "experiment-borrowed-child-keys-observer")]
            observer::update(|counts| counts.seen_collisions += 1);
            link = entry.next;
        }
        None
    }

    pub(super) fn insert(&mut self, hash: u64, job: usize) {
        let next = self.heads.insert(hash, self.links.len());
        self.links.push(Link { job, next });
        #[cfg(feature = "experiment-borrowed-child-keys-observer")]
        observer::update(|counts| counts.seen_links += 1);
    }
}

/// This observer can compile with runtime OFF as well as ON. No reference-mode
/// selector or instrumentation exists when the observer feature is absent.
#[cfg(feature = "experiment-borrowed-child-keys-observer")]
pub mod observer {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub key_captures: usize,
        pub job_captures: usize,
        pub borrowed_queries: usize,
        pub seen_hits: usize,
        pub seen_collisions: usize,
        pub seen_links: usize,
    }

    thread_local! {
        static COUNTS: Cell<Counts> = Cell::new(Counts::default());
    }

    pub fn reset() {
        COUNTS.with(|value| value.set(Counts::default()));
    }

    pub fn counts() -> Counts {
        COUNTS.with(Cell::get)
    }

    pub(super) fn update(update: impl FnOnce(&mut Counts)) {
        COUNTS.with(|value| {
            let mut counts = value.get();
            update(&mut counts);
            value.set(counts);
        });
    }
}

#[cfg(feature = "experiment-borrowed-child-keys-observer")]
pub(super) fn observe(update: impl FnOnce(&mut observer::Counts)) {
    observer::update(update);
}
