//! Battle state, generic over the number of active slots per side.
//!
//! Persistent data (HP, status, item, PP) lives on the party [`Pokemon`]; data that resets
//! on switch-out (boosts, volatiles, substitute) lives on the [`Slot`]. poke-engine keeps the
//! latter on the side, which only works with one active Pokémon.

use crate::field::{Effect, FIELD_EFFECT_COUNT, SIDE_EFFECT_COUNT};

pub const PARTY_SIZE: usize = 6;
pub const BOOST_COUNT: usize = 7; // atk, def, spa, spd, spe, accuracy, evasion

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SideId {
    One = 0,
    Two = 1,
}

impl SideId {
    pub fn other(self) -> SideId {
        match self {
            SideId::One => SideId::Two,
            SideId::Two => SideId::One,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// An active position on the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotRef {
    pub side: SideId,
    pub slot: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Status {
    #[default]
    None,
    Burn,
    Freeze,
    Paralyze,
    Poison,
    Toxic,
    Sleep,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoveSlot {
    pub id: u16,
    pub pp: u8,
    pub disabled: bool,
}

/// Party member state that survives switching.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pokemon {
    pub species: u16,
    pub level: u8,
    pub types: [u8; 2],
    pub hp: i16,
    pub max_hp: i16,
    /// atk, def, spa, spd, spe (before boosts).
    pub stats: [i16; 5],
    pub status: Status,
    pub status_turns: i8,
    pub item: u16,
    pub ability: u16,
    pub moves: [MoveSlot; 4],
}

impl Pokemon {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

/// Active-position state that resets on switch-out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    /// Index into the side's party; `None` for an empty slot (fainted, not yet replaced).
    pub party_index: Option<u8>,
    pub boosts: [i8; BOOST_COUNT],
    /// Bitset of volatile statuses (protect, taunt, encore, ...).
    pub volatiles: u128,
    pub substitute_hp: i16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Side<const N: usize> {
    pub slots: [Slot; N],
    pub party: [Pokemon; PARTY_SIZE],
    pub effects: [Effect; SIDE_EFFECT_COUNT],
}

impl<const N: usize> Default for Side<N> {
    fn default() -> Self {
        Side {
            slots: std::array::from_fn(|_| Slot::default()),
            party: Default::default(),
            effects: [Effect::NONE; SIDE_EFFECT_COUNT],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State<const N: usize> {
    pub sides: [Side<N>; 2],
    pub field: [Effect; FIELD_EFFECT_COUNT],
    pub turn: u16,
}

impl<const N: usize> Default for State<N> {
    fn default() -> Self {
        State {
            sides: [Side::default(), Side::default()],
            field: [Effect::NONE; FIELD_EFFECT_COUNT],
            turn: 0,
        }
    }
}

impl<const N: usize> State<N> {
    pub const SLOTS: usize = N;

    pub fn side(&self, side: SideId) -> &Side<N> {
        &self.sides[side.index()]
    }

    pub fn side_mut(&mut self, side: SideId) -> &mut Side<N> {
        &mut self.sides[side.index()]
    }

    pub fn slot(&self, r: SlotRef) -> &Slot {
        &self.sides[r.side.index()].slots[r.slot as usize]
    }

    pub fn slot_mut(&mut self, r: SlotRef) -> &mut Slot {
        &mut self.sides[r.side.index()].slots[r.slot as usize]
    }

    /// The Pokémon in an active slot, if any.
    pub fn active(&self, r: SlotRef) -> Option<&Pokemon> {
        let side = &self.sides[r.side.index()];
        side.slots[r.slot as usize]
            .party_index
            .map(|i| &side.party[i as usize])
    }

    pub fn active_mut(&mut self, r: SlotRef) -> Option<&mut Pokemon> {
        let side = &mut self.sides[r.side.index()];
        let index = side.slots[r.slot as usize].party_index?;
        Some(&mut side.party[index as usize])
    }

    /// All slot references in a fixed order (side one first).
    pub fn slot_refs() -> impl Iterator<Item = SlotRef> {
        [SideId::One, SideId::Two].into_iter().flat_map(|side| {
            (0..N as u8).map(move |slot| SlotRef { side, slot })
        })
    }
}
