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
}

pub const SIDE_EFFECT_COUNT: usize = 13;

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
