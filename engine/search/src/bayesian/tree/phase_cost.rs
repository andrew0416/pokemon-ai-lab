//! Opt-in diagnostic wall-clock spans, absent from uninstrumented builds.
//! Only the calling thread is recorded. A transition batch includes scheduling
//! and waiting; worker CPU times are deliberately not added to elapsed time.
use std::{cell::RefCell, marker::PhantomData, rc::Rc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Phase {
    Compute,
    Admission,
    Transitions,
    Compile,
    Solver,
    Kernel,
    Cfr,
    Certificate,
    CompressedFilter,
    Scores,
    Selection,
}
pub const NAMES: [&str; 11] = [
    "compute_other",
    "admission_other",
    "transitions",
    "compile",
    "solver_setup_cleanup",
    "kernel",
    "cfr",
    "certificate",
    "compressed_filter",
    "scores",
    "selection",
];
#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    pub calls: u64,
    pub inclusive_ns: u128,
    pub exclusive_ns: u128,
}
#[derive(Debug)]
pub struct Report {
    pub entries: [Entry; NAMES.len()],
}
impl Report {
    pub fn total_ns(&self) -> u128 {
        self.entries[Phase::Compute as usize].inclusive_ns
    }
    pub fn balanced(&self) -> bool {
        self.entries.iter().map(|x| x.exclusive_ns).sum::<u128>() == self.total_ns()
    }
}
struct Frame {
    phase: Phase,
    start: Instant,
    children: u128,
}
struct Ledger {
    entries: [Entry; NAMES.len()],
    stack: Vec<Frame>,
}
impl Ledger {
    fn close(&mut self, phase: Phase, elapsed: u128, children: u128) {
        let row = &mut self.entries[phase as usize];
        row.calls += 1;
        row.inclusive_ns += elapsed;
        row.exclusive_ns += elapsed
            .checked_sub(children)
            .expect("nested phase accounting");
        if let Some(parent) = self.stack.last_mut() {
            parent.children += elapsed;
        }
    }
}
thread_local! { static ACTIVE: RefCell<Option<Ledger>> = const { RefCell::new(None) }; }

/// Thread-bound RAII guard; errors and unwinding close nested spans as usual.
pub struct Span {
    active: bool,
    phase: Phase,
    _thread: PhantomData<Rc<()>>,
}
impl Span {
    pub fn new(phase: Phase) -> Self {
        let active = ACTIVE.with_borrow_mut(|state| {
            if let Some(l) = state {
                l.stack.push(Frame {
                    phase,
                    start: Instant::now(),
                    children: 0,
                });
                true
            } else {
                false
            }
        });
        Self {
            active,
            phase,
            _thread: PhantomData,
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if self.active {
            ACTIVE.with_borrow_mut(|state| {
                let l = state.as_mut().expect("phase session outlived by span");
                let frame = l.stack.pop().expect("phase stack underflow");
                assert_eq!(frame.phase, self.phase, "phase spans must close in order");
                l.close(
                    frame.phase,
                    frame.start.elapsed().as_nanos(),
                    frame.children,
                );
            });
        }
    }
}
struct Session;
impl Drop for Session {
    fn drop(&mut self) {
        ACTIVE.with_borrow_mut(|s| *s = None);
    }
}
pub fn record<T>(f: impl FnOnce() -> T) -> (T, Report) {
    ACTIVE.with_borrow_mut(|s| {
        assert!(s.is_none(), "nested recording session");
        *s = Some(Ledger {
            entries: [Entry::default(); NAMES.len()],
            stack: Vec::with_capacity(16),
        });
    });
    let _session = Session;
    let root = Span::new(Phase::Compute);
    let value = f();
    drop(root);
    let report = ACTIVE.with_borrow_mut(|s| {
        let l = s.take().unwrap();
        assert!(l.stack.is_empty());
        Report { entries: l.entries }
    });
    assert!(report.balanced());
    (value, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_wall_costs_do_not_double_count_and_workers_are_isolated() {
        let (value, r) = record(|| {
            let _outer = Span::new(Phase::Admission);
            {
                let _inner = Span::new(Phase::Transitions);
                std::thread::spawn(|| {
                    let _s = Span::new(Phase::Transitions);
                })
                .join()
                .unwrap();
            }
            42
        });
        assert_eq!(value, 42);
        assert!(r.balanced());
        assert_eq!(r.entries[Phase::Transitions as usize].calls, 1);
        assert_eq!(r.entries[Phase::Admission as usize].calls, 1);
        assert!(
            r.entries[Phase::Admission as usize].inclusive_ns
                >= r.entries[Phase::Transitions as usize].inclusive_ns
        );
    }
    #[test]
    fn errors_and_panics_restore_the_next_session() {
        let (v, r) = record(|| -> Result<(), ()> {
            let _s = Span::new(Phase::Compile);
            Err(())
        });
        assert!(v.is_err());
        assert!(r.balanced());
        let failed = std::panic::catch_unwind(|| {
            record(|| {
                let _s = Span::new(Phase::Cfr);
                panic!("test");
            })
        });
        assert!(failed.is_err());
        assert!(record(|| ()).1.balanced());
    }
}
