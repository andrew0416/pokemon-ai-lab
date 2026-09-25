//! Selectable battle gimmicks (activation modes) and their compact bitsets.
//!
//! Every activation mode the engine may ever support stays in [`Gimmick`], even when the
//! current ruleset disables it: Champions M-C allows only Mega Evolution, and the others are
//! switched off by [`crate::rules::Ruleset`], not removed from the engine. This keeps action
//! encoding, search statistics and serialized data stable when a ruleset enables more.
//!
//! Not every form change is a gimmick. Primal Reversion (and similar ability/item triggers)
//! happens automatically on switch-in, is never chosen, and uses no per-side budget, so it
//! belongs to the form-change mechanics of the turn engine, not here.

use crate::dex::{ItemId, SpeciesId};

/// What a move action activates along with the move. At most one per slot action.
///
/// Gigantamax is not a separate mode: choosing [`Gimmick::Dynamax`] on a Pokémon with the
/// Gigantamax factor produces [`DynamaxState::Gigantamax`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Gimmick {
    #[default]
    None = 0,
    Mega,
    UltraBurst,
    ZMove,
    Dynamax,
    Tera,
}

impl Gimmick {
    /// Every activation mode (everything except [`Gimmick::None`]), in bit order.
    pub const ACTIVATIONS: [Gimmick; 5] = [
        Gimmick::Mega,
        Gimmick::UltraBurst,
        Gimmick::ZMove,
        Gimmick::Dynamax,
        Gimmick::Tera,
    ];

    pub const fn is_none(self) -> bool {
        matches!(self, Gimmick::None)
    }
}

/// A set of activation modes in one byte. Used for ruleset capabilities, a Pokémon's
/// eligibility, and a side's once-per-battle usage. [`Gimmick::None`] is never a member.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GimmickSet(u8);

impl GimmickSet {
    pub const EMPTY: GimmickSet = GimmickSet(0);
    pub const MEGA: GimmickSet = GimmickSet::of(Gimmick::Mega);
    pub const ALL: GimmickSet = GimmickSet((1 << Gimmick::ACTIVATIONS.len()) - 1);

    /// The singleton set for `gimmick`; empty for [`Gimmick::None`].
    pub const fn of(gimmick: Gimmick) -> GimmickSet {
        match gimmick {
            Gimmick::None => GimmickSet::EMPTY,
            g => GimmickSet(1 << (g as u8 - 1)),
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Always false for [`Gimmick::None`].
    pub const fn contains(self, gimmick: Gimmick) -> bool {
        self.0 & GimmickSet::of(gimmick).0 != 0
    }

    pub const fn with(self, gimmick: Gimmick) -> GimmickSet {
        GimmickSet(self.0 | GimmickSet::of(gimmick).0)
    }

    pub const fn without(self, gimmick: Gimmick) -> GimmickSet {
        GimmickSet(self.0 & !GimmickSet::of(gimmick).0)
    }

    pub const fn union(self, other: GimmickSet) -> GimmickSet {
        GimmickSet(self.0 | other.0)
    }

    pub const fn intersection(self, other: GimmickSet) -> GimmickSet {
        GimmickSet(self.0 & other.0)
    }

    pub const fn difference(self, other: GimmickSet) -> GimmickSet {
        GimmickSet(self.0 & !other.0)
    }

    /// Members in bit order.
    pub fn iter(self) -> impl Iterator<Item = Gimmick> {
        Gimmick::ACTIVATIONS
            .into_iter()
            .filter(move |&g| self.contains(g))
    }
}

impl FromIterator<Gimmick> for GimmickSet {
    fn from_iter<I: IntoIterator<Item = Gimmick>>(iter: I) -> Self {
        iter.into_iter().fold(GimmickSet::EMPTY, GimmickSet::with)
    }
}

/// The Mega form `species` reaches holding `item`, as Champions decides it: the stone's
/// `megaStone` table keyed by the exact species (so `Gardevoir-Mega` holding Gardevoirite has
/// none). Showdown's Mega Rayquaza path (Dragon Ascent, no stone) needs a past/future tag rule
/// that the Champions formats do not have, so it is not modelled.
pub fn mega_evolution(species: SpeciesId, item: ItemId) -> Option<SpeciesId> {
    item.data()
        .mega_stone
        .iter()
        .find(|&&(base, _)| base == species)
        .map(|&(_, mega)| mega)
}

/// Activation modes an individual is structurally eligible for from its species and held
/// item. Only Mega Evolution is derived for now. Eligibility is not permission: the ruleset
/// still decides ([`crate::rules::Ruleset::available_gimmicks`]), so deriving more modes here
/// later does not unlock them under Champions M-C.
pub fn structural_gimmicks(species: SpeciesId, item: ItemId) -> GimmickSet {
    if mega_evolution(species, item).is_some() {
        GimmickSet::MEGA
    } else {
        GimmickSet::EMPTY
    }
}

/// Dynamax state of an active slot (it ends on switch-out). Gigantamax is Dynamax with a
/// G-Max form, so both share this state and the Dynamax budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DynamaxState {
    #[default]
    None,
    Dynamax {
        turns: u8,
    },
    Gigantamax {
        turns: u8,
    },
}

impl DynamaxState {
    pub const fn is_active(self) -> bool {
        !matches!(self, DynamaxState::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_are_one_byte_and_exclude_none() {
        assert_eq!(std::mem::size_of::<GimmickSet>(), 1);
        assert_eq!(std::mem::size_of::<Gimmick>(), 1);
        assert_eq!(std::mem::size_of::<DynamaxState>(), 2);
        assert!(GimmickSet::of(Gimmick::None).is_empty());
        assert!(!GimmickSet::ALL.contains(Gimmick::None));
        assert_eq!(GimmickSet::ALL.iter().count(), Gimmick::ACTIVATIONS.len());
        for g in Gimmick::ACTIVATIONS {
            assert!(GimmickSet::ALL.contains(g));
            assert_eq!(GimmickSet::of(g).iter().collect::<Vec<_>>(), vec![g]);
        }
        let set: GimmickSet = [Gimmick::Mega, Gimmick::Tera].into_iter().collect();
        assert_eq!(set.without(Gimmick::Tera), GimmickSet::MEGA);
        assert_eq!(
            set.difference(GimmickSet::MEGA),
            GimmickSet::of(Gimmick::Tera)
        );
    }

    #[test]
    fn mega_eligibility_comes_from_the_stone_and_exact_species() {
        use crate::dex::{items, species};
        assert_eq!(
            mega_evolution(species::GARDEVOIR, items::GARDEVOIRITE),
            Some(species::GARDEVOIR_MEGA)
        );
        assert_eq!(
            mega_evolution(species::CHARIZARD, items::CHARIZARDITE_Y),
            Some(species::CHARIZARD_MEGA_Y)
        );
        assert_eq!(mega_evolution(species::GARDEVOIR, items::LEFTOVERS), None);
        assert_eq!(mega_evolution(species::GARDEVOIR, ItemId::NONE), None);
        assert_eq!(
            mega_evolution(species::GARDEVOIR_MEGA, items::GARDEVOIRITE),
            None
        );
        assert_eq!(
            mega_evolution(species::CHARIZARD, items::GARDEVOIRITE),
            None
        );
        assert_eq!(
            structural_gimmicks(species::GARDEVOIR, items::GARDEVOIRITE),
            GimmickSet::MEGA
        );
        assert!(structural_gimmicks(species::GARDEVOIR, items::LEFTOVERS).is_empty());
    }
}
