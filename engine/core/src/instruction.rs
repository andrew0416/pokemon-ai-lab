//! Reversible state changes. Every variant carries what it needs to undo itself, so search
//! never clones the state: apply on the way down, reverse on the way up.

use crate::field::{Effect, FieldEffect, SideEffect};
use crate::state::{SideId, Slot, SlotRef, State, Status};

#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    /// `amount` is the HP actually removed (already clamped to the target's HP).
    Damage { target: SlotRef, amount: i16 },
    /// `amount` is the HP actually restored (already clamped to max HP).
    Heal { target: SlotRef, amount: i16 },
    /// `amount` is the stage change actually applied (already clamped to -6..=6).
    Boost { target: SlotRef, stat: u8, amount: i8 },
    ChangeStatus { target: SlotRef, old: Status, new: Status },
    /// Replaces the slot wholesale; `previous` restores boosts/volatiles on reverse.
    Switch { slot: SlotRef, previous: Slot, party_index: Option<u8> },
    SetField { effect: FieldEffect, old: Effect, new: Effect },
    SetSideEffect { side: SideId, effect: SideEffect, old: Effect, new: Effect },
}

/// One weighted result of a turn (or of a single action while a turn is being built).
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub probability: f32,
    pub instructions: Vec<Instruction>,
}

impl<const N: usize> State<N> {
    pub fn apply(&mut self, instructions: &[Instruction]) {
        for instruction in instructions {
            self.apply_one(instruction);
        }
    }

    pub fn reverse(&mut self, instructions: &[Instruction]) {
        for instruction in instructions.iter().rev() {
            self.reverse_one(instruction);
        }
    }

    fn apply_one(&mut self, instruction: &Instruction) {
        match instruction {
            Instruction::Damage { target, amount } => self.active_hp_mut(*target, -amount),
            Instruction::Heal { target, amount } => self.active_hp_mut(*target, *amount),
            Instruction::Boost { target, stat, amount } => {
                self.slot_mut(*target).boosts[*stat as usize] += amount
            }
            Instruction::ChangeStatus { target, new, .. } => self.set_status(*target, *new),
            Instruction::Switch { slot, party_index, .. } => {
                *self.slot_mut(*slot) = Slot {
                    party_index: *party_index,
                    ..Slot::default()
                }
            }
            Instruction::SetField { effect, new, .. } => self.field[*effect as usize] = *new,
            Instruction::SetSideEffect { side, effect, new, .. } => {
                self.side_mut(*side).effects[*effect as usize] = *new
            }
        }
    }

    fn reverse_one(&mut self, instruction: &Instruction) {
        match instruction {
            Instruction::Damage { target, amount } => self.active_hp_mut(*target, *amount),
            Instruction::Heal { target, amount } => self.active_hp_mut(*target, -amount),
            Instruction::Boost { target, stat, amount } => {
                self.slot_mut(*target).boosts[*stat as usize] -= amount
            }
            Instruction::ChangeStatus { target, old, .. } => self.set_status(*target, *old),
            Instruction::Switch { slot, previous, .. } => *self.slot_mut(*slot) = previous.clone(),
            Instruction::SetField { effect, old, .. } => self.field[*effect as usize] = *old,
            Instruction::SetSideEffect { side, effect, old, .. } => {
                self.side_mut(*side).effects[*effect as usize] = *old
            }
        }
    }

    fn active_hp_mut(&mut self, target: SlotRef, delta: i16) {
        let pokemon = self
            .active_mut(target)
            .expect("HP instruction on an empty slot");
        pokemon.hp += delta;
    }

    fn set_status(&mut self, target: SlotRef, status: Status) {
        let pokemon = self
            .active_mut(target)
            .expect("status instruction on an empty slot");
        pokemon.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Weather;

    fn doubles_with_leads() -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, pokemon) in state.side_mut(side).party.iter_mut().enumerate() {
                pokemon.species = i as u16 + 1;
                pokemon.max_hp = 200;
                pokemon.hp = 200;
            }
            for slot in 0..2u8 {
                state.side_mut(side).slots[slot as usize].party_index = Some(slot);
            }
        }
        state
    }

    #[test]
    fn apply_then_reverse_restores_state() {
        let original = doubles_with_leads();
        let mut state = original.clone();
        let foe = SlotRef { side: SideId::Two, slot: 1 };
        let me = SlotRef { side: SideId::One, slot: 0 };
        let mut boosted = Slot {
            party_index: Some(0),
            ..Slot::default()
        };
        boosted.boosts[0] = 2;

        let instructions = vec![
            Instruction::Damage { target: foe, amount: 120 },
            Instruction::Boost { target: me, stat: 0, amount: 2 },
            Instruction::ChangeStatus { target: foe, old: Status::None, new: Status::Sleep },
            Instruction::Switch { slot: me, previous: boosted, party_index: Some(3) },
            Instruction::SetField {
                effect: FieldEffect::Weather,
                old: Effect::NONE,
                new: Effect { value: Weather::Sand as u8, turns: 5 },
            },
            Instruction::SetField {
                effect: FieldEffect::Gravity,
                old: Effect::NONE,
                new: Effect { value: 0, turns: 5 },
            },
        ];

        state.apply(&instructions);
        assert_eq!(state.active(foe).unwrap().hp, 80);
        assert_eq!(state.slot(me).party_index, Some(3));
        assert!(state.field[FieldEffect::Gravity as usize].is_active());

        state.reverse(&instructions);
        assert_eq!(state, original);
    }

    #[test]
    fn singles_is_one_slot() {
        assert_eq!(State::<1>::SLOTS, 1);
        assert_eq!(State::<1>::slot_refs().count(), 2);
        assert_eq!(State::<2>::slot_refs().count(), 4);
    }
}
