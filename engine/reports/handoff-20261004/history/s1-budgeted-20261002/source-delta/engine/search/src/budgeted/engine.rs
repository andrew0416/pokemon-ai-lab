use super::{Domain, Phase};
use crate::{decision, legal_choices, transitions, Choice, Decision, Pruning, WIN};
use lab_engine::eval::Evaluator;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{EnumerateOptions, Suspension};

/// Full-information input. A suspension may contain committed actions that would be
/// private in a live battle: this adapter deliberately does not implement a belief model.
#[derive(Clone, Debug)]
pub struct Position<const N: usize> {
    pub state: State<N>,
    pub suspension: Option<Suspension>,
}

/// The same Evaluator used by the existing solver; no learned model is required.
pub struct EngineDomain<'a, const N: usize, E: Evaluator<N> + ?Sized> {
    pub ruleset: Ruleset,
    pub options: EnumerateOptions,
    pub pruning: Pruning,
    pub us: SideId,
    pub evaluator: &'a E,
}

impl<const N: usize, E: Evaluator<N> + ?Sized> Domain for EngineDomain<'_, N, E> {
    type Position = Position<N>;
    type Action = Choice<N>;

    fn phase(&self, position: &Self::Position) -> Result<Phase, String> {
        Ok(
            match decision(&position.state, position.suspension.as_ref())
                .map_err(|e| e.to_string())?
            {
                Decision::Over(_) => Phase::Terminal,
                Decision::Turn => Phase::Turn,
                Decision::Replacement | Decision::MidTurn => Phase::Switch,
            },
        )
    }

    fn actions(&self, position: &Self::Position, player: usize) -> Result<Vec<Choice<N>>, String> {
        let side = if player == 0 {
            self.us
        } else {
            self.us.other()
        };
        let d =
            decision(&position.state, position.suspension.as_ref()).map_err(|e| e.to_string())?;
        Ok(legal_choices(
            &position.state,
            self.ruleset,
            d,
            side,
            self.pruning,
        ))
    }

    fn value(&self, position: &Self::Position) -> f32 {
        match position.state.result {
            BattleResult::Win(side) => {
                if side == self.us {
                    WIN
                } else {
                    -WIN
                }
            }
            BattleResult::Tie => 0.0,
            BattleResult::Ongoing => {
                let value = self.evaluator.evaluate(&position.state);
                if self.us == SideId::One {
                    value
                } else {
                    -value
                }
            }
        }
    }

    fn transitions(
        &self,
        position: &Self::Position,
        actions: [&Choice<N>; 2],
    ) -> Result<Vec<(f64, Self::Position)>, String> {
        let d =
            decision(&position.state, position.suspension.as_ref()).map_err(|e| e.to_string())?;
        let pair = if self.us == SideId::One {
            [*actions[0], *actions[1]]
        } else {
            [*actions[1], *actions[0]]
        };
        // Caller-owned inputs are immutable even if an engine error interrupts resolution.
        let mut state = position.state.clone();
        let outcomes = transitions(
            &mut state,
            self.ruleset,
            self.options,
            d,
            position.suspension.as_ref(),
            pair,
        )
        .map_err(|e| e.to_string())?;
        if state != position.state {
            return Err("engine transition failed to restore input".into());
        }
        let mut children = Vec::with_capacity(outcomes.len());
        for outcome in outcomes {
            let mut child = state.clone();
            child.apply(&outcome.instructions);
            children.push((
                outcome.probability,
                Position {
                    state: child,
                    suspension: outcome.suspension,
                },
            ));
        }
        Ok(children)
    }
}
