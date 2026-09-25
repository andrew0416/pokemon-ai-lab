//! Volatile conditions: per-slot effects that end on switch-out (Showdown `pokemon.volatiles`).
//!
//! Like field and side effects, volatiles are a table indexed by kind instead of one struct
//! field each. A variant exists only once the turn engine implements it; moves, abilities and
//! items that would create any other volatile are rejected before the turn runs.
//!
//! A few kinds are slot state Showdown keeps elsewhere (an ability's `abilityState`); they live
//! here because they reset exactly like volatiles, and [`Volatile::showdown_state`] hides them
//! from the canonical state.

use crate::dex::{conditions, ConditionId, MoveId, Type};

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
    /// Confusion: `time` turns (2–5), 33% self-hit before each move.
    Confusion,
    /// Outrage / Petal Dance / Thrash: locked into `mv` (duration 2, hidden `trueDuration`
    /// 2–3), confusion when it ends by fatigue.
    LockedMove,
    /// Hyper Beam's recharge turn (duration 2, `recharge` locks the next action).
    MustRecharge,
    /// Encore: locked into `mv` (duration 3, one more if the target already moved).
    Encore,
    /// Flash Fire's boost after absorbing a Fire move (the ability's own `condition`, no
    /// duration; `noCopy`).
    FlashFire,
    /// Choice item lock (Showdown `choicelock`, no duration): `counter` holds the locked
    /// move's `MoveId` (Showdown `effectState.move`).
    ChoiceLock,
    /// Roost: Flying is left out of the holder's types until the end of the turn (duration 1,
    /// residual order 25). Showdown filters the types in `onType`; the engine changes them with
    /// `SetTypes` and keeps the types from before in `counter` ([`encode_types`]; 0 = nothing
    /// was removed) to restore them when Roost ends.
    Roost,
    /// Yawn: the holder falls asleep when it ends (duration 2, residual order 23).
    Yawn,
    /// Perish Song's count (duration 4, residual order 24): the holder faints when it ends.
    /// Showdown adds it by name in the move's `onHitField`, so the dex has no condition id.
    PerishSong,
    /// Endure: a move's damage leaves the holder at 1 HP at least (duration 1).
    Endure,
    /// Not a Showdown volatile: Protean's / Libero's `abilityState.protean` / `.libero` flag
    /// (the type already changed since switching in). No duration; hidden in the canonical
    /// state.
    ProteanUsed,
    /// Helping Hand: the holder's moves this turn get more power (duration 1). `counter` counts
    /// the applications (Showdown keeps `multiplier` = 1.5 per application instead, which the
    /// canonical state does not print, so neither is `counter`).
    HelpingHand,
    /// Taunt: status moves can be neither chosen nor used (duration 3, one more if the holder
    /// was active since the turn started and has no move left; residual order 15).
    Taunt,
    /// Disable: `mv` (the holder's last move when it started) can be neither chosen nor used
    /// (duration 5, one less if the holder still has a move to come; residual order 17).
    Disable,
    /// Torment: the holder's last move cannot be chosen (no duration).
    Torment,
    /// Imprison, on its user: the user's foes can neither choose nor use a move the user knows
    /// (no duration).
    Imprison,
    /// Glaive Rush's drawback on its user until its next move attempt (no duration): moves
    /// against it cannot miss and deal double damage.
    GlaiveRush,
    /// Sparkling Aria's secondary effect on a target it hit (no duration): the move's
    /// `onAfterMove` removes it again, curing a burn.
    SparklingAria,
}

pub const VOLATILE_COUNT: usize = 24;

impl Volatile {
    pub const ALL: [Volatile; VOLATILE_COUNT] = [
        Volatile::Protect,
        Volatile::Stall,
        Volatile::Flinch,
        Volatile::FollowMe,
        Volatile::RagePowder,
        Volatile::Spotlight,
        Volatile::Confusion,
        Volatile::LockedMove,
        Volatile::MustRecharge,
        Volatile::Encore,
        Volatile::FlashFire,
        Volatile::ChoiceLock,
        Volatile::Roost,
        Volatile::Yawn,
        Volatile::PerishSong,
        Volatile::Endure,
        Volatile::ProteanUsed,
        Volatile::HelpingHand,
        Volatile::Taunt,
        Volatile::Disable,
        Volatile::Torment,
        Volatile::Imprison,
        Volatile::GlaiveRush,
        Volatile::SparklingAria,
    ];

    /// The Showdown condition this volatile is. `ConditionId::NONE` for a volatile that is an
    /// ability's own `condition` (Flash Fire), which the dex does not export as a named
    /// condition: no move data can refer to it.
    pub fn condition(self) -> ConditionId {
        match self {
            Volatile::Protect => conditions::PROTECT,
            Volatile::Stall => conditions::STALL,
            Volatile::Flinch => conditions::FLINCH,
            Volatile::FollowMe => conditions::FOLLOWME,
            Volatile::RagePowder => conditions::RAGEPOWDER,
            Volatile::Spotlight => conditions::SPOTLIGHT,
            Volatile::Confusion => conditions::CONFUSION,
            Volatile::LockedMove => conditions::LOCKEDMOVE,
            Volatile::MustRecharge => conditions::MUSTRECHARGE,
            Volatile::Encore => conditions::ENCORE,
            Volatile::FlashFire => ConditionId::NONE,
            Volatile::ChoiceLock => conditions::CHOICELOCK,
            Volatile::Roost => conditions::ROOST,
            Volatile::Yawn => conditions::YAWN,
            Volatile::Endure => conditions::ENDURE,
            Volatile::HelpingHand => conditions::HELPINGHAND,
            Volatile::Taunt => conditions::TAUNT,
            Volatile::Disable => conditions::DISABLE,
            Volatile::Torment => conditions::TORMENT,
            Volatile::Imprison => conditions::IMPRISON,
            Volatile::GlaiveRush => conditions::GLAIVERUSH,
            Volatile::SparklingAria => conditions::SPARKLINGARIA,
            Volatile::PerishSong | Volatile::ProteanUsed => ConditionId::NONE,
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
            Volatile::Confusion => "confusion",
            Volatile::LockedMove => "lockedmove",
            Volatile::MustRecharge => "mustrecharge",
            Volatile::Encore => "encore",
            Volatile::FlashFire => "flashfire",
            Volatile::ChoiceLock => "choicelock",
            Volatile::Roost => "roost",
            Volatile::Yawn => "yawn",
            Volatile::PerishSong => "perishsong",
            Volatile::Endure => "endure",
            Volatile::ProteanUsed => "protean",
            Volatile::HelpingHand => "helpinghand",
            Volatile::Taunt => "taunt",
            Volatile::Disable => "disable",
            Volatile::Torment => "torment",
            Volatile::Imprison => "imprison",
            Volatile::GlaiveRush => "glaiverush",
            Volatile::SparklingAria => "sparklingaria",
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
            | Volatile::Roost
            | Volatile::Endure
            | Volatile::HelpingHand => 1,
            Volatile::Stall | Volatile::LockedMove | Volatile::MustRecharge | Volatile::Yawn => 2,
            Volatile::Encore | Volatile::Taunt => 3,
            Volatile::PerishSong => 4,
            Volatile::Disable => 5,
            Volatile::Confusion
            | Volatile::FlashFire
            | Volatile::ChoiceLock
            | Volatile::ProteanUsed
            | Volatile::Torment
            | Volatile::Imprison
            | Volatile::GlaiveRush
            | Volatile::SparklingAria => 0,
        }
    }

    /// The condition's `onResidualOrder` (`None`: Showdown's default, after every ordered
    /// handler). Its duration is counted down by that residual handler.
    pub fn residual_order(self) -> Option<u32> {
        match self {
            Volatile::Taunt => Some(15),
            Volatile::Encore => Some(16),
            Volatile::Disable => Some(17),
            Volatile::Yawn => Some(23),
            Volatile::PerishSong => Some(24),
            Volatile::Roost => Some(25),
            _ => None,
        }
    }

    /// What Showdown's `pokemon.volatiles` holds for this kind: `None` for engine-only kinds,
    /// and the effect state without engine-only payload (Roost's saved types, Helping Hand's
    /// application count).
    pub fn showdown_state(self, state: VolatileState) -> Option<VolatileState> {
        match self {
            Volatile::ProteanUsed => None,
            Volatile::Roost | Volatile::HelpingHand => Some(VolatileState {
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

/// One volatile's state: Showdown's effect-state fields the canonical output writes
/// (`duration`, `counter`, `time`, `move`, 0/none = unset) plus `hidden` for state Showdown
/// keeps but does not print (a locked move's `trueDuration`; written as `trueDuration` since it
/// decides later outcomes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VolatileState {
    pub active: bool,
    pub duration: u8,
    pub counter: u16,
    pub time: u8,
    pub mv: MoveId,
    pub hidden: u8,
}

impl VolatileState {
    pub const NONE: VolatileState = VolatileState {
        active: false,
        duration: 0,
        counter: 0,
        time: 0,
        mv: MoveId::NONE,
        hidden: 0,
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
        assert_eq!(Volatile::from_condition(ConditionId::NONE), None);
        for v in Volatile::ALL {
            if v.condition().is_none() {
                // Only kinds the dex never names: an ability's own condition (Flash Fire), Perish
                // Song (added by name) and engine state.
                assert!(matches!(
                    v,
                    Volatile::FlashFire | Volatile::PerishSong | Volatile::ProteanUsed
                ));
                continue;
            }
            assert_eq!(v.condition().id(), v.id());
            assert_eq!(Volatile::from_condition(v.condition()), Some(v));
        }
        assert_eq!(Volatile::from_condition(ConditionId::NONE), None);
        assert_eq!(
            Volatile::ProteanUsed.showdown_state(VolatileState::NONE),
            None
        );
    }

    /// Durations and residual orders are the dex's (the move's `condition`).
    #[test]
    fn durations_and_residual_orders_match_the_moves() {
        for (volatile, id) in [
            (Volatile::Roost, moves::ROOST),
            (Volatile::Yawn, moves::YAWN),
            (Volatile::PerishSong, moves::PERISH_SONG),
            (Volatile::HelpingHand, moves::HELPING_HAND),
            (Volatile::Taunt, moves::TAUNT),
            (Volatile::Disable, moves::DISABLE),
        ] {
            let data = id.data();
            assert_eq!(
                data.condition_duration,
                volatile.initial_duration(),
                "{id:?}"
            );
            match volatile.residual_order() {
                Some(order) => assert!(
                    data.event_orders
                        .contains(&("condition.onResidualOrder", order as i16)),
                    "{id:?}"
                ),
                None => assert!(
                    !data
                        .event_orders
                        .iter()
                        .any(|(n, _)| *n == "condition.onResidualOrder"),
                    "{id:?}"
                ),
            }
        }
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
