//! P7h: retain a last-target damage-roll prefix inside one exact Full group.
//! This is not a new stage/key and never materializes HP or caches get_damage effects.
use super::{
    battle::{Battle, HitCheckpoint},
    branch::ChooserCheckpoint,
    lazy, moves, Pending,
};

pub(super) struct Frame<const N: usize> {
    pub chooser: ChooserCheckpoint,
    pub battle: HitCheckpoint<N>,
    pub pending: Pending,
    pub damage: moves::DamageSuffix,
}

pub(super) fn capture<const N: usize>(b: &mut Battle<'_, N>, damage: moves::DamageSuffix) {
    assert!(b.hit_suffix_allowed && b.hit_suffix_seed.is_some());
    assert!(
        !lazy::request_pending(),
        "cannot retain an invalid lazy run"
    );
    assert!(b.hit_suffix_frame.is_none());
    let pending = b
        .hit_suffix_pending
        .as_ref()
        .expect("concrete stage opted in")
        .clone();
    let frame = Frame {
        chooser: b.rng.checkpoint(),
        battle: b.hit_checkpoint(),
        pending,
        damage,
    };
    b.hit_suffix_frame = Some(Box::new(frame));
    b.hit_suffix_allowed = false;
    captured();
}

#[cfg(test)]
mod observer {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Counts {
        pub captures: usize,
        pub resumes: usize,
        pub invalidations: usize,
        pub lazy_discards: usize,
        pub discarded_reached: usize,
        pub error_rollbacks: usize,
        pub damage_entries: usize,
        pub saved_prefix_instructions: usize,
    }
    thread_local! {
        pub(super) static COUNTS: Cell<Counts> = Cell::new(Counts::default());
        pub(super) static DISABLED: Cell<bool> = const { Cell::new(false) };
    }
    pub(super) fn change(f: impl FnOnce(&mut Counts)) {
        COUNTS.with(|c| {
            let mut n = c.get();
            f(&mut n);
            c.set(n);
        });
    }
    pub(crate) struct TestDisableGuard(bool);
    impl TestDisableGuard {
        pub(crate) fn new(disabled: bool) -> Self {
            Self(DISABLED.with(|c| c.replace(disabled)))
        }
    }
    impl Drop for TestDisableGuard {
        fn drop(&mut self) {
            DISABLED.with(|c| c.set(self.0));
        }
    }
    pub(crate) fn test_observer_reset() {
        COUNTS.with(|c| c.set(Counts::default()));
    }
    pub(crate) fn test_observer_snapshot() -> Counts {
        COUNTS.with(Cell::get)
    }
}
#[cfg(test)]
pub(super) use observer::{test_observer_reset, test_observer_snapshot, Counts, TestDisableGuard};
#[inline]
pub(super) fn enabled() -> bool {
    #[cfg(test)]
    {
        return !observer::DISABLED.with(std::cell::Cell::get);
    }
    #[cfg(not(test))]
    {
        true
    }
}
#[inline]
fn captured() {
    #[cfg(test)]
    observer::change(|c| c.captures += 1);
}
#[inline]
pub(super) fn resumed(prefix: usize) {
    #[cfg(test)]
    observer::change(|c| {
        c.resumes += 1;
        c.saved_prefix_instructions += prefix;
    });
    #[cfg(not(test))]
    let _ = prefix;
}
#[inline]
pub(super) fn invalidated() {
    #[cfg(test)]
    observer::change(|c| c.invalidations += 1);
}
#[inline]
pub(super) fn lazy_discarded(reached: usize) {
    #[cfg(test)]
    observer::change(|c| {
        c.lazy_discards += 1;
        c.discarded_reached += reached;
    });
    #[cfg(not(test))]
    let _ = reached;
}
#[inline]
pub(super) fn error_rollback() {
    #[cfg(test)]
    observer::change(|c| c.error_rollbacks += 1);
}
#[inline]
pub(super) fn damage_entry() {
    #[cfg(test)]
    observer::change(|c| c.damage_entries += 1);
}
