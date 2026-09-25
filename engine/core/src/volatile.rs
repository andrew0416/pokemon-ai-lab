//! Volatile conditions: per-slot effects that end on switch-out (Showdown `pokemon.volatiles`).
//!
//! Like field and side effects, volatiles are a table indexed by kind instead of one struct
//! field each. A variant exists only once the turn engine implements it; moves, abilities and
//! items that would create any other volatile are rejected before the turn runs.

use crate::dex::{conditions, ConditionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Volatile {
    /// Protect's single-turn shield (Showdown `protect`, duration 1).
    Protect = 0,
    /// Consecutive-protection counter (Showdown `stall`, duration 2, `counter` 3, 9, ...).
    Stall,
    /// Flinch (duration 1).
    Flinch,
}

pub const VOLATILE_COUNT: usize = 3;

impl Volatile {
    pub const ALL: [Volatile; VOLATILE_COUNT] =
        [Volatile::Protect, Volatile::Stall, Volatile::Flinch];

    /// The Showdown condition this volatile is.
    pub fn condition(self) -> ConditionId {
        match self {
            Volatile::Protect => conditions::PROTECT,
            Volatile::Stall => conditions::STALL,
            Volatile::Flinch => conditions::FLINCH,
        }
    }

    /// Showdown id, as written in canonical states.
    pub fn id(self) -> &'static str {
        match self {
            Volatile::Protect => "protect",
            Volatile::Stall => "stall",
            Volatile::Flinch => "flinch",
        }
    }

    /// The volatile implementing `condition`, if any.
    pub fn from_condition(condition: ConditionId) -> Option<Volatile> {
        Volatile::ALL
            .into_iter()
            .find(|v| v.condition() == condition)
    }

    /// Duration a fresh instance starts with (0 = none).
    pub fn initial_duration(self) -> u8 {
        match self {
            Volatile::Protect | Volatile::Flinch => 1,
            Volatile::Stall => 2,
        }
    }
}

/// One volatile's state. `duration` is Showdown's remaining `duration` (0 = no duration);
/// `counter` is Showdown's `counter` (0 = unset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VolatileState {
    pub active: bool,
    pub duration: u8,
    pub counter: u16,
}

impl VolatileState {
    pub const NONE: VolatileState = VolatileState {
        active: false,
        duration: 0,
        counter: 0,
    };
}

/// All volatiles of one slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Volatiles(pub [VolatileState; VOLATILE_COUNT]);

impl Volatiles {
    pub fn get(&self, volatile: Volatile) -> VolatileState {
        self.0[volatile as usize]
    }

    pub fn set(&mut self, volatile: Volatile, state: VolatileState) {
        self.0[volatile as usize] = state;
    }

    pub fn has(&self, volatile: Volatile) -> bool {
        self.0[volatile as usize].active
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|v| !v.active)
    }

    /// Active volatiles with their state, in [`Volatile::ALL`] order.
    pub fn iter(&self) -> impl Iterator<Item = (Volatile, VolatileState)> + '_ {
        Volatile::ALL
            .into_iter()
            .map(|v| (v, self.get(v)))
            .filter(|(_, s)| s.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_dex_conditions() {
        for v in Volatile::ALL {
            assert_eq!(v.condition().id(), v.id());
            assert_eq!(Volatile::from_condition(v.condition()), Some(v));
        }
    }
}
