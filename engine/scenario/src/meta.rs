//! Sidecar identity data kept next to (never inside) the search `State`.
//!
//! The canonical state (`engine/oracle/canonical.cjs`, schema 1) keys Pokémon by display name
//! and sorts them by it. Everything else it prints is derivable from the `State` and the dex:
//! `species` from `SpeciesId::data().name`, `item`/`ability` and the `pp` keys from the
//! Showdown ids (`ItemId::id()`, ...), `slot` from `Slot::party_index`. So the sidecar only has
//! to map party indices to names, plus set data the hot state does not carry (nature and
//! SP are in `Pokemon` since forme changes recalculate stats from them).

use lab_engine::dex::{AbilityId, Gender, SpeciesId, Type};

use crate::json::TurnJson;

/// One party member. Index in [`SideMeta::members`] = index in `Side::party`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberMeta {
    /// Unique per side; at most 20 characters, as Showdown truncates it.
    pub name: String,
    /// 0-based position in the team JSON (party order is the team preview order).
    pub team_index: u8,
    pub gender: Gender,
    /// `Type::None` when not given. Not an eligibility: Tera is locked under Champions M-C
    /// and `Pokemon` has no Tera field yet.
    pub tera_type: Type,
    /// The set's species and ability as loaded (before any forme change, Transform or ability
    /// change in battle): what [`crate::from_canonical`] derives a member's stats and base
    /// ability from.
    pub species: SpeciesId,
    pub ability: AbilityId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SideMeta {
    /// Party order. Entries past the end of this list are empty `Pokemon::default()` slots.
    pub members: Vec<MemberMeta>,
}

impl SideMeta {
    pub fn party_index(&self, name: &str) -> Option<u8> {
        self.members
            .iter()
            .position(|m| m.name == name)
            .map(|i| i as u8)
    }

    pub fn name(&self, party_index: u8) -> Option<&str> {
        self.members
            .get(party_index as usize)
            .map(|m| m.name.as_str())
    }

    /// Party indices in canonical order (by name, as `canonical.cjs` sorts `side.pokemon`).
    /// JavaScript's `<` compares UTF-16 code units, so this does too (it differs from code
    /// point order only for names mixing supplementary-plane and U+E000..U+FFFF characters).
    pub fn canonical_order(&self) -> Vec<u8> {
        let mut order: Vec<u8> = (0..self.members.len() as u8).collect();
        order.sort_by(|&a, &b| {
            let a = self.members[a as usize].name.encode_utf16();
            let b = self.members[b as usize].name.encode_utf16();
            a.cmp(b)
        });
        order
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioMeta {
    pub format: String,
    pub description: String,
    /// Indexed by `SideId::index()`.
    pub sides: [SideMeta; 2],
    /// The checked decision, verbatim (not parsed yet).
    pub turn: Option<TurnJson>,
}
