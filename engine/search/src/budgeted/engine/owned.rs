//! Opt-in representation change; the original Domain transition method is the fallback.
use super::*;

impl<const N: usize, E: Evaluator<N> + ?Sized> EngineDomain<'_, N, E> {
    /// Return fully merged flat turn endings by ownership. All other phases and factored
    /// enumeration retain the existing instruction-based adapter. An invalid choice is
    /// rejected by the same validator, in the same order; errors publish no partial batch.
    pub fn transitions_owned(
        &self,
        position: &Position<N>,
        actions: [&Choice<N>; 2],
    ) -> Result<Vec<(f64, Position<N>)>, String> {
        let d =
            decision(&position.state, position.suspension.as_ref()).map_err(|e| e.to_string())?;
        let pair = if self.us == SideId::One {
            [*actions[0], *actions[1]]
        } else {
            [*actions[1], *actions[0]]
        };
        let [Choice::Turn(a), Choice::Turn(b)] = pair else {
            return self.transitions(position, actions);
        };
        if d != Decision::Turn {
            return self.transitions(position, actions);
        }
        let mut state = position.state.clone();
        let batch = lab_engine::turn::try_enumerate_turn_final_states(
            &mut state,
            self.ruleset,
            [a, b],
            self.options,
        )
        .map_err(|e| e.to_string())?;
        let Some(batch) = batch else {
            return self.transitions(position, actions);
        };
        if state != position.state {
            return Err("engine transition failed to restore input".into());
        }
        // Flatten's size hint is not the total ending count. Reserve explicitly so the
        // vector never repeatedly reallocates and moves its large Position elements.
        let mut children = Vec::with_capacity(batch.state_count());
        children.extend(
            batch.into_owned().map(|(state, probability, suspension)| {
                (probability, Position { state, suspension })
            }),
        );
        Ok(children)
    }
}
