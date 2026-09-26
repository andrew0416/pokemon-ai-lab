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
