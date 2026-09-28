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
    /// after the damage, possibly 0). Not for `directDamage`. Recorded only in a battle with a
    /// reader (F18, P1b): otherwise positions that differ only in it merge, and a lazy HP is not
    /// read.
    pub(crate) fn record_hurt(&mut self, slot: SlotRef) {
        if !self.history_readers.hurt_this_turn {
            return;
        }
        let Some(pokemon) = self.occupant(slot) else {
            return;
        };
        let mut history = self.slot_history(slot);
        history.hurt_this_turn = Some(self.mon(pokemon).hp_value());
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

    /// `pokemon.usedItemThisTurn = true` (`useItem`, `eatItem`, Fling's condition) for the
    /// Pokémon in `slot`, for a battle with Pickup (F18).
    pub(crate) fn record_used_item(&mut self, slot: SlotRef) {
        if !self.history_readers.used_item || self.occupant(slot).is_none() {
            return;
        }
        let mut history = self.slot_history(slot);
        history.used_item_this_turn = true;
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
    /// `statsRaisedThisTurn`, `statsLoweredThisTurn` and `usedItemThisTurn` (not when the battle
    /// starts: turn 1 still sees what the leads' switch-in effects changed or used).
    pub(crate) fn reset_stat_changes(&mut self) {
        for slot in State::<N>::slot_refs() {
            if self.state.slot(slot).party_index.is_none() {
                continue;
            }
            let mut history = self.slot_history(slot);
            history.stats_raised_this_turn = false;
            history.stats_lowered_this_turn = false;
            history.used_item_this_turn = false;
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

    // ---- side.pokemon order (R9a) ---------------------------------------------------------------

    /// Showdown `switchIn`'s `side.pokemon[pokemon.position] = pokemon; side.pokemon[oldActive
    /// .position] = oldActive`: `incoming` took `outgoing`'s position (an occupant, or a fainted
    /// Pokémon holding it) and `outgoing` took `incoming`'s. Ally Switch (`swapPosition`)
    /// exchanges two positions the same way. Recorded only while a Beat Up can be used
    /// ([`super::battle::HistoryReaders::party_order`]).
    pub(crate) fn swap_party_order(&mut self, side: SideId, outgoing: u8, incoming: u8) {
        if !self.history_readers.party_order || outgoing == incoming {
            return;
        }
        let old = self.state.side(side).party_order;
        let (Some(a), Some(b)) = (
            old.iter().position(|&p| p == outgoing),
            old.iter().position(|&p| p == incoming),
        ) else {
            return;
        };
        let mut new = old;
        new.swap(a, b);
        self.apply(Instruction::SetPartyOrder { side, old, new });
    }

    // ---- abilityState.effectOrder (R4) --------------------------------------------------------

    /// The occupied slots in the order their occupants' ability states started (Showdown
    /// `abilityState.effectOrder`, low to high): by `(Slot::ability_order, side, slot)`.
    pub(crate) fn ability_state_order(&self) -> Vec<SlotRef> {
        let mut order: Vec<SlotRef> = State::<N>::slot_refs()
            .filter(|&s| self.state.slot(s).party_index.is_some())
            .collect();
        order.sort_by_key(|&s| Self::ability_order_key(self.state.slot(s).ability_order, s));
        order
    }

    /// The sort key of a slot's ability state start ([`Battle::ability_state_order`]).
    pub(crate) fn ability_order_key(ability_order: u8, slot: SlotRef) -> (u8, usize, u8) {
        (ability_order, slot.side.index(), slot.slot)
    }

    /// Writes `order` (occupied slots, earliest ability state first) as the smallest
    /// `Slot::ability_order` values that give it: a slot keeps the previous one's value when it
    /// comes after it in `(side, slot)` order, else takes the next value. The start's order
    /// (p1a, p1b, p2a, p2b) is all 0, and equal orders get equal values, so positions merge.
    pub(crate) fn set_ability_state_order(&mut self, order: &[SlotRef]) {
        let mut value = 0u8;
        let mut previous: Option<SlotRef> = None;
        for &slot in order {
            if previous.is_some_and(|p| (slot.side.index(), slot.slot) < (p.side.index(), p.slot)) {
                value += 1;
            }
            let old = self.state.slot(slot).ability_order;
            if old != value {
                self.apply(Instruction::SetAbilityOrder {
                    target: slot,
                    old,
                    new: value,
                });
            }
            previous = Some(slot);
        }
    }

    /// Showdown `abilityState = initEffectState({id, target})` for an active Pokémon: its ability
    /// state now started after every other active one's (`battle.effectOrder++`). `switchIn`,
    /// `setAbility` (also from a permanent forme change and Transform, and when the ability stays
    /// the same) and Skill Swap. Recorded only while a redirection tie is possible
    /// ([`super::battle::HistoryReaders::ability_order`]).
    pub(crate) fn restart_ability_state(&mut self, slot: SlotRef) {
        if !self.history_readers.ability_order {
            return;
        }
        let mut order = self.ability_state_order();
        order.retain(|&s| s != slot);
        if self.state.slot(slot).party_index.is_some() {
            order.push(slot);
        }
        self.set_ability_state_order(&order);
    }

    /// Ally Switch exchanged the slots `a` and `b` (their `ability_order` values with them):
    /// the Pokémon keep their ability states, so the order `before` the exchange holds with the
    /// two positions swapped.
    pub(crate) fn swap_ability_state_order(&mut self, before: &[SlotRef], a: SlotRef, b: SlotRef) {
        if !self.history_readers.ability_order {
            return;
        }
        let order: Vec<SlotRef> = before
            .iter()
            .map(|&s| match s {
                s if s == a => b,
                s if s == b => a,
                s => s,
            })
            .filter(|&s| self.state.slot(s).party_index.is_some())
            .collect();
        self.set_ability_state_order(&order);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::SpeciesId;

    const P1A: SlotRef = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    const P1B: SlotRef = SlotRef {
        side: SideId::One,
        slot: 1,
    };
    const P2A: SlotRef = SlotRef {
        side: SideId::Two,
        slot: 0,
    };
    const P2B: SlotRef = SlotRef {
        side: SideId::Two,
        slot: 1,
    };

    fn leads() -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, p) in state.side_mut(side).party.iter_mut().enumerate() {
                p.species = SpeciesId(i as u16 + 1);
                p.max_hp = 100;
                p.hp = 100;
            }
            for s in 0..2 {
                state.side_mut(side).slots[s].party_index = Some(s as u8);
            }
        }
        state
    }

    /// Opus OO R4: `Slot::ability_order` keeps the order the ability states started in
    /// (`abilityState.effectOrder`) through restarts, an Ally Switch and an emptied slot, with the
    /// smallest values (the battle start's order is all 0), and reverses.
    #[test]
    fn ability_state_order_follows_restarts_and_ally_switch() {
        let mut state = leads();
        let original = state.clone();
        let mut chooser = super::super::branch::Chooser::new();
        let mut b = Battle::new(&mut state, &mut chooser);
        b.history_readers.ability_order = true;
        assert_eq!(b.ability_state_order(), [P1A, P1B, P2A, P2B]);

        // p1a's ability state restarts (a switch-in, setAbility): last.
        b.restart_ability_state(P1A);
        assert_eq!(b.ability_state_order(), [P1B, P2A, P2B, P1A]);
        let values =
            |b: &Battle<'_, 2>| [P1A, P1B, P2A, P2B].map(|s| b.state.slot(s).ability_order);
        assert_eq!(values(&b), [1, 0, 0, 0]);

        // Ally Switch on side one: the Pokémon take their ability states along.
        let before = b.ability_state_order();
        let (a, c) = (b.state.slot(P1A).clone(), b.state.slot(P1B).clone());
        let mut swap = Vec::new();
        super::super::diff::slot_changes(&mut swap, P1A, &a, &c);
        super::super::diff::slot_changes(&mut swap, P1B, &c, &a);
        for instruction in swap {
            b.apply(instruction);
        }
        b.swap_ability_state_order(&before, P1A, P1B);
        assert_eq!(b.ability_state_order(), [P1A, P2A, P2B, P1B]);
        assert_eq!(values(&b), [0, 1, 0, 0]);

        // p2a restarts, then p1b: the earliest is now p1a.
        b.restart_ability_state(P2A);
        b.restart_ability_state(P1B);
        assert_eq!(b.ability_state_order(), [P1A, P2B, P2A, P1B]);

        // Restarting everyone in the start's order gives the start's values back.
        for s in [P1A, P1B, P2A, P2B] {
            b.restart_ability_state(s);
        }
        assert_eq!(values(&b), [0, 0, 0, 0]);

        let log = std::mem::take(&mut b.log);
        drop(b);
        state.reverse(&log);
        assert_eq!(state, original);
    }

    /// Without a redirector in the battle nothing is recorded.
    #[test]
    fn ability_state_order_is_not_recorded_without_readers() {
        let mut state = leads();
        let mut chooser = super::super::branch::Chooser::new();
        let mut b = Battle::new(&mut state, &mut chooser);
        assert!(!b.history_readers.ability_order);
        b.restart_ability_state(P1A);
        assert!(b.log.is_empty());
    }
}
