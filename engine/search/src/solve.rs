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
use crate::tt::{self, TranspositionTable};

/// What a chance node continues into: the maximin tree with `depth` turns left, or a fixed
/// plan (`Solver::evaluate_plan`) at its next entry.
#[derive(Clone, Copy, Debug)]
enum Next<'p, const N: usize> {
    Depth(u32),
    Plan(&'p [Choice<N>], usize),
    /// The position is worth the equilibrium value of its own matrix game (one more turn,
    /// mixed strategies, leaf evaluation below); the chance node above it keeps only its
    /// `Config::outcome_cap` most probable outcomes.
    Nash,
}

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
    /// [`Solver::evaluate_plan`]: after the plan, value each child position by its own
    /// matrix-game equilibrium (one more turn, mixed strategies) instead of the maximin tree.
    pub child_nash: bool,
    /// With `child_nash`: only this many of the opponent's root replies (the worst for us by
    /// the plain plan value) get the expensive child valuation.
    pub reply_beam: Option<usize>,
    /// With `child_nash`: a chance node keeps only its most probable outcomes (renormalised)
    /// before valuing children by equilibrium.
    pub outcome_cap: Option<usize>,
    /// Worker threads for the payoff matrix ([`Solver::analyse_mixed`], and [`Solver::analyse`]
    /// with `exact_lines`); 0 uses the machine's parallelism. Each thread works on its own
    /// copy of the state; the result does not depend on the count.
    pub threads: usize,
    /// Reuse child equilibrium values by position ([`crate::tt`]); off only to check that the
    /// table changes nothing but the time.
    pub transposition: bool,
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
            child_nash: false,
            reply_beam: Some(6),
            outcome_cap: Some(4),
            transposition: true,
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
    /// Every choice at the root runs into effects the engine does not implement (the
    /// reasons, deduplicated). Elsewhere in the tree such pairs are dropped and reported.
    Unsupported(Vec<String>),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::Turn(e) => write!(f, "{e}"),
            SearchError::NoChoice(side) => write!(f, "{side:?} has no legal choice"),
            SearchError::Budget => write!(f, "the turn budget was reached"),
            SearchError::Unsupported(reasons) => {
                write!(
                    f,
                    "nothing evaluable: not implemented: {}",
                    reasons.join("; ")
                )
            }
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
    /// Effects the engine refused inside the tree; pairs hitting them were dropped.
    pub unsupported: Vec<String>,
    pub omitted_pairs: usize,
}

/// Where a search spent its work (`lab-plan --stats`, `engine/scripts/search_bench.py`): the
/// transposition table of child equilibria, the matrix games solved and the time in the turn
/// enumeration. Times are summed over worker threads (CPU time, not wall time).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SearchStats {
    /// [`Solver::nash_value`] lookups answered by the table.
    pub tt_hits: u64,
    /// Lookups that solved the position's matrix game (and stored it).
    pub tt_misses: u64,
    /// Matrix games solved by regret matching (roots and children).
    pub nash_solves: u64,
    /// Their RM+ iterations, summed.
    pub nash_iterations: u64,
    pub nash_seconds: f64,
    /// Time inside `game::transitions` (turn enumeration).
    pub enumerate_seconds: f64,
}

impl SearchStats {
    fn add(&mut self, other: &SearchStats) {
        self.tt_hits += other.tt_hits;
        self.tt_misses += other.tt_misses;
        self.nash_solves += other.nash_solves;
        self.nash_iterations += other.nash_iterations;
        self.nash_seconds += other.nash_seconds;
        self.enumerate_seconds += other.enumerate_seconds;
    }
}

impl fmt::Display for SearchStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tt {} hits / {} misses; {} matrix games ({} RM+ iterations, {:.2} s); enumeration {:.2} s (thread-summed)",
            self.tt_hits,
            self.tt_misses,
            self.nash_solves,
            self.nash_iterations,
            self.nash_seconds,
            self.enumerate_seconds
        )
    }
}

pub struct Solver<'e, const N: usize, E: Evaluator<N> + ?Sized> {
    pub config: Config,
    evaluator: &'e E,
    nodes: u64,
    turns: u64,
    stats: SearchStats,
    plan_broken: u32,
    /// Effects the engine refused somewhere in the tree (deduplicated); the pairs of choices
    /// whose subtree hit one were dropped from the min/max.
    unsupported: Vec<String>,
    /// Pairs of choices dropped that way.
    omitted_pairs: usize,
    /// [`Solver::nash_value`] results by position (identical children recur across replies and
    /// outcomes), [`crate::tt`].
    tt: TranspositionTable<N>,
}

impl<'e, const N: usize, E: Evaluator<N> + ?Sized + Sync> Solver<'e, N, E> {
    pub fn new(config: Config, evaluator: &'e E) -> Self {
        Solver {
            config,
            evaluator,
            nodes: 0,
            turns: 0,
            plan_broken: 0,
            unsupported: Vec::new(),
            omitted_pairs: 0,
            stats: SearchStats::default(),
            tt: TranspositionTable::new(config.transposition, tt::DEFAULT_CAPACITY),
        }
    }

    /// Positions in the transposition table.
    pub fn tt_len(&self) -> usize {
        self.tt.len()
    }

    /// The work counters since the last analysis started.
    pub fn stats(&self) -> SearchStats {
        self.stats
    }

    /// Solves a matrix game (RM+, 20 000 iterations, tolerance 0.01), counted in the stats.
    fn solve_matrix(&mut self, matrix: &Matrix) -> Equilibrium {
        let started = Instant::now();
        let equilibrium = nash::solve(matrix, 20_000, 0.01);
        self.stats.nash_solves += 1;
        self.stats.nash_iterations += equilibrium.iterations as u64;
        self.stats.nash_seconds += started.elapsed().as_secs_f64();
        equilibrium
    }

    fn note_unsupported(&mut self, why: String) {
        self.omitted_pairs += 1;
        if !self.unsupported.contains(&why) {
            self.unsupported.push(why);
        }
    }

    fn reset_counters(&mut self) {
        self.nodes = 0;
        self.turns = 0;
        self.plan_broken = 0;
        self.unsupported.clear();
        self.omitted_pairs = 0;
        self.stats = SearchStats::default();
    }

    /// Values every choice of ours at the decision `state` (with `suspension`, if the turn is
    /// suspended) asks for. `state` is left unchanged.
    pub fn analyse(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
    ) -> Result<Analysis<N>, SearchError> {
        let started = Instant::now();
        self.reset_counters();
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
                unsupported: Vec::new(),
                omitted_pairs: 0,
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
                    state,
                    suspension,
                    decision,
                    &ours,
                    &theirs,
                    Next::Depth(next_depth),
                    threads,
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
                    if worst == f32::INFINITY {
                        continue;
                    }
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
                    let v = self.chance(
                        state,
                        decision,
                        suspension,
                        pair,
                        Next::Depth(next_depth),
                        alpha,
                        worst,
                    )?;
                    if v.is_nan() {
                        continue;
                    }
                    if v < worst {
                        worst = v;
                        reply = Some(b);
                    }
                    if worst <= alpha {
                        cut = true;
                        break;
                    }
                }
                if worst == f32::INFINITY {
                    // No reply could be evaluated against this choice.
                    continue;
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
        if lines.is_empty() {
            return Err(SearchError::Unsupported(self.unsupported.clone()));
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
            unsupported: self.unsupported.clone(),
            omitted_pairs: self.omitted_pairs,
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
                let v = self.chance(
                    state,
                    decision,
                    suspension,
                    pair,
                    Next::Depth(next_depth),
                    lo,
                    hi,
                )?;
                if v.is_nan() {
                    continue;
                }
                worst = worst.min(v);
                if worst <= lo {
                    break;
                }
            }
            if worst == f32::INFINITY {
                continue;
            }
            best = best.max(worst);
            if best >= beta {
                break;
            }
        }
        if best == f32::NEG_INFINITY {
            // Nothing here could be evaluated: the pair above is dropped.
            return Ok(f32::NAN);
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
        next: Next<'_, N>,
        alpha: f32,
        beta: f32,
    ) -> Result<f32, SearchError> {
        if let Some(max) = self.config.max_turns {
            if self.turns >= max {
                return Err(SearchError::Budget);
            }
        }
        self.turns += 1;
        let started = Instant::now();
        let transitions = game::transitions(
            state,
            self.config.ruleset,
            self.config.enumerate_options(),
            decision,
            suspension,
            pair,
        );
        self.stats.enumerate_seconds += started.elapsed().as_secs_f64();
        let outcomes = match transitions {
            Ok(outcomes) => outcomes,
            Err(TurnError::Unsupported(why)) => {
                // The pair cannot be valued; the caller drops it (NaN).
                self.note_unsupported(why);
                return Ok(f32::NAN);
            }
            Err(e) => return Err(e.into()),
        };
        let outcomes = match (next, self.config.outcome_cap) {
            (Next::Nash, Some(cap)) if outcomes.len() > cap => {
                let mut kept = outcomes;
                kept.sort_by(|a, b| b.probability.total_cmp(&a.probability));
                kept.truncate(cap);
                let total: f64 = kept.iter().map(|o| o.probability).sum();
                for o in &mut kept {
                    o.probability /= total;
                }
                kept
            }
            _ => outcomes,
        };
        match self.config.chance {
            Chance::Worst => {
                let mut worst = f32::INFINITY;
                for outcome in &outcomes {
                    state.apply(&outcome.instructions);
                    let v = self.continue_at(
                        state,
                        outcome.suspension.as_ref(),
                        next,
                        alpha,
                        beta.min(worst),
                    );
                    state.reverse(&outcome.instructions);
                    let v = v?;
                    if v.is_nan() {
                        return Ok(f32::NAN);
                    }
                    worst = worst.min(v);
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
                    let v = self.continue_at(
                        state,
                        outcome.suspension.as_ref(),
                        next,
                        lo as f32,
                        hi as f32,
                    );
                    state.reverse(&outcome.instructions);
                    let v = v?;
                    if v.is_nan() {
                        return Ok(f32::NAN);
                    }
                    sum += p * v as f64;
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

/// One worker's cells `(index, value)` of the payoff matrix with its node and turn counts,
/// the unsupported reasons it met and the pairs it dropped.
type CellValues = (
    Vec<(usize, f32)>,
    u64,
    u64,
    Vec<String>,
    usize,
    u32,
    SearchStats,
);

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
    /// Effects the engine refused in some cell; the columns (their replies) and then the rows
    /// (our choices) containing such a cell were dropped before solving.
    pub unsupported: Vec<String>,
    pub omitted_theirs: usize,
    pub omitted_ours: usize,
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

/// A choice is in a side's beam of [`Solver::analyse_deep_mixed`] whenever its shallow
/// equilibrium probability is at least this, besides the `beam` best by expected value.
pub const MIXED_SUPPORT: f32 = 0.05;

/// Result of [`Solver::analyse_deep_mixed`]: the depth-2 matrix game over both sides' beams.
#[derive(Clone, Debug)]
pub struct DeepMixedAnalysis<const N: usize> {
    pub decision: Decision,
    /// Our beam and their beam (choices kept after dropping unevaluable pairs).
    pub ours: Vec<Choice<N>>,
    pub theirs: Vec<Choice<N>>,
    /// `ours.len() × theirs.len()`, from our side: each cell the expected next-turn equilibrium
    /// value of the pair's most probable outcomes.
    pub matrix: Matrix,
    pub equilibrium: Equilibrium,
    /// The pure maximin (row, value) of the deep matrix.
    pub maximin: (usize, f32),
    /// The one-turn analysis the beams were taken from.
    pub shallow: MixedAnalysis<N>,
    pub beam: usize,
    pub outcome_cap: Option<usize>,
    pub nodes: u64,
    pub turns: u64,
    pub elapsed: Duration,
    pub unsupported: Vec<String>,
    pub omitted_theirs: usize,
    pub omitted_ours: usize,
}

impl<const N: usize> DeepMixedAnalysis<N> {
    /// Our beam choices with deep equilibrium probability at least `min`, most likely first.
    pub fn our_support(&self, min: f32) -> Vec<(Choice<N>, f32)> {
        support(&self.ours, &self.equilibrium.rows, min)
    }

    /// Their beam choices with deep equilibrium probability at least `min`, most likely first.
    pub fn their_support(&self, min: f32) -> Vec<(Choice<N>, f32)> {
        support(&self.theirs, &self.equilibrium.cols, min)
    }
}

fn support<const N: usize>(
    choices: &[Choice<N>],
    probabilities: &[f32],
    min: f32,
) -> Vec<(Choice<N>, f32)> {
    let mut out: Vec<(Choice<N>, f32)> = choices
        .iter()
        .zip(probabilities)
        .filter(|(_, &p)| p >= min)
        .map(|(c, &p)| (*c, p))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

/// The indices of the `beam` best entries of `value` (descending when `descending`, else
/// ascending) plus every index whose `support` probability is at least [`MIXED_SUPPORT`], in
/// their original order.
fn beam_indices(value: &[f32], support: &[f32], beam: usize, descending: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..value.len()).collect();
    order.sort_by(|&a, &b| {
        if descending {
            value[b].total_cmp(&value[a])
        } else {
            value[a].total_cmp(&value[b])
        }
    });
    let mut keep: Vec<usize> = order.into_iter().take(beam.max(1)).collect();
    for (i, &p) in support.iter().enumerate() {
        if p >= MIXED_SUPPORT && !keep.contains(&i) {
            keep.push(i);
        }
    }
    keep.sort_unstable();
    keep
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
        self.reset_counters();
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
                state,
                suspension,
                decision,
                &ours,
                &theirs,
                Next::Depth(next_depth),
                threads,
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
                        Next::Depth(next_depth),
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    )?;
                    values.push(v);
                }
            }
            values
        };
        let (ours, theirs, values, omitted_ours, omitted_theirs) =
            drop_unevaluable(ours, theirs, values);
        if ours.is_empty() || theirs.is_empty() {
            return Err(SearchError::Unsupported(self.unsupported.clone()));
        }
        let matrix = Matrix::new(ours.len(), theirs.len(), values);
        let equilibrium = self.solve_matrix(&matrix);
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
            unsupported: self.unsupported.clone(),
            omitted_theirs,
            omitted_ours,
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
        next: Next<'_, N>,
        threads: usize,
    ) -> Result<Vec<f32>, SearchError> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let config = self.config;
        let evaluator = self.evaluator;
        let cells = ours.len() * theirs.len();
        // Cells are handed out one at a time: pairs differ a lot in cost (a double Protect
        // is one outcome, two spread moves thousands), so static row chunks leave threads idle.
        let counter = AtomicUsize::new(0);
        let results: Vec<Result<CellValues, SearchError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads.max(1))
                .map(|_| {
                    let mut local_state = state.clone();
                    let suspension = suspension.cloned();
                    let counter = &counter;
                    scope.spawn(move || {
                        let mut local = Solver::new(config, evaluator);
                        let mut values = Vec::new();
                        loop {
                            let i = counter.fetch_add(1, Ordering::Relaxed);
                            if i >= cells {
                                break;
                            }
                            let (a, b) = (ours[i / theirs.len()], theirs[i % theirs.len()]);
                            let pair = local.pair(a, b);
                            let v = local.chance(
                                &mut local_state,
                                decision,
                                suspension.as_ref(),
                                pair,
                                next,
                                f32::NEG_INFINITY,
                                f32::INFINITY,
                            )?;
                            values.push((i, v));
                        }
                        Ok((
                            values,
                            local.nodes,
                            local.turns,
                            local.unsupported,
                            local.omitted_pairs,
                            local.plan_broken,
                            local.stats,
                        ))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("a search thread panicked"))
                .collect()
        });
        let mut values = vec![f32::NAN; cells];
        for result in results {
            let (cells, nodes, turns, reasons, omitted, broken, stats) = result?;
            self.stats.add(&stats);
            for (i, v) in cells {
                values[i] = v;
            }
            self.nodes += nodes;
            self.turns += turns;
            self.omitted_pairs += omitted;
            self.plan_broken += broken;
            for why in reasons {
                if !self.unsupported.contains(&why) {
                    self.unsupported.push(why);
                }
            }
        }
        Ok(values)
    }

    fn continue_at(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        next: Next<'_, N>,
        alpha: f32,
        beta: f32,
    ) -> Result<f32, SearchError> {
        match next {
            Next::Depth(depth) => self.value(state, suspension, depth, alpha, beta),
            Next::Plan(plan, index) => self.plan_value(state, suspension, plan, index, alpha, beta),
            Next::Nash => self.nash_value(state, suspension),
        }
    }

    /// The equilibrium value of the position's own matrix game with leaf evaluation below
    /// (the child valuation of [`Config::child_nash`]); NaN when nothing is evaluable, the
    /// terminal value when the battle is over.
    pub fn nash_value(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
    ) -> Result<f32, SearchError> {
        self.nodes += 1;
        let decision = game::decision(state, suspension)?;
        if let Decision::Over(result) = decision {
            return Ok(self.terminal(result, 0));
        }
        let key = if self.tt.enabled() {
            let key = (state.clone(), suspension.cloned());
            if let Some(v) = self.tt.get(&key) {
                self.stats.tt_hits += 1;
                return Ok(v);
            }
            self.stats.tt_misses += 1;
            Some(key)
        } else {
            None
        };
        let them = self.config.us.other();
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs = self.choices(state, decision, them)?;
        let next_depth = if decision == Decision::Turn { 0 } else { 1 };
        let threads = self.config.worker_threads(ours.len() * theirs.len());
        let values = if threads > 1 {
            self.parallel_matrix(
                state,
                suspension,
                decision,
                &ours,
                &theirs,
                Next::Depth(next_depth),
                threads,
            )?
        } else {
            let mut values = Vec::with_capacity(ours.len() * theirs.len());
            for &a in &ours {
                for &b in &theirs {
                    let pair = self.pair(a, b);
                    values.push(self.chance(
                        state,
                        decision,
                        suspension,
                        pair,
                        Next::Depth(next_depth),
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    )?);
                }
            }
            values
        };
        let (ours, theirs, values, _, _) = drop_unevaluable(ours, theirs, values);
        let value = if ours.is_empty() || theirs.is_empty() {
            f32::NAN
        } else {
            let matrix = Matrix::new(ours.len(), theirs.len(), values);
            self.solve_matrix(&matrix).value
        };
        if let Some(key) = key {
            self.tt.insert(key, value);
        }
        Ok(value)
    }

    /// Two turns deep, approximately: the root matrix with leaf values orders our choices
    /// (row minimum) and their replies (per row); then the `beam` best rows are valued again
    /// against their `beam` worst columns with each child position worth its own next-turn
    /// equilibrium ([`Next::Nash`], `Config::outcome_cap`). `state` is left unchanged.
    /// Depth-2 mixed analysis (WORKPLAN S21): the matrix game over both sides' beams in which
    /// each cell is the expected next-turn equilibrium value of the pair's outcomes (as
    /// [`Solver::analyse_deep`]'s children: the `Config::outcome_cap` most probable outcomes,
    /// each child worth its own one-turn equilibrium, cached by position), solved for a mixed
    /// equilibrium of both sides. A side's beam is its `beam` best choices by expected value
    /// against the other side's shallow (one-turn) equilibrium strategy plus every choice in
    /// its own shallow support ([`MIXED_SUPPORT`]). Unlike [`Solver::analyse_deep`], which
    /// values our beam against the worst replies, this gives both players a strategy and so
    /// serves as a rollout policy (`lab-rollout --policy deep-nash`). `state` is left unchanged.
    pub fn analyse_deep_mixed(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        beam: usize,
    ) -> Result<DeepMixedAnalysis<N>, SearchError> {
        let started = Instant::now();
        let shallow = self.analyse_mixed(state, suspension)?;
        let decision = shallow.decision;
        let (n, m) = (shallow.ours.len(), shallow.theirs.len());
        // Expected values against the other side's shallow equilibrium strategy.
        let our_value: Vec<f32> = (0..n)
            .map(|r| {
                (0..m)
                    .map(|c| shallow.equilibrium.cols[c] * shallow.matrix.at(r, c))
                    .sum()
            })
            .collect();
        let their_value: Vec<f32> = (0..m)
            .map(|c| {
                (0..n)
                    .map(|r| shallow.equilibrium.rows[r] * shallow.matrix.at(r, c))
                    .sum()
            })
            .collect();
        let our_beam = beam_indices(&our_value, &shallow.equilibrium.rows, beam, true);
        let their_beam = beam_indices(&their_value, &shallow.equilibrium.cols, beam, false);
        let ours: Vec<Choice<N>> = our_beam.iter().map(|&r| shallow.ours[r]).collect();
        let theirs: Vec<Choice<N>> = their_beam.iter().map(|&c| shallow.theirs[c]).collect();
        let mut values = Vec::with_capacity(ours.len() * theirs.len());
        for &a in &ours {
            for &b in &theirs {
                let pair = self.pair(a, b);
                values.push(self.chance(
                    state,
                    decision,
                    suspension,
                    pair,
                    Next::Nash,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                )?);
            }
        }
        let (ours, theirs, values, omitted_theirs, omitted_ours) =
            drop_unevaluable(ours, theirs, values);
        if ours.is_empty() || theirs.is_empty() {
            return Err(SearchError::Unsupported(self.unsupported.clone()));
        }
        let matrix = Matrix::new(ours.len(), theirs.len(), values);
        let equilibrium = self.solve_matrix(&matrix);
        let maximin = matrix.maximin();
        Ok(DeepMixedAnalysis {
            decision,
            ours,
            theirs,
            matrix,
            equilibrium,
            maximin,
            shallow,
            beam: beam.max(1),
            outcome_cap: self.config.outcome_cap,
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
            unsupported: self.unsupported.clone(),
            omitted_theirs,
            omitted_ours,
        })
    }

    pub fn analyse_deep(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        beam: usize,
    ) -> Result<DeepAnalysis<N>, SearchError> {
        let started = Instant::now();
        self.reset_counters();
        let decision = game::decision(state, suspension)?;
        if matches!(decision, Decision::Over(_)) {
            return Err(SearchError::Turn(TurnError::BattleOver));
        }
        let them = self.config.us.other();
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs = self.choices(state, decision, them)?;
        let next_depth = if decision == Decision::Turn { 0 } else { 1 };
        let threads = self.config.worker_threads(ours.len() * theirs.len());
        let values = self.parallel_matrix(
            state,
            suspension,
            decision,
            &ours,
            &theirs,
            Next::Depth(next_depth),
            threads.max(1),
        )?;
        let m = theirs.len();
        // Rows by their minimum (pure maximin), NaN cells ignored.
        let mut rows: Vec<(usize, f32, Vec<usize>)> = (0..ours.len())
            .filter_map(|r| {
                let mut cols: Vec<usize> =
                    (0..m).filter(|&c| !values[r * m + c].is_nan()).collect();
                if cols.is_empty() {
                    return None;
                }
                cols.sort_by(|&a, &b| values[r * m + a].total_cmp(&values[r * m + b]));
                let worst = values[r * m + cols[0]];
                Some((r, worst, cols))
            })
            .collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        let beam = beam.max(1);
        let mut lines = Vec::new();
        for (r, shallow, cols) in rows.iter().take(beam) {
            let a = ours[*r];
            let mut replies = Vec::new();
            for &c in cols.iter().take(beam) {
                let b = theirs[c];
                let pair = self.pair(a, b);
                let v = self.chance(
                    state,
                    decision,
                    suspension,
                    pair,
                    Next::Nash,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                )?;
                if !v.is_nan() {
                    replies.push((b, v));
                }
            }
            replies.sort_by(|x, y| x.1.total_cmp(&y.1));
            let deep = replies.first().map_or(f32::NAN, |&(_, v)| v);
            lines.push(DeepLine {
                ours: a,
                shallow: *shallow,
                deep,
                replies,
            });
        }
        lines.sort_by(|x, y| y.deep.total_cmp(&x.deep));
        let shallow_rest: Vec<(Choice<N>, f32)> = rows
            .iter()
            .skip(beam)
            .map(|(r, worst, _)| (ours[*r], *worst))
            .collect();
        Ok(DeepAnalysis {
            decision,
            lines,
            shallow_rest,
            beam,
            outcome_cap: self.config.outcome_cap,
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
            unsupported: self.unsupported.clone(),
            omitted_pairs: self.omitted_pairs,
        })
    }

    /// Values a fixed plan of ours (one turn choice per entry, in the form `legal_choices`
    /// produces) against a perfect-information opponent (model ①): at each turn our choice is
    /// the plan's, the opponent answers with the reply that hurts us most, chance as
    /// configured. Replacement and mid-turn switch decisions are not part of the plan and are
    /// solved by maximin. A plan entry that is not legal in the position it reaches (the
    /// actives changed) counts as broken there and the solver falls back to maximin for that
    /// turn. After the plan's last turn the maximin tree continues for `config.depth - 1`
    /// turns, then the evaluator. `state` is left unchanged.
    pub fn evaluate_plan(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        plan: &[Choice<N>],
    ) -> Result<PlanReport<N>, SearchError> {
        let started = Instant::now();
        self.reset_counters();
        let decision = game::decision(state, suspension)?;
        if matches!(decision, Decision::Over(_)) {
            return Err(SearchError::Turn(TurnError::BattleOver));
        }
        let them = self.config.us.other();
        let theirs = self.choices(state, decision, them)?;
        let (ours, broken_here) = self.plan_choices(state, decision, plan, 0)?;
        let next = if decision == Decision::Turn {
            Next::Plan(plan, 1)
        } else {
            Next::Plan(plan, 0)
        };
        let threads = self.config.worker_threads(ours.len() * theirs.len());
        let values = if threads > 1 {
            self.parallel_matrix(state, suspension, decision, &ours, &theirs, next, threads)?
        } else {
            let mut values = Vec::with_capacity(ours.len() * theirs.len());
            for &a in &ours {
                for &b in &theirs {
                    let pair = self.pair(a, b);
                    values.push(self.chance(
                        state,
                        decision,
                        suspension,
                        pair,
                        next,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    )?);
                }
            }
            values
        };
        let mut replies: Vec<(Choice<N>, f32)> = Vec::new();
        let mut best = f32::NEG_INFINITY;
        for (r, _) in ours.iter().enumerate() {
            let mut worst = f32::INFINITY;
            let mut row = Vec::with_capacity(theirs.len());
            for (c, &b) in theirs.iter().enumerate() {
                let v = values[r * theirs.len() + c];
                if v.is_nan() {
                    continue;
                }
                worst = worst.min(v);
                row.push((b, v));
            }
            if worst > best && worst != f32::INFINITY {
                best = worst;
                replies = row;
            }
        }
        if best == f32::NEG_INFINITY {
            return Err(SearchError::Unsupported(self.unsupported.clone()));
        }
        replies.sort_by(|x, y| x.1.total_cmp(&y.1));
        // Phase 2: the worst replies again, with the child positions after the plan valued by
        // their own equilibrium (only for a one-turn plan at a turn decision: the chance node
        // right after our choice).
        let mut child = None;
        if self.config.child_nash && plan.len() == 1 && decision == Decision::Turn {
            let beam = self.config.reply_beam.unwrap_or(replies.len()).max(1);
            let a = plan[0];
            let mut child_replies = Vec::new();
            for &(b, _) in replies.iter().take(beam) {
                let pair = self.pair(a, b);
                let v = self.chance(
                    state,
                    decision,
                    suspension,
                    pair,
                    Next::Nash,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                )?;
                if !v.is_nan() {
                    child_replies.push((b, v));
                }
            }
            child_replies.sort_by(|x, y| x.1.total_cmp(&y.1));
            let value = child_replies.first().map_or(f32::NAN, |&(_, v)| v);
            child = Some(ChildValues {
                value,
                replies: child_replies,
                beam,
                outcome_cap: self.config.outcome_cap,
            });
        }
        Ok(PlanReport {
            decision,
            value: best,
            replies,
            broken: self.plan_broken + u32::from(broken_here),
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
            unsupported: self.unsupported.clone(),
            omitted_pairs: self.omitted_pairs,
            child,
        })
    }

    /// Our choices at a plan node: the plan's entry when it is legal here, else every legal
    /// choice (a broken plan). Non-turn decisions always take every legal choice.
    fn plan_choices(
        &mut self,
        state: &State<N>,
        decision: Decision,
        plan: &[Choice<N>],
        index: usize,
    ) -> Result<(Vec<Choice<N>>, bool), SearchError> {
        let legal = self.choices(state, decision, self.config.us)?;
        if decision != Decision::Turn {
            return Ok((legal, false));
        }
        match plan.get(index) {
            Some(choice) if legal.contains(choice) => Ok((vec![*choice], false)),
            Some(_) => Ok((legal, true)),
            None => Ok((legal, false)),
        }
    }

    /// The plan's value from a position: our plan entry (or maximin) against the worst reply.
    fn plan_value(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        plan: &[Choice<N>],
        index: usize,
        alpha: f32,
        beta: f32,
    ) -> Result<f32, SearchError> {
        if index >= plan.len() {
            // The plan is over: the ordinary tree for the remaining depth.
            return self.value(
                state,
                suspension,
                self.config.depth.saturating_sub(1),
                alpha,
                beta,
            );
        }
        self.nodes += 1;
        let decision = game::decision(state, suspension)?;
        if let Decision::Over(result) = decision {
            return Ok(self.terminal(result, self.config.depth));
        }
        let them = self.config.us.other();
        let (ours, broken) = self.plan_choices(state, decision, plan, index)?;
        if broken {
            self.plan_broken += 1;
        }
        let theirs = self.choices(state, decision, them)?;
        let next = if decision == Decision::Turn {
            Next::Plan(plan, index + 1)
        } else {
            Next::Plan(plan, index)
        };
        let mut best = f32::NEG_INFINITY;
        for &a in &ours {
            let mut worst = f32::INFINITY;
            for &b in &theirs {
                let lo = alpha.max(best);
                let hi = beta.min(worst);
                let pair = self.pair(a, b);
                let v = self.chance(state, decision, suspension, pair, next, lo, hi)?;
                if v.is_nan() {
                    continue;
                }
                worst = worst.min(v);
                if worst <= lo {
                    break;
                }
            }
            if worst == f32::INFINITY {
                continue;
            }
            best = best.max(worst);
            if best >= beta {
                break;
            }
        }
        if best == f32::NEG_INFINITY {
            return Ok(f32::NAN);
        }
        Ok(best)
    }
}

/// Removes their replies (columns) with an unevaluable cell, then our choices (rows) still
/// holding one. Returns the kept choices, the dense matrix and how many were dropped.
#[allow(clippy::type_complexity)]
fn drop_unevaluable<const N: usize>(
    ours: Vec<Choice<N>>,
    theirs: Vec<Choice<N>>,
    values: Vec<f32>,
) -> (Vec<Choice<N>>, Vec<Choice<N>>, Vec<f32>, usize, usize) {
    let (n, m) = (ours.len(), theirs.len());
    let keep_col: Vec<bool> = (0..m)
        .map(|c| (0..n).all(|r| !values[r * m + c].is_nan()))
        .collect();
    let keep_row: Vec<bool> = (0..n)
        .map(|r| (0..m).all(|c| !keep_col[c] || !values[r * m + c].is_nan()))
        .collect();
    let mut dense = Vec::new();
    for r in 0..n {
        if !keep_row[r] {
            continue;
        }
        for c in 0..m {
            if keep_col[c] {
                dense.push(values[r * m + c]);
            }
        }
    }
    let omitted_theirs = keep_col.iter().filter(|k| !**k).count();
    let omitted_ours = keep_row.iter().filter(|k| !**k).count();
    let ours: Vec<Choice<N>> = ours
        .into_iter()
        .zip(&keep_row)
        .filter(|(_, k)| **k)
        .map(|(c, _)| c)
        .collect();
    let theirs: Vec<Choice<N>> = theirs
        .into_iter()
        .zip(&keep_col)
        .filter(|(_, k)| **k)
        .map(|(c, _)| c)
        .collect();
    (ours, theirs, dense, omitted_ours, omitted_theirs)
}

/// [`Solver::evaluate_plan`]'s result.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanReport<const N: usize> {
    pub decision: Decision,
    /// The plan's value against the worst reply sequence (from our side).
    pub value: f32,
    /// The opponent's root replies with the value each leads to, worst first.
    pub replies: Vec<(Choice<N>, f32)>,
    /// How many positions along the way had a plan entry that was not legal there.
    pub broken: u32,
    pub nodes: u64,
    pub turns: u64,
    pub elapsed: Duration,
    pub unsupported: Vec<String>,
    pub omitted_pairs: usize,
    /// [`Config::child_nash`]: the plan's value when the positions after it are worth their
    /// own next-turn equilibrium (the worst replies only, capped outcomes).
    pub child: Option<ChildValues<N>>,
}

/// [`Solver::best_response`]'s result: our choices valued against a fixed mixed strategy
/// of the opponent (opponent model ③ of DESIGN.md at one turn: the strategy comes from the
/// matrix game on the team the opponent believes we have; the values are on the real
/// position).
#[derive(Clone, Debug, PartialEq)]
pub struct BestResponse<const N: usize> {
    pub decision: Decision,
    /// Our choices with their expected value against the strategy, best first.
    pub lines: Vec<(Choice<N>, f32)>,
    /// The strategy that was answered (their choices with probabilities), as given.
    pub strategy: Vec<(Choice<N>, f32)>,
    pub nodes: u64,
    pub turns: u64,
    pub elapsed: Duration,
    pub unsupported: Vec<String>,
    pub omitted_pairs: usize,
}

/// [`Solver::analyse_deep`]'s result.
#[derive(Clone, Debug, PartialEq)]
pub struct DeepAnalysis<const N: usize> {
    pub decision: Decision,
    /// The beam's choices, best deep value first.
    pub lines: Vec<DeepLine<N>>,
    /// The other choices with their shallow (leaf-valued maximin) value only, best first.
    pub shallow_rest: Vec<(Choice<N>, f32)>,
    pub beam: usize,
    pub outcome_cap: Option<usize>,
    pub nodes: u64,
    pub turns: u64,
    pub elapsed: Duration,
    pub unsupported: Vec<String>,
    pub omitted_pairs: usize,
}

/// One choice of ours in a [`DeepAnalysis`].
#[derive(Clone, Debug, PartialEq)]
pub struct DeepLine<const N: usize> {
    pub ours: Choice<N>,
    /// Its maximin value with leaf evaluation after this turn.
    pub shallow: f32,
    /// Its value against the beam's worst replies with children worth their next-turn
    /// equilibrium (NaN when none was evaluable).
    pub deep: f32,
    /// The replies valued deeply, worst first.
    pub replies: Vec<(Choice<N>, f32)>,
}

/// [`PlanReport::child`].
#[derive(Clone, Debug, PartialEq)]
pub struct ChildValues<const N: usize> {
    /// The worst reply's value under the child equilibrium (NaN when none was evaluable).
    pub value: f32,
    /// The replies valued, worst first.
    pub replies: Vec<(Choice<N>, f32)>,
    pub beam: usize,
    pub outcome_cap: Option<usize>,
}

impl<'e, const N: usize, E: Evaluator<N> + ?Sized + Sync> Solver<'e, N, E> {
    /// Values every choice of ours on `state` against the opponent's fixed mixed `strategy`
    /// (their choices with probabilities; choices missing from it have probability 0; pairs
    /// the engine cannot value are dropped and the row is renormalised over what remains).
    /// Depth and chance as configured. `state` is left unchanged.
    pub fn best_response(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        strategy: &[(Choice<N>, f32)],
    ) -> Result<BestResponse<N>, SearchError> {
        let started = Instant::now();
        self.reset_counters();
        let decision = game::decision(state, suspension)?;
        if matches!(decision, Decision::Over(_)) {
            return Err(SearchError::Turn(TurnError::BattleOver));
        }
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs: Vec<Choice<N>> = strategy
            .iter()
            .filter(|(_, p)| *p > 0.0)
            .map(|(c, _)| *c)
            .collect();
        if theirs.is_empty() {
            return Err(SearchError::NoChoice(self.config.us.other()));
        }
        let weights: Vec<f32> = strategy
            .iter()
            .filter(|(_, p)| *p > 0.0)
            .map(|(_, p)| *p)
            .collect();
        let next_depth = if decision == Decision::Turn {
            self.config.depth.max(1) - 1
        } else {
            self.config.depth.max(1)
        };
        let threads = self.config.worker_threads(ours.len() * theirs.len());
        let values = self.parallel_matrix(
            state,
            suspension,
            decision,
            &ours,
            &theirs,
            Next::Depth(next_depth),
            threads.max(1),
        )?;
        let m = theirs.len();
        let mut lines: Vec<(Choice<N>, f32)> = Vec::with_capacity(ours.len());
        for (r, &a) in ours.iter().enumerate() {
            let mut sum = 0.0f64;
            let mut mass = 0.0f64;
            for (c, &w) in weights.iter().enumerate() {
                let v = values[r * m + c];
                if v.is_nan() {
                    continue;
                }
                sum += f64::from(w) * f64::from(v);
                mass += f64::from(w);
            }
            if mass > 0.0 {
                lines.push((a, (sum / mass) as f32));
            }
        }
        if lines.is_empty() {
            return Err(SearchError::Unsupported(self.unsupported.clone()));
        }
        lines.sort_by(|x, y| y.1.total_cmp(&x.1));
        Ok(BestResponse {
            decision,
            lines,
            strategy: strategy.to_vec(),
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
            unsupported: self.unsupported.clone(),
            omitted_pairs: self.omitted_pairs,
        })
    }
}
