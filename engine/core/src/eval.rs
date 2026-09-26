//! Evaluation is a trait so heuristic, linear and learned evaluators are interchangeable.
//! Scores are from side one's perspective.

use crate::field::SideEffect;
use crate::state::{SideId, State, Status};
use crate::volatile::Volatile;

pub trait Evaluator<const N: usize> {
    fn evaluate(&self, state: &State<N>) -> f32;
}

/// Baseline: alive count and HP fraction only.
#[derive(Clone, Copy, Debug, Default)]
pub struct Material;

impl Material {
    const ALIVE: f32 = 30.0;
    const HP: f32 = 100.0;

    fn side_score<const N: usize>(state: &State<N>, side: SideId) -> f32 {
        state
            .side(side)
            .party
            .iter()
            .filter(|p| p.is_alive())
            .map(|p| Self::ALIVE + Self::HP * p.hp as f32 / p.max_hp.max(1) as f32)
            .sum()
    }
}

impl<const N: usize> Evaluator<N> for Material {
    fn evaluate(&self, state: &State<N>) -> f32 {
        Self::side_score(state, SideId::One) - Self::side_score(state, SideId::Two)
    }
}

/// [`Material`] plus hand-set terms for what a one-turn search cannot see the value of:
/// status conditions (sleep and freeze weigh most: the Pokémon loses turns), stat stages on
/// the active Pokémon, a few volatiles (confusion, Leech Seed, a substitute) and side
/// conditions (Tailwind, screens). The weights are a starting point in HP-percent units (100 =
/// one full HP bar), not tuned against results; DESIGN.md's plan is to replace them by a
/// learned or table-driven evaluation once the search runs on real teams.
#[derive(Clone, Copy, Debug, Default)]
pub struct Heuristic;

impl Heuristic {
    const SLEEP: f32 = 45.0;
    const FREEZE: f32 = 50.0;
    const PARALYZE: f32 = 18.0;
    const BURN: f32 = 14.0;
    const POISON: f32 = 10.0;
    const TOXIC: f32 = 16.0;
    /// Per stage of Attack, Special Attack or Speed on an active Pokémon.
    const OFFENSIVE_STAGE: f32 = 9.0;
    /// Per stage of Defense or Special Defense.
    const DEFENSIVE_STAGE: f32 = 6.0;
    /// Per stage of accuracy or evasion.
    const ACCURACY_STAGE: f32 = 4.0;
    const CONFUSION: f32 = 12.0;
    const LEECH_SEED: f32 = 10.0;
    const SUBSTITUTE: f32 = 12.0;
    const TAUNT: f32 = 6.0;
    const ENCORE: f32 = 8.0;
    const PERISH_SONG: f32 = 20.0;
    const YAWN: f32 = 25.0;
    const TAILWIND: f32 = 12.0;
    const SCREEN: f32 = 8.0;
    /// Protect (or a relative) used last turn: the `stall` counter makes the next one
    /// unreliable (1/3), so the position is tempo down. Without this a one-turn game rates a
    /// double Protect as free.
    const STALL: f32 = 10.0;

    fn side_score<const N: usize>(state: &State<N>, side: SideId) -> f32 {
        let s = state.side(side);
        let mut score = Material::side_score(state, side);
        for p in s.party.iter().filter(|p| p.is_alive()) {
            score -= match p.status {
                Status::Sleep => Self::SLEEP,
                Status::Freeze => Self::FREEZE,
                Status::Paralyze => Self::PARALYZE,
                Status::Burn => Self::BURN,
                Status::Poison => Self::POISON,
                Status::Toxic => Self::TOXIC,
                _ => 0.0,
            };
        }
        for slot in &s.slots {
            let Some(party) = slot.party_index else {
                continue;
            };
            if !s.party[party as usize].is_alive() {
                continue;
            }
            let b = &slot.boosts;
            // atk, def, spa, spd, spe, accuracy, evasion
            score += Self::OFFENSIVE_STAGE * f32::from(b[0] + b[2] + b[4])
                + Self::DEFENSIVE_STAGE * f32::from(b[1] + b[3])
                + Self::ACCURACY_STAGE * f32::from(b[5] + b[6]);
            let v = &slot.volatiles;
            if v.has(Volatile::Confusion) {
                score -= Self::CONFUSION;
            }
            if v.has(Volatile::LeechSeed) {
                score -= Self::LEECH_SEED;
            }
            if v.has(Volatile::Substitute) {
                score += Self::SUBSTITUTE;
            }
            if v.has(Volatile::Taunt) {
                score -= Self::TAUNT;
            }
            if v.has(Volatile::Encore) {
                score -= Self::ENCORE;
            }
            if v.has(Volatile::PerishSong) {
                score -= Self::PERISH_SONG;
            }
            if v.has(Volatile::Yawn) {
                score -= Self::YAWN;
            }
            if v.has(Volatile::Stall) {
                score -= Self::STALL;
            }
        }
        if s.effects[SideEffect::Tailwind as usize].is_active() {
            score += Self::TAILWIND;
        }
        for screen in [
            SideEffect::Reflect,
            SideEffect::LightScreen,
            SideEffect::AuroraVeil,
        ] {
            if s.effects[screen as usize].is_active() {
                score += Self::SCREEN;
            }
        }
        score
    }
}

impl<const N: usize> Evaluator<N> for Heuristic {
    fn evaluate(&self, state: &State<N>) -> f32 {
        Self::side_score(state, SideId::One) - Self::side_score(state, SideId::Two)
    }
}

/// The number of [`features`] terms.
pub const FEATURE_COUNT: usize = 21;

/// Names of the [`features`] terms, in order.
pub const FEATURE_NAMES: [&str; FEATURE_COUNT] = [
    "alive",
    "hp_fraction",
    "sleep",
    "freeze",
    "paralyze",
    "burn",
    "poison",
    "toxic",
    "offensive_stages",
    "defensive_stages",
    "accuracy_stages",
    "confusion",
    "leech_seed",
    "substitute",
    "taunt",
    "encore",
    "perish_song",
    "yawn",
    "stall",
    "tailwind",
    "screens",
];

/// The position as side one's counts minus side two's, one entry per [`FEATURE_NAMES`]
/// term: alive Pokémon, summed HP fractions, statused Pokémon per status, stat stages of the
/// living actives (offensive: atk + spa + spe; defensive: def + spd; accuracy + evasion),
/// volatiles of the living actives, Tailwind and screens up. [`Heuristic`] is the dot product
/// with [`Heuristic::WEIGHTS`]; [`Weighted`] uses fitted weights.
pub fn features<const N: usize>(state: &State<N>) -> [f32; FEATURE_COUNT] {
    let one = side_features(state, SideId::One);
    let two = side_features(state, SideId::Two);
    let mut out = [0.0; FEATURE_COUNT];
    for i in 0..FEATURE_COUNT {
        out[i] = one[i] - two[i];
    }
    out
}

fn side_features<const N: usize>(state: &State<N>, side: SideId) -> [f32; FEATURE_COUNT] {
    let s = state.side(side);
    let mut f = [0.0f32; FEATURE_COUNT];
    for p in s.party.iter().filter(|p| p.is_alive()) {
        f[0] += 1.0;
        f[1] += p.hp as f32 / p.max_hp.max(1) as f32;
        let i = match p.status {
            Status::Sleep => 2,
            Status::Freeze => 3,
            Status::Paralyze => 4,
            Status::Burn => 5,
            Status::Poison => 6,
            Status::Toxic => 7,
            _ => continue,
        };
        f[i] += 1.0;
    }
    for slot in &s.slots {
        let Some(party) = slot.party_index else {
            continue;
        };
        if !s.party[party as usize].is_alive() {
            continue;
        }
        let b = &slot.boosts;
        f[8] += f32::from(b[0] + b[2] + b[4]);
        f[9] += f32::from(b[1] + b[3]);
        f[10] += f32::from(b[5] + b[6]);
        let v = &slot.volatiles;
        for (i, volatile) in [
            (11, Volatile::Confusion),
            (12, Volatile::LeechSeed),
            (13, Volatile::Substitute),
            (14, Volatile::Taunt),
            (15, Volatile::Encore),
            (16, Volatile::PerishSong),
            (17, Volatile::Yawn),
            (18, Volatile::Stall),
        ] {
            if v.has(volatile) {
                f[i] += 1.0;
            }
        }
    }
    if s.effects[SideEffect::Tailwind as usize].is_active() {
        f[19] += 1.0;
    }
    for screen in [
        SideEffect::Reflect,
        SideEffect::LightScreen,
        SideEffect::AuroraVeil,
    ] {
        if s.effects[screen as usize].is_active() {
            f[20] += 1.0;
        }
    }
    f
}

impl Heuristic {
    /// The weights that make `features · WEIGHTS` equal [`Heuristic::evaluate`] (statuses
    /// and volatiles count negatively; substitute, Tailwind and screens positively).
    pub const WEIGHTS: [f32; FEATURE_COUNT] = [
        Material::ALIVE,
        Material::HP,
        -Self::SLEEP,
        -Self::FREEZE,
        -Self::PARALYZE,
        -Self::BURN,
        -Self::POISON,
        -Self::TOXIC,
        Self::OFFENSIVE_STAGE,
        Self::DEFENSIVE_STAGE,
        Self::ACCURACY_STAGE,
        -Self::CONFUSION,
        -Self::LEECH_SEED,
        Self::SUBSTITUTE,
        -Self::TAUNT,
        -Self::ENCORE,
        -Self::PERISH_SONG,
        -Self::YAWN,
        -Self::STALL,
        Self::TAILWIND,
        Self::SCREEN,
    ];
}

/// A linear evaluation over [`features`] with given weights (fitted offline, WORKPLAN S12).
#[derive(Clone, Debug, PartialEq)]
pub struct Weighted {
    pub weights: [f32; FEATURE_COUNT],
}

impl<const N: usize> Evaluator<N> for Weighted {
    fn evaluate(&self, state: &State<N>) -> f32 {
        features(state)
            .iter()
            .zip(&self.weights)
            .map(|(f, w)| f * w)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Pokemon;

    fn state() -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for i in 0..2u8 {
                let mon = &mut state.side_mut(side).party[i as usize];
                *mon = Pokemon {
                    hp: 150,
                    max_hp: 150,
                    ..Pokemon::default()
                };
                mon.species = crate::dex::species::GARDEVOIR;
                state.side_mut(side).slots[i as usize].party_index = Some(i);
            }
        }
        state
    }

    #[test]
    fn symmetric_position_scores_zero() {
        let state = state();
        assert_eq!(Material.evaluate(&state), 0.0);
        assert_eq!(Heuristic.evaluate(&state), 0.0);
    }

    /// The feature vector with the heuristic's weights reproduces the heuristic exactly.
    #[test]
    fn features_times_weights_is_the_heuristic() {
        let mut state = state();
        state.side_mut(SideId::Two).party[0].status = Status::Sleep;
        state.side_mut(SideId::Two).slots[1].boosts[0] = 2;
        state.side_mut(SideId::One).slots[0].boosts[3] = -1;
        state.side_mut(SideId::One).effects[SideEffect::Tailwind as usize] =
            crate::field::Effect { value: 0, turns: 3 };
        state.side_mut(SideId::One).party[1].hp = 40;
        state.side_mut(SideId::Two).slots[0].volatiles.set(
            Volatile::Stall,
            crate::volatile::VolatileState {
                active: true,
                counter: 3,
                ..crate::volatile::VolatileState::NONE
            },
        );
        let weighted = Weighted {
            weights: Heuristic::WEIGHTS,
        };
        assert!((weighted.evaluate(&state) - Heuristic.evaluate(&state)).abs() < 1e-3);
        assert_eq!(FEATURE_NAMES.len(), FEATURE_COUNT);
    }

    /// Sleep, boosts, volatiles and side conditions move the score the way a player would
    /// read them; Material ignores all of them.
    #[test]
    fn heuristic_sees_what_material_does_not() {
        let mut state = state();
        state.side_mut(SideId::Two).party[0].status = Status::Sleep;
        assert_eq!(Material.evaluate(&state), 0.0);
        let asleep = Heuristic.evaluate(&state);
        assert!(asleep > 0.0);
        state.side_mut(SideId::Two).slots[1].boosts[0] = 2;
        let boosted = Heuristic.evaluate(&state);
        assert!(boosted < asleep);
        state.side_mut(SideId::One).effects[SideEffect::Tailwind as usize] =
            crate::field::Effect { value: 0, turns: 3 };
        assert!(Heuristic.evaluate(&state) > boosted);
        // A fainted Pokémon's stages and status do not count.
        state.side_mut(SideId::Two).party[1].hp = 0;
        let fainted = Heuristic.evaluate(&state);
        state.side_mut(SideId::Two).slots[1].boosts[0] = 6;
        assert_eq!(Heuristic.evaluate(&state), fainted);
    }
}
