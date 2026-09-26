//! Showdown's damage-history bookkeeping (WORKPLAN F13): `hurtThisTurn`, `attackedBy`,
//! `timesAttacked`, `moveThisTurnResult` / `moveLastTurnResult` and `newlySwitched` on the
//! active slot ([`SlotHistory`]), `totalFainted` / `faintedThisTurn` / `faintedLastTurn` on the
//! side ([`SideHistory`]). Every change is an `Instruction::SetSlotHistory` /
//! `SetSideHistory`, so it is reversible and part of the state the enumeration merges on.
//! Readers: `moves/handlers.rs` (Assurance, Payback, Avalanche, Stomping Tantrum, Temper
//! Flare, Rage Fist, Last Respects, Metal Burst, Comeuppance).

use super::battle::Battle;
use crate::instruction::Instruction;
use crate::state::{
    DamagedBy, MoveResult, PokemonRef, SideHistory, SideId, SlotHistory, SlotRef, State,
};

impl<const N: usize> Battle<'_, N> {
    pub(crate) fn slot_history(&self, slot: SlotRef) -> SlotHistory {
        self.state.slot(slot).history
    }

    pub(crate) fn set_slot_history(&mut self, slot: SlotRef, new: SlotHistory) {
        let old = self.state.slot(slot).history;
        if old != new {
            self.apply(Instruction::SetSlotHistory {
                target: slot,
                old,
                new,
            });
        }
    }

    pub(crate) fn set_side_history(&mut self, side: SideId, new: SideHistory) {
        let old = self.state.side(side).history;
        if old != new {
            self.apply(Instruction::SetSideHistory { side, old, new });
        }
    }

    /// `spreadDamage`: `if (targetDamage !== 0) target.hurtThisTurn = target.hp` (the HP left
    /// after the damage, possibly 0). Not for `directDamage`.
    pub(crate) fn record_hurt(&mut self, slot: SlotRef) {
        let Some(pokemon) = self.occupant(slot) else {
            return;
        };
        let mut history = self.slot_history(slot);
        history.hurt_this_turn = Some(self.mon(pokemon).hp);
        self.set_slot_history(slot, history);
    }

    /// `hitStepMoveHitLoop` after the hits, for each target of the last hit other than the
    /// user: `gotAttacked(move, damage, source)` (an `attackedBy` entry, this turn), and for
    /// a numeric `damage` (`Hit::Damage`, even 0) `timesAttacked += hits`. A target that
    /// fainted from the hit has left its slot; Showdown's entry on the fainted Pokémon is
    /// never read.
    pub(crate) fn record_attack(
        &mut self,
        target: SlotRef,
        source: SlotRef,
        damage: Option<i32>,
        hits: u8,
    ) {
        let (Some(attacker), Some(_)) = (self.occupant(source), self.occupant(target)) else {
            return;
        };
        let Some(damage) = damage else {
            return;
        };
        // Fields nobody in this battle reads are left unrecorded (F18): the distribution of
        // everything observable is unchanged and positions differing only in them merge.
        let readers = self.history_readers;
        let mut history = self.slot_history(target);
        if source.side != target.side && readers.last_damaged_by {
            history.last_damaged_by = Some(DamagedBy {
                source: attacker,
                slot: source,
                damage: damage.clamp(0, i32::from(i16::MAX)) as i16,
            });
        }
        if damage > 0 {
            history.damaged_by_this_turn |= SlotHistory::attacker_bit(attacker);
        }
        if readers.times_attacked {
            history.times_attacked = history.times_attacked.saturating_add(hits);
        }
        self.set_slot_history(target, history);
    }

    /// `pokemon.moveThisTurnResult = ...` (`runMove`: the BeforeMove verdict; `useMove`:
    /// `undefined` before the move runs).
    pub(crate) fn set_move_result(&mut self, slot: SlotRef, result: MoveResult) {
        if self.occupant(slot).is_none() {
            return;
        }
        let mut history = self.slot_history(slot);
        history.move_this_turn_result = result;
        self.set_slot_history(slot, history);
    }

    /// The end of `useMove`: `if (oldMoveResult === pokemon.moveThisTurnResult)
    /// pokemon.moveThisTurnResult = moveResult` — the move's own result stands unless a move it
    /// called (Sleep Talk) set one meanwhile.
    pub(crate) fn finish_move_result(&mut self, slot: SlotRef, ok: bool) {
        if self.occupant(slot).is_none() {
            return;
        }
        if self.slot_history(slot).move_this_turn_result == MoveResult::Undefined {
            let result = if ok {
                MoveResult::Succeeded
            } else {
                MoveResult::Failed
            };
            self.set_move_result(slot, result);
        }
    }

    /// The end of a successful `boost()`: `if (Object.values(boost).some(x => x > 0))
    /// target.statsRaisedThisTurn = true;` and the same with `< 0` for `statsLoweredThisTurn`,
    /// over the applied table. Recorded only for a battle with a reader (F18).
    pub(crate) fn record_stat_changes(&mut self, slot: SlotRef, boost: &[i8]) {
        let readers = self.history_readers;
        if (!readers.stats_raised && !readers.stats_lowered) || self.occupant(slot).is_none() {
            return;
        }
        let mut history = self.slot_history(slot);
        if readers.stats_raised && boost.iter().any(|&b| b > 0) {
            history.stats_raised_this_turn = true;
        }
        if readers.stats_lowered && boost.iter().any(|&b| b < 0) {
            history.stats_lowered_this_turn = true;
        }
        self.set_slot_history(slot, history);
    }

    /// `pokemon.ateBerry = true` (`eatItem`, Bug Bite / Pluck), for a battle with Belch (F18).
    pub(crate) fn record_ate_berry(&mut self, pokemon: PokemonRef) {
        if !self.history_readers.ate_berry {
            return;
        }
        let mut history = self.state.side(pokemon.side).history;
        history.ate_berry |= 1 << pokemon.party;
        self.set_side_history(pokemon.side, history);
    }

    /// `deductPP`'s `moveSlot.used = true` for move index `index` of the Pokémon in `slot`, for a
    /// battle with Last Resort (F18).
    pub(crate) fn record_move_used(&mut self, slot: SlotRef, index: usize) {
        if !self.history_readers.moves_used || index >= 4 || self.occupant(slot).is_none() {
            return;
        }
        let mut history = self.slot_history(slot);
        history.moves_used |= 1 << index;
        self.set_slot_history(slot, history);
    }

    /// `endTurn`'s `if (this.turn !== 1)` resets for every active Pokémon:
    /// `statsRaisedThisTurn` and `statsLoweredThisTurn` (not when the battle starts: turn 1 still
    /// sees what the leads' switch-in effects changed).
    pub(crate) fn reset_stat_changes(&mut self) {
        for slot in State::<N>::slot_refs() {
            if self.state.slot(slot).party_index.is_none() {
                continue;
            }
            let mut history = self.slot_history(slot);
            history.stats_raised_this_turn = false;
            history.stats_lowered_this_turn = false;
            self.set_slot_history(slot, history);
        }
    }

    /// `faintMessages`: `if (side.totalFainted < 100) side.totalFainted++` and
    /// `side.faintedThisTurn = pokemon`.
    pub(crate) fn record_faint(&mut self, side: SideId) {
        let mut history = self.state.side(side).history;
        if history.total_fainted < 100 {
            history.total_fainted += 1;
        }
        history.fainted_this_turn = true;
        self.set_side_history(side, history);
    }

    /// `endTurn` for every occupied slot: `newlySwitched = false`, `moveLastTurnResult =
    /// moveThisTurnResult`, `moveThisTurnResult = undefined`, `hurtThisTurn = null`, and the
    /// `attackedBy` entries lose `thisTurn` (their only readers need it, so they are dropped);
    /// for each side `faintedLastTurn = faintedThisTurn`, `faintedThisTurn = null`. An
    /// emptied slot (its occupant fainted) keeps the default it was reset to.
    pub(crate) fn end_turn_history(&mut self) {
        // Carry-overs nobody in this battle reads are dropped (F18), as in `record_attack`.
        let readers = self.history_readers;
        for slot in State::<N>::slot_refs() {
            if self.state.slot(slot).party_index.is_none() {
                continue;
            }
            let mut history = self.slot_history(slot);
            history.newly_switched = false;
            history.move_last_turn_result = if readers.move_last_turn_result {
                history.move_this_turn_result
            } else {
                MoveResult::Undefined
            };
            history.move_this_turn_result = MoveResult::Undefined;
            history.hurt_this_turn = None;
            history.last_damaged_by = None;
            history.damaged_by_this_turn = 0;
            self.set_slot_history(slot, history);
        }
        for side in [SideId::One, SideId::Two] {
            let mut history = self.state.side(side).history;
            history.fainted_last_turn = readers.fainted_last_turn && history.fainted_this_turn;
            history.fainted_this_turn = false;
            self.set_side_history(side, history);
        }
    }
}
