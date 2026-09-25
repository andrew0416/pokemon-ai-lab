//! Volatile conditions: per-slot effects that end on switch-out (Showdown `pokemon.volatiles`).
//!
//! Like field and side effects, volatiles are a table indexed by kind instead of one struct
//! field each. A variant exists only once the turn engine implements it; moves, abilities and
//! items that would create any other volatile are rejected before the turn runs.
//!
//! A few kinds are slot state Showdown keeps elsewhere (an ability's `abilityState`); they live
//! here because they reset exactly like volatiles, and [`Volatile::showdown_state`] hides them
//! from the canonical state.

use crate::dex::{conditions, ConditionId, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Volatile {
    /// Protect's single-turn shield (Showdown `protect`, duration 1).
    Protect = 0,
    /// Consecutive-protection counter (Showdown `stall`, duration 2, `counter` 3, 9, ...).
    Stall,
    /// Flinch (duration 1).
    Flinch,
    /// Follow Me: redirects foes' single-target moves to the holder (duration 1).
    FollowMe,
    /// Rage Powder: like Follow Me, but not for powder-immune attackers (duration 1).
    RagePowder,
    /// Spotlight: like Follow Me with higher redirect priority (duration 1).
    Spotlight,
    /// Roost: Flying is left out of the holder's types until the end of the turn (duration 1,
    /// residual order 25). Showdown filters the types in `onType`; the engine changes them with
    /// `SetTypes` and keeps the types from before in `counter` ([`encode_types`]; 0 = nothing
    /// was removed) to restore them when Roost ends.
    Roost,
    /// Not a Showdown volatile: Protean's / Libero's `abilityState.protean` / `.libero` flag
    /// (the type already changed since switching in). No duration; hidden in the canonical
    /// state.
    ProteanUsed,
}

pub const VOLATILE_COUNT: usize = 8;

impl Volatile {
    pub const ALL: [Volatile; VOLATILE_COUNT] = [
        Volatile::Protect,
        Volatile::Stall,
        Volatile::Flinch,
        Volatile::FollowMe,
        Volatile::RagePowder,
        Volatile::Spotlight,
        Volatile::Roost,
        Volatile::ProteanUsed,
    ];

    /// The Showdown condition this volatile is (`NONE` for engine-only kinds).
    pub fn condition(self) -> ConditionId {
        match self {
            Volatile::Protect => conditions::PROTECT,
            Volatile::Stall => conditions::STALL,
            Volatile::Flinch => conditions::FLINCH,
            Volatile::FollowMe => conditions::FOLLOWME,
            Volatile::RagePowder => conditions::RAGEPOWDER,
            Volatile::Spotlight => conditions::SPOTLIGHT,
            Volatile::Roost => conditions::ROOST,
            Volatile::ProteanUsed => ConditionId::NONE,
        }
    }

    /// Showdown id, as written in canonical states.
    pub fn id(self) -> &'static str {
        match self {
            Volatile::Protect => "protect",
            Volatile::Stall => "stall",
            Volatile::Flinch => "flinch",
            Volatile::FollowMe => "followme",
            Volatile::RagePowder => "ragepowder",
            Volatile::Spotlight => "spotlight",
            Volatile::Roost => "roost",
            Volatile::ProteanUsed => "protean",
        }
    }

    /// The volatile implementing `condition`, if any (never for `NONE`).
    pub fn from_condition(condition: ConditionId) -> Option<Volatile> {
        if condition.is_none() {
            return None;
        }
        Volatile::ALL
            .into_iter()
            .find(|v| v.condition() == condition)
    }

    /// Duration a fresh instance starts with (0 = none).
    pub fn initial_duration(self) -> u8 {
        match self {
            Volatile::Protect
            | Volatile::Flinch
            | Volatile::FollowMe
            | Volatile::RagePowder
            | Volatile::Spotlight
            | Volatile::Roost => 1,
            Volatile::Stall => 2,
            Volatile::ProteanUsed => 0,
        }
    }

    /// The condition's `onResidualOrder` (`None`: Showdown's default, after every ordered
    /// handler). Its duration is counted down by that residual handler.
    pub fn residual_order(self) -> Option<u32> {
        match self {
            Volatile::Roost => Some(25),
            _ => None,
        }
    }

    /// What Showdown's `pokemon.volatiles` holds for this kind: `None` for engine-only kinds,
    /// and the effect state without engine-only payload (Roost's saved types).
    pub fn showdown_state(self, state: VolatileState) -> Option<VolatileState> {
        match self {
            Volatile::ProteanUsed => None,
            Volatile::Roost => Some(VolatileState {
                counter: 0,
                ..state
            }),
            _ => Some(state),
        }
    }
}

/// Two types in one `counter` (first type in the high byte).
pub fn encode_types(types: [Type; 2]) -> u16 {
    (u16::from(types[0] as u8) << 8) | u16::from(types[1] as u8)
}

/// The types [`encode_types`] stored.
pub fn decode_types(counter: u16) -> [Type; 2] {
    let decode = |v: u16| {
        Type::ALL
            .into_iter()
            .find(|&t| u16::from(t as u8) == v)
            .unwrap_or(Type::None)
    };
    [decode(counter >> 8), decode(counter & 0xff)]
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
    use crate::dex::moves;

    #[test]
    fn ids_match_the_dex_conditions() {
        for v in Volatile::ALL {
            if v.condition().is_none() {
                assert_eq!(v.showdown_state(VolatileState::NONE), None, "{v:?}");
                continue;
            }
            assert_eq!(v.condition().id(), v.id());
            assert_eq!(Volatile::from_condition(v.condition()), Some(v));
        }
        assert_eq!(Volatile::from_condition(ConditionId::NONE), None);
    }

    /// Durations and residual orders are the dex's (`condition.duration` of the move).
    #[test]
    fn durations_and_residual_orders_match_the_moves() {
        let roost = moves::ROOST.data();
        assert_eq!(roost.condition_duration, Volatile::Roost.initial_duration());
        assert!(roost
            .event_orders
            .contains(&("condition.onResidualOrder", 25)));
        assert_eq!(Volatile::Roost.residual_order(), Some(25));
    }

    #[test]
    fn types_round_trip() {
        for types in [
            [Type::Flying, Type::None],
            [Type::Normal, Type::Flying],
            [Type::Steel, Type::Flying],
            [Type::Water, Type::Stellar],
        ] {
            assert_eq!(decode_types(encode_types(types)), types);
            assert_ne!(encode_types(types), 0);
        }
    }
}
