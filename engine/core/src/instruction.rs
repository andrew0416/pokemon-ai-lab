//! Reversible state changes. Every variant carries what it needs to undo itself, so search
//! never clones the state: apply on the way down, reverse on the way up.
//!
//! Changes to a party member address it by [`PokemonRef`] (it may have left its slot by the
//! time the change happens, e.g. `fnt` is set after fainting); changes to what resets on
//! switch-out address the [`SlotRef`].

use crate::dex::{AbilityId, ItemId, MoveId, Type};
use crate::field::{Effect, FieldEffect, SideEffect, SlotCondition, SlotEffect};
use crate::gimmick::Gimmick;
use crate::state::{
    BattleResult, Forme, MoveSlot, PokemonRef, SideHistory, SideId, Slot, SlotHistory, SlotRef,
    State, Status, SwitchFlag, TransformBase,
};
use crate::volatile::{Volatile, VolatileState};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Instruction {
    /// `amount` is the HP actually removed (already clamped to the target's HP).
    Damage {
        target: PokemonRef,
        amount: i16,
    },
    /// `amount` is the HP actually restored (already clamped to max HP).
    Heal {
        target: PokemonRef,
        amount: i16,
    },
    /// `amount` is the stage change actually applied (already clamped to -6..=6).
    Boost {
        target: SlotRef,
        stat: u8,
        amount: i8,
    },
    ChangeStatus {
        target: PokemonRef,
        old: Status,
        new: Status,
    },
    SetStatusTurns {
        target: PokemonRef,
        old: i8,
        new: i8,
    },
    SetItem {
        target: PokemonRef,
        old: ItemId,
        new: ItemId,
    },
    SetLastItem {
        target: PokemonRef,
        old: ItemId,
        new: ItemId,
    },
    SetAbility {
        target: PokemonRef,
        old: AbilityId,
        new: AbilityId,
    },
    /// A forme change (Mega Evolution, Primal Reversion, ...): species, types, max HP, stats
    /// and both abilities at once. HP itself changes by a separate `Damage`/`Heal`.
    SetForme {
        target: PokemonRef,
        old: Forme,
        new: Forme,
    },
    /// A type change that keeps the species (Roost, Soak, Protean, ...).
    SetTypes {
        target: PokemonRef,
        old: [Type; 2],
        new: [Type; 2],
    },
    /// `Pokemon::autotomized` (Autotomize's weight loss; reset by `setSpecies`).
    SetAutotomized {
        target: PokemonRef,
        old: u8,
        new: u8,
    },
    SetPp {
        target: PokemonRef,
        move_index: u8,
        old: u8,
        new: u8,
    },
    /// Replaces all four move slots (Transform's virtual copies, and the own slots coming back
    /// when a transformed Pokémon leaves the field).
    SetMoves {
        target: PokemonRef,
        old: [MoveSlot; 4],
        new: [MoveSlot; 4],
    },
    /// `Pokemon::transformed` (Showdown `transformed` with the base kept for `clearVolatile`).
    SetTransformed {
        target: PokemonRef,
        old: Option<TransformBase>,
        new: Option<TransformBase>,
    },
    /// Replaces the slot wholesale; `previous` restores boosts/volatiles on reverse (boxed:
    /// a `Slot` carries every volatile's state and would dominate the enum's size).
    Switch {
        slot: SlotRef,
        previous: Box<Slot>,
        party_index: Option<u8>,
    },
    /// Records (or clears) which fainted party member an empty slot still holds.
    SetFaintedOccupant {
        slot: SlotRef,
        old: Option<u8>,
        new: Option<u8>,
    },
    SetVolatile {
        target: SlotRef,
        volatile: Volatile,
        old: VolatileState,
        new: VolatileState,
    },
    SetLastMove {
        target: SlotRef,
        old: MoveId,
        new: MoveId,
    },
    SetMoveActions {
        target: SlotRef,
        old: u8,
        new: u8,
    },
    /// The slot's damage history (F13), replaced wholesale.
    SetSlotHistory {
        target: SlotRef,
        old: SlotHistory,
        new: SlotHistory,
    },
    /// The side's faint counters (F13), replaced wholesale.
    SetSideHistory {
        side: SideId,
        old: SideHistory,
        new: SideHistory,
    },
    /// `Slot::switch_flag` (F6).
    SetSwitchFlag {
        target: SlotRef,
        old: SwitchFlag,
        new: SwitchFlag,
    },
    /// `Slot::substitute_hp` (F11): the HP left on the slot's substitute (Showdown
    /// `volatiles.substitute.hp`), 0 without one.
    SetSubstituteHp {
        target: SlotRef,
        old: i16,
        new: i16,
    },
    SetField {
        effect: FieldEffect,
        old: Effect,
        new: Effect,
    },
    SetSideEffect {
        side: SideId,
        effect: SideEffect,
        old: Effect,
        new: Effect,
    },
    /// A slot condition of one position (F12).
    SetSlotCondition {
        side: SideId,
        slot: u8,
        condition: SlotCondition,
        old: SlotEffect,
        new: SlotEffect,
    },
    /// Spends `side`'s once-per-battle budget for `gimmick`. The budget was unspent before
    /// (validation rejects a second use), so reverse just clears the bit.
    UseGimmick {
        side: SideId,
        gimmick: Gimmick,
    },
    SetTurn {
        old: u16,
        new: u16,
    },
    SetResult {
        old: BattleResult,
        new: BattleResult,
    },
    /// `State::last_move` (Showdown `battle.lastMove`, hidden; Copycat).
    SetBattleLastMove {
        old: MoveId,
        new: MoveId,
    },
}

/// One weighted result of a turn (or of a single action while a turn is being built). With
/// `suspension`, the turn stopped for a mid-turn switch decision (Showdown's `request:
/// switch` while actions remain: U-turn, Parting Shot, ...) and `crate::turn::resume_turn`
/// continues it from the state the instructions lead to.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub probability: f64,
    pub instructions: Vec<Instruction>,
    pub suspension: Option<crate::turn::Suspension>,
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

    pub fn apply_one(&mut self, instruction: &Instruction) {
        match *instruction {
            Instruction::Damage { target, amount } => self.pokemon_mut(target).hp -= amount,
            Instruction::Heal { target, amount } => self.pokemon_mut(target).hp += amount,
            Instruction::Boost {
                target,
                stat,
                amount,
            } => self.slot_mut(target).boosts[stat as usize] += amount,
            Instruction::ChangeStatus { target, new, .. } => self.pokemon_mut(target).status = new,
            Instruction::SetStatusTurns { target, new, .. } => {
                self.pokemon_mut(target).status_turns = new
            }
            Instruction::SetItem { target, new, .. } => self.pokemon_mut(target).item = new,
            Instruction::SetLastItem { target, new, .. } => {
                self.pokemon_mut(target).last_item = new
            }
            Instruction::SetAbility { target, new, .. } => self.pokemon_mut(target).ability = new,
            Instruction::SetForme { target, new, .. } => self.pokemon_mut(target).set_forme(new),
            Instruction::SetTypes { target, new, .. } => self.pokemon_mut(target).types = new,
            Instruction::SetAutotomized { target, new, .. } => {
                self.pokemon_mut(target).autotomized = new
            }
            Instruction::SetPp {
                target,
                move_index,
                new,
                ..
            } => self.pokemon_mut(target).moves[move_index as usize].pp = new,
            Instruction::SetMoves { target, new, .. } => self.pokemon_mut(target).moves = new,
            Instruction::SetTransformed { target, new, .. } => {
                self.pokemon_mut(target).transformed = new
            }
            Instruction::Switch {
                slot, party_index, ..
            } => {
                *self.slot_mut(slot) = Slot {
                    party_index,
                    ..Slot::default()
                }
            }
            Instruction::SetFaintedOccupant { slot, new, .. } => {
                self.slot_mut(slot).fainted_occupant = new
            }
            Instruction::SetVolatile {
                target,
                volatile,
                new,
                ..
            } => self.slot_mut(target).volatiles.set(volatile, new),
            Instruction::SetLastMove { target, new, .. } => self.slot_mut(target).last_move = new,
            Instruction::SetMoveActions { target, new, .. } => {
                self.slot_mut(target).move_actions = new
            }
            Instruction::SetSlotHistory { target, new, .. } => self.slot_mut(target).history = new,
            Instruction::SetSideHistory { side, new, .. } => self.side_mut(side).history = new,
            Instruction::SetSwitchFlag { target, new, .. } => {
                self.slot_mut(target).switch_flag = new
            }
            Instruction::SetSubstituteHp { target, new, .. } => {
                self.slot_mut(target).substitute_hp = new
            }
            Instruction::SetField { effect, new, .. } => self.field[effect as usize] = new,
            Instruction::SetSideEffect {
                side, effect, new, ..
            } => self.side_mut(side).effects[effect as usize] = new,
            Instruction::SetSlotCondition {
                side,
                slot,
                condition,
                new,
                ..
            } => self.side_mut(side).slot_conditions[usize::from(slot)][condition as usize] = new,
            Instruction::UseGimmick { side, gimmick } => {
                let used = &mut self.side_mut(side).gimmicks_used;
                debug_assert!(!gimmick.is_none() && !used.contains(gimmick));
                *used = used.with(gimmick);
            }
            Instruction::SetTurn { new, .. } => self.turn = new,
            Instruction::SetResult { new, .. } => self.result = new,
            Instruction::SetBattleLastMove { new, .. } => self.last_move = new,
        }
    }

    pub fn reverse_one(&mut self, instruction: &Instruction) {
        match *instruction {
            Instruction::Damage { target, amount } => self.pokemon_mut(target).hp += amount,
            Instruction::Heal { target, amount } => self.pokemon_mut(target).hp -= amount,
            Instruction::Boost {
                target,
                stat,
                amount,
            } => self.slot_mut(target).boosts[stat as usize] -= amount,
            Instruction::ChangeStatus { target, old, .. } => self.pokemon_mut(target).status = old,
            Instruction::SetStatusTurns { target, old, .. } => {
                self.pokemon_mut(target).status_turns = old
            }
            Instruction::SetItem { target, old, .. } => self.pokemon_mut(target).item = old,
            Instruction::SetLastItem { target, old, .. } => {
                self.pokemon_mut(target).last_item = old
            }
            Instruction::SetAbility { target, old, .. } => self.pokemon_mut(target).ability = old,
            Instruction::SetForme { target, old, .. } => self.pokemon_mut(target).set_forme(old),
            Instruction::SetTypes { target, old, .. } => self.pokemon_mut(target).types = old,
            Instruction::SetAutotomized { target, old, .. } => {
                self.pokemon_mut(target).autotomized = old
            }
            Instruction::SetPp {
                target,
                move_index,
                old,
                ..
            } => self.pokemon_mut(target).moves[move_index as usize].pp = old,
            Instruction::SetMoves { target, old, .. } => self.pokemon_mut(target).moves = old,
            Instruction::SetTransformed { target, old, .. } => {
                self.pokemon_mut(target).transformed = old
            }
            Instruction::Switch {
                slot, ref previous, ..
            } => *self.slot_mut(slot) = previous.as_ref().clone(),
            Instruction::SetFaintedOccupant { slot, old, .. } => {
                self.slot_mut(slot).fainted_occupant = old
            }
            Instruction::SetVolatile {
                target,
                volatile,
                old,
                ..
            } => self.slot_mut(target).volatiles.set(volatile, old),
            Instruction::SetLastMove { target, old, .. } => self.slot_mut(target).last_move = old,
            Instruction::SetMoveActions { target, old, .. } => {
                self.slot_mut(target).move_actions = old
            }
            Instruction::SetSlotHistory { target, old, .. } => self.slot_mut(target).history = old,
            Instruction::SetSideHistory { side, old, .. } => self.side_mut(side).history = old,
            Instruction::SetSwitchFlag { target, old, .. } => {
                self.slot_mut(target).switch_flag = old
            }
            Instruction::SetSubstituteHp { target, old, .. } => {
                self.slot_mut(target).substitute_hp = old
            }
            Instruction::SetField { effect, old, .. } => self.field[effect as usize] = old,
            Instruction::SetSideEffect {
                side, effect, old, ..
            } => self.side_mut(side).effects[effect as usize] = old,
            Instruction::SetSlotCondition {
                side,
                slot,
                condition,
                old,
                ..
            } => self.side_mut(side).slot_conditions[usize::from(slot)][condition as usize] = old,
            Instruction::UseGimmick { side, gimmick } => {
                let used = &mut self.side_mut(side).gimmicks_used;
                *used = used.without(gimmick);
            }
            Instruction::SetTurn { old, .. } => self.turn = old,
            Instruction::SetResult { old, .. } => self.result = old,
            Instruction::SetBattleLastMove { old, .. } => self.last_move = old,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::{abilities, items, species, SpeciesId, Type};
    use crate::field::Weather;

    fn doubles_with_leads() -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, pokemon) in state.side_mut(side).party.iter_mut().enumerate() {
                pokemon.species = SpeciesId(i as u16 + 1);
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
        let foe = SlotRef {
            side: SideId::Two,
            slot: 1,
        };
        let foe_mon = PokemonRef {
            side: SideId::Two,
            party: 1,
        };
        let me = SlotRef {
            side: SideId::One,
            slot: 0,
        };
        let foe_lead = SlotRef {
            side: SideId::Two,
            slot: 0,
        };
        let mut boosted = Slot {
            party_index: Some(0),
            ..Slot::default()
        };
        boosted.boosts[0] = 2;

        let my_mon = PokemonRef {
            side: SideId::One,
            party: 0,
        };
        let mega = Forme {
            species: species::TYRANITAR_MEGA,
            types: species::TYRANITAR_MEGA.data().types,
            max_hp: 200,
            stats: [184, 170, 115, 140, 91],
            ability: abilities::SAND_STREAM,
            base_ability: abilities::SAND_STREAM,
        };
        let instructions = vec![
            Instruction::Damage {
                target: foe_mon,
                amount: 120,
            },
            Instruction::SetForme {
                target: my_mon,
                old: state.pokemon(my_mon).forme(),
                new: mega,
            },
            Instruction::SetTypes {
                target: foe_mon,
                old: state.pokemon(foe_mon).types,
                new: [Type::Water, Type::None],
            },
            // Transform: the base kept, the virtual copies in place.
            Instruction::SetTransformed {
                target: foe_mon,
                old: None,
                new: Some(crate::state::TransformBase {
                    species: state.pokemon(foe_mon).species,
                    moves: state.pokemon(foe_mon).moves,
                }),
            },
            Instruction::SetMoves {
                target: foe_mon,
                old: state.pokemon(foe_mon).moves,
                new: [
                    crate::state::MoveSlot {
                        id: crate::dex::moves::PROTECT,
                        pp: 5,
                        disabled: false,
                    },
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ],
            },
            Instruction::Boost {
                target: me,
                stat: 0,
                amount: 2,
            },
            Instruction::ChangeStatus {
                target: foe_mon,
                old: Status::None,
                new: Status::Sleep,
            },
            Instruction::SetStatusTurns {
                target: foe_mon,
                old: 0,
                new: 3,
            },
            Instruction::SetVolatile {
                target: foe,
                volatile: Volatile::Flinch,
                old: VolatileState::NONE,
                new: VolatileState {
                    active: true,
                    duration: 1,
                    ..VolatileState::NONE
                },
            },
            Instruction::SetSubstituteHp {
                target: foe,
                old: 0,
                new: 50,
            },
            Instruction::SetItem {
                target: foe_mon,
                old: ItemId::NONE,
                new: items::LEFTOVERS,
            },
            Instruction::Switch {
                slot: me,
                previous: Box::new(boosted),
                party_index: Some(3),
            },
            // The foe's lead faints: its slot empties but remembers it.
            Instruction::Switch {
                slot: foe_lead,
                previous: Box::new(Slot {
                    party_index: Some(0),
                    ..Slot::default()
                }),
                party_index: None,
            },
            Instruction::SetFaintedOccupant {
                slot: foe_lead,
                old: None,
                new: Some(0),
            },
            Instruction::SetField {
                effect: FieldEffect::Weather,
                old: Effect::NONE,
                new: Effect {
                    value: Weather::Sand as u8,
                    turns: 5,
                },
            },
            Instruction::SetField {
                effect: FieldEffect::Gravity,
                old: Effect::NONE,
                new: Effect { value: 0, turns: 5 },
            },
            Instruction::UseGimmick {
                side: SideId::One,
                gimmick: Gimmick::Mega,
            },
            Instruction::SetTurn { old: 0, new: 1 },
            Instruction::SetResult {
                old: BattleResult::Ongoing,
                new: BattleResult::Win(SideId::One),
            },
            Instruction::SetBattleLastMove {
                old: MoveId::NONE,
                new: crate::dex::moves::PROTECT,
            },
        ];

        state.apply(&instructions);
        assert_eq!(state.active(foe).unwrap().hp, 80);
        assert_eq!(state.pokemon(my_mon).forme(), mega);
        assert_eq!(state.pokemon(my_mon).species, species::TYRANITAR_MEGA);
        assert_eq!(state.pokemon(my_mon).ability, abilities::SAND_STREAM);
        assert_eq!(state.pokemon(foe_mon).types, [Type::Water, Type::None]);
        assert!(state.pokemon(foe_mon).transformed.is_some());
        assert_eq!(state.pokemon(foe_mon).moves[0].pp, 5);
        assert_eq!(state.active(foe).unwrap().status_turns, 3);
        assert!(state.slot(foe).volatiles.has(Volatile::Flinch));
        assert!(state
            .side(SideId::One)
            .gimmicks_used
            .contains(Gimmick::Mega));
        assert!(state.side(SideId::Two).gimmicks_used.is_empty());
        assert_eq!(state.slot(me).party_index, Some(3));
        assert_eq!(state.slot(foe_lead).party_index, None);
        assert_eq!(state.slot(foe_lead).fainted_occupant, Some(0));
        assert!(state.field[FieldEffect::Gravity as usize].is_active());
        assert_eq!(state.result, BattleResult::Win(SideId::One));
        assert_eq!(state.last_move, crate::dex::moves::PROTECT);

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
