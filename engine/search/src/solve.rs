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

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use lab_engine::eval::Evaluator;
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State};
use lab_engine::turn::{EnumerateOptions, RollMode, Suspension, TurnError};

use crate::choice::Choice;
use crate::game::{self, Decision, Pruning};
use crate::nash::{self, Equilibrium, Matrix};
use crate::tt::{self, DeepTable, TranspositionTable};

/// What a chance node continues into: the maximin tree with `depth` turns left, or a fixed
/// plan (`Solver::evaluate_plan`) at its next entry. (Children worth their own equilibrium,
/// with only the `Config::outcome_cap` most probable outcomes, are valued in batches by
/// `Solver::nash_cells`.)
#[derive(Clone, Copy, Debug)]
enum Next<'p, const N: usize> {
    Depth(u32),
    Plan(&'p [Choice<N>], usize),
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
    /// Solve child matrix games (the depth-2 children of `deep`, `deep-nash`, `--child-nash`)
    /// on the game reduced by iterated weak dominance ([`nash::solve_reduced`], board S24d):
    /// the same value within the solver's tolerance, several times fewer cells per RM+
    /// iteration. Root games keep the full matrix, so their reported strategies and the
    /// deep-nash beams taken from them do not change.
    pub dominance: bool,
    /// Solve child matrix games by double oracle over lazily valued cells ([`LazyGame`],
    /// board S24d): the value within the solver's tolerance from a fraction of the cells.
    /// Off: every cell of every child is valued (`lab-plan --full-children`).
    pub double_oracle: bool,
    /// Split the cells of replacement and mid-turn child games (each a whole maximin turn)
    /// into rows on the pool (board S24-t2; `lab-plan --split`). Off by default: measured on
    /// the bench (runs/search-bench-20260927) the lagging cutoff costs 9–16% more
    /// enumerations and the wall time did not improve at 1–6 threads (same values).
    pub split_heavy_cells: bool,
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
            dominance: true,
            double_oracle: true,
            split_heavy_cells: false,
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
    /// Depth 3 and beyond (board S24c): deep child analyses answered by the deep table.
    pub deep_tt_hits: u64,
    /// Deep child analyses run (and stored).
    pub deep_tt_misses: u64,
    /// Replacement / mid-turn child cells valued row by row (board S24-t2).
    pub split_cells: u64,
}

impl SearchStats {
    fn add(&mut self, other: &SearchStats) {
        self.tt_hits += other.tt_hits;
        self.tt_misses += other.tt_misses;
        self.nash_solves += other.nash_solves;
        self.nash_iterations += other.nash_iterations;
        self.nash_seconds += other.nash_seconds;
        self.enumerate_seconds += other.enumerate_seconds;
        self.deep_tt_hits += other.deep_tt_hits;
        self.deep_tt_misses += other.deep_tt_misses;
        self.split_cells += other.split_cells;
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
        )?;
        if self.deep_tt_hits + self.deep_tt_misses > 0 {
            write!(
                f,
                "; deep children {} hits / {} analysed",
                self.deep_tt_hits, self.deep_tt_misses
            )?;
        }
        if self.split_cells > 0 {
            write!(
                f,
                "; {} replacement cells split into rows",
                self.split_cells
            )?;
        }
        Ok(())
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
    /// Deep child values by position and levels (depth 3 and beyond, board S24c).
    deep_tt: DeepTable<N>,
    /// The worker pool, built on first parallel use ([`Config::threads`]).
    pool: Option<Arc<rayon::ThreadPool>>,
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
            deep_tt: DeepTable::new(config.transposition, tt::DEFAULT_CAPACITY),
            pool: None,
        }
    }

    /// Positions in the transposition table.
    pub fn tt_len(&self) -> usize {
        self.tt.len()
    }

    /// Deep child values in the deep table (depth 3 and beyond).
    pub fn deep_tt_len(&self) -> usize {
        self.deep_tt.len()
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
            if self.config.exact_lines {
                // Every row is valued in full, so the rows are independent: the payoff matrix
                // (in parallel with more than one thread; every cell by a fresh solver, so the
                // node counts and, among equal replies, the first in `theirs` order do not
                // depend on the thread count: board S24-t4), then each line is its row's
                // minimum. `max_turns` bounds the whole matrix.
                let values = self.parallel_matrix(
                    state,
                    suspension,
                    decision,
                    &ours,
                    &theirs,
                    Next::Depth(next_depth),
                    threads,
                )?;
                if self.config.max_turns.is_some_and(|max| self.turns > max) {
                    return Err(SearchError::Budget);
                }
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
                let alpha = best;
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

/// One cell valued by a worker's own solver: the value with that solver's node and turn
/// counts, stats, the unsupported reasons it met, the pairs it dropped and broken plans.
struct CellOut {
    value: Result<f32, SearchError>,
    nodes: u64,
    turns: u64,
    stats: SearchStats,
    unsupported: Vec<String>,
    omitted: usize,
    broken: u32,
}

impl CellOut {
    fn from_solver<const N: usize, E: Evaluator<N> + ?Sized>(
        value: Result<f32, SearchError>,
        solver: Solver<'_, N, E>,
    ) -> CellOut {
        CellOut {
            value,
            nodes: solver.nodes,
            turns: solver.turns,
            stats: solver.stats,
            unsupported: solver.unsupported,
            omitted: solver.omitted_pairs,
            broken: solver.plan_broken,
        }
    }
}

/// A child position whose one-turn equilibrium a batch solves ([`Solver::nash_cells`]).
struct ChildJob<const N: usize> {
    state: State<N>,
    suspension: Option<Suspension>,
    decision: Decision,
}

/// One outcome of a split heavy cell ([`Solver::child_cells`], board S24-t2): its value, or
/// the row group valuing its turn, or the error met there.
enum SplitValue {
    Fixed(f32),
    Group(usize),
    Error(SearchError),
}

/// The turn after a heavy cell's outcome, valued by maximin one row (our choice) per pool
/// item: the best row so far is the cutoff of the next round's rows.
struct RowGroup<const N: usize> {
    state: State<N>,
    suspension: Option<Suspension>,
    ours: Vec<Choice<N>>,
    theirs: Vec<Choice<N>>,
    /// The rows handed out so far.
    next_row: usize,
    /// The best row value so far (`-inf`: none evaluable yet).
    best: f32,
    /// The first error in row order.
    error: Option<(usize, SearchError)>,
}

/// The `cap` most probable outcomes, renormalised (all of them without a cap): the children
/// valued by their equilibrium ([`Solver::nash_cells`]).
fn cap_outcomes(mut outcomes: Vec<Outcome>, cap: Option<usize>) -> Vec<Outcome> {
    match cap {
        Some(cap) if outcomes.len() > cap => {
            outcomes.sort_by(|a, b| b.probability.total_cmp(&a.probability));
            outcomes.truncate(cap);
            let total: f64 = outcomes.iter().map(|o| o.probability).sum();
            for o in &mut outcomes {
                o.probability /= total;
            }
            outcomes
        }
        _ => outcomes,
    }
}

/// A lazy child game is valued in full once it has asked for more than this share of its
/// cells (double oracle then saves little) or after [`LAZY_MAX_ROUNDS`] rounds.
const LAZY_FULL_SHARE: f64 = 0.6;
const LAZY_MAX_ROUNDS: usize = 64;
/// Double oracle stops once the restricted equilibrium's exploitability in the full game is
/// at most this (evaluation points; the full RM+ solve typically ends at 0.02–0.08), or when
/// neither side's best response is new.
const LAZY_TOLERANCE: f32 = 0.05;

/// A child's matrix game (our choices × theirs) whose cells are valued on demand: double
/// oracle (McMahan, Gordon & Blum 2003) over a restricted set of rows and columns that grows
/// by the full game's best responses to the restricted equilibrium. Every row and column in
/// the restricted set is valued in full, so the best responses — and the exploitability of
/// the restricted equilibrium in the full game, which bounds the error of its value — are
/// exact. On the bench's positions it values 11–17% of the cells of a full game with the
/// value within 0.04 of the full solve (board S24d).
struct LazyGame<const N: usize> {
    ours: Vec<Choice<N>>,
    theirs: Vec<Choice<N>>,
    next_depth: u32,
    known: Vec<Option<f32>>,
    requested: Vec<bool>,
    pending: Vec<usize>,
    rows: Vec<usize>,
    cols: Vec<usize>,
    full: bool,
    rounds: usize,
    asked: usize,
    /// The restricted equilibrium the game ended with and its full-game exploitability.
    last: Option<(Equilibrium, f32)>,
}

/// What a lazy game does after a round.
enum StepOutcome {
    Value(f32),
    Grow {
        row: Option<usize>,
        col: Option<usize>,
    },
    Full,
}

struct Step {
    outcome: StepOutcome,
    solved: Option<Equilibrium>,
    /// The restricted equilibrium's exploitability in the full game (exact: the restricted
    /// rows and columns are valued in full); the full solve's own for a full game.
    exploitability: f32,
}

impl<const N: usize> LazyGame<N> {
    fn new(ours: Vec<Choice<N>>, theirs: Vec<Choice<N>>, next_depth: u32, full: bool) -> Self {
        let cells = ours.len() * theirs.len();
        let mut game = LazyGame {
            ours,
            theirs,
            next_depth,
            known: vec![None; cells],
            requested: vec![false; cells],
            pending: Vec::new(),
            rows: Vec::new(),
            cols: Vec::new(),
            full: false,
            rounds: 0,
            asked: 0,
            last: None,
        };
        if full {
            game.go_full();
        } else {
            game.grow(Some(0), Some(0));
        }
        game
    }

    fn request(&mut self, i: usize) {
        if !self.requested[i] {
            self.requested[i] = true;
            self.pending.push(i);
            self.asked += 1;
        }
    }

    /// The cells asked for since the last call, in ascending order.
    fn take_requests(&mut self) -> Vec<usize> {
        let mut pending = std::mem::take(&mut self.pending);
        pending.sort_unstable();
        pending
    }

    /// Adds a row and/or a column to the restricted game and asks for all their cells.
    fn grow(&mut self, row: Option<usize>, col: Option<usize>) {
        let (n, m) = (self.ours.len(), self.theirs.len());
        self.rounds += 1;
        if let Some(r) = row {
            self.rows.push(r);
            for c in 0..m {
                self.request(r * m + c);
            }
        }
        if let Some(c) = col {
            self.cols.push(c);
            for r in 0..n {
                self.request(r * m + c);
            }
        }
        if self.asked as f64 > LAZY_FULL_SHARE * (n * m) as f64 || self.rounds > LAZY_MAX_ROUNDS {
            self.go_full();
        }
    }

    fn go_full(&mut self) {
        self.full = true;
        for i in 0..self.known.len() {
            self.request(i);
        }
    }

    /// The next step once every requested cell is known.
    fn step(&self, dominance: bool) -> Step {
        let m = self.theirs.len();
        if self.full || self.known.iter().flatten().any(|v| v.is_nan()) {
            if !self.full {
                // An unevaluable pair: `drop_unevaluable` needs the whole matrix.
                return Step {
                    outcome: StepOutcome::Full,
                    solved: None,
                    exploitability: f32::NAN,
                };
            }
            let values: Vec<f32> = self
                .known
                .iter()
                .map(|v| v.expect("a full game knows every cell"))
                .collect();
            let (ours, theirs, values, _, _) =
                drop_unevaluable(self.ours.clone(), self.theirs.clone(), values);
            if ours.is_empty() || theirs.is_empty() {
                return Step {
                    outcome: StepOutcome::Value(f32::NAN),
                    solved: None,
                    exploitability: f32::NAN,
                };
            }
            let matrix = Matrix::new(ours.len(), theirs.len(), values);
            let eq = if dominance {
                nash::solve_reduced(&matrix, 20_000, 0.01)
            } else {
                nash::solve(&matrix, 20_000, 0.01)
            };
            let exploitability = eq.exploitability;
            return Step {
                outcome: StepOutcome::Value(eq.value),
                solved: Some(eq),
                exploitability,
            };
        }
        let at = |r: usize, c: usize| self.known[r * m + c].expect("a restricted row or column");
        let mut sub = Vec::with_capacity(self.rows.len() * self.cols.len());
        for &r in &self.rows {
            for &c in &self.cols {
                sub.push(at(r, c));
            }
        }
        let eq = nash::solve(
            &Matrix::new(self.rows.len(), self.cols.len(), sub),
            20_000,
            0.001,
        );
        // Best responses in the full game: every row against their restricted strategy (the
        // restricted columns are known in full), every column against ours.
        let mut best_row = (0, f32::NEG_INFINITY);
        for r in 0..self.ours.len() {
            let v: f32 = self
                .cols
                .iter()
                .zip(&eq.cols)
                .map(|(&c, &p)| p * at(r, c))
                .sum();
            if v > best_row.1 {
                best_row = (r, v);
            }
        }
        let mut best_col = (0, f32::INFINITY);
        for c in 0..m {
            let v: f32 = self
                .rows
                .iter()
                .zip(&eq.rows)
                .map(|(&r, &p)| p * at(r, c))
                .sum();
            if v < best_col.1 {
                best_col = (c, v);
            }
        }
        let exploitability = best_row.1 - best_col.1;
        let row = (!self.rows.contains(&best_row.0) && best_row.1 > eq.value).then_some(best_row.0);
        let col = (!self.cols.contains(&best_col.0) && best_col.1 < eq.value).then_some(best_col.0);
        let outcome = if exploitability <= LAZY_TOLERANCE || (row.is_none() && col.is_none()) {
            StepOutcome::Value(eq.value)
        } else {
            StepOutcome::Grow { row, col }
        };
        Step {
            outcome,
            solved: Some(eq),
            exploitability,
        }
    }
}

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
    /// Every level (the first is `beam` and `outcome_cap`); the analysis looks
    /// `levels.len() + 1` turns ahead.
    pub levels: Vec<DeepLevel>,
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

/// Both sides' beams of a deep level from its shallow game (board S21): each side's `beam`
/// best choices by expected value against the other side's shallow equilibrium strategy plus
/// its own shallow support ([`beam_indices`]), in the shallow game's order. Cells a lazy
/// shallow game never valued have probability 0 on the other side and are skipped.
#[allow(clippy::type_complexity)]
fn deep_beams<const N: usize>(
    shallow: &MixedAnalysis<N>,
    beam: usize,
) -> (Vec<Choice<N>>, Vec<Choice<N>>) {
    let (n, m) = (shallow.ours.len(), shallow.theirs.len());
    let eq = &shallow.equilibrium;
    let our_value: Vec<f32> = (0..n)
        .map(|r| {
            (0..m)
                .filter(|&c| eq.cols[c] > 0.0)
                .map(|c| eq.cols[c] * shallow.matrix.at(r, c))
                .sum()
        })
        .collect();
    let their_value: Vec<f32> = (0..m)
        .map(|c| {
            (0..n)
                .filter(|&r| eq.rows[r] > 0.0)
                .map(|r| eq.rows[r] * shallow.matrix.at(r, c))
                .sum()
        })
        .collect();
    let our_beam = beam_indices(&our_value, &eq.rows, beam, true);
    let their_beam = beam_indices(&their_value, &eq.cols, beam, false);
    (
        our_beam.iter().map(|&r| shallow.ours[r]).collect(),
        their_beam.iter().map(|&c| shallow.theirs[c]).collect(),
    )
}

/// How many deep levels a decision inside the tree uses (board S24c): a turn one, a
/// replacement or mid-turn switch none (a one-turn equilibrium already looks through it to the
/// next turn). The one place the depth budget is counted, so a cost budget (a turn 1, a
/// replacement a fraction, as PokaiTrainer grows its subgames) can replace the per-level
/// lists later without touching the recursion.
fn level_cost(decision: Decision) -> usize {
    match decision {
        Decision::Turn => 1,
        Decision::MidTurn | Decision::Replacement | Decision::Over(_) => 0,
    }
}

/// One level of a deep mixed analysis ([`Solver::analyse_deep_mixed_levels`], board S24c):
/// how many choices of each side's shallow ranking enter the level's matrix (besides the
/// shallow support, [`MIXED_SUPPORT`]) and how many of a pair's most probable outcomes are
/// followed (renormalised; `None`: every outcome).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeepLevel {
    pub beam: usize,
    pub outcomes: Option<usize>,
}

impl<'e, const N: usize, E: Evaluator<N> + ?Sized + Sync> Solver<'e, N, E> {
    /// [`Solver::analyse_mixed`] by double oracle over lazily valued cells (`lab-plan --solve
    /// nash --lazy`, `lab-rollout --lazy`; board S24d): the same equilibrium within the
    /// solver's tolerance from a fraction of the pairs. The result differs in what it knows:
    /// `matrix` holds NaN for the pairs never valued (every row and column that ever entered
    /// the restricted game is valued in full), `equilibrium.exploitability` is the exact
    /// exploitability in the full game, and `maximin` is the best pure row among the rows
    /// valued in full (a lower bound on the full game's pure maximin). A root that meets an
    /// unevaluable pair, or would value most of its pairs anyway, falls back to
    /// [`Solver::analyse_mixed`].
    pub fn analyse_mixed_lazy(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
    ) -> Result<MixedAnalysis<N>, SearchError> {
        self.reset_counters();
        self.mixed_lazy_at(state, suspension, self.config.depth)
    }

    /// [`Solver::analyse_mixed_lazy`] at `depth` without resetting the counters (also the
    /// shallow game of a deep child, board S24c).
    fn mixed_lazy_at(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        depth: u32,
    ) -> Result<MixedAnalysis<N>, SearchError> {
        let started = Instant::now();
        let decision = game::decision(state, suspension)?;
        if matches!(decision, Decision::Over(_)) {
            return Err(SearchError::Turn(TurnError::BattleOver));
        }
        let them = self.config.us.other();
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs = self.choices(state, decision, them)?;
        let depth = depth.max(1);
        let next_depth = if decision == Decision::Turn {
            depth - 1
        } else {
            depth
        };
        let jobs = [(
            0,
            ChildJob {
                state: state.clone(),
                suspension: suspension.cloned(),
                decision,
            },
        )];
        let mut games = [Ok(LazyGame::new(
            ours.clone(),
            theirs.clone(),
            next_depth,
            false,
        ))];
        let value = self.drive_games(&jobs, &mut games).pop().expect("one game");
        let [game] = games;
        let game = game?;
        value?;
        let (n, m) = (ours.len(), theirs.len());
        let restricted = game.last.clone().filter(|_| !game.full);
        let Some((eq, exploitability)) = restricted else {
            // Valued in full after all (an unevaluable pair, or most pairs needed): the full
            // analysis from the known cells, as `analyse_mixed` would finish it.
            let values: Vec<f32> = game
                .known
                .iter()
                .map(|v| v.expect("a full game knows every cell"))
                .collect();
            let (ours, theirs, values, omitted_ours, omitted_theirs) =
                drop_unevaluable(ours, theirs, values);
            if ours.is_empty() || theirs.is_empty() {
                return Err(SearchError::Unsupported(self.unsupported.clone()));
            }
            let matrix = Matrix::new(ours.len(), theirs.len(), values);
            let equilibrium = self.solve_matrix(&matrix);
            let maximin = matrix.maximin();
            return Ok(MixedAnalysis {
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
            });
        };
        let mut rows = vec![0.0f32; n];
        for (&r, &p) in game.rows.iter().zip(&eq.rows) {
            rows[r] = p;
        }
        let mut cols = vec![0.0f32; m];
        for (&c, &p) in game.cols.iter().zip(&eq.cols) {
            cols[c] = p;
        }
        let values: Vec<f32> = game.known.iter().map(|v| v.unwrap_or(f32::NAN)).collect();
        let matrix = Matrix::new(n, m, values);
        let maximin = game
            .rows
            .iter()
            .map(|&r| {
                let worst = (0..m)
                    .map(|c| matrix.at(r, c))
                    .fold(f32::INFINITY, f32::min);
                (r, worst)
            })
            .fold(
                (0, f32::NEG_INFINITY),
                |best, x| if x.1 > best.1 { x } else { best },
            );
        Ok(MixedAnalysis {
            decision,
            ours,
            theirs,
            matrix,
            equilibrium: Equilibrium {
                rows,
                cols,
                value: eq.value,
                exploitability,
                iterations: eq.iterations,
            },
            maximin,
            depth,
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
            unsupported: self.unsupported.clone(),
            omitted_theirs: 0,
            omitted_ours: 0,
        })
    }

    /// Values every pair of choices at the root exactly (no cutoffs) and solves the matrix
    /// game by regret matching. `state` is left unchanged. Costs `ours × theirs` chance nodes,
    /// each with the full subtree of depth `config.depth - 1`.
    pub fn analyse_mixed(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
    ) -> Result<MixedAnalysis<N>, SearchError> {
        self.reset_counters();
        self.mixed_at(state, suspension, self.config.depth)
    }

    /// [`Solver::analyse_mixed`] at `depth` without resetting the counters (also the shallow
    /// game of a deep child, board S24c).
    fn mixed_at(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        depth: u32,
    ) -> Result<MixedAnalysis<N>, SearchError> {
        let started = Instant::now();
        let decision = game::decision(state, suspension)?;
        if matches!(decision, Decision::Over(_)) {
            return Err(SearchError::Turn(TurnError::BattleOver));
        }
        let them = self.config.us.other();
        let ours = self.choices(state, decision, self.config.us)?;
        let theirs = self.choices(state, decision, them)?;
        let depth = depth.max(1);
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

    /// The worker pool (`Config::threads`, 0 = every core), built on first use.
    fn pool(&mut self) -> Option<Arc<rayon::ThreadPool>> {
        let threads = self.config.worker_threads(usize::MAX);
        if threads <= 1 {
            return None;
        }
        if self.pool.is_none() {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("lab-search-{i}"))
                .build()
                .expect("a search thread pool");
            self.pool = Some(Arc::new(pool));
        }
        self.pool.clone()
    }

    /// `f` over `items` on the pool (in order on this thread with one worker); every worker
    /// split starts from `init()`. The results are in item order, so nothing downstream depends
    /// on how the items were scheduled.
    fn par_map<T, S, R>(
        &mut self,
        items: &[T],
        init: impl Fn() -> S + Sync + Send,
        f: impl Fn(&mut S, &T) -> R + Sync + Send,
    ) -> Vec<R>
    where
        T: Sync,
        R: Send,
    {
        match self.pool() {
            Some(pool) if items.len() > 1 => {
                pool.install(|| items.par_iter().map_init(&init, &f).collect())
            }
            _ => {
                let mut s = init();
                items.iter().map(|x| f(&mut s, x)).collect()
            }
        }
    }

    /// Adds a worker's counters and refusals (in the order the cells are merged).
    fn absorb(&mut self, cell: &CellOut) {
        self.nodes += cell.nodes;
        self.turns += cell.turns;
        self.omitted_pairs += cell.omitted;
        self.plan_broken += cell.broken;
        self.stats.add(&cell.stats);
        for why in &cell.unsupported {
            if !self.unsupported.contains(why) {
                self.unsupported.push(why.clone());
            }
        }
    }

    /// The payoff matrix (`ours × theirs`, row-major, exact chance values), every cell valued
    /// by its own solver on a copy of the state, on the worker pool (board S24p). Cells are
    /// handed out one at a time (pairs differ a lot in cost: a double Protect is one outcome,
    /// two spread moves thousands) and merged in cell order, so the values, the counters and
    /// the order of refusal reasons do not depend on the thread count.
    #[allow(clippy::too_many_arguments)]
    fn parallel_matrix(
        &mut self,
        state: &State<N>,
        suspension: Option<&Suspension>,
        decision: Decision,
        ours: &[Choice<N>],
        theirs: &[Choice<N>],
        next: Next<'_, N>,
        _threads: usize,
    ) -> Result<Vec<f32>, SearchError> {
        let config = self.config;
        let evaluator = self.evaluator;
        let cells: Vec<usize> = (0..ours.len() * theirs.len()).collect();
        let outs = self.par_map(
            &cells,
            || state.clone(),
            |local_state, &i| {
                let mut local = Solver::new(config, evaluator);
                let (a, b) = (ours[i / theirs.len()], theirs[i % theirs.len()]);
                let pair = local.pair(a, b);
                let value = local.chance(
                    local_state,
                    decision,
                    suspension,
                    pair,
                    next,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                );
                CellOut::from_solver(value, local)
            },
        );
        let mut values = Vec::with_capacity(outs.len());
        for out in outs {
            self.absorb(&out);
            values.push(out.value?);
        }
        Ok(values)
    }

    /// The values a chance node over the capped outcomes (window (-inf, inf)) gives for each
    /// of `pairs` at `state`, with every child position's matrix game solved in one batch on
    /// the worker pool (board S24p): the pairs are enumerated here, their capped outcomes'
    /// children looked up in the transposition table (repeats within the batch count as hits,
    /// as they would one after another), the cells of every child matrix run as one parallel
    /// pool of work, and the children's RM+ solves in parallel after them. The cell values,
    /// the counters and the table are what the pairs valued in turn would give; so is the
    /// first error, except that a pair's enumeration error is met before the child errors of
    /// earlier pairs, and `Config::max_turns` is checked per pair before the batch rather than
    /// per cell inside it.
    ///
    /// `cap` is the outcome cap of the pairs' chance nodes. With `rest` non-empty (depth 3
    /// and beyond, board S24c) each child is worth its own deep mixed analysis over the
    /// levels `rest` ([`Solver::deep_child`], one child after another, each on the pool
    /// inside) instead of its one-turn equilibrium.
    fn nash_cells(
        &mut self,
        state: &mut State<N>,
        decision: Decision,
        suspension: Option<&Suspension>,
        pairs: &[[Choice<N>; 2]],
        cap: Option<usize>,
        rest: &[DeepLevel],
    ) -> Result<Vec<f32>, SearchError> {
        enum Plan {
            Done(Result<f32, SearchError>),
            Outcomes(Vec<(f64, usize)>),
        }
        let mut plans = Vec::with_capacity(pairs.len());
        // One slot per child occurrence (a repeat within the batch points at the first).
        let mut slots: Vec<Option<Result<f32, SearchError>>> = Vec::new();
        let mut jobs: Vec<(usize, ChildJob<N>)> = Vec::new();
        let mut seen: HashMap<tt::PositionKey<N>, usize> = HashMap::new();
        for &pair in pairs {
            if let Some(max) = self.config.max_turns {
                if self.turns >= max {
                    plans.push(Plan::Done(Err(SearchError::Budget)));
                    continue;
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
                Ok(outcomes) => cap_outcomes(outcomes, cap),
                Err(TurnError::Unsupported(why)) => {
                    self.note_unsupported(why);
                    plans.push(Plan::Done(Ok(f32::NAN)));
                    continue;
                }
                Err(e) => {
                    plans.push(Plan::Done(Err(e.into())));
                    continue;
                }
            };
            let mut refs = Vec::with_capacity(outcomes.len());
            for outcome in &outcomes {
                state.apply(&outcome.instructions);
                self.nodes += 1;
                let slot = match game::decision(state, outcome.suspension.as_ref()) {
                    Err(e) => {
                        slots.push(Some(Err(e.into())));
                        slots.len() - 1
                    }
                    Ok(Decision::Over(result)) => {
                        slots.push(Some(Ok(self.terminal(result, 0))));
                        slots.len() - 1
                    }
                    Ok(_) if !rest.is_empty() => {
                        let value = self.deep_child(state, outcome.suspension.as_ref(), rest);
                        slots.push(Some(value));
                        slots.len() - 1
                    }
                    Ok(child_decision) => {
                        let key = if self.tt.enabled() {
                            Some((state.clone(), outcome.suspension.clone()))
                        } else {
                            None
                        };
                        let known = key.as_ref().and_then(|key| {
                            self.tt
                                .get(key)
                                .map(|v| {
                                    slots.push(Some(Ok(v)));
                                    slots.len() - 1
                                })
                                .or_else(|| seen.get(key).copied())
                        });
                        match known {
                            Some(slot) => {
                                self.stats.tt_hits += 1;
                                slot
                            }
                            None => {
                                if let Some(key) = key {
                                    self.stats.tt_misses += 1;
                                    seen.insert(key, slots.len());
                                }
                                slots.push(None);
                                jobs.push((
                                    slots.len() - 1,
                                    ChildJob {
                                        state: state.clone(),
                                        suspension: outcome.suspension.clone(),
                                        decision: child_decision,
                                    },
                                ));
                                slots.len() - 1
                            }
                        }
                    }
                };
                state.reverse(&outcome.instructions);
                refs.push((outcome.probability, slot));
            }
            plans.push(Plan::Outcomes(refs));
        }
        let values = self.solve_children(&jobs);
        for ((slot, job), value) in jobs.into_iter().zip(values) {
            if let Ok(v) = &value {
                if self.tt.enabled() {
                    self.tt.insert((job.state, job.suspension), *v);
                }
            }
            slots[slot] = Some(value);
        }
        let child = |slot: usize| -> Result<f32, SearchError> {
            slots[slot].clone().expect("every child slot is filled")
        };
        let mut out = Vec::with_capacity(plans.len());
        for plan in plans {
            let refs = match plan {
                Plan::Done(value) => {
                    out.push(value?);
                    continue;
                }
                Plan::Outcomes(refs) => refs,
            };
            // As `chance` with the window (-inf, inf): no cutoff, the first NaN child drops
            // the pair.
            let value = match self.config.chance {
                Chance::Worst => {
                    let mut worst = f32::INFINITY;
                    for &(_, slot) in &refs {
                        let v = child(slot)?;
                        if v.is_nan() {
                            worst = f32::NAN;
                            break;
                        }
                        worst = worst.min(v);
                    }
                    worst
                }
                Chance::Expect => {
                    let mut sum = 0.0f64;
                    let mut nan = false;
                    for &(p, slot) in &refs {
                        let v = child(slot)?;
                        if v.is_nan() {
                            nan = true;
                            break;
                        }
                        sum += p * v as f64;
                    }
                    if nan {
                        f32::NAN
                    } else {
                        sum as f32
                    }
                }
            };
            out.push(value);
        }
        Ok(out)
    }

    /// Each child's one-turn equilibrium value ([`Solver::nash_value`] without the table),
    /// all children at once: their cells on the worker pool, their matrix games solved in
    /// parallel. With [`Config::double_oracle`] a child's game is solved by double oracle over
    /// lazily valued cells ([`LazyGame`]); otherwise (and for a child whose lazy game meets an
    /// unevaluable cell or grows past [`LAZY_FULL_SHARE`] of its cells) every cell is valued.
    /// Results in job order; a child's error is its first failing cell's.
    fn solve_children(&mut self, jobs: &[(usize, ChildJob<N>)]) -> Vec<Result<f32, SearchError>> {
        let them = self.config.us.other();
        let mut games: Vec<Result<LazyGame<N>, SearchError>> = Vec::with_capacity(jobs.len());
        for (_, job) in jobs {
            let choices = self
                .choices(&job.state, job.decision, self.config.us)
                .and_then(|ours| Ok((ours, self.choices(&job.state, job.decision, them)?)));
            games.push(choices.map(|(ours, theirs)| {
                let next_depth = if job.decision == Decision::Turn { 0 } else { 1 };
                LazyGame::new(ours, theirs, next_depth, !self.config.double_oracle)
            }));
        }
        self.drive_games(jobs, &mut games)
    }

    /// Runs `games` (one per job) to their values: each round the cells every open game asks
    /// for are valued in one pool batch, then every open game takes its next step in
    /// parallel. A game ending on a restricted equilibrium keeps it in `LazyGame::last`.
    fn drive_games(
        &mut self,
        jobs: &[(usize, ChildJob<N>)],
        games: &mut [Result<LazyGame<N>, SearchError>],
    ) -> Vec<Result<f32, SearchError>> {
        let mut results: Vec<Option<Result<f32, SearchError>>> = games
            .iter()
            .map(|g| g.as_ref().err().map(|e| Err(e.clone())))
            .collect();
        loop {
            // The cells every open game asks for this round, valued in one batch.
            let mut tasks: Vec<(usize, usize)> = Vec::new();
            for (j, game) in games.iter_mut().enumerate() {
                if results[j].is_some() {
                    continue;
                }
                if let Ok(game) = game {
                    tasks.extend(game.take_requests().into_iter().map(|i| (j, i)));
                }
            }
            if tasks.is_empty() {
                break;
            }
            let values = self.child_cells(jobs, games, &tasks);
            for (&(j, i), value) in tasks.iter().zip(values) {
                if results[j].is_some() {
                    continue;
                }
                match value {
                    Ok(v) => {
                        if let Ok(game) = &mut games[j] {
                            game.known[i] = Some(v);
                        }
                    }
                    Err(e) => results[j] = Some(Err(e)),
                }
            }
            // Each open game's next step, in parallel: its restricted equilibrium and best
            // responses (double oracle), or its full game once every cell is known.
            let open: Vec<usize> = (0..games.len()).filter(|&j| results[j].is_none()).collect();
            let dominance = self.config.dominance;
            let games_ref = &*games;
            let steps = self.par_map(
                &open,
                || (),
                |_, &j| {
                    let game = games_ref[j].as_ref().expect("open games are prepared");
                    let started = Instant::now();
                    let step = game.step(dominance);
                    (step, started.elapsed().as_secs_f64())
                },
            );
            for (&j, (step, seconds)) in open.iter().zip(steps) {
                let game = games[j].as_mut().expect("open games are prepared");
                if let Some(eq) = &step.solved {
                    self.stats.nash_solves += 1;
                    self.stats.nash_iterations += eq.iterations as u64;
                }
                self.stats.nash_seconds += seconds;
                match step.outcome {
                    StepOutcome::Value(v) => {
                        if !game.full {
                            game.last = step.solved.map(|eq| (eq, step.exploitability));
                        }
                        results[j] = Some(Ok(v));
                    }
                    StepOutcome::Grow { row, col } => game.grow(row, col),
                    StepOutcome::Full => game.go_full(),
                }
            }
        }
        results
            .into_iter()
            .map(|r| r.expect("every child game ends with a value or an error"))
            .collect()
    }

    /// Values child cells `(job, cell)` on the pool, merging the workers' counters in task
    /// order.
    ///
    /// A cell of a replacement or mid-turn child (a child game with `next_depth` 1) is a whole
    /// maximin turn after the switch, often a thousand times a turn cell's work: as one pool
    /// item it left one worker busy long after the others ran dry (sand-owen deep-nash: 42 of
    /// 3,937 cells, 88% of the batch's CPU, the largest 16.5 s). Such cells are split (board
    /// S24-t2): their pair is enumerated here, and each outcome's next turn is valued row by
    /// row on the pool ([`RowGroup`]), in rounds of 1, 2, 4, ... rows whose cutoff is the best
    /// row of the earlier rounds (alpha-beta with a lagging alpha). The values are those of
    /// the unsplit cells bit for bit; the node and turn counts depend on the round sizes only,
    /// never on the thread count.
    fn child_cells(
        &mut self,
        jobs: &[(usize, ChildJob<N>)],
        games: &[Result<LazyGame<N>, SearchError>],
        tasks: &[(usize, usize)],
    ) -> Vec<Result<f32, SearchError>> {
        let config = self.config;
        let evaluator = self.evaluator;
        // Split the heavy cells: their outcomes, and a row group for each outcome's turn.
        let mut results: Vec<Option<Result<f32, SearchError>>> = vec![None; tasks.len()];
        let mut splits: Vec<(usize, Vec<(f64, SplitValue)>)> = Vec::new();
        let mut groups: Vec<RowGroup<N>> = Vec::new();
        let mut light: Vec<usize> = Vec::new();
        for (t, &(j, i)) in tasks.iter().enumerate() {
            let game = games[j].as_ref().expect("tasks come from prepared games");
            if game.next_depth == 0 || !self.config.split_heavy_cells {
                light.push(t);
                continue;
            }
            let job = &jobs[j].1;
            let pair = self.pair(
                game.ours[i / game.theirs.len()],
                game.theirs[i % game.theirs.len()],
            );
            match self.split_cell(job, pair, game.next_depth, &mut groups) {
                Ok(Some(outcomes)) => {
                    self.stats.split_cells += 1;
                    splits.push((t, outcomes));
                }
                Ok(None) => results[t] = Some(Ok(f32::NAN)),
                Err(e) => results[t] = Some(Err(e)),
            }
        }
        // Rounds: every light cell and the first row of every group, then 2, 4, ... more rows
        // of every group with the cutoff of the rows valued so far.
        enum Item {
            Light(usize),
            Row(usize, usize, f32),
        }
        let mut round = 0u32;
        loop {
            let mut items: Vec<Item> = Vec::new();
            if round == 0 {
                items.extend(light.iter().map(|&t| Item::Light(t)));
            }
            for (g, group) in groups.iter_mut().enumerate() {
                if group.error.is_some() {
                    continue;
                }
                let take = (1usize << round.min(20)).min(group.ours.len() - group.next_row);
                for r in group.next_row..group.next_row + take {
                    items.push(Item::Row(g, r, group.best));
                }
                group.next_row += take;
            }
            if items.is_empty() {
                break;
            }
            let groups_ref = &groups;
            let outs = self.par_map(
                &items,
                HashMap::<(bool, usize), State<N>>::new,
                |states, item| match *item {
                    Item::Light(t) => {
                        let (j, i) = tasks[t];
                        let job = &jobs[j].1;
                        let game = games[j].as_ref().expect("tasks come from prepared games");
                        let state = states
                            .entry((false, j))
                            .or_insert_with(|| job.state.clone());
                        let mut local = Solver::new(config, evaluator);
                        let (a, b) = (
                            game.ours[i / game.theirs.len()],
                            game.theirs[i % game.theirs.len()],
                        );
                        let pair = local.pair(a, b);
                        let value = local.chance(
                            state,
                            job.decision,
                            job.suspension.as_ref(),
                            pair,
                            Next::Depth(game.next_depth),
                            f32::NEG_INFINITY,
                            f32::INFINITY,
                        );
                        CellOut::from_solver(value, local)
                    }
                    Item::Row(g, r, lo) => {
                        let group = &groups_ref[g];
                        let state = states
                            .entry((true, g))
                            .or_insert_with(|| group.state.clone());
                        let mut local = Solver::new(config, evaluator);
                        let value = local.row_min(state, group, r, lo);
                        CellOut::from_solver(value, local)
                    }
                },
            );
            for (item, out) in items.iter().zip(outs) {
                self.absorb(&out);
                match *item {
                    Item::Light(t) => results[t] = Some(out.value),
                    Item::Row(g, r, _) => {
                        let group = &mut groups[g];
                        match out.value {
                            // The first error in row order is the group's.
                            Err(e) => {
                                if group.error.as_ref().is_none_or(|(at, _)| r < *at) {
                                    group.error = Some((r, e));
                                }
                            }
                            // No evaluable reply: the row does not count (as in `value`).
                            Ok(v) if v == f32::INFINITY => {}
                            Ok(v) => group.best = group.best.max(v),
                        }
                    }
                }
            }
            round += 1;
        }
        // Each split cell: its outcomes' values combined as `chance` does over (-inf, inf).
        for (t, outcomes) in splits {
            let value = |v: &SplitValue| -> Result<f32, SearchError> {
                match v {
                    SplitValue::Fixed(v) => Ok(*v),
                    SplitValue::Error(e) => Err(e.clone()),
                    SplitValue::Group(g) => {
                        let group = &groups[*g];
                        match &group.error {
                            Some((_, e)) => Err(e.clone()),
                            None if group.best == f32::NEG_INFINITY => Ok(f32::NAN),
                            None => Ok(group.best),
                        }
                    }
                }
            };
            let combined = match self.config.chance {
                Chance::Worst => {
                    let mut worst = f32::INFINITY;
                    let mut out = Ok(f32::NAN);
                    for (_, v) in &outcomes {
                        match value(v) {
                            Err(e) => {
                                out = Err(e);
                                break;
                            }
                            Ok(v) if v.is_nan() => {
                                out = Ok(f32::NAN);
                                break;
                            }
                            Ok(v) => {
                                worst = worst.min(v);
                                out = Ok(worst);
                            }
                        }
                    }
                    if outcomes.is_empty() {
                        Ok(f32::INFINITY)
                    } else {
                        out
                    }
                }
                Chance::Expect => {
                    let mut sum = 0.0f64;
                    let mut out = None;
                    for (p, v) in &outcomes {
                        match value(v) {
                            Err(e) => {
                                out = Some(Err(e));
                                break;
                            }
                            Ok(v) if v.is_nan() => {
                                out = Some(Ok(f32::NAN));
                                break;
                            }
                            Ok(v) => sum += p * v as f64,
                        }
                    }
                    out.unwrap_or(Ok(sum as f32))
                }
            };
            results[t] = Some(combined);
        }
        results
            .into_iter()
            .map(|r| r.expect("every child cell is valued"))
            .collect()
    }

    /// Enumerates a heavy cell's pair (the job's replacement or mid-turn decision) and makes a
    /// [`RowGroup`] for each outcome whose next decision is a turn (a battle over is its
    /// terminal value, anything else is valued whole here). `None`: the engine refused the
    /// pair (noted; the cell is NaN).
    #[allow(clippy::type_complexity)]
    fn split_cell(
        &mut self,
        job: &ChildJob<N>,
        pair: [Choice<N>; 2],
        depth: u32,
        groups: &mut Vec<RowGroup<N>>,
    ) -> Result<Option<Vec<(f64, SplitValue)>>, SearchError> {
        if let Some(max) = self.config.max_turns {
            if self.turns >= max {
                return Err(SearchError::Budget);
            }
        }
        self.turns += 1;
        let mut state = job.state.clone();
        let started = Instant::now();
        let transitions = game::transitions(
            &mut state,
            self.config.ruleset,
            self.config.enumerate_options(),
            job.decision,
            job.suspension.as_ref(),
            pair,
        );
        self.stats.enumerate_seconds += started.elapsed().as_secs_f64();
        let outcomes = match transitions {
            Ok(outcomes) => outcomes,
            Err(TurnError::Unsupported(why)) => {
                self.note_unsupported(why);
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let them = self.config.us.other();
        let mut out = Vec::with_capacity(outcomes.len());
        for outcome in &outcomes {
            state.apply(&outcome.instructions);
            // `value`'s entry at the outcome.
            self.nodes += 1;
            let value = match game::decision(&state, outcome.suspension.as_ref()) {
                Err(e) => Err(e.into()),
                Ok(Decision::Over(result)) => Ok(SplitValue::Fixed(self.terminal(result, depth))),
                Ok(Decision::Turn) if depth == 1 => {
                    match (
                        self.choices(&state, Decision::Turn, self.config.us),
                        self.choices(&state, Decision::Turn, them),
                    ) {
                        (Ok(ours), Ok(theirs)) => {
                            groups.push(RowGroup {
                                state: state.clone(),
                                suspension: outcome.suspension.clone(),
                                ours,
                                theirs,
                                next_row: 0,
                                best: f32::NEG_INFINITY,
                                error: None,
                            });
                            Ok(SplitValue::Group(groups.len() - 1))
                        }
                        (Err(e), _) | (_, Err(e)) => Err(e),
                    }
                }
                Ok(_) => {
                    // Rare (a replacement after a replacement, a deeper tree): whole, here.
                    self.nodes -= 1;
                    self.value(
                        &mut state,
                        outcome.suspension.as_ref(),
                        depth,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                    )
                    .map(SplitValue::Fixed)
                }
            };
            state.reverse(&outcome.instructions);
            // An error counts where `chance` would meet it: after the earlier outcomes.
            out.push((outcome.probability, value.unwrap_or_else(SplitValue::Error)));
        }
        Ok(Some(out))
    }

    /// One row of a [`RowGroup`]'s turn: the minimum over their replies of our choice `r`,
    /// stopping once it is at most `lo` (as `value` with alpha `lo`); `f32::INFINITY` when no
    /// reply is evaluable.
    fn row_min(
        &mut self,
        state: &mut State<N>,
        group: &RowGroup<N>,
        r: usize,
        lo: f32,
    ) -> Result<f32, SearchError> {
        let a = group.ours[r];
        let mut worst = f32::INFINITY;
        for &b in &group.theirs {
            let pair = self.pair(a, b);
            let v = self.chance(
                state,
                Decision::Turn,
                group.suspension.as_ref(),
                pair,
                Next::Depth(0),
                lo,
                worst,
            )?;
            if v.is_nan() {
                continue;
            }
            worst = worst.min(v);
            if worst <= lo {
                break;
            }
        }
        Ok(worst)
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
        let job = ChildJob {
            state: state.clone(),
            suspension: suspension.cloned(),
            decision,
        };
        let value = self
            .solve_children(&[(0, job)])
            .pop()
            .expect("one job, one value")?;
        if let Some(key) = key {
            self.tt.insert(key, value);
        }
        Ok(value)
    }

    /// Two turns deep, approximately: the root matrix with leaf values orders our choices
    /// (row minimum) and their replies (per row); then the `beam` best rows are valued again
    /// against their `beam` worst columns with each child position worth its own next-turn
    /// equilibrium ([`Solver::nash_cells`], `Config::outcome_cap`). `state` is left unchanged.
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
        let level = DeepLevel {
            beam,
            outcomes: self.config.outcome_cap,
        };
        self.analyse_deep_mixed_levels(state, suspension, &[level])
    }

    /// Deep mixed analysis over `levels.len() + 1` turns (board S24c; `lab-plan --solve
    /// deep-nash --beam 4,3 --outcomes 4,2`): the root as [`Solver::analyse_deep_mixed`] with
    /// the first level's beam and outcome cap, but each child position worth its own deep
    /// mixed analysis over the remaining levels (`Solver::deep_child`: its shallow game, both
    /// sides' beams, the beam pairs' capped outcomes valued one level further down) and the
    /// last level's children worth their one-turn equilibrium ([`Solver::nash_value`]). One
    /// level is [`Solver::analyse_deep_mixed`] itself.
    ///
    /// A level is a turn: inside the tree a replacement or mid-turn switch decision does not
    /// use one (its children continue with the same levels, as a one-turn equilibrium already
    /// looks through a replacement to the next turn). The root always uses its level, as
    /// [`Solver::analyse_deep_mixed`] always did. Child values are cached by position and
    /// levels ([`crate::tt::DeepTable`]). The root keeps its full shallow game (its strategies
    /// are reported); a child's shallow game is solved by double oracle with
    /// [`Config::double_oracle`] (the beams only need the value of every choice against the
    /// other side's restricted strategy, which the restricted rows and columns give exactly).
    /// `state` is left unchanged.
    pub fn analyse_deep_mixed_levels(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        levels: &[DeepLevel],
    ) -> Result<DeepMixedAnalysis<N>, SearchError> {
        let started = Instant::now();
        let first = *levels.first().ok_or_else(|| {
            SearchError::Unsupported(vec!["a deep analysis needs at least one level".into()])
        })?;
        let shallow = self.analyse_mixed(state, suspension)?;
        let decision = shallow.decision;
        let (ours, theirs) = deep_beams(&shallow, first.beam);
        let pairs: Vec<[Choice<N>; 2]> = ours
            .iter()
            .flat_map(|&a| theirs.iter().map(move |&b| (a, b)))
            .map(|(a, b)| self.pair(a, b))
            .collect();
        let values = self.nash_cells(
            state,
            decision,
            suspension,
            &pairs,
            first.outcomes,
            &levels[1..],
        )?;
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
            beam: first.beam.max(1),
            outcome_cap: first.outcomes,
            levels: levels.to_vec(),
            nodes: self.nodes,
            turns: self.turns,
            elapsed: started.elapsed(),
            unsupported: self.unsupported.clone(),
            omitted_theirs,
            omitted_ours,
        })
    }

    /// A child's deep value over `levels` (non-empty; board S24c): its shallow one-turn game
    /// (double oracle with [`Config::double_oracle`]), both sides' beams of `levels[0]`, the
    /// beam pairs' `levels[0].outcomes` most probable outcomes valued by the next level (the
    /// same levels after a replacement or mid-turn decision), the equilibrium value of that
    /// matrix. NaN when nothing is evaluable. Cached by position and levels.
    fn deep_child(
        &mut self,
        state: &mut State<N>,
        suspension: Option<&Suspension>,
        levels: &[DeepLevel],
    ) -> Result<f32, SearchError> {
        let key = if self.deep_tt.enabled() {
            let key = (
                levels.iter().map(|l| (l.beam, l.outcomes)).collect(),
                (state.clone(), suspension.cloned()),
            );
            if let Some(v) = self.deep_tt.get(&key) {
                self.stats.deep_tt_hits += 1;
                return Ok(v);
            }
            Some(key)
        } else {
            None
        };
        self.stats.deep_tt_misses += 1;
        let shallow = if self.config.double_oracle {
            self.mixed_lazy_at(state, suspension, 1)
        } else {
            self.mixed_at(state, suspension, 1)
        };
        let shallow = match shallow {
            Ok(shallow) => shallow,
            // Nothing evaluable here (the reasons are already noted): the pair above drops.
            Err(SearchError::Unsupported(_)) => return Ok(f32::NAN),
            Err(e) => return Err(e),
        };
        let decision = shallow.decision;
        let (ours, theirs) = deep_beams(&shallow, levels[0].beam);
        let pairs: Vec<[Choice<N>; 2]> = ours
            .iter()
            .flat_map(|&a| theirs.iter().map(move |&b| (a, b)))
            .map(|(a, b)| self.pair(a, b))
            .collect();
        let rest = &levels[level_cost(decision).min(levels.len())..];
        let values = self.nash_cells(
            state,
            decision,
            suspension,
            &pairs,
            levels[0].outcomes,
            rest,
        )?;
        let (ours, theirs, values, _, _) = drop_unevaluable(ours, theirs, values);
        let value = if ours.is_empty() || theirs.is_empty() {
            f32::NAN
        } else {
            let matrix = Matrix::new(ours.len(), theirs.len(), values);
            let started = Instant::now();
            let eq = if self.config.dominance {
                nash::solve_reduced(&matrix, 20_000, 0.01)
            } else {
                nash::solve(&matrix, 20_000, 0.01)
            };
            self.stats.nash_solves += 1;
            self.stats.nash_iterations += eq.iterations as u64;
            self.stats.nash_seconds += started.elapsed().as_secs_f64();
            eq.value
        };
        if let Some(key) = key {
            self.deep_tt.insert(key, value);
        }
        Ok(value)
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
        // Every beam row against its beam replies, the children solved in one batch.
        let pairs: Vec<[Choice<N>; 2]> = rows
            .iter()
            .take(beam)
            .flat_map(|(r, _, cols)| cols.iter().take(beam).map(move |&c| (*r, c)))
            .map(|(r, c)| self.pair(ours[r], theirs[c]))
            .collect();
        let mut cell_values = self
            .nash_cells(
                state,
                decision,
                suspension,
                &pairs,
                self.config.outcome_cap,
                &[],
            )?
            .into_iter();
        let mut lines = Vec::new();
        for (r, shallow, cols) in rows.iter().take(beam) {
            let a = ours[*r];
            let mut replies = Vec::new();
            for &c in cols.iter().take(beam) {
                let b = theirs[c];
                let v = cell_values.next().expect("one value per pair");
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
            let pairs: Vec<[Choice<N>; 2]> = replies
                .iter()
                .take(beam)
                .map(|&(b, _)| self.pair(a, b))
                .collect();
            let values = self.nash_cells(
                state,
                decision,
                suspension,
                &pairs,
                self.config.outcome_cap,
                &[],
            )?;
            let mut child_replies = Vec::new();
            for (&(b, _), v) in replies.iter().take(beam).zip(values) {
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
