//! Serial matrix integration. Parallel/lazy/plan callers keep the ordinary path.

use lab_engine::instruction::Outcome;
use lab_engine::state::State;
use lab_engine::turn::{EnumerateOptions, TurnError};

use crate::choice::Choice;
use crate::game::Decision;
use crate::solve::{Config, SearchStats};

pub(crate) struct PreparedMatrix<const N: usize> {
    #[cfg(feature = "experiment-prepared-turn")]
    turn: lab_engine::turn::PreparedTurn<N>,
    #[cfg(feature = "experiment-prepared-turn")]
    reversed: bool,
}

impl<const N: usize> PreparedMatrix<N> {
    #[allow(unused_variables)]
    pub(crate) fn new(
        state: &State<N>,
        config: Config,
        decision: Decision,
        ours: &[Choice<N>],
        theirs: &[Choice<N>],
        stats: &mut SearchStats,
    ) -> Option<Self> {
        #[cfg(feature = "experiment-prepared-turn")]
        {
            use lab_engine::state::SideId;
            use lab_engine::turn::PreparedTurn;
            if !config.prepared_turn
                || decision != Decision::Turn
                || !PreparedTurn::<N>::eligible(config.enumerate_options())
            {
                return None;
            }
            let started = std::time::Instant::now();
            let turns = |choices: &[Choice<N>]| {
                choices
                    .iter()
                    .map(|c| match c {
                        Choice::Turn(action) => Some(*action),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
            };
            let (ours, theirs) = (turns(ours)?, turns(theirs)?);
            let reversed = config.us == SideId::Two;
            let sides = if reversed {
                [theirs, ours]
            } else {
                [ours, theirs]
            };
            let prepared = Self {
                turn: PreparedTurn::new(state, config.ruleset, sides),
                reversed,
            };
            stats.enumerate_seconds += started.elapsed().as_secs_f64();
            Some(prepared)
        }
        #[cfg(not(feature = "experiment-prepared-turn"))]
        None
    }

    #[allow(unused_variables)]
    pub(crate) fn enumerate(
        &mut self,
        row: usize,
        col: usize,
        options: EnumerateOptions,
    ) -> Result<Vec<Outcome>, TurnError> {
        #[cfg(feature = "experiment-prepared-turn")]
        {
            let indices = if self.reversed {
                [col, row]
            } else {
                [row, col]
            };
            self.turn.enumerate(indices, options)
        }
        #[cfg(not(feature = "experiment-prepared-turn"))]
        unreachable!("PreparedMatrix::new returns None without the experiment feature")
    }
}
