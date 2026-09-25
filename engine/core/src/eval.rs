//! Evaluation is a trait so heuristic, linear and learned evaluators are interchangeable.
//! Scores are from side one's perspective.

use crate::state::{SideId, State};

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
