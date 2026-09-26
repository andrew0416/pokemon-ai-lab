//! Field-wide and side-wide effects as one table indexed by kind, instead of one struct field
//! per effect. Adding an effect (e.g. Gravity) means adding an enum variant, not new
//! State fields, instructions and serialization code.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FieldEffect {
    /// `value` holds the [`Weather`] kind.
    Weather = 0,
    /// `value` holds the [`Terrain`] kind.
    Terrain,
    TrickRoom,
    Gravity,
    MagicRoom,
    WonderRoom,
}

pub const FIELD_EFFECT_COUNT: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SideEffect {
    Reflect = 0,
    LightScreen,
    AuroraVeil,
    Tailwind,
    Safeguard,
    Mist,
    /// Hazards use [`Effect::PERMANENT`]; `value` is the layer count where it applies.
    StealthRock,
    Spikes,
    ToxicSpikes,
    StickyWeb,
    /// Single-turn protections (doubles).
    WideGuard,
    QuickGuard,
    /// No critical hits against the side.
    LuckyChant,
    /// Single-turn protections: Crafty Shield blocks status moves, Mat Block damaging ones.
    CraftyShield,
    MatBlock,
}

pub const SIDE_EFFECT_COUNT: usize = 15;

/// Slot conditions (Showdown `side.slotConditions[position]`, WORKPLAN F12): they belong to the
/// position and outlast the Pokémon that stood there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotCondition {
    /// Wish: heals half the wisher's max HP at the residual of the next turn.
    Wish = 0,
    /// Healing Wish: fully heals the next Pokémon to switch into the slot that needs it.
    HealingWish,
    /// Revival Blessing: the user must pick a fainted party member to revive (a mid-turn
    /// decision); duration 1.
    RevivalBlessing,
    /// Future Sight / Doom Desire (`futuremove`): the stored move hits whoever holds the
    /// position at the residual of the turn after next.
    FutureMove,
}

pub const SLOT_CONDITION_COUNT: usize = 4;

impl SlotCondition {
    pub const ALL: [SlotCondition; SLOT_CONDITION_COUNT] = [
        SlotCondition::Wish,
        SlotCondition::HealingWish,
        SlotCondition::RevivalBlessing,
        SlotCondition::FutureMove,
    ];

    /// Showdown's condition id.
    pub fn id(self) -> &'static str {
        match self {
            SlotCondition::Wish => "wish",
            SlotCondition::HealingWish => "healingwish",
            SlotCondition::RevivalBlessing => "revivalblessing",
            SlotCondition::FutureMove => "futuremove",
        }
    }
}

/// `SlotEffect::value` bit of a `futuremove` condition holding Doom Desire (Future Sight
/// otherwise); the low bits are the user (`volatile::encode_pokemon`).
pub const FUTURE_MOVE_DOOM_DESIRE: u16 = 0x8000;

/// A slot condition's state: `value` is 0 while the condition is absent; Wish keeps the
/// wisher's max HP there (it heals half of it, `effectState.hp`) and its starting turn in
/// `turn` (`startingTurn`); Healing Wish keeps 1; Revival Blessing keeps 1 and its duration in
/// `turn`; a future move keeps its user (`volatile::encode_pokemon`, plus
/// [`FUTURE_MOVE_DOOM_DESIRE`] for Doom Desire) and the turn it was used in `turn` (it hits at
/// the residual of that turn + 2: `endingTurn`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SlotEffect {
    pub value: u16,
    pub turn: u16,
}

impl SlotEffect {
    pub const NONE: SlotEffect = SlotEffect { value: 0, turn: 0 };

    pub fn is_active(self) -> bool {
        self.value != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Weather {
    None = 0,
    Sun,
    Rain,
    Sand,
    Snow,
    HarshSun,
    HeavyRain,
    StrongWinds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Terrain {
    None = 0,
    Electric,
    Grassy,
    Misty,
    Psychic,
}

/// A kind-specific value plus remaining turns. `turns == 0` means inactive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Effect {
    pub value: u8,
    pub turns: u8,
}

impl Effect {
    pub const NONE: Effect = Effect { value: 0, turns: 0 };
    /// Lasts until removed (hazards, ability-set weather in some formats).
    pub const PERMANENT: u8 = u8::MAX;

    pub fn is_active(self) -> bool {
        self.turns > 0
    }
}
