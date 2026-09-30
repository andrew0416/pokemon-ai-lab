//! P8d: reuse only the pre-action keys of one restored ordinary stage input.
//!
//! The owner lives inside `enumerate_stages`' `(work, pending)` loop. Every replay reverses
//! the preceding log, clones that same Pending, and restores the same RunStart snapshot.
//! No key survives that input. In particular this is not a State-hash cache, and no selected
//! action is cached: `pick_action` still consumes the original tie draw on every replay.

use super::{battle::Battle, pick_action, queue::Action, Pending, Small};
use crate::state::PokemonRef;

type Key = (u32, i32, i32);
const INLINE: usize = 24;

#[derive(Default)]
pub(super) struct ReplayActionKeys {
    keys: Small<Key, INLINE>,
    queue: Small<Action, INLINE>,
    snapshot: Small<(PokemonRef, i32), 12>,
}

/// Reject before any initial draw/item consumption, switch, or resumed hit can run. A
/// future pre-action mutation through the instruction log also forces the uncached path.
pub(super) fn guard<const N: usize>(b: &mut Battle<'_, N>, pending: &Pending) {
    #[cfg(feature = "experiment-replay-action-keys-observer")]
    observer::update(|c| {
        c.stage_runs += 1;
        c.initial_priority_runs += u64::from(!pending.fractional_drawn);
        c.midturn_switch_runs += u64::from(!pending.switches.is_empty());
        c.multihit_resume_runs += u64::from(pending.in_progress.is_some());
    });
    let eligible = pending.fractional_drawn
        && pending.switches.is_empty()
        && pending.in_progress.is_none()
        && !pending.done
        && !pending.residual_done
        && !pending.queue.is_empty()
        && pending.queue.len() <= INLINE
        && b.log.is_empty()
        && b.hash_delta == 0;
    #[cfg(feature = "experiment-replay-action-keys-observer")]
    let eligible = eligible && observer::reuse_enabled();
    if !eligible {
        b.replay_action_keys = None;
    }
}

pub(super) fn pick<const N: usize>(b: &mut Battle<'_, N>) -> usize {
    // Take the loan away before executing the action: the cache must never be read again
    // after state/queue/speed changes inside this run. The next replay receives a new loan.
    if let Some(cache) = b.replay_action_keys.take() {
        // The loop owns the restored State identity and RunStart. These exact comparisons
        // additionally protect the queue ordering/content and snapshot at the use site.
        let pristine = b.log.is_empty()
            && b.hash_delta == 0
            && b.raw_speed.is_empty()
            && b.active_move.is_none();
        if pristine && !b.queue.is_empty() && b.queue.len() <= INLINE {
            if cache.keys.is_empty() {
                cache.queue.extend_from_slice(&b.queue);
                cache.snapshot.extend_from_slice(&b.speed_snapshot);
                cache
                    .keys
                    .extend(b.queue.iter().map(|action| b.action_key(action)));
                #[cfg(feature = "experiment-replay-action-keys-observer")]
                observer::update(|c| {
                    c.filled_inputs += 1;
                    c.stored_keys += cache.keys.len() as u64;
                });
                return pick_action(b, &cache.keys);
            }
            if cache.queue.as_slice() == b.queue.as_slice()
                && cache.snapshot.as_slice() == b.speed_snapshot.as_slice()
            {
                #[cfg(feature = "experiment-replay-action-keys-observer")]
                observer::update(|c| {
                    c.reused_runs += 1;
                    c.reused_keys += cache.keys.len() as u64;
                });
                return pick_action(b, &cache.keys);
            }
        }
        #[cfg(feature = "experiment-replay-action-keys-observer")]
        observer::update(|c| c.rejected_at_pick += 1);
    }
    // Original stack/heap split, key iteration, comparison and RNG call. Oversized queues
    // are intentionally excluded from the experiment and retain the original Vec path.
    let n = b.queue.len();
    if n <= INLINE {
        let mut keys = [(0u32, 0i32, 0i32); INLINE];
        for (key, action) in keys.iter_mut().zip(&b.queue) {
            *key = b.action_key(action);
        }
        pick_action(b, &keys[..n])
    } else {
        let keys: Vec<Key> = b.queue.iter().map(|action| b.action_key(action)).collect();
        pick_action(b, &keys)
    }
}

/// Optional thread-local counters and an oracle switch for correctness tests. Neither TLS
/// accesses nor counters exist in the timing build. The switch is panic-safe and nestable.
#[cfg(feature = "experiment-replay-action-keys-observer")]
pub mod observer {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub stage_runs: u64,
        pub initial_priority_runs: u64,
        pub midturn_switch_runs: u64,
        pub multihit_resume_runs: u64,
        pub filled_inputs: u64,
        pub stored_keys: u64,
        pub reused_runs: u64,
        pub reused_keys: u64,
        pub rejected_at_pick: u64,
        pub action_key_calls: u64,
    }

    thread_local! {
        static COUNTS: Cell<Counts> = Cell::new(Counts::default());
        static ENABLED: Cell<bool> = const { Cell::new(true) };
    }

    pub fn counts() -> Counts {
        COUNTS.with(Cell::get)
    }

    pub fn reset() {
        COUNTS.with(|counts| counts.set(Counts::default()));
    }

    pub(crate) fn update(change: impl FnOnce(&mut Counts)) {
        COUNTS.with(|counts| {
            let mut next = counts.get();
            change(&mut next);
            counts.set(next);
        });
    }

    pub(super) fn reuse_enabled() -> bool {
        ENABLED.with(Cell::get)
    }

    pub fn without_reuse<T>(run: impl FnOnce() -> T) -> T {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                ENABLED.with(|enabled| enabled.set(self.0));
            }
        }
        let _restore = Restore(ENABLED.with(|enabled| enabled.replace(false)));
        run()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{branch::Chooser, queue::ActionKind};
    use super::*;
    use crate::state::{SideId, SlotRef, State};

    fn action(slot: u8) -> Action {
        Action {
            slot: SlotRef {
                side: SideId::One,
                slot,
            },
            pokemon: PokemonRef {
                side: SideId::One,
                party: 0,
            },
            kind: ActionKind::BeforeTurn,
            order: None,
        }
    }

    #[test]
    fn guard_only_admits_pristine_ordinary_action_inputs() {
        let check = |pending: Pending, dirty: bool| {
            let mut state = State::<2>::default();
            let mut rng = Chooser::new();
            let mut cache = ReplayActionKeys::default();
            let mut b = Battle::new(&mut state, &mut rng);
            if dirty {
                b.apply(crate::instruction::Instruction::SetTurn { old: 0, new: 1 });
            }
            b.replay_action_keys = Some(&mut cache);
            guard(&mut b, &pending);
            b.replay_action_keys.is_some()
        };
        let mut pending = Pending::new(vec![action(0)]);
        assert!(!check(pending.clone(), false), "initial priority draw");
        pending.fractional_drawn = true;
        assert!(check(pending.clone(), false));
        assert!(!check(pending.clone(), true), "pre-action state mutation");
        pending.switches.push((action(0).slot, 1));
        assert!(!check(pending.clone(), false), "midturn switch");
        pending.switches.clear();
        pending.done = true;
        assert!(!check(pending.clone(), false), "finished input");
        pending.done = false;
        pending.residual_done = true;
        assert!(!check(pending.clone(), false), "post-residual input");
        pending.residual_done = false;
        pending.queue = vec![action(0); INLINE + 1];
        assert!(!check(pending.clone(), false), "oversized fallback");
        pending.queue.clear();
        assert!(!check(pending, false), "empty queue");
    }

    #[test]
    fn exact_queue_and_snapshot_mismatch_falls_back() {
        #[cfg(feature = "experiment-replay-action-keys-observer")]
        observer::reset();
        let mut state = State::<2>::default();
        let mut rng = Chooser::new();
        let mut cache = ReplayActionKeys::default();
        let mut b = Battle::new(&mut state, &mut rng);
        b.queue = vec![action(0), action(1)];
        b.replay_action_keys = Some(&mut cache);
        let _ = pick(&mut b);
        drop(b);
        // Reborrow the same input cache deliberately with an altered queue. A wrong reuse
        // would pick queue index 0; the fresh explicit order override must pick index 1.
        let mut b = Battle::new(&mut state, &mut rng);
        b.queue = vec![action(0), action(1)];
        b.queue[1].order = Some(0);
        b.replay_action_keys = Some(&mut cache);
        assert_eq!(pick(&mut b), 1);
        drop(b);
        let mut b = Battle::new(&mut state, &mut rng);
        b.queue = vec![action(0), action(1)];
        b.speed_snapshot.push((b.queue[0].pokemon, 123));
        b.replay_action_keys = Some(&mut cache);
        let _ = pick(&mut b);
        #[cfg(feature = "experiment-replay-action-keys-observer")]
        assert_eq!(observer::counts().rejected_at_pick, 2);
    }

    #[test]
    fn tie_choice_and_probability_sequence_are_identical() {
        fn run(reuse: bool) -> Vec<(usize, u64)> {
            let mut state = State::<2>::default();
            let mut chooser = Chooser::new();
            let mut cache = ReplayActionKeys::default();
            let mut outcomes = Vec::new();
            loop {
                chooser.begin_run();
                let mut b = Battle::new(&mut state, &mut chooser);
                b.queue = vec![action(0), action(1)];
                if reuse {
                    b.replay_action_keys = Some(&mut cache);
                }
                let choice = pick(&mut b);
                // A later branch drives replay, just as an accuracy or damage draw does.
                b.rng.uniform(3);
                outcomes.push((choice, b.rng.probability().to_bits()));
                if !chooser.advance() {
                    break;
                }
            }
            outcomes
        }
        let cached = run(true);
        assert_eq!(cached, run(false));
        assert_eq!(cached.len(), 6);
        assert_eq!(
            cached.iter().map(|o| o.0).collect::<Vec<_>>(),
            [0, 0, 0, 1, 1, 1]
        );
    }
}
