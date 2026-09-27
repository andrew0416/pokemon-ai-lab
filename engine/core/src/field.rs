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
    /// Fairy Lock (`fairylock`, duration 2): every active Pokémon is trapped (`onTrapPokemon`:
    /// `tryTrap`) at the next choice.
    FairyLock,
}

pub const FIELD_EFFECT_COUNT: usize = 7;

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

/// The entry hazards, in the index order of [`HazardOrder`].
pub const HAZARDS: [SideEffect; 4] = [
    SideEffect::StealthRock,
    SideEffect::Spikes,
    SideEffect::ToxicSpikes,
    SideEffect::StickyWeb,
];

/// The order a side's entry hazards were set in (Showdown's side-condition
/// `effectState.effectOrder`): their `onSwitchIn` handlers tie on everything else, so a
/// newcomer meets them in that order. Two bits per hazard of [`HAZARDS`], holding its rank
/// among the side's active hazards (0 = first set); an inactive hazard holds 0 and the active
/// ones are numbered densely, so equal orders compare equal. Hidden from the canonical output
/// (`SideHistory::hazard_order`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HazardOrder(pub u8);

impl HazardOrder {
    fn index(effect: SideEffect) -> Option<usize> {
        HAZARDS.iter().position(|&h| h == effect)
    }

    /// The rank of `effect` (meaningful while it is active).
    pub fn rank(self, effect: SideEffect) -> u8 {
        Self::index(effect).map_or(0, |i| (self.0 >> (2 * i)) & 3)
    }

    fn with_rank(self, i: usize, rank: u8) -> HazardOrder {
        HazardOrder((self.0 & !(3 << (2 * i))) | ((rank & 3) << (2 * i)))
    }

    /// The order after `effect` turned active (`added`) or inactive on a side whose effects were
    /// `effects` before the change: a new hazard comes after the others; a removed one leaves
    /// its rank and the later ones move up. Not a hazard: unchanged.
    pub fn changed(self, effects: &[Effect], effect: SideEffect, added: bool) -> HazardOrder {
        let Some(i) = Self::index(effect) else {
            return self;
        };
        let active = |h: SideEffect| effects[h as usize].is_active();
        if added {
            let others = HAZARDS
                .iter()
                .filter(|&&h| h != effect && active(h))
                .count() as u8;
            return self.with_rank(i, others);
        }
        let removed = self.rank(effect);
        let mut out = self.with_rank(i, 0);
        for (j, &h) in HAZARDS.iter().enumerate() {
            if h != effect && active(h) && self.rank(h) > removed {
                out = out.with_rank(j, self.rank(h) - 1);
            }
        }
        out
    }

    /// The active hazards of `effects`, in the order they were set.
    pub fn sorted(self, effects: &[Effect]) -> Vec<SideEffect> {
        let mut present: Vec<SideEffect> = HAZARDS
            .into_iter()
            .filter(|&h| effects[h as usize].is_active())
            .collect();
        present.sort_by_key(|&h| self.rank(h));
        present
    }
}

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
