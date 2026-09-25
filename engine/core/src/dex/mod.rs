//! Static Champions dex: species, moves, items, abilities, types, natures and the named
//! conditions they refer to.
//!
//! The tables live in `generated.rs`, produced by `engine/data/gen-rust.cjs` from
//! `engine/data/champions.json` (Showdown's `champions` mod). This file defines the types
//! the tables use and the lookup API; it holds no data.
//!
//! - Ids are indices into the tables. Index 0 is the "none" entry of every table, so
//!   `Default` ids (no item, empty move slot) are valid and [`MoveId::data`] etc. never fail.
//! - Every entry has a constant named after it: [`species::GARDEVOIR_MEGA`],
//!   [`moves::HYPNOSIS`], [`conditions::GRAVITY`]. Rules code uses these, never raw numbers:
//!   indices change whenever the export changes.
//! - Behaviour Showdown implements as callbacks is not data. Each entry lists those callbacks
//!   by name in `handlers`: the hand-written rules must cover them.
//! - The roster is not limited to the current regulation (legality is a separate validator);
//!   [`Nonstandard`] is kept as data only.

use std::fmt;

use crate::state::{Status, BOOST_COUNT};

#[rustfmt::skip]
mod generated;

pub use generated::*;

/// Showdown's `toID`: lower case, ASCII letters and digits only.
pub fn to_id(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

macro_rules! dex_id {
    ($(#[$doc:meta])* $name:ident($repr:ty) => $data:ty, $table:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub $repr);

        impl $name {
            pub const NONE: $name = $name(0);

            pub const fn is_none(self) -> bool {
                self.0 == 0
            }

            pub fn data(self) -> &'static $data {
                &$table[self.0 as usize]
            }

            /// The Showdown id (`"gardevoirmega"`); empty for `NONE`.
            pub fn id(self) -> &'static str {
                self.data().id
            }

            /// Looks up a Showdown id (already normalized, see [`to_id`]).
            pub fn from_id(id: &str) -> Option<$name> {
                $table[1..]
                    .binary_search_by(|entry| entry.id.cmp(id))
                    .ok()
                    .map(|i| $name((i + 1) as $repr))
            }

            /// Looks up a display name or id in any spelling (`"Gardevoir-Mega"`).
            pub fn from_name(name: &str) -> Option<$name> {
                $name::from_id(&to_id(name))
            }

            /// Every entry except `NONE`, in id order.
            pub fn all() -> impl Iterator<Item = $name> {
                (1..$table.len()).map(|i| $name(i as $repr))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.is_none() {
                    write!(f, "{}::NONE", stringify!($name))
                } else {
                    write!(f, "{}({})", stringify!($name), self.id())
                }
            }
        }
    };
}

dex_id!(SpeciesId(u16) => SpeciesData, SPECIES);
dex_id!(MoveId(u16) => MoveData, MOVES);
dex_id!(ItemId(u16) => ItemData, ITEMS);
dex_id!(AbilityId(u16) => AbilityData, ABILITIES);
dex_id!(
    /// A named condition data refers to: status, volatile, side, slot or field condition,
    /// weather or terrain. Which state it maps to is the rules' business.
    ConditionId(u8) => ConditionData, CONDITIONS
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Stat {
    Hp = 0,
    Atk,
    Def,
    Spa,
    Spd,
    Spe,
}

/// Stage changes in `state::Slot::boosts` order: atk, def, spa, spd, spe, accuracy, evasion.
pub type Boosts = [i8; BOOST_COUNT];

/// Showdown's `isNonstandard`. Data only; the engine never filters on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Nonstandard {
    #[default]
    None,
    Past,
    Future,
    Lgpe,
    Gmax,
    Cap,
    Custom,
    Unobtainable,
}

/// How an attacking type fares against one defending type (Showdown `damageTaken`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeRelation {
    Neutral,
    Super,
    Resist,
    Immune,
}

/// The neutral nature, for states built without set data.
impl Default for Nature {
    fn default() -> Nature {
        Nature::Hardy
    }
}

impl Type {
    /// Relation of an attacking type to one defending type. `Type::None` on either side is
    /// neutral.
    pub fn against(self, defending: Type) -> TypeRelation {
        TYPE_CHART[defending as usize][self as usize]
    }

    pub fn immunities(self) -> TypeImmunities {
        TYPE_IMMUNITIES[self as usize]
    }
}

/// Non-type immunities a type grants (constants are generated from the type chart).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TypeImmunities(pub u16);

impl TypeImmunities {
    pub const EMPTY: TypeImmunities = TypeImmunities(0);

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, other: TypeImmunities) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Showdown move flags (constants are generated from the flags present in the data).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveFlags(pub u64);

impl MoveFlags {
    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: MoveFlags) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Showdown ability flags (constants are generated from the flags present in the data).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilityFlags(pub u16);

impl AbilityFlags {
    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, other: AbilityFlags) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Gender {
    /// Chosen by the team (or at random); the species has a gender ratio.
    #[default]
    Random,
    Male,
    Female,
    Genderless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveCategory {
    Physical,
    Special,
    Status,
}

/// Showdown move targets. Showdown's `self` is [`MoveTarget::User`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveTarget {
    Normal,
    Any,
    AdjacentAlly,
    AdjacentAllyOrSelf,
    AdjacentFoe,
    AllAdjacent,
    AllAdjacentFoes,
    Allies,
    AllySide,
    AllyTeam,
    All,
    FoeSide,
    RandomNormal,
    Scripted,
    User,
}

/// `numerator / denominator` of damage dealt (drain, recoil) or max HP (heal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fraction(pub u8, pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixedDamage {
    /// The user's level (Seismic Toss, Night Shade).
    Level,
    Hp(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ohko {
    No,
    Any,
    /// Fails against this type and is less accurate for users of other types (Sheer Cold).
    Typed(Type),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IgnoreImmunity {
    No,
    All,
    /// Ignores the immunity of one defending type (Thousand Arrows: Ground vs Flying).
    Type(Type),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelfSwitch {
    No,
    Yes,
    /// Baton Pass.
    CopyVolatile,
    /// Shed Tail.
    ShedTail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelfDestruct {
    No,
    Always,
    IfHit,
}

/// One secondary effect roll (Showdown `secondaries[i]`). Effects Showdown implements as
/// callbacks (`secondary.onHit`) show up in the move's `handlers`, not here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Secondary {
    /// Percent.
    pub chance: u8,
    pub status: Status,
    pub volatile_status: ConditionId,
    pub boosts: Boosts,
    pub self_boosts: Boosts,
}

/// Effects on the user (Showdown `self`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelfEffect {
    /// Percent.
    pub chance: u8,
    pub boosts: Boosts,
    pub volatile_status: ConditionId,
    pub side_condition: ConditionId,
    pub pseudo_weather: ConditionId,
}

/// Z-Move conversion of a move. Kept although Champions M-C disables Z-Moves: gimmicks are
/// switched off by the ruleset, not removed from the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZMoveData {
    pub base_power: u8,
    pub boosts: Boosts,
    /// Showdown's `zMove.effect` (`"heal"`, `"clearnegativeboost"`, ...); empty for none.
    pub effect: &'static str,
}

impl ZMoveData {
    pub const NONE: ZMoveData = ZMoveData {
        base_power: 0,
        boosts: NO_BOOSTS,
        effect: "",
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fling {
    pub base_power: u8,
    pub status: Status,
    pub volatile_status: ConditionId,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZCrystal {
    /// The species-specific Z-Move; `NONE` for type Z-Crystals.
    pub move_id: MoveId,
    /// The move a species-specific Z-Move upgrades.
    pub from: MoveId,
    pub move_type: Type,
}

#[derive(Debug, PartialEq)]
pub struct SpeciesData {
    pub id: &'static str,
    pub name: &'static str,
    pub num: i16,
    pub nonstandard: Nonstandard,
    pub base_species: SpeciesId,
    pub forme: &'static str,
    pub types: [Type; 2],
    /// hp, atk, def, spa, spd, spe.
    pub base_stats: [u8; 6],
    /// Ability slots 0, 1, H, S.
    pub abilities: [AbilityId; 4],
    pub weight_hg: u16,
    pub gender: Gender,
    /// Not fully evolved (Eviolite).
    pub nfe: bool,
    pub is_mega: bool,
    pub is_primal: bool,
    /// The G-Max move; `NONE` if the species cannot Gigantamax.
    pub gigantamax_move: MoveId,
    pub cannot_dynamax: bool,
    /// Forms this in-battle form reverts to.
    pub battle_only: &'static [SpeciesId],
    pub changes_from: SpeciesId,
    pub required_items: &'static [ItemId],
    pub required_ability: AbilityId,
    pub required_move: MoveId,
    pub required_tera_type: Type,
    /// Fixed max HP (Shedinja: 1); 0 when the stat formula applies.
    pub fixed_max_hp: u16,
    pub event_orders: &'static [(&'static str, i16)],
    pub handlers: &'static [&'static str],
}

#[derive(Debug, PartialEq)]
pub struct MoveData {
    pub id: &'static str,
    pub name: &'static str,
    pub num: i16,
    pub nonstandard: Nonstandard,
    pub move_type: Type,
    pub category: MoveCategory,
    pub base_power: u8,
    /// Percent; `None` never misses (Showdown `accuracy: true`).
    pub accuracy: Option<u8>,
    /// Base PP (Champions values).
    pub pp: u8,
    pub priority: i8,
    pub target: MoveTarget,
    /// Target when used by a non-Ghost (Curse).
    pub non_ghost_target: Option<MoveTarget>,
    pub flags: MoveFlags,
    pub crit_ratio: u8,
    pub will_crit: bool,
    /// (min, max) hits.
    pub multihit: Option<(u8, u8)>,
    /// Each hit checks accuracy (Triple Axel).
    pub multiaccuracy: bool,
    pub drain: Option<Fraction>,
    pub recoil: Option<Fraction>,
    pub heal: Option<Fraction>,
    pub fixed_damage: Option<FixedDamage>,
    pub ohko: Ohko,
    pub ignore_immunity: IgnoreImmunity,
    /// Primary status inflicted on the target.
    pub status: Status,
    pub volatile_status: ConditionId,
    pub side_condition: ConditionId,
    pub slot_condition: ConditionId,
    pub pseudo_weather: ConditionId,
    pub weather: ConditionId,
    pub terrain: ConditionId,
    /// Primary stage changes on the target.
    pub boosts: Boosts,
    pub self_effect: Option<SelfEffect>,
    /// Stage changes on the user after the move (Showdown `selfBoost`).
    pub self_boost: Boosts,
    pub secondaries: &'static [Secondary],
    /// Duration of the condition the move creates (0 = none or set by a callback).
    pub condition_duration: u8,
    /// Showdown's `condition.counterMax` (stall counter cap).
    pub condition_counter_max: u16,
    /// The condition locks the user into this move (Rollout, Uproar, ...).
    pub condition_locks_move: bool,
    /// The condition suppresses the semi-invulnerable check (Shadow Force, Phantom Force).
    pub condition_no_invulnerability: bool,
    /// The condition prevents critical hits (Lucky Chant).
    pub condition_blocks_crits: bool,
    pub self_switch: SelfSwitch,
    pub selfdestruct: SelfDestruct,
    /// Uses the target's attacking stat (Foul Play).
    pub override_offensive_pokemon_target: bool,
    pub override_offensive_stat: Option<Stat>,
    pub override_defensive_stat: Option<Stat>,
    pub z_move: ZMoveData,
    pub max_move_power: u8,
    pub force_switch: bool,
    pub breaks_protect: bool,
    pub stalling_move: bool,
    pub thaws_target: bool,
    pub tracks_target: bool,
    pub smart_target: bool,
    pub sleep_usable: bool,
    pub steals_boosts: bool,
    pub calls_move: bool,
    pub has_crash_damage: bool,
    pub mind_blown_recoil: bool,
    pub struggle_recoil: bool,
    pub chloroblast_recoil: bool,
    pub ignore_ability: bool,
    pub ignore_evasion: bool,
    pub ignore_defensive: bool,
    pub ignore_offensive: bool,
    pub ignore_negative_offensive: bool,
    pub ignore_positive_defensive: bool,
    pub has_sheer_force_boost: bool,
    pub force_stab: bool,
    pub no_pp_boosts: bool,
    pub is_z: bool,
    pub is_max: bool,
    /// Showdown event ordering constants (`condition.` prefix for the move's condition).
    pub event_orders: &'static [(&'static str, i16)],
    pub handlers: &'static [&'static str],
}

#[derive(Debug, PartialEq)]
pub struct ItemData {
    pub id: &'static str,
    pub name: &'static str,
    pub num: i16,
    pub nonstandard: Nonstandard,
    pub fling: Option<Fling>,
    pub is_berry: bool,
    pub is_gem: bool,
    pub is_choice: bool,
    pub is_pokeball: bool,
    pub is_primal_orb: bool,
    pub ignore_klutz: bool,
    /// Cannot be removed or swapped (Showdown `onTakeItem: false`).
    pub cannot_be_taken: bool,
    /// Showdown `onEat: false`.
    pub no_eat_effect: bool,
    /// Showdown `onNegateImmunity: false`.
    pub no_negate_immunity: bool,
    /// Fractional priority in tenths (Lagging Tail: -1).
    pub fractional_priority_tenths: i8,
    /// (base form, Mega form) pairs this stone enables.
    pub mega_stone: &'static [(SpeciesId, SpeciesId)],
    pub item_users: &'static [SpeciesId],
    pub forced_forme: SpeciesId,
    pub plate_type: Type,
    pub memory_type: Type,
    pub drive_type: Type,
    pub z_crystal: Option<ZCrystal>,
    /// Stage changes when consumed (seeds, Room Service, ...).
    pub boosts: Boosts,
    /// (base power, type).
    pub natural_gift: Option<(u8, Type)>,
    pub condition_duration: u8,
    pub event_orders: &'static [(&'static str, i16)],
    pub handlers: &'static [&'static str],
}

#[derive(Debug, PartialEq)]
pub struct AbilityData {
    pub id: &'static str,
    pub name: &'static str,
    pub num: i16,
    pub nonstandard: Nonstandard,
    pub flags: AbilityFlags,
    pub suppress_weather: bool,
    /// Showdown `onCriticalHit: false` (Battle Armor, Shell Armor).
    pub cannot_be_crit: bool,
    /// Fractional priority in tenths (Stall: -1).
    pub fractional_priority_tenths: i8,
    pub event_orders: &'static [(&'static str, i16)],
    pub handlers: &'static [&'static str],
}

#[derive(Debug, PartialEq)]
pub struct ConditionData {
    pub id: &'static str,
    /// Whether the export has this condition's data. Conditions only referenced by name
    /// (most volatiles) have their behaviour in the referring move's handlers.
    pub exported: bool,
    pub duration: u8,
    /// Showdown `counterMax` (Protect's stall counter cap).
    pub counter_max: u16,
    pub event_orders: &'static [(&'static str, i16)],
    pub handlers: &'static [&'static str],
}

#[cfg(test)]
mod tests;
