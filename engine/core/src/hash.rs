//! Position hashes (board P3a-incremental-hash).
//!
//! [`State::position_hash`] is a sum (wrapping `u64` addition) of one pseudo-random word per
//! *cell* of the state: every field of every party member, of every active slot (one cell per
//! boost stage and per volatile), of each side and of the field. A cell's word depends on
//! its position and its value only, so the hash is a function of the state and the same for
//! equal states, whichever path reached them.
//!
//! Because the hash is a sum over cells, a change to some cells changes it by the difference of
//! those cells' words: [`State::instruction_hash`] sums the words of the cells an
//! [`Instruction`] writes, read from the state as it is, so its value after applying the
//! instruction minus its value before is exactly the change of the position hash. The turn
//! engine keeps that running difference while a run applies its instructions
//! (`turn::battle::Battle::apply`), and the staged enumeration then knows the hash of every
//! position it merges without hashing the ~5 KB state again (Opus GG measured whole-state
//! hashing at 18 % of an enumeration; board P3). A volatile that is not set
//! ([`VolatileState::NONE`]) contributes 0, so switching a slot in or out hashes only its set
//! volatiles.
//!
//! The hash is only an index: every user resolves equal hashes by comparing the states
//! (`turn::merge::Merger`, the search's transposition table), so a collision costs time, never a
//! wrong result. `lab_engine::turn::verify_position_hashes` makes the enumeration check every
//! incrementally kept hash against a full recomputation (the tests turn it on).

use std::hash::{Hash, Hasher};

use crate::instruction::Instruction;
use crate::state::{Pokemon, PokemonRef, Side, SideId, Slot, SlotRef, State};
use crate::volatile::VolatileState;

/// FxHash-style mixing of derived `Hash` writes (one rotate, xor and multiply per word),
/// finished with the MurmurHash3 64-bit mixer so the low bits can index a table. Fast and
/// deterministic (no per-process seed), for keys whose collisions are resolved by `Eq`.
#[derive(Default, Clone, Copy)]
pub struct KeyHasher(u64);

impl KeyHasher {
    const SEED: u64 = 0x517c_c1b7_2722_0a95;

    pub fn new() -> KeyHasher {
        KeyHasher(0)
    }

    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(Self::SEED);
    }

    /// The state before the finalizer (what a cell's word is made of).
    #[inline]
    fn raw(&self) -> u64 {
        self.0
    }
}

impl Hasher for KeyHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.add(u64::from_le_bytes(*chunk));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        fmix(self.0)
    }
}

/// The MurmurHash3 64-bit finalizer (a bijection).
#[inline]
fn fmix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// A value's derived `Hash` writes folded into one word.
#[inline]
fn word<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut hasher = KeyHasher::new();
    value.hash(&mut hasher);
    hasher.raw()
}

/// The word of the cell `key` holding a value whose word is `value`.
#[inline]
fn cell(key: u64, value: u64) -> u64 {
    const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;
    fmix(value.wrapping_add(key.wrapping_mul(GOLDEN)))
}

// Cell keys: the kind in bits 40.., the side in bits 20.., the party member or slot in bits
// 12.., the field below.
const POKEMON: u64 = 1 << 40;
const SLOT: u64 = 2 << 40;
const SIDE: u64 = 3 << 40;
const GLOBAL: u64 = 4 << 40;

// Party member fields.
const P_SPECIES: u64 = 0;
const P_TYPES: u64 = 1;
const P_HP: u64 = 2;
const P_MAX_HP: u64 = 3;
const P_STATS: u64 = 4;
const P_STATUS: u64 = 5;
const P_STATUS_TURNS: u64 = 6;
const P_ITEM: u64 = 7;
const P_LAST_ITEM: u64 = 8;
const P_AUTOTOMIZED: u64 = 9;
const P_ABILITY: u64 = 10;
const P_BASE_ABILITY: u64 = 11;
/// Move slots 0..4: 12..16.
const P_MOVE: u64 = 12;
const P_TRANSFORMED: u64 = 16;
const P_ILLUSION: u64 = 17;
/// The fields no instruction writes (level, nature, SP, gender, gimmick eligibility), one cell.
const P_FIXED: u64 = 18;

// Slot fields.
const S_PARTY_INDEX: u64 = 0;
const S_FAINTED_OCCUPANT: u64 = 1;
/// Boost stages 0..7: 2..9.
const S_BOOST: u64 = 2;
const S_LAST_MOVE: u64 = 9;
const S_LAST_MOVE_TARGET_LOC: u64 = 10;
const S_MOVE_ACTIONS: u64 = 11;
const S_HISTORY: u64 = 12;
const S_SWITCH_FLAG: u64 = 13;
const S_SUBSTITUTE_HP: u64 = 14;
const S_DYNAMAX: u64 = 15;
const S_ABILITY_ORDER: u64 = 16;
/// Volatiles: 256 + the volatile's index.
const S_VOLATILE: u64 = 256;

// Side fields.
/// Side effects: 0 + the effect's index.
const D_EFFECT: u64 = 0;
const D_GIMMICKS_USED: u64 = 64;
const D_HISTORY: u64 = 65;
const D_PARTY_ORDER: u64 = 66;
/// Slot conditions: 256 + 16 × slot + condition.
const D_SLOT_CONDITION: u64 = 256;

// Battle fields.
/// Field effects: 0 + the effect's index.
const G_FIELD: u64 = 0;
const G_TURN: u64 = 64;
const G_RESULT: u64 = 65;
const G_LAST_MOVE: u64 = 66;

const _: () = {
    assert!(crate::field::SIDE_EFFECT_COUNT <= 64);
    assert!(crate::field::FIELD_EFFECT_COUNT <= 64);
    assert!(crate::field::SLOT_CONDITION_COUNT <= 16);
    assert!(S_VOLATILE + (crate::volatile::VOLATILE_COUNT as u64) < 1 << 12);
};

#[inline]
fn side_bits(side: SideId) -> u64 {
    (side.index() as u64) << 20
}

#[inline]
fn pokemon_cell<T: Hash + ?Sized>(p: PokemonRef, field: u64, value: &T) -> u64 {
    cell(
        POKEMON | side_bits(p.side) | u64::from(p.party) << 12 | field,
        word(value),
    )
}

#[inline]
fn slot_cell<T: Hash + ?Sized>(s: SlotRef, field: u64, value: &T) -> u64 {
    cell(
        SLOT | side_bits(s.side) | u64::from(s.slot) << 12 | field,
        word(value),
    )
}

#[inline]
fn volatile_cell(s: SlotRef, index: usize, value: &VolatileState) -> u64 {
    if *value == VolatileState::NONE {
        0
    } else {
        slot_cell(s, S_VOLATILE + index as u64, value)
    }
}

#[inline]
fn side_cell<T: Hash + ?Sized>(side: SideId, field: u64, value: &T) -> u64 {
    cell(SIDE | side_bits(side) | field, word(value))
}

#[inline]
fn global_cell<T: Hash + ?Sized>(field: u64, value: &T) -> u64 {
    cell(GLOBAL | field, word(value))
}

#[inline]
fn slot_condition_field(slot: usize, condition: usize) -> u64 {
    D_SLOT_CONDITION + 16 * slot as u64 + condition as u64
}

/// The sum of a party member's cells.
fn pokemon_hash(p: PokemonRef, mon: &Pokemon) -> u64 {
    // Destructured so that a new field fails to compile until it is hashed.
    let Pokemon {
        species,
        level,
        types,
        hp,
        max_hp,
        stats,
        nature,
        stat_points,
        gender,
        status,
        status_turns,
        item,
        last_item,
        autotomized,
        ability,
        base_ability,
        moves,
        transformed,
        illusion,
        gimmicks,
        gigantamax_factor,
    } = mon;
    let mut h = pokemon_cell(p, P_SPECIES, species)
        .wrapping_add(pokemon_cell(p, P_TYPES, types))
        .wrapping_add(pokemon_cell(p, P_HP, hp))
        .wrapping_add(pokemon_cell(p, P_MAX_HP, max_hp))
        .wrapping_add(pokemon_cell(p, P_STATS, stats))
        .wrapping_add(pokemon_cell(p, P_STATUS, status))
        .wrapping_add(pokemon_cell(p, P_STATUS_TURNS, status_turns))
        .wrapping_add(pokemon_cell(p, P_ITEM, item))
        .wrapping_add(pokemon_cell(p, P_LAST_ITEM, last_item))
        .wrapping_add(pokemon_cell(p, P_AUTOTOMIZED, autotomized))
        .wrapping_add(pokemon_cell(p, P_ABILITY, ability))
        .wrapping_add(pokemon_cell(p, P_BASE_ABILITY, base_ability))
        .wrapping_add(pokemon_cell(p, P_TRANSFORMED, transformed))
        .wrapping_add(pokemon_cell(p, P_ILLUSION, illusion))
        .wrapping_add(pokemon_cell(
            p,
            P_FIXED,
            &(
                level,
                nature,
                stat_points,
                gender,
                gimmicks,
                gigantamax_factor,
            ),
        ));
    for (k, slot) in moves.iter().enumerate() {
        h = h.wrapping_add(pokemon_cell(p, P_MOVE + k as u64, slot));
    }
    h
}

/// The sum of an active slot's cells (its set volatiles only).
fn slot_hash(s: SlotRef, slot: &Slot) -> u64 {
    let Slot {
        party_index,
        fainted_occupant,
        boosts,
        volatiles,
        last_move,
        last_move_target_loc,
        move_actions,
        history,
        switch_flag,
        substitute_hp,
        dynamax,
        ability_order,
    } = slot;
    let mut h = slot_cell(s, S_PARTY_INDEX, party_index)
        .wrapping_add(slot_cell(s, S_FAINTED_OCCUPANT, fainted_occupant))
        .wrapping_add(slot_cell(s, S_LAST_MOVE, last_move))
        .wrapping_add(slot_cell(s, S_LAST_MOVE_TARGET_LOC, last_move_target_loc))
        .wrapping_add(slot_cell(s, S_MOVE_ACTIONS, move_actions))
        .wrapping_add(slot_cell(s, S_HISTORY, history))
        .wrapping_add(slot_cell(s, S_SWITCH_FLAG, switch_flag))
        .wrapping_add(slot_cell(s, S_SUBSTITUTE_HP, substitute_hp))
        .wrapping_add(slot_cell(s, S_DYNAMAX, dynamax))
        .wrapping_add(slot_cell(s, S_ABILITY_ORDER, ability_order));
    for (k, boost) in boosts.iter().enumerate() {
        h = h.wrapping_add(slot_cell(s, S_BOOST + k as u64, boost));
    }
    for (k, v) in volatiles.0.iter().enumerate() {
        if *v != VolatileState::NONE {
            h = h.wrapping_add(volatile_cell(s, k, v));
        }
    }
    h
}

/// The sum of a side's own cells (not its party members' or slots').
fn side_hash<const N: usize>(id: SideId, side: &Side<N>) -> u64 {
    let Side {
        slots: _,
        party: _,
        effects,
        gimmicks_used,
        history,
        slot_conditions,
        party_order,
    } = side;
    let mut h = side_cell(id, D_GIMMICKS_USED, gimmicks_used)
        .wrapping_add(side_cell(id, D_HISTORY, history))
        .wrapping_add(side_cell(id, D_PARTY_ORDER, party_order));
    for (e, effect) in effects.iter().enumerate() {
        h = h.wrapping_add(side_cell(id, D_EFFECT + e as u64, effect));
    }
    for (j, conditions) in slot_conditions.iter().enumerate() {
        for (c, condition) in conditions.iter().enumerate() {
            h = h.wrapping_add(side_cell(id, slot_condition_field(j, c), condition));
        }
    }
    h
}

impl<const N: usize> State<N> {
    /// The position hash: equal states have equal hashes (see the module documentation).
    /// Computed in full here; the turn engine keeps it incrementally.
    pub fn position_hash(&self) -> u64 {
        let State {
            sides,
            field,
            turn,
            result,
            last_move,
        } = self;
        let mut h = global_cell(G_TURN, turn)
            .wrapping_add(global_cell(G_RESULT, result))
            .wrapping_add(global_cell(G_LAST_MOVE, last_move));
        for (e, effect) in field.iter().enumerate() {
            h = h.wrapping_add(global_cell(G_FIELD + e as u64, effect));
        }
        for (id, side) in [SideId::One, SideId::Two].into_iter().zip(sides) {
            h = h.wrapping_add(side_hash(id, side));
            for (party, mon) in side.party.iter().enumerate() {
                let p = PokemonRef {
                    side: id,
                    party: party as u8,
                };
                h = h.wrapping_add(pokemon_hash(p, mon));
            }
            for (slot, s) in side.slots.iter().enumerate() {
                let r = SlotRef {
                    side: id,
                    slot: slot as u8,
                };
                h = h.wrapping_add(slot_hash(r, s));
            }
        }
        h
    }

    /// The sum of the words of the cells `instruction` writes, as the state holds them now:
    /// its value after applying the instruction minus its value before is the change of
    /// [`State::position_hash`] (wrapping arithmetic).
    #[inline]
    pub fn instruction_hash(&self, instruction: &Instruction) -> u64 {
        match *instruction {
            Instruction::Damage { target, .. } | Instruction::Heal { target, .. } => {
                pokemon_cell(target, P_HP, &self.pokemon(target).hp)
            }
            Instruction::Boost { target, stat, .. } => slot_cell(
                target,
                S_BOOST + u64::from(stat),
                &self.slot(target).boosts[usize::from(stat)],
            ),
            Instruction::ChangeStatus { target, .. } => {
                pokemon_cell(target, P_STATUS, &self.pokemon(target).status)
            }
            Instruction::SetStatusTurns { target, .. } => {
                pokemon_cell(target, P_STATUS_TURNS, &self.pokemon(target).status_turns)
            }
            Instruction::SetItem { target, .. } => {
                pokemon_cell(target, P_ITEM, &self.pokemon(target).item)
            }
            Instruction::SetLastItem { target, .. } => {
                pokemon_cell(target, P_LAST_ITEM, &self.pokemon(target).last_item)
            }
            Instruction::SetAbility { target, .. } => {
                pokemon_cell(target, P_ABILITY, &self.pokemon(target).ability)
            }
            Instruction::SetForme { target, .. } => {
                let mon = self.pokemon(target);
                pokemon_cell(target, P_SPECIES, &mon.species)
                    .wrapping_add(pokemon_cell(target, P_TYPES, &mon.types))
                    .wrapping_add(pokemon_cell(target, P_MAX_HP, &mon.max_hp))
                    .wrapping_add(pokemon_cell(target, P_STATS, &mon.stats))
                    .wrapping_add(pokemon_cell(target, P_ABILITY, &mon.ability))
                    .wrapping_add(pokemon_cell(target, P_BASE_ABILITY, &mon.base_ability))
            }
            Instruction::SetTypes { target, .. } => {
                pokemon_cell(target, P_TYPES, &self.pokemon(target).types)
            }
            Instruction::SetAutotomized { target, .. } => {
                pokemon_cell(target, P_AUTOTOMIZED, &self.pokemon(target).autotomized)
            }
            Instruction::SetPp {
                target, move_index, ..
            } => pokemon_cell(
                target,
                P_MOVE + u64::from(move_index),
                &self.pokemon(target).moves[usize::from(move_index)],
            ),
            Instruction::SetMoves { target, .. } => {
                let mon = self.pokemon(target);
                let mut h = 0u64;
                for (k, slot) in mon.moves.iter().enumerate() {
                    h = h.wrapping_add(pokemon_cell(target, P_MOVE + k as u64, slot));
                }
                h
            }
            Instruction::SetTransformed { target, .. } => {
                pokemon_cell(target, P_TRANSFORMED, &self.pokemon(target).transformed)
            }
            Instruction::SetIllusion { target, .. } => {
                pokemon_cell(target, P_ILLUSION, &self.pokemon(target).illusion)
            }
            Instruction::Switch { slot, .. } => slot_hash(slot, self.slot(slot)),
            Instruction::SetFaintedOccupant { slot, .. } => {
                slot_cell(slot, S_FAINTED_OCCUPANT, &self.slot(slot).fainted_occupant)
            }
            Instruction::SetVolatile {
                target, volatile, ..
            } => volatile_cell(
                target,
                volatile as usize,
                &self.slot(target).volatiles.get(volatile),
            ),
            Instruction::SetLastMove { target, .. } => {
                slot_cell(target, S_LAST_MOVE, &self.slot(target).last_move)
            }
            Instruction::SetLastMoveTargetLoc { target, .. } => slot_cell(
                target,
                S_LAST_MOVE_TARGET_LOC,
                &self.slot(target).last_move_target_loc,
            ),
            Instruction::SetMoveActions { target, .. } => {
                slot_cell(target, S_MOVE_ACTIONS, &self.slot(target).move_actions)
            }
            Instruction::SetSlotHistory { target, .. } => {
                slot_cell(target, S_HISTORY, &self.slot(target).history)
            }
            Instruction::SetSwitchFlag { target, .. } => {
                slot_cell(target, S_SWITCH_FLAG, &self.slot(target).switch_flag)
            }
            Instruction::SetSubstituteHp { target, .. } => {
                slot_cell(target, S_SUBSTITUTE_HP, &self.slot(target).substitute_hp)
            }
            Instruction::SetAbilityOrder { target, .. } => {
                slot_cell(target, S_ABILITY_ORDER, &self.slot(target).ability_order)
            }
            Instruction::SetSideHistory { side, .. } => {
                side_cell(side, D_HISTORY, &self.side(side).history)
            }
            Instruction::SetPartyOrder { side, .. } => {
                side_cell(side, D_PARTY_ORDER, &self.side(side).party_order)
            }
            Instruction::SetField { effect, .. } => {
                global_cell(G_FIELD + effect as u64, &self.field[effect as usize])
            }
            Instruction::SetSideEffect { side, effect, .. } => side_cell(
                side,
                D_EFFECT + effect as u64,
                &self.side(side).effects[effect as usize],
            ),
            Instruction::SetSlotCondition {
                side,
                slot,
                condition,
                ..
            } => side_cell(
                side,
                slot_condition_field(usize::from(slot), condition as usize),
                &self.side(side).slot_conditions[usize::from(slot)][condition as usize],
            ),
            Instruction::UseGimmick { side, .. } => {
                side_cell(side, D_GIMMICKS_USED, &self.side(side).gimmicks_used)
            }
            Instruction::SetTurn { .. } => global_cell(G_TURN, &self.turn),
            Instruction::SetResult { .. } => global_cell(G_RESULT, &self.result),
            Instruction::SetBattleLastMove { .. } => global_cell(G_LAST_MOVE, &self.last_move),
        }
    }

    /// Applies `instruction` and returns the change of [`State::position_hash`] it made.
    #[inline]
    pub fn apply_hashed(&mut self, instruction: &Instruction) -> u64 {
        let before = self.instruction_hash(instruction);
        self.apply_one(instruction);
        self.instruction_hash(instruction).wrapping_sub(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::{abilities, items, moves, species, ItemId, MoveId, Type};
    use crate::field::{Effect, FieldEffect, SideEffect, SlotCondition, SlotEffect, Weather};
    use crate::gimmick::Gimmick;
    use crate::state::{
        BattleResult, Forme, MoveSlot, SideHistory, SlotHistory, Status, SwitchFlag, TransformBase,
    };
    use crate::volatile::Volatile;

    fn doubles_with_leads() -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, pokemon) in state.side_mut(side).party.iter_mut().enumerate() {
                pokemon.species = crate::dex::SpeciesId(i as u16 + 1);
                pokemon.max_hp = 200;
                pokemon.hp = 200;
                pokemon.moves[0] = MoveSlot::full(moves::PROTECT);
            }
            for slot in 0..2u8 {
                state.side_mut(side).slots[slot as usize].party_index = Some(slot);
            }
        }
        state
    }

    /// Every instruction variant, applied in turn: the hash kept by the instructions' deltas
    /// equals the full hash after every step, and returns to the start's after the reversal.
    #[test]
    fn instruction_deltas_track_the_full_hash() {
        let mut state = doubles_with_leads();
        let start = state.clone();
        let p = |side, party| PokemonRef { side, party };
        let s = |side, slot| SlotRef { side, slot };
        let foe = p(SideId::Two, 1);
        let me = p(SideId::One, 0);
        let foe_slot = s(SideId::Two, 1);
        let my_slot = s(SideId::One, 0);
        let history = SlotHistory {
            times_attacked: 2,
            ..SlotHistory::default()
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
                target: foe,
                amount: 120,
            },
            Instruction::Heal {
                target: foe,
                amount: 20,
            },
            Instruction::Boost {
                target: my_slot,
                stat: 3,
                amount: 2,
            },
            Instruction::ChangeStatus {
                target: foe,
                old: Status::None,
                new: Status::Sleep,
            },
            Instruction::SetStatusTurns {
                target: foe,
                old: 0,
                new: 3,
            },
            Instruction::SetItem {
                target: foe,
                old: ItemId::NONE,
                new: items::LEFTOVERS,
            },
            Instruction::SetLastItem {
                target: me,
                old: ItemId::NONE,
                new: items::SITRUS_BERRY,
            },
            Instruction::SetAbility {
                target: me,
                old: state.pokemon(me).ability,
                new: abilities::TRACE,
            },
            Instruction::SetForme {
                target: me,
                old: state.pokemon(me).forme(),
                new: mega,
            },
            Instruction::SetTypes {
                target: foe,
                old: state.pokemon(foe).types,
                new: [Type::Water, Type::None],
            },
            Instruction::SetAutotomized {
                target: foe,
                old: 0,
                new: 1,
            },
            Instruction::SetPp {
                target: me,
                move_index: 0,
                old: 8,
                new: 7,
            },
            Instruction::SetTransformed {
                target: foe,
                old: None,
                new: Some(TransformBase {
                    species: state.pokemon(foe).species,
                    moves: state.pokemon(foe).moves,
                }),
            },
            Instruction::SetMoves {
                target: foe,
                old: state.pokemon(foe).moves,
                new: [MoveSlot::full(moves::FAKE_OUT); 4],
            },
            Instruction::SetIllusion {
                target: me,
                old: false,
                new: true,
            },
            Instruction::SetVolatile {
                target: foe_slot,
                volatile: Volatile::Confusion,
                old: VolatileState::NONE,
                new: VolatileState {
                    active: true,
                    duration: 3,
                    ..VolatileState::NONE
                },
            },
            Instruction::SetLastMove {
                target: foe_slot,
                old: MoveId::NONE,
                new: moves::PROTECT,
            },
            Instruction::SetLastMoveTargetLoc {
                target: foe_slot,
                old: 0,
                new: -1,
            },
            Instruction::SetMoveActions {
                target: foe_slot,
                old: 0,
                new: 1,
            },
            Instruction::SetSlotHistory {
                target: foe_slot,
                old: SlotHistory::default(),
                new: history,
            },
            Instruction::SetSwitchFlag {
                target: foe_slot,
                old: SwitchFlag::None,
                new: SwitchFlag::Move,
            },
            Instruction::SetSubstituteHp {
                target: foe_slot,
                old: 0,
                new: 50,
            },
            Instruction::SetAbilityOrder {
                target: foe_slot,
                old: 0,
                new: 2,
            },
            // The switched-out slots' boosts and volatiles go (`previous` is filled in below
            // from the state at that point).
            Instruction::Switch {
                slot: my_slot,
                previous: Box::new(Slot::default()),
                party_index: Some(3),
            },
            Instruction::Switch {
                slot: foe_slot,
                previous: Box::new(Slot::default()),
                party_index: None,
            },
            Instruction::SetFaintedOccupant {
                slot: foe_slot,
                old: None,
                new: Some(1),
            },
            Instruction::SetSideHistory {
                side: SideId::Two,
                old: SideHistory::default(),
                new: SideHistory {
                    total_fainted: 1,
                    ..SideHistory::default()
                },
            },
            Instruction::SetPartyOrder {
                side: SideId::One,
                old: crate::state::IDENTITY_ORDER,
                new: [3, 1, 2, 0, 4, 5],
            },
            Instruction::SetField {
                effect: FieldEffect::Weather,
                old: Effect::NONE,
                new: Effect {
                    value: Weather::Sand as u8,
                    turns: 5,
                },
            },
            Instruction::SetSideEffect {
                side: SideId::Two,
                effect: SideEffect::Tailwind,
                old: Effect::NONE,
                new: Effect { value: 0, turns: 4 },
            },
            Instruction::SetSlotCondition {
                side: SideId::One,
                slot: 1,
                condition: SlotCondition::Wish,
                old: SlotEffect::NONE,
                new: SlotEffect { value: 90, turn: 2 },
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
                new: moves::PROTECT,
            },
        ];
        let mut hash = state.position_hash();
        let start_hash = hash;
        let mut seen = vec![hash];
        let mut applied = Vec::new();
        for instruction in &instructions {
            let mut instruction = instruction.clone();
            if let Instruction::Switch { slot, previous, .. } = &mut instruction {
                **previous = state.slot(*slot).clone();
            }
            let instruction = &instruction;
            hash = hash.wrapping_add(state.apply_hashed(instruction));
            applied.push(instruction.clone());
            assert_eq!(hash, state.position_hash(), "after {instruction:?}");
            seen.push(hash);
        }
        // Every step changed the state, and no two of these positions share a hash.
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len());
        for instruction in applied.iter().rev() {
            let before = state.instruction_hash(instruction);
            state.reverse_one(instruction);
            hash = hash.wrapping_add(state.instruction_hash(instruction).wrapping_sub(before));
            assert_eq!(hash, state.position_hash(), "reversing {instruction:?}");
        }
        assert_eq!(state, start);
        assert_eq!(hash, start_hash);
    }

    /// The hash tells apart the same value in different cells (a swap of two party members'
    /// HP, a boost moved to another stat).
    #[test]
    fn same_values_in_other_cells_hash_differently() {
        let base = doubles_with_leads();
        let mut a = base.clone();
        a.side_mut(SideId::One).party[0].hp = 150;
        let mut b = base.clone();
        b.side_mut(SideId::One).party[1].hp = 150;
        assert_ne!(a.position_hash(), b.position_hash());
        let mut c = base.clone();
        c.side_mut(SideId::Two).party[0].hp = 150;
        assert_ne!(a.position_hash(), c.position_hash());
        let mut d = base.clone();
        d.side_mut(SideId::One).slots[0].boosts[0] = 1;
        let mut e = base.clone();
        e.side_mut(SideId::One).slots[0].boosts[1] = 1;
        assert_ne!(d.position_hash(), e.position_hash());
        assert_eq!(base.position_hash(), base.clone().position_hash());
    }
}
