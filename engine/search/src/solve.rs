//! Depth-limited maximin over pure strategies with exact chance.
//!
//! At every decision both sides choose at once. The solver takes our side's view: for each of
//! our choices the opponent's reply that hurts us most decides the choice's value, and we
//! pick the choice with the best such value (opponent model ① of DESIGN.md: the opponent
//! knows everything and answers our exact choice; a conservative lower bound). Chance nodes
//! are the engine's exact outcome distributions, averaged ([`Chance::Expect`]) or taken at
//! their worst ([`Chance::Worst`], the "최악 난수 보장" mode where a line must win under every
//! roll). Depth counts turns; replacement and mid-turn decisions inside a turn are free.
//!
//! Cutoffs: alpha-beta at the adversary node (a reply that already drops a choice below the
//! best choice so far ends its evaluation), Star1 at Expect chance nodes (a partial sum that
//! can no longer reach the window ends the node), plain min cutoffs at Worst chance nodes.
//! The root reports every one of our choices; a choice cut off early carries an upper bound
//! (`Line::exact == false`) unless [`Config::exact_lines`] asks for full values. Iterative
//! deepening orders the root's choices and replies by the previous depth's results.

use std::fmt;
use std::time::{Duration, Instant};

use lab_engine::eval::Evaluator;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{EnumerateOptions, RollMode, Suspension, TurnError};

use crate::choice::Choice;
use crate::game::{self, Decision, Pruning};
use crate::nash::{self, Equilibrium, Matrix};

/// The value of a won battle (a lost one is its negative); a leaf evaluation must stay well
/// inside `(-WIN, WIN)`. A faster win scores slightly higher (`WIN + remaining depth`).
pub const WIN: f32 = 10_000.0;

/// The bound of every value (wins at any remaining depth included), for the Star1 cutoffs.
const BOUND: f32 = WIN + 1_000.0;

/// How chance nodes are valued.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Chance {
    /// Probability-weighted average: maximizes the expected score (win rate at depth).
    #[default]
    Expect,
    /// The worst outcome: a value holds under every roll.
    Worst,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub ruleset: Ruleset,
    /// The side the plan is for.
    pub us: SideId,
    /// Turns to look ahead (at least 1).
    pub depth: u32,
    pub chance: Chance,
    pub pruning: Pruning,
    /// Which damage rolls the enumeration branches on. The default is
    /// [`RollMode::Extremes`]: a realistic turn with two spread moves has millions of exact
    /// outcomes (distinct HP tuples), two rolls per hit keep it in the thousands, and a
    /// worst-case value sees the true worst roll whenever the value is monotone in damage.
    pub rolls: RollMode,
    /// Value every root choice fully instead of cutting it off once it falls below the best.
    pub exact_lines: bool,
    /// Stop with [`SearchError::Budget`] after this many turn enumerations.
    pub max_turns: Option<u64>,
    /// Worker threads for the payoff matrix ([`Solver::analyse_mixed`], and [`Solver::analyse`]
    /// with `exact_lines`); 0 uses the machine's parallelism. Each thread works on its own
    /// copy of the state; the result does not depend on the count.
    pub threads: usize,
}

impl Config {
    /// The enumeration options the config asks for.
    pub fn enumerate_options(&self) -> EnumerateOptions {
        EnumerateOptions { rolls: self.rolls }
    }

    pub fn new(ruleset: Ruleset, us: SideId) -> Config {
        Config {
            ruleset,
            us,
            depth: 1,
            chance: Chance::Expect,
            pruning: Pruning::Sensible,
            rolls: RollMode::Extremes,
            exact_lines: false,
            max_turns: None,
            threads: 0,
        }
    }

    /// The thread count to use for `rows` independent rows.
    pub fn worker_threads(&self, rows: usize) -> usize {
        let n = if self.threads == 0 {
            std::thread::available_parallelism().map_or(1, |n| n.get())
        } else {
            self.threads
        };
        n.clamp(1, rows.max(1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchError {
    Turn(TurnError),
    /// A side has no legal choice although the battle is not over.
    NoChoice(SideId),
    /// `Config::max_turns` was reached.
    Budget,
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::Turn(e) => write!(f, "{e}"),
            SearchError::NoChoice(side) => write!(f, "{side:?} has no legal choice"),
            SearchError::Budget => write!(f, "the turn budget was reached"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<TurnError> for SearchError {
    fn from(e: TurnError) -> Self {
        SearchError::Turn(e)
    }
}

/// One of our root choices.
#[derive(Clone, Debug, PartialEq)]
pub struct Line<const N: usize> {
    pub ours: Choice<N>,
    /// The choice's maximin value (from our side), or an upper bound when `exact` is false.
    pub value: f32,
    pub exact: bool,
    /// The reply that holds the value (the last one that lowered it).
    pub reply: Option<Choice<N>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Analysis<const N: usize> {
    pub decision: Decision,
    /// Our choices, best first. Empty when the battle is over.
    pub lines: Vec<Line<N>>,
    /// The value of the position (the best line's, or the terminal value).
    pub value: f32,
    pub depth: u32,
    pub nodes: u64,
    /// Turn/replacement/resume enumerations.
    pub turns: u64,
    pub elapsed: Duration,
}

pub struct Solver<'e, const N: usize, E: Evaluator<N> + ?Sized> {
    pub config: Config,
    evaluator: &'e E,
    nodes: u64,
    turns: u64,
}

impl<'e, const N: usize, E: Evaluator<N> + ?Sized + Sync> Solver<'e, N, E> {
    pub fn new(config: Config, evaluator: &'e E) -> Self {
        Solver {
            config,
            evaluator,
            nodes: 0,
            turns: 0,
        }
    }

    /// Values every choice of ours at the decision `state` (with `suspension`, if the turn is
    /// suspended) asks for. `state` is left unchanged.
    pub fn analyse(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
    ) -> Result<Analysis<N>, SearchError> {
        let started = Instant::now();
        self.nodes = 0;
        self.turns = 0;
        let decision = game::decision(state, suspension)?;
        if let Decision::Over(result) = decision {
            return Ok(Analysis {
                decision,
                lines: Vec::new(),
                value: self.terminal(result, self.config.depth),
                depth: 0,
                nodes: 0,
                turns: 0,
                elapsed: started.elapsed(),
            });
        }
        let them = self.config.us.other();
        let mut ours = self.choices(state, decision, self.config.us)?;
        let mut theirs = self.choices(state, decision, them)?;
        let mut lines: Vec<Line<N>> = Vec::new();
        let max_depth = self.config.depth.max(1);
        for depth in 1..=max_depth {
            let next_depth = if decision == Decision::Turn {
                depth - 1
            } else {
                depth
            };
            let mut best = f32::NEG_INFINITY;
            let mut new_lines = Vec::with_capacity(ours.len());
            let threads = self.config.worker_threads(ours.len());
            if self.config.exact_lines && threads > 1 {
                // Every row is valued in full, so the rows are independent: the payoff matrix
                // in parallel, then each line is its row's minimum.
                let values = self.parallel_matrix(
                    state, suspension, decision, &ours, &theirs, next_depth, threads,
                )?;
                for (r, &a) in ours.iter().enumerate() {
                    let row = &values[r * theirs.len()..(r + 1) * theirs.len()];
                    let (c, &worst) = row.iter().enumerate().fold((0, &f32::INFINITY), |m, x| {
                        if x.1 < m.1 {
                            x
                        } else {
                            m
                        }
                    });
                    new_lines.push(Line {
                        ours: a,
                        value: worst,
                        exact: true,
                        reply: Some(theirs[c]),
                    });
                }
                new_lines.sort_by(|x, y| y.value.total_cmp(&x.value));
                ours = new_lines.iter().map(|l| l.ours).collect();
                lines = new_lines;
                continue;
            }
            for &a in &ours {
                let alpha = if self.config.exact_lines {
                    f32::NEG_INFINITY
                } else {
                    best
                };
                let mut worst = f32::INFINITY;
                let mut reply = None;
                let mut cut = false;
                for &b in &theirs {
                    let pair = self.pair(a, b);
                    let v =
                        self.chance(state, decision, suspension, pair, next_depth, alpha, worst)?;
                    if v < worst {
                        worst = v;
                        reply = Some(b);
                    }
                    if worst <= alpha {
                        cut = true;
                        break;
                    }
                }
                best = best.max(worst);
                new_lines.push(Line {
                    ours: a,
                    value: worst,
                    exact: !cut,
                    reply,
                });
            }
            new_lines.sort_by(|x, y| y.value.total_cmp(&x.value));
            // Move ordering for the next depth: our choices best first, replies by how often
            // they were the one that held a line down.
            ours = new_lines.iter().map(|l| l.ours).collect();
            let mut counts: Vec<(Choice<N>, usize)> = theirs.iter().map(|&b| (b, 0)).collect();
            for line in &new_lines {
                if let Some(reply) = line.reply {
                    if let Some(entry) = counts.iter_mut().find(|(b, _)| *b == reply) {
                        entry.1 += 1;
                    }
                }
            }
            counts.sort_by_key(|x| std::cmp::Reverse(x.1));
            theirs = counts.into_iter().map(|(b, _)| b).collect();
            lines = new_lines;
        }
        let value = lines.first().map_or(f32::NEG_INFINITY, |l| l.value);
        Ok(Analysis {
            decision,
            lines,
            value,
            depth: max_depth,
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
        })
    }

    fn choices(
        &self,
        state: &State<N>,
        decision: Decision,
        side: SideId,
    ) -> Result<Vec<Choice<N>>, SearchError> {
        let choices = game::legal_choices(
            state,
            self.config.ruleset,
            decision,
            side,
            self.config.pruning,
        );
        if choices.is_empty() {
            return Err(SearchError::NoChoice(side));
        }
        Ok(choices)
    }

    /// `[side one's choice, side two's choice]` from ours and theirs.
    fn pair(&self, ours: Choice<N>, theirs: Choice<N>) -> [Choice<N>; 2] {
        match self.config.us {
            SideId::One => [ours, theirs],
            SideId::Two => [theirs, ours],
        }
    }

    fn terminal(&self, result: BattleResult, depth: u32) -> f32 {
        match result {
            BattleResult::Win(side) if side == self.config.us => WIN + depth as f32,
            BattleResult::Win(_) => -(WIN + depth as f32),
            BattleResult::Tie | BattleResult::Ongoing => 0.0,
        }
    }

    fn leaf(&self, state: &State<N>) -> f32 {
        let score = self.evaluator.evaluate(state);
        match self.config.us {
            SideId::One => score,
            SideId::Two => -score,
        }
    }

    /// The maximin value of the position within `(alpha, beta)` (fail-soft: a value at or
    /// below `alpha` is an upper bound, at or above `beta` a lower bound).
    fn value(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        depth: u32,
        alpha: f32,
        beta: f32,
    ) -> Result<f32, SearchError> {
        self.nodes += 1;
        let decision = game::decision(state, suspension)?;
        if let Decision::Over(result) = decision {
            return Ok(self.terminal(result, depth));
        }
        if depth == 0 {
            return Ok(self.leaf(state));
        }
        let them = self.config.us.other();
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs = self.choices(state, decision, them)?;
        let next_depth = if decision == Decision::Turn {
            depth - 1
        } else {
            depth
        };
        let mut best = f32::NEG_INFINITY;
        for &a in &ours {
            let mut worst = f32::INFINITY;
            for &b in &theirs {
                let lo = alpha.max(best);
                let hi = beta.min(worst);
                let pair = self.pair(a, b);
                let v = self.chance(state, decision, suspension, pair, next_depth, lo, hi)?;
                worst = worst.min(v);
                if worst <= lo {
                    break;
                }
            }
            best = best.max(worst);
            if best >= beta {
                break;
            }
        }
        Ok(best)
    }

    /// The value of the chance node after both sides chose `pair`.
    #[allow(clippy::too_many_arguments)]
    fn chance(
        &mut self,
        state: &mut State<N>,
        decision: Decision,
        suspension: Option<&Suspension>,
        pair: [Choice<N>; 2],
        depth: u32,
        alpha: f32,
        beta: f32,
    ) -> Result<f32, SearchError> {
        if let Some(max) = self.config.max_turns {
            if self.turns >= max {
                return Err(SearchError::Budget);
            }
        }
        self.turns += 1;
        let outcomes = game::transitions(
            state,
            self.config.ruleset,
            self.config.enumerate_options(),
            decision,
            suspension,
            pair,
        )?;
        match self.config.chance {
            Chance::Worst => {
                let mut worst = f32::INFINITY;
                for outcome in &outcomes {
                    state.apply(&outcome.instructions);
                    let v = self.value(
                        state,
                        outcome.suspension.as_ref(),
                        depth,
                        alpha,
                        beta.min(worst),
                    );
                    state.reverse(&outcome.instructions);
                    worst = worst.min(v?);
                    if worst <= alpha {
                        break;
                    }
                }
                Ok(worst)
            }
            Chance::Expect => {
                // Star1: children get the window that the total could still reach.
                let mut sum = 0.0f64;
                let mut remaining = 1.0f64;
                for outcome in &outcomes {
                    let p = outcome.probability;
                    let rest = (remaining - p).max(0.0);
                    let lo = ((alpha as f64 - (sum + rest * BOUND as f64)) / p).max(-BOUND as f64);
                    let hi = ((beta as f64 - (sum - rest * BOUND as f64)) / p).min(BOUND as f64);
                    state.apply(&outcome.instructions);
                    let v = self.value(
                        state,
                        outcome.suspension.as_ref(),
                        depth,
                        lo as f32,
                        hi as f32,
                    );
                    state.reverse(&outcome.instructions);
                    sum += p * v? as f64;
                    remaining = rest;
                    if sum - remaining * BOUND as f64 >= beta as f64 {
                        return Ok((sum - remaining * BOUND as f64) as f32);
                    }
                    if sum + remaining * BOUND as f64 <= alpha as f64 {
                        return Ok((sum + remaining * BOUND as f64) as f32);
                    }
                }
                Ok(sum as f32)
            }
        }
    }
}

/// One worker's rows of the payoff matrix with its node and turn counts.
type RowValues = (Vec<f32>, u64, u64);

/// The root decision solved as a zero-sum matrix game over both sides' choices, each pair
/// valued by the exact chance node below it (deeper nodes by maximin as in [`Analysis`]).
#[derive(Clone, Debug, PartialEq)]
pub struct MixedAnalysis<const N: usize> {
    pub decision: Decision,
    pub ours: Vec<Choice<N>>,
    pub theirs: Vec<Choice<N>>,
    /// `ours.len() × theirs.len()`, from our side.
    pub matrix: Matrix,
    pub equilibrium: Equilibrium,
    /// The pure maximin (row, value) of the same matrix, for comparison.
    pub maximin: (usize, f32),
    pub depth: u32,
    pub nodes: u64,
    pub turns: u64,
    pub elapsed: Duration,
}

impl<const N: usize> MixedAnalysis<N> {
    /// Our choices with equilibrium probability at least `min`, most likely first.
    pub fn our_support(&self, min: f32) -> Vec<(Choice<N>, f32)> {
        let mut out: Vec<(Choice<N>, f32)> = self
            .ours
            .iter()
            .zip(&self.equilibrium.rows)
            .filter(|(_, &p)| p >= min)
            .map(|(c, &p)| (*c, p))
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }

    /// Their choices with equilibrium probability at least `min`, most likely first.
    pub fn their_support(&self, min: f32) -> Vec<(Choice<N>, f32)> {
        let mut out: Vec<(Choice<N>, f32)> = self
            .theirs
            .iter()
            .zip(&self.equilibrium.cols)
            .filter(|(_, &p)| p >= min)
            .map(|(c, &p)| (*c, p))
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }
}

impl<'e, const N: usize, E: Evaluator<N> + ?Sized + Sync> Solver<'e, N, E> {
    /// Values every pair of choices at the root exactly (no cutoffs) and solves the matrix
    /// game by regret matching. `state` is left unchanged. Costs `ours × theirs` chance nodes,
    /// each with the full subtree of depth `config.depth - 1`.
    pub fn analyse_mixed(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
    ) -> Result<MixedAnalysis<N>, SearchError> {
        let started = Instant::now();
        self.nodes = 0;
        self.turns = 0;
        let decision = game::decision(state, suspension)?;
        if matches!(decision, Decision::Over(_)) {
            return Err(SearchError::Turn(TurnError::BattleOver));
        }
        let them = self.config.us.other();
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs = self.choices(state, decision, them)?;
        let depth = self.config.depth.max(1);
        let next_depth = if decision == Decision::Turn {
            depth - 1
        } else {
            depth
        };
        let threads = self.config.worker_threads(ours.len());
        let values = if threads > 1 {
            self.parallel_matrix(
                state, suspension, decision, &ours, &theirs, next_depth, threads,
            )?
        } else {
            let mut values = Vec::with_capacity(ours.len() * theirs.len());
            for &a in &ours {
                for &b in &theirs {
                    let pair = self.pair(a, b);
                    let v = self.chance(
                        state,
                        decision,
                        suspension,
                        pair,
                        next_depth,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    )?;
                    values.push(v);
                }
            }
            values
        };
        let matrix = Matrix::new(ours.len(), theirs.len(), values);
        let equilibrium = nash::solve(&matrix, 20_000, 0.01);
        let maximin = matrix.maximin();
        Ok(MixedAnalysis {
            decision,
            ours,
            theirs,
            matrix,
            equilibrium,
            maximin,
            depth,
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
        })
    }

    /// The payoff matrix (`ours × theirs`, row-major, exact chance values) computed by
    /// `threads` workers on their own copies of the state; node and turn counts are summed.
    #[allow(clippy::too_many_arguments)]
    fn parallel_matrix(
        &mut self,
        state: &State<N>,
        suspension: Option<&Suspension>,
        decision: Decision,
        ours: &[Choice<N>],
        theirs: &[Choice<N>],
        depth: u32,
        threads: usize,
    ) -> Result<Vec<f32>, SearchError> {
        let config = self.config;
        let evaluator = self.evaluator;
        let chunk = ours.len().div_ceil(threads.max(1)).max(1);
        let results: Vec<Result<RowValues, SearchError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = ours
                .chunks(chunk)
                .map(|rows| {
                    let mut local_state = state.clone();
                    let suspension = suspension.cloned();
                    scope.spawn(move || {
                        let mut local = Solver::new(config, evaluator);
                        let mut values = Vec::with_capacity(rows.len() * theirs.len());
                        for &a in rows {
                            for &b in theirs {
                                let pair = local.pair(a, b);
                                let v = local.chance(
                                    &mut local_state,
                                    decision,
                                    suspension.as_ref(),
                                    pair,
                                    depth,
                                    f32::NEG_INFINITY,
                                    f32::INFINITY,
                                )?;
                                values.push(v);
                            }
                        }
                        Ok((values, local.nodes, local.turns))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("a search thread panicked"))
                .collect()
        });
        let mut values = Vec::with_capacity(ours.len() * theirs.len());
        for result in results {
            let (v, nodes, turns) = result?;
            values.extend(v);
            self.nodes += nodes;
            self.turns += turns;
        }
        Ok(values)
    }
}
