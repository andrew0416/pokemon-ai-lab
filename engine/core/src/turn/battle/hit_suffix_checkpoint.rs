//! Exhaustive mechanical Battle snapshot. No State clone in release.
use super::*;
use crate::state::{LazyTag, PARTY_SIZE};
use crate::turn::lazy::MAX_UNITS;

#[derive(Clone, Debug)]
pub(crate) struct HitCheckpoint<const N: usize> {
    pub(crate) log_len: usize,
    tags: [LazyTag; MAX_UNITS],
    #[cfg(test)]
    state: State<N>,
    #[cfg(test)]
    log: Vec<Instruction>,
    hash_delta: u64,
    first_hit_policy: crate::turn::first_hit::Policy,
    faint_queue: Vec<(PokemonRef, SlotRef, Option<PokemonRef>)>,
    active_move: Option<ActiveMoveRef>,
    queue: Vec<crate::turn::queue::Action>,
    battle_start: bool,
    hit_type_mod: [[Option<i8>; N]; 2],
    hit_crit: [[bool; N]; 2],
    mirror_herb: Vec<(PokemonRef, [i8; BOOST_COUNT])>,
    umbrella_inactive: Vec<PokemonRef>,
    move_self_switch: bool,
    force_switch: Vec<SlotRef>,
    busted: Vec<PokemonRef>,
    history_readers: HistoryReaders,
    raw_speed: Vec<(PokemonRef, i32)>,
    speed_snapshot: Vec<(PokemonRef, i32)>,
    awaiting_run_switch: bool,
    unstarted: Vec<PokemonRef>,
    queue_done: bool,
    absent_user: Option<SlotRef>,
    absent_occupant: Option<Box<crate::state::Slot>>,
    external_move: bool,
    called_move: Option<crate::turn::moves::ActiveMove>,
    called_suspension: Option<crate::turn::moves::MoveProgress>,
    active_target: Option<(SlotRef, bool)>,
    suppression: bool,
}
impl<const N: usize> Battle<'_, N> {
    pub(crate) fn hit_checkpoint(&self) -> HitCheckpoint<N> {
        let Self {
            state,
            log,
            rng: _,
            hash_delta,
            first_hit_policy,
            faint_queue,
            active_move,
            queue,
            battle_start,
            hit_type_mod,
            hit_crit,
            mirror_herb,
            umbrella_inactive,
            move_self_switch,
            force_switch,
            busted,
            history_readers,
            raw_speed,
            speed_snapshot,
            awaiting_run_switch,
            unstarted,
            queue_done,
            absent_user,
            absent_occupant,
            external_move,
            called_move,
            called_suspension,
            active_target,
            suppression,
            #[cfg(feature = "experiment-replay-action-keys")]
            replay_action_keys,
            hit_suffix_frame: _,
            hit_suffix_retained: _,
            hit_suffix_allowed: _,
            hit_suffix_pending: _,
            hit_suffix_seed: _,
        } = self;
        #[cfg(feature = "experiment-replay-action-keys")]
        assert!(
            replay_action_keys.is_none(),
            "factored capture cannot retain a borrowed cache"
        );
        HitCheckpoint {
            log_len: log.len(),
            tags: std::array::from_fn(|u| state.sides[u / PARTY_SIZE].party[u % PARTY_SIZE].lazy),
            #[cfg(test)]
            state: (**state).clone(),
            #[cfg(test)]
            log: log.clone(),
            hash_delta: hash_delta.clone(),
            first_hit_policy: first_hit_policy.clone(),
            faint_queue: faint_queue.clone(),
            active_move: active_move.clone(),
            queue: queue.clone(),
            battle_start: battle_start.clone(),
            hit_type_mod: hit_type_mod.clone(),
            hit_crit: hit_crit.clone(),
            mirror_herb: mirror_herb.clone(),
            umbrella_inactive: umbrella_inactive.clone(),
            move_self_switch: move_self_switch.clone(),
            force_switch: force_switch.clone(),
            busted: busted.clone(),
            history_readers: history_readers.clone(),
            raw_speed: raw_speed.clone(),
            speed_snapshot: speed_snapshot.clone(),
            awaiting_run_switch: awaiting_run_switch.clone(),
            unstarted: unstarted.clone(),
            queue_done: queue_done.clone(),
            absent_user: absent_user.clone(),
            absent_occupant: absent_occupant.clone(),
            external_move: external_move.clone(),
            called_move: called_move.clone(),
            called_suspension: called_suspension.clone(),
            active_target: active_target.clone(),
            suppression: suppression.clone(),
        }
    }
    pub(crate) fn rollback_hit_suffix(&mut self, saved: &HitCheckpoint<N>) {
        assert!(self.log.len() >= saved.log_len, "hit prefix log was lost");
        self.state.reverse(&self.log[saved.log_len..]);
        self.log.truncate(saved.log_len);
        // P1e clears live tags outside the instruction log. Restore all boundary tags.
        for (u, tag) in saved.tags.iter().enumerate() {
            self.state.sides[u / PARTY_SIZE].party[u % PARTY_SIZE].lazy = *tag;
        }
        let Self {
            state: _,
            log: _,
            rng: _,
            hash_delta,
            first_hit_policy,
            faint_queue,
            active_move,
            queue,
            battle_start,
            hit_type_mod,
            hit_crit,
            mirror_herb,
            umbrella_inactive,
            move_self_switch,
            force_switch,
            busted,
            history_readers,
            raw_speed,
            speed_snapshot,
            awaiting_run_switch,
            unstarted,
            queue_done,
            absent_user,
            absent_occupant,
            external_move,
            called_move,
            called_suspension,
            active_target,
            suppression,
            #[cfg(feature = "experiment-replay-action-keys")]
            replay_action_keys,
            hit_suffix_frame,
            hit_suffix_retained,
            hit_suffix_allowed,
            hit_suffix_pending,
            hit_suffix_seed,
        } = self;
        #[cfg(feature = "experiment-replay-action-keys")]
        {
            *replay_action_keys = None;
        }
        hash_delta.clone_from(&saved.hash_delta);
        first_hit_policy.clone_from(&saved.first_hit_policy);
        faint_queue.clone_from(&saved.faint_queue);
        active_move.clone_from(&saved.active_move);
        queue.clone_from(&saved.queue);
        battle_start.clone_from(&saved.battle_start);
        hit_type_mod.clone_from(&saved.hit_type_mod);
        hit_crit.clone_from(&saved.hit_crit);
        mirror_herb.clone_from(&saved.mirror_herb);
        umbrella_inactive.clone_from(&saved.umbrella_inactive);
        move_self_switch.clone_from(&saved.move_self_switch);
        force_switch.clone_from(&saved.force_switch);
        busted.clone_from(&saved.busted);
        history_readers.clone_from(&saved.history_readers);
        raw_speed.clone_from(&saved.raw_speed);
        speed_snapshot.clone_from(&saved.speed_snapshot);
        awaiting_run_switch.clone_from(&saved.awaiting_run_switch);
        unstarted.clone_from(&saved.unstarted);
        queue_done.clone_from(&saved.queue_done);
        absent_user.clone_from(&saved.absent_user);
        absent_occupant.clone_from(&saved.absent_occupant);
        external_move.clone_from(&saved.external_move);
        called_move.clone_from(&saved.called_move);
        called_suspension.clone_from(&saved.called_suspension);
        active_target.clone_from(&saved.active_target);
        suppression.clone_from(&saved.suppression);
        *hit_suffix_frame = None;
        *hit_suffix_retained = true;
        *hit_suffix_allowed = false;
        *hit_suffix_pending = None;
        *hit_suffix_seed = None;
        #[cfg(test)]
        {
            assert_eq!(*self.state, saved.state, "suffix rollback complete State");
            assert_eq!(
                format!("{:?}", self.state),
                format!("{:?}", saved.state),
                "lazy tags too"
            );
            assert_eq!(
                self.log, saved.log,
                "prefix instructions remain exactly once"
            );
            assert_eq!(self.hash_delta, saved.hash_delta);
        }
    }
}
