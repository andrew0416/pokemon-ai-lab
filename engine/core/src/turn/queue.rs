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
        /// Showdown `action.moveid`, fixed when the action is queued: the chosen move, the
        /// locked move of a locked Pokémon (`chooseMove` pushes `moveid: lockedMoveID`, which may
        /// be a move it does not know: one Copycat called), Struggle, or `MoveId::NONE` for the
        /// `recharge` pseudo-move.
        id: MoveId,
        target: i8,
        /// Showdown `action.fractionalPriority`, fixed when the action is queued.
        fractional_tenths: i8,
        /// Showdown `action.sourceEffect` when it is a Round that moved this action up (Round's
        /// `onTry`: `queue.prioritizeAction(action, move)`): the move then has
        /// `sourceEffect === 'round'` (double power) and that Round's `ignoreAbility` (the
        /// bool). `None` otherwise.
        round_source: Option<bool>,
    },
    Switch {
        party_index: u8,
    },
    /// Showdown `megaEvo`, queued before the Pokémon's move.
    Mega,
    /// Showdown's one `beforeTurn` action per turn (order 4, before every switch and move):
    /// it does nothing itself, but its `runAction` tail runs `eachEvent('Update')`, so a
    /// berry condition that already holds at the start of the turn (a patched position) acts
    /// before the first move. Attached to an arbitrary active Pokémon; neither `willAct` nor
    /// `willMove` counts it.
    BeforeTurn,
    /// Showdown `beforeTurnMove` (order 5, before every switch and move): the chosen move's
    /// `beforeTurnCallback` (Counter, Mirror Coat), queued with the move action. Neither
    /// `willAct` nor `willMove` counts it.
    BeforeTurnMove {
        id: MoveId,
    },
    /// Showdown `priorityChargeMove` (order 107: after switches and Mega Evolution, before the
    /// moves): the chosen move's `priorityChargeCallback` (Focus Punch, Beak Blast, Shell Trap).
    /// Neither `willAct` nor `willMove` counts it.
    PriorityCharge {
        id: MoveId,
    },
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
            id,
            fractional_tenths,
            ..
        } = action.kind
        else {
            return None;
        };
        if id.is_none() {
            // `recharge` is a status pseudo-move with priority 0.
            return Some((crate::dex::MoveId::NONE, MoveCategory::Status, 0));
        }
        let priority = self.move_priority(slot, id) * 10 + i32::from(fractional_tenths);
        Some((id, id.data().category, priority))
    }

    /// Showdown `queue.prioritizeAction`: the action runs next (order 3).
    pub fn prioritize_action(&mut self, index: usize) {
        self.queue[index].order = Some(3);
    }

    /// Round's `onTry`: among the queued move actions choosing Round (Showdown walks
    /// `queue.list`, sorted by order, priority and Speed with ties at random, and takes the first),
    /// the first is prioritized (order 3) with the Round in progress as its source effect
    /// (`ignore_ability`: that Round's `ignoreAbility`). Ties for first are drawn uniformly.
    pub fn prioritize_round(&mut self, ignore_ability: bool) {
        let rounds: Vec<usize> = (0..self.queue.len())
            .filter(|&i| {
                let action = self.queue[i];
                matches!(action.kind, ActionKind::Move { id, .. } if id == crate::dex::moves::ROUND)
            })
            .collect();
        if rounds.is_empty() {
            return;
        }
        let keys: Vec<(u32, i32, i32)> = rounds
            .iter()
            .map(|&i| self.action_key(&self.queue[i]))
            .collect();
        let best = keys
            .iter()
            .copied()
            .min_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)).then(y.2.cmp(&x.2)))
            .expect("non-empty");
        let tied: Vec<usize> = (0..rounds.len()).filter(|&k| keys[k] == best).collect();
        let pick = if tied.len() == 1 {
            tied[0]
        } else {
            tied[self.rng.uniform(tied.len())]
        };
        let index = rounds[pick];
        self.queue[index].order = Some(3);
        if let ActionKind::Move { round_source, .. } = &mut self.queue[index].kind {
            *round_source = Some(ignore_ability);
        }
    }

    /// Quash's `action.order = 201`: after every ordinary move.
    pub fn quash_action(&mut self, index: usize) {
        self.queue[index].order = Some(201);
    }
}
