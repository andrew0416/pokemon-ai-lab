//! The action queue as moves see it (WORKPLAN F8): Showdown `battle.queue` during a turn.
//!
//! The stage loop pops the action it runs, so what a handler sees is what Showdown's
//! `queue.list` holds while that action runs: the actions not yet run. Reads: `willAct`
//! (Protect), `willMove` (Sucker Punch, Thunderclap, Upper Hand, Quash, After You). Writes:
//! `prioritizeAction` (After You: order 3, next to act) and Quash's `action.order = 201`
//! (after every move, whatever its priority). The order override lives on the action and is
//! part of the enumeration's merge key.

use crate::dex::{MoveCategory, MoveId};
use crate::state::{PokemonRef, SlotRef};

use super::battle::Battle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Action {
    pub slot: SlotRef,
    pub pokemon: PokemonRef,
    pub kind: ActionKind,
    /// Showdown `action.order` when a move changed it (After You 3, Quash 201).
    pub order: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ActionKind {
    Move {
        index: u8,
        target: i8,
        /// Showdown `action.fractionalPriority`, fixed when the action is queued.
        fractional_tenths: i8,
    },
    Switch {
        party_index: u8,
    },
    /// Showdown `megaEvo`, queued before the Pokémon's move.
    Mega,
}

/// Showdown `queue.willAct()`: a move or switch is still to come.
impl<const N: usize> Battle<'_, N> {
    pub fn will_act(&self) -> bool {
        self.queue
            .iter()
            .any(|a| matches!(a.kind, ActionKind::Move { .. } | ActionKind::Switch { .. }))
    }

    /// Showdown `queue.willMove(pokemon)` for the Pokémon at `slot`: the index of its pending
    /// move action, if it is alive and has one.
    pub fn will_move(&self, slot: SlotRef) -> Option<usize> {
        let pokemon = self.alive(slot)?;
        self.queue
            .iter()
            .position(|a| a.pokemon == pokemon && matches!(a.kind, ActionKind::Move { .. }))
    }

    /// The move the Pokémon at `slot` still has queued, with its category and its priority in
    /// tenths as Showdown's `action.move.priority` (after ModifyPriority and the fractional
    /// part) would be.
    pub fn queued_move(&self, slot: SlotRef) -> Option<(MoveId, MoveCategory, i32)> {
        let index = self.will_move(slot)?;
        let action = self.queue[index];
        let ActionKind::Move {
            index: move_index,
            fractional_tenths,
            ..
        } = action.kind
        else {
            return None;
        };
        if move_index == super::lock::RECHARGE_INDEX {
            // `recharge` is a status pseudo-move with priority 0.
            return Some((crate::dex::MoveId::NONE, MoveCategory::Status, 0));
        }
        let id = super::lock::action_move_id(self.mon(action.pokemon), move_index);
        let priority = self.move_priority(slot, id) * 10 + i32::from(fractional_tenths);
        Some((id, id.data().category, priority))
    }

    /// Showdown `queue.prioritizeAction`: the action runs next (order 3).
    pub fn prioritize_action(&mut self, index: usize) {
        self.queue[index].order = Some(3);
    }

    /// Quash's `action.order = 201`: after every ordinary move.
    pub fn quash_action(&mut self, index: usize) {
        self.queue[index].order = Some(201);
    }
}
