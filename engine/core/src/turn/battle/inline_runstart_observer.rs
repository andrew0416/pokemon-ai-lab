//! P11 observer and isolated allocation-test access. This module is absent from timing
//! builds. It never selects a different production algorithm or changes capture timing.

use std::cell::Cell;

use super::{Battle, HistoryReaders, RunBuffers, RunStart};
use crate::state::{PokemonRef, State};
use crate::turn::{branch::Chooser, Small};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub captures: usize,
    pub inline_snapshots: usize,
    pub spilled_snapshots: usize,
    pub empty_snapshots: usize,
    pub snapshot_entries: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = Cell::new(Counts::default());
}

pub fn reset() {
    COUNTS.with(|counts| counts.set(Counts::default()));
}

pub fn counts() -> Counts {
    COUNTS.with(Cell::get)
}

pub(super) fn capture(source: &[(PokemonRef, i32)]) -> Small<(PokemonRef, i32), 4> {
    let snapshot = Small::from_slice(source);
    COUNTS.with(|counts| {
        let mut next = counts.get();
        next.captures += 1;
        next.inline_snapshots += usize::from(!snapshot.spilled());
        next.spilled_snapshots += usize::from(snapshot.spilled());
        next.empty_snapshots += usize::from(snapshot.is_empty());
        next.snapshot_entries += snapshot.len();
        counts.set(next);
    });
    snapshot
}

/// Keeps all setup and its mutable Vec outside an allocation measurement. `capture` calls
/// the actual Battle::run_start used by ordinary and factored enumeration.
pub struct CaptureProbe {
    state: State<1>,
    chooser: Chooser,
    buffers: RunBuffers,
}

impl CaptureProbe {
    pub fn new(snapshot: Vec<(PokemonRef, i32)>) -> Self {
        Self {
            state: State::default(),
            chooser: Chooser::new(),
            buffers: RunBuffers {
                speed_snapshot: snapshot,
                ..RunBuffers::default()
            },
        }
    }

    pub fn capture(&mut self) -> CapturedSnapshot {
        let battle = Battle::with(
            &mut self.state,
            &mut self.chooser,
            HistoryReaders::default(),
            false,
            std::mem::take(&mut self.buffers),
        );
        let start = battle.run_start();
        self.buffers = battle.into_buffers();
        CapturedSnapshot(start)
    }

    /// The precise old field operation (`self.speed_snapshot.clone()`), measured separately
    /// by the allocation test. This never participates in turn enumeration.
    pub fn reference_snapshot_clone(&self) -> Vec<(PokemonRef, i32)> {
        self.buffers.speed_snapshot.clone()
    }
}

#[derive(Clone)]
pub struct CapturedSnapshot(RunStart);

impl CapturedSnapshot {
    pub fn as_slice(&self) -> &[(PokemonRef, i32)] {
        &self.0.speed_snapshot
    }

    pub fn spilled(&self) -> bool {
        self.0.speed_snapshot.spilled()
    }
}
