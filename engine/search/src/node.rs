//! A battle position as a library value: the core of the Python API (`engine/py`, PokaiEngine
//! style: load a position, list legal choices, step it exactly or by sampling, read it as
//! canonical JSON, evaluate it).
//!
//! A [`Node`] is everything needed to continue a battle from a decision: the state, both sides'
//! Showdown party orders (what `switch N` counts), the suspended turn when a mid-turn switch is
//! pending, and the scenario's sidecar names (for canonical JSON). Every step goes through
//! Showdown choice strings: [`Node::legal_choices`] writes them the way `lab-plan` prints them
//! ([`crate::format_choice`], [`crate::format_switches`]), [`Node::parse`] reads them the way
//! scenario files are read (`lab_scenario::parse_choice`, `parse_replacement`,
//! `parse_mid_turn`), and [`Node::enumerate`] / [`Node::sample`] return the next nodes.
//! Nothing here is game logic of its own: it composes `lab_scenario` (loading, parsing,
//! canonical JSON) with [`crate::game`] and [`crate::solve`].
//!
//! A step answers whichever decision the node asks for ([`Node::decision`]): a turn, the
//! replacement of fainted Pokémon, or the mid-turn switch of a suspended turn (U-turn, Eject
//! Button, ...). A side the decision does not ask gives the empty string.

use std::sync::Arc;

use lab_engine::eval::{features, Evaluator, Heuristic, Weighted, FEATURE_COUNT, FEATURE_NAMES};
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, State, Status};
use lab_engine::turn::{
    enumerate_replacements, sample_resume_turn, sample_turn, EnumerateOptions, RollMode,
    Suspension, TurnError,
};
use lab_scenario::canonical::format_ruleset;
use lab_scenario::{
    advance_order, canonical_json, parse_choice, parse_mid_turn, parse_replacement,
    scenario_positions_consistent, scenario_positions_with, LoadedScenario, PartyOrder,
    ScenarioMeta,
};

use crate::game::{self, asked_slots, Decision, Pruning};
use crate::solve::{Config, MixedAnalysis, SearchError, Solver};
use crate::{format_choice, format_switches, Choice};

/// Why a step was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeError {
    /// A choice string that does not parse, a choice the position does not allow, a step at a
    /// finished battle, or a bad argument.
    Invalid(String),
    /// The engine does not implement something the step needs (`TurnError::Unsupported`, a
    /// switch-in handler, a state with no canonical form yet). Search callers skip such pairs,
    /// as `lab-plan` does.
    Unsupported(String),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Invalid(why) | NodeError::Unsupported(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for NodeError {}

impl From<TurnError> for NodeError {
    fn from(e: TurnError) -> Self {
        match e {
            TurnError::Unsupported(_) => NodeError::Unsupported(e.to_string()),
            other => NodeError::Invalid(other.to_string()),
        }
    }
}

impl From<SearchError> for NodeError {
    fn from(e: SearchError) -> Self {
        match e {
            SearchError::Turn(e) => e.into(),
            SearchError::Unsupported(_) => NodeError::Unsupported(e.to_string()),
            other => NodeError::Invalid(other.to_string()),
        }
    }
}

/// An error message from the string-typed scenario functions: `TurnError::Unsupported`
/// (`"not implemented: ..."`) and the unsupported switch-in handlers (`"... that is not
/// implemented"`) are [`NodeError::Unsupported`], the rest [`NodeError::Invalid`].
fn classify(message: String) -> NodeError {
    if message.contains("not implemented") {
        NodeError::Unsupported(message)
    } else {
        NodeError::Invalid(message)
    }
}

/// A decision point of a battle (see the module documentation).
#[derive(Clone, Debug)]
pub struct Node<const N: usize> {
    pub state: State<N>,
    /// Showdown's party order per side (`lab_scenario::PartyOrder`).
    pub order: [PartyOrder; 2],
    /// The rest of a turn that stopped for a mid-turn switch (`Outcome::suspension`).
    pub suspension: Option<Suspension>,
    /// Names and format of the scenario the node comes from.
    pub meta: Arc<ScenarioMeta>,
    pub ruleset: Ruleset,
}

/// Two nodes are equal when the battle is: the same state, party orders and suspended turn
/// (the sidecar is not compared).
impl<const N: usize> PartialEq for Node<N> {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.order == other.order
            && self.suspension == other.suspension
    }
}

impl<const N: usize> Eq for Node<N> {}

impl<const N: usize> std::hash::Hash for Node<N> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.state.hash(h);
        self.order.hash(h);
        self.suspension.hash(h);
    }
}

/// One party member as [`Node::party`] reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// The scenario's display name (unique per side).
    pub name: String,
    /// The current species name (a Mega Evolution shows as its Mega forme).
    pub species: &'static str,
    pub hp: i16,
    pub max_hp: i16,
    /// Showdown's status id: `""`, `brn`, `frz`, `par`, `psn`, `tox`, `slp`, `fnt`.
    pub status: &'static str,
    /// Showdown ids; empty when none.
    pub item: &'static str,
    pub ability: &'static str,
    /// Move ids with their remaining PP.
    pub moves: Vec<(&'static str, u8)>,
    /// The active slot the member stands in (0-based), if any.
    pub slot: Option<usize>,
}

/// The name [`Node::decision`] kinds have in the Python API: `turn`, `replace`,
/// `mid_turn_switch`, `finished`.
pub fn decision_name(decision: Decision) -> &'static str {
    match decision {
        Decision::Turn => "turn",
        Decision::Replacement => "replace",
        Decision::MidTurn => "mid_turn_switch",
        Decision::Over(_) => "finished",
    }
}

/// A damage-roll mode by name: `full`, `extremes`, `quartiles`, `median`, `pessimistic-p1`,
/// `pessimistic-p2`, and `pessimistic` for `us` (the minimum roll for `us`'s attacks and the
/// maximum against it; only where a side is given).
pub fn parse_rolls(text: &str, us: Option<SideId>) -> Result<RollMode, NodeError> {
    Ok(match text {
        "full" => RollMode::Full,
        "extremes" => RollMode::Extremes,
        "quartiles" => RollMode::Quartiles,
        "median" => RollMode::Median,
        "pessimistic-p1" => RollMode::Pessimistic(SideId::One),
        "pessimistic-p2" => RollMode::Pessimistic(SideId::Two),
        "pessimistic" => match us {
            Some(side) => RollMode::Pessimistic(side),
            None => {
                return Err(NodeError::Invalid(
                    "rolls \"pessimistic\" needs a side here: pessimistic-p1 or pessimistic-p2"
                        .into(),
                ))
            }
        },
        other => {
            return Err(NodeError::Invalid(format!(
                "unknown rolls {other:?}: full, extremes, quartiles, median, pessimistic-p1, pessimistic-p2"
            )))
        }
    })
}

/// A pruning of the legal choices by name: `all` or `sensible` ([`Pruning`]).
pub fn parse_pruning(text: &str) -> Result<Pruning, NodeError> {
    match text {
        "all" => Ok(Pruning::All),
        "sensible" => Ok(Pruning::Sensible),
        other => Err(NodeError::Invalid(format!(
            "unknown pruning {other:?}: all or sensible"
        ))),
    }
}

/// Evaluation weights from `(feature name, weight)` pairs ([`FEATURE_NAMES`]); features not
/// named keep the [`Heuristic`] weight. An unknown name is an error.
pub fn weights_by_name(named: &[(String, f32)]) -> Result<[f32; FEATURE_COUNT], NodeError> {
    let mut weights = Heuristic::WEIGHTS;
    for (name, weight) in named {
        let i = FEATURE_NAMES
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| {
                NodeError::Invalid(format!(
                    "unknown feature {name:?}; the features are {}",
                    FEATURE_NAMES.join(", ")
                ))
            })?;
        weights[i] = *weight;
    }
    Ok(weights)
}

/// The positions a scenario's decision is made in (after the leads' switch-ins, the setup
/// turns and the patch; `lab_scenario::scenario_positions_with`), each with its probability.
/// `setup` is the damage-roll mode of the setup turns. `lenient` drops the replayed branches
/// in which a setup turn's choices are not legal instead of failing
/// (`scenario_positions_consistent`, `lab-plan --setup-lenient`).
pub fn scenario_nodes(
    loaded: &LoadedScenario,
    setup: EnumerateOptions,
    lenient: bool,
) -> Result<Vec<(f64, Node<2>)>, NodeError> {
    let meta = Arc::new(loaded.meta.clone());
    let ruleset = format_ruleset(&meta.format).map_err(|e| NodeError::Invalid(e.to_string()))?;
    let positions = if lenient {
        scenario_positions_consistent(loaded, setup, &mut |_, positions| positions)
    } else {
        scenario_positions_with(loaded, setup)
    }
    .map_err(classify)?;
    Ok(positions
        .into_iter()
        .map(|p| {
            (
                p.probability,
                Node {
                    state: p.state,
                    order: p.order,
                    suspension: None,
                    meta: Arc::clone(&meta),
                    ruleset,
                },
            )
        })
        .collect())
}

impl<const N: usize> Node<N> {
    /// The decision the node asks for.
    pub fn decision(&self) -> Result<Decision, NodeError> {
        Ok(game::decision(&self.state, self.suspension.as_ref())?)
    }

    /// `None` while the battle goes on; otherwise its result.
    pub fn result(&self) -> Option<BattleResult> {
        self.state.result.is_over().then_some(self.state.result)
    }

    /// `side`'s legal choices at the node's decision ([`game::legal_choices`]; a side the
    /// decision does not ask has the one choice `Choice::WAIT`). Empty once the battle is over.
    pub fn legal(&self, side: SideId, pruning: Pruning) -> Result<Vec<Choice<N>>, NodeError> {
        let decision = self.decision()?;
        Ok(game::legal_choices(
            &self.state,
            self.ruleset,
            decision,
            side,
            pruning,
        ))
    }

    /// [`Node::legal`] as Showdown choice strings (`move hypervoice, move protect`,
    /// `switch 3`; `""` for a side that waits), as `lab-plan` prints them.
    pub fn legal_choices(&self, side: SideId, pruning: Pruning) -> Result<Vec<String>, NodeError> {
        let decision = self.decision()?;
        Ok(self
            .legal(side, pruning)?
            .iter()
            .map(|c| self.describe(decision, side, c))
            .collect())
    }

    /// `choice` of `side` at `decision` as a Showdown choice string against the node's party
    /// order (the inverse of [`Node::parse`]).
    pub fn describe(&self, decision: Decision, side: SideId, choice: &Choice<N>) -> String {
        let order = &self.order[side.index()];
        match choice {
            Choice::Turn(action) => format_choice(&self.state, side, order, action),
            Choice::Switches(switches) => {
                let slots = asked_slots(&self.state, decision, side);
                format_switches(order, &slots, switches)
            }
        }
    }

    /// Parses `side`'s choice string for the node's decision. It is not checked for legality
    /// here; the step that runs it is.
    pub fn parse(&self, side: SideId, text: &str) -> Result<Choice<N>, NodeError> {
        let order = &self.order[side.index()];
        let parsed = match self.decision()? {
            Decision::Over(_) => return Err(NodeError::Invalid("the battle is over".into())),
            Decision::Turn => parse_choice(&self.state, side, order, text).map(Choice::Turn),
            Decision::Replacement => {
                parse_replacement(&self.state, side, order, text).map(Choice::Switches)
            }
            Decision::MidTurn => {
                parse_mid_turn(&self.state, side, order, text).map(Choice::Switches)
            }
        };
        parsed.map_err(|why| {
            let player = if side == SideId::One { "p1" } else { "p2" };
            NodeError::Invalid(format!("{player}: {why}"))
        })
    }

    fn parse_both(&self, p1: &str, p2: &str) -> Result<[Choice<N>; 2], NodeError> {
        Ok([self.parse(SideId::One, p1)?, self.parse(SideId::Two, p2)?])
    }

    /// Every outcome of `choices` (side one's first) with `options`, as instruction lists from
    /// this node's state ([`game::transitions`]).
    pub fn outcomes(
        &self,
        choices: [Choice<N>; 2],
        options: EnumerateOptions,
    ) -> Result<Vec<Outcome>, NodeError> {
        let decision = self.decision()?;
        let mut state = self.state.clone();
        Ok(game::transitions(
            &mut state,
            self.ruleset,
            options,
            decision,
            self.suspension.as_ref(),
            choices,
        )?)
    }

    /// The node `outcome` (one of this node's) leads to.
    pub fn child(&self, outcome: &Outcome) -> Node<N> {
        let mut state = self.state.clone();
        state.apply(&outcome.instructions);
        let mut order = self.order.clone();
        advance_order(&mut order, &outcome.instructions);
        Node {
            state,
            order,
            suspension: outcome.suspension.clone(),
            meta: Arc::clone(&self.meta),
            ruleset: self.ruleset,
        }
    }

    /// The outcome distribution of both sides' choice strings: `(probability, next node)` per
    /// distinct end state, probabilities summing to 1. `options` picks the damage rolls
    /// (`RollMode::Full` is exact; replacements have no rolls). A turn that stops for a
    /// mid-turn switch yields suspended nodes (decision `MidTurn`).
    pub fn enumerate(
        &self,
        p1: &str,
        p2: &str,
        options: EnumerateOptions,
    ) -> Result<Vec<(f64, Node<N>)>, NodeError> {
        let choices = self.parse_both(p1, p2)?;
        self.enumerate_choices(choices, options)
    }

    /// [`Node::enumerate`] with parsed choices.
    pub fn enumerate_choices(
        &self,
        choices: [Choice<N>; 2],
        options: EnumerateOptions,
    ) -> Result<Vec<(f64, Node<N>)>, NodeError> {
        Ok(self
            .outcomes(choices, options)?
            .iter()
            .map(|o| (o.probability, self.child(o)))
            .collect())
    }

    /// One outcome of both sides' choice strings drawn at random with exact chance (every
    /// damage roll; `sample_turn` / `sample_resume_turn` with one sample; a replacement's
    /// outcomes drawn by probability). Deterministic for a given `seed`.
    pub fn sample(&self, p1: &str, p2: &str, seed: u64) -> Result<Node<N>, NodeError> {
        let choices = self.parse_both(p1, p2)?;
        self.sample_choices(choices, seed)
    }

    /// [`Node::sample`] with parsed choices.
    pub fn sample_choices(&self, choices: [Choice<N>; 2], seed: u64) -> Result<Node<N>, NodeError> {
        let decision = self.decision()?;
        let mut state = self.state.clone();
        let outcomes = match (decision, choices) {
            (Decision::Over(_), _) => return Err(TurnError::BattleOver.into()),
            (Decision::Turn, [Choice::Turn(a), Choice::Turn(b)]) => {
                sample_turn(&mut state, self.ruleset, [a, b], 1, seed)?
            }
            (Decision::MidTurn, [Choice::Switches(a), Choice::Switches(b)]) => {
                let suspension = self.suspension.as_ref().ok_or_else(|| {
                    NodeError::Invalid("a mid-turn decision without a suspended turn".into())
                })?;
                sample_resume_turn(&mut state, suspension, [a, b], 1, seed)?
            }
            (Decision::Replacement, [Choice::Switches(a), Choice::Switches(b)]) => {
                let outcomes = enumerate_replacements(&mut state, [a, b])?;
                let weights: Vec<f64> = outcomes.iter().map(|o| o.probability).collect();
                let k = SplitMix64(seed).pick(&weights);
                outcomes.into_iter().skip(k).take(1).collect()
            }
            (d, c) => {
                return Err(NodeError::Invalid(format!(
                    "choices {c:?} do not fit the decision {d:?}"
                )))
            }
        };
        let outcome = outcomes
            .into_iter()
            .next()
            .ok_or_else(|| NodeError::Invalid("the step produced no outcome".into()))?;
        Ok(self.child(&outcome))
    }

    /// The state in the oracle's canonical JSON (schema 1, `engine/oracle/canonical.cjs`):
    /// the exact string `canonicalKey` gives for the same Showdown position.
    pub fn state_json(&self) -> Result<String, NodeError> {
        canonical_json(&self.state, &self.meta).map_err(|e| NodeError::Unsupported(e.to_string()))
    }

    /// The [`Heuristic`] evaluation (or a linear one with `weights` over [`FEATURE_NAMES`]),
    /// from `side`'s point of view: HP-bar units (100 = one full HP bar), positive when `side`
    /// is ahead. It does not look at the result: a finished battle is still scored by material.
    pub fn evaluate(&self, side: SideId, weights: Option<&[f32; FEATURE_COUNT]>) -> f32 {
        let value = match weights {
            Some(w) => Weighted { weights: *w }.evaluate(&self.state),
            None => Heuristic.evaluate(&self.state),
        };
        if side == SideId::One {
            value
        } else {
            -value
        }
    }

    /// The evaluation features, side one's counts minus side two's ([`FEATURE_NAMES`] order).
    pub fn features(&self) -> [f32; FEATURE_COUNT] {
        features(&self.state)
    }

    /// `side`'s party in party order (the team preview order; `switch N` counts
    /// [`Node::order`] instead).
    pub fn party(&self, side: SideId) -> Vec<Member> {
        let s = self.state.side(side);
        let meta = &self.meta.sides[side.index()];
        s.party
            .iter()
            .enumerate()
            .filter(|(_, mon)| !mon.species.is_none())
            .map(|(i, mon)| Member {
                name: meta.name(i as u8).unwrap_or("?").to_owned(),
                species: mon.species.data().name,
                hp: mon.hp,
                max_hp: mon.max_hp,
                status: status_id(mon.status),
                item: mon.item.id(),
                ability: mon.ability.id(),
                moves: mon
                    .moves
                    .iter()
                    .filter(|m| !m.id.is_none())
                    .map(|m| (m.id.id(), m.pp))
                    .collect(),
                slot: s
                    .slots
                    .iter()
                    .position(|slot| slot.party_index == Some(i as u8)),
            })
            .collect()
    }

    /// The one-turn matrix game at this node (`lab-plan --solve nash`,
    /// [`Solver::analyse_mixed`]): both sides' choices, the payoff matrix from `config.us`'s
    /// side valued by `evaluator` after the exact chance node (depth `config.depth`), and the
    /// regret-matching equilibrium. Pairs that reach unimplemented effects are dropped and
    /// listed (`MixedAnalysis::unsupported`).
    pub fn nash_turn<E: Evaluator<N> + ?Sized + Sync>(
        &self,
        config: Config,
        evaluator: &E,
    ) -> Result<NashTurn<N>, NodeError> {
        let mut solver = Solver::new(config, evaluator);
        let mut state = self.state.clone();
        let analysis = solver.analyse_mixed(&mut state, self.suspension.as_ref())?;
        let us = config.us;
        let ours = analysis
            .ours
            .iter()
            .map(|c| self.describe(analysis.decision, us, c))
            .collect();
        let theirs = analysis
            .theirs
            .iter()
            .map(|c| self.describe(analysis.decision, us.other(), c))
            .collect();
        Ok(NashTurn {
            analysis,
            ours,
            theirs,
        })
    }
}

/// [`Node::nash_turn`]'s result: the analysis with both choice lists as strings.
#[derive(Clone, Debug)]
pub struct NashTurn<const N: usize> {
    pub analysis: MixedAnalysis<N>,
    /// `analysis.ours` / `analysis.theirs` as choice strings (rows / columns of the matrix).
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
}

fn status_id(status: Status) -> &'static str {
    match status {
        Status::None => "",
        Status::Burn => "brn",
        Status::Freeze => "frz",
        Status::Paralyze => "par",
        Status::Poison => "psn",
        Status::Toxic => "tox",
        Status::Sleep => "slp",
        Status::Fainted => "fnt",
    }
}

/// SplitMix64 (Steele, Lea and Flood 2014): the draw of a replacement's outcome in
/// [`Node::sample`].
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// An index drawn with probability proportional to `weights` (the last one on rounding).
    fn pick(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut x = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * total;
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        weights.len().saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use lab_engine::turn::enumerate_turn;
    use lab_scenario::{load_scenario_file, load_scenario_str};

    use super::*;

    fn scenarios() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios")
    }

    fn nodes(name: &str) -> (LoadedScenario, Vec<(f64, Node<2>)>) {
        let loaded = load_scenario_file(scenarios().join(format!("{name}.json"))).unwrap();
        let nodes = scenario_nodes(&loaded, EnumerateOptions::default(), false).unwrap();
        (loaded, nodes)
    }

    fn turn(loaded: &LoadedScenario) -> (String, String) {
        let t = loaded.meta.turn.as_ref().unwrap();
        (t.p1.clone(), t.p2.clone())
    }

    const EXACT: EnumerateOptions = EnumerateOptions {
        rolls: RollMode::Full,
    };

    /// The node's choice strings are `game::legal_choices` written out, and each parses back
    /// to its choice.
    #[test]
    fn legal_choice_strings_round_trip() {
        let (_, nodes) = nodes("cc-lib-psy-cona-vs-sand-owen");
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0].1;
        assert_eq!(node.decision().unwrap(), Decision::Turn);
        // `lab-plan --rolls median` prints 54 / 110 choices (sensible pruning) and 78 / 178
        // with `--all-targets` for this scenario.
        for (side, sensible, all) in [(SideId::One, 54, 78), (SideId::Two, 110, 178)] {
            assert_eq!(node.legal(side, Pruning::Sensible).unwrap().len(), sensible);
            let choices = node.legal(side, Pruning::All).unwrap();
            let strings = node.legal_choices(side, Pruning::All).unwrap();
            assert_eq!(choices.len(), all);
            assert_eq!(strings.len(), all);
            for (choice, text) in choices.iter().zip(&strings) {
                assert_eq!(node.parse(side, text).unwrap(), *choice, "{text}");
            }
        }
    }

    /// `enumerate` is `enumerate_turn` with the outcomes applied: the same count and
    /// probabilities, children whose state is the outcome's.
    #[test]
    fn enumerate_matches_the_turn_engine() {
        let (loaded, nodes) = nodes("hypnosis-gravity");
        let (p1, p2) = turn(&loaded);
        for (_, node) in &nodes {
            let children = node.enumerate(&p1, &p2, EXACT).unwrap();
            let Choice::Turn(a) = node.parse(SideId::One, &p1).unwrap() else {
                panic!()
            };
            let Choice::Turn(b) = node.parse(SideId::Two, &p2).unwrap() else {
                panic!()
            };
            let mut state = node.state.clone();
            let outcomes = enumerate_turn(&mut state, node.ruleset, [a, b]).unwrap();
            assert_eq!(children.len(), outcomes.len());
            let total: f64 = children.iter().map(|(p, _)| p).sum();
            assert!((total - 1.0).abs() < 1e-12);
            for ((p, child), outcome) in children.iter().zip(&outcomes) {
                assert_eq!(*p, outcome.probability);
                let mut end = node.state.clone();
                end.apply(&outcome.instructions);
                assert_eq!(child.state, end);
                assert_eq!(child.decision().unwrap(), Decision::Turn);
                assert_eq!(child.state.turn, node.state.turn + 1);
            }
        }
    }

    /// A seed reproduces its sample; the sample is one of the enumerated outcomes.
    #[test]
    fn sample_is_reproducible() {
        let (loaded, nodes) = nodes("single-hit");
        let (p1, p2) = turn(&loaded);
        let node = &nodes[0].1;
        let children = node.enumerate(&p1, &p2, EXACT).unwrap();
        for seed in [1u64, 2, 20260927] {
            let a = node.sample(&p1, &p2, seed).unwrap();
            let b = node.sample(&p1, &p2, seed).unwrap();
            assert_eq!(a, b);
            assert!(children.iter().any(|(_, c)| *c == a), "seed {seed}");
        }
    }

    /// The initial node's JSON is the scenario position's canonical JSON.
    #[test]
    fn state_json_is_canonical() {
        let (loaded, nodes) = nodes("single-hit");
        let positions = lab_scenario::scenario_positions(&loaded).unwrap();
        assert_eq!(positions.len(), nodes.len());
        for ((_, node), position) in nodes.iter().zip(&positions) {
            assert_eq!(
                node.state_json().unwrap(),
                canonical_json(&position.state, &loaded.meta).unwrap()
            );
        }
    }

    /// A replacement decision: both sides replace, the strings are `switch N`, and the
    /// outcomes (Speed-tied weather starts) come back as turn nodes.
    #[test]
    fn replacement_steps() {
        let (loaded, nodes) = nodes("ko-replace");
        let (p1, p2) = turn(&loaded);
        let node = &nodes[0].1;
        assert_eq!(node.decision().unwrap(), Decision::Replacement);
        for side in [SideId::One, SideId::Two] {
            let choices = node.legal_choices(side, Pruning::All).unwrap();
            assert!(choices.contains(&"switch 3".to_owned()), "{choices:?}");
        }
        let children = node.enumerate(&p1, &p2, EXACT).unwrap();
        assert_eq!(children.len(), 2);
        for (p, child) in &children {
            assert!((p - 0.5).abs() < 1e-12);
            assert_eq!(child.decision().unwrap(), Decision::Turn);
        }
        let sampled = node.sample(&p1, &p2, 7).unwrap();
        assert!(children.iter().any(|(_, c)| *c == sampled));
    }

    /// A U-turn suspends the turn: the child asks p1 for a switch (p2 waits with `""`), and
    /// resuming it with `switch 3` gives the same distribution as the scenario with that
    /// `midTurn` choice.
    #[test]
    fn mid_turn_switch_steps() {
        let (loaded, nodes) = nodes("uturn-pause");
        let (p1, p2) = turn(&loaded);
        let node = &nodes[0].1;
        let paused = node.enumerate(&p1, &p2, EXACT).unwrap();
        assert!(!paused.is_empty());
        let mut resumed: Vec<(f64, Node<2>)> = Vec::new();
        for (p, child) in &paused {
            assert_eq!(child.decision().unwrap(), Decision::MidTurn);
            assert_eq!(
                child.legal_choices(SideId::Two, Pruning::All).unwrap(),
                vec![String::new()]
            );
            assert_eq!(
                child.legal_choices(SideId::One, Pruning::All).unwrap(),
                vec!["switch 3".to_owned()]
            );
            for (q, next) in child.enumerate("switch 3", "", EXACT).unwrap() {
                assert_ne!(next.decision().unwrap(), Decision::MidTurn);
                match resumed.iter_mut().find(|(_, n)| *n == next) {
                    Some(entry) => entry.0 += p * q,
                    None => resumed.push((p * q, next)),
                }
            }
            // Sampling a resume is one of its outcomes.
            let s = child.sample("switch 3", "", 3).unwrap();
            assert_ne!(s.decision().unwrap(), Decision::MidTurn);
        }
        let switch_loaded = load_scenario_file(scenarios().join("uturn-switch.json")).unwrap();
        let position = &lab_scenario::scenario_positions(&switch_loaded).unwrap()[0];
        let decision = lab_scenario::scenario_decision(&switch_loaded, position).unwrap();
        let mut state = position.state.clone();
        let reference = lab_scenario::run_decision_mid_turn(
            &mut state,
            &position.order,
            &decision,
            &switch_loaded.mid_turn,
        )
        .unwrap();
        // `run_decision_mid_turn` does not merge end states across paused outcomes.
        let mut merged: Vec<(f64, State<2>)> = Vec::new();
        for outcome in &reference {
            let mut end = position.state.clone();
            end.apply(&outcome.instructions);
            match merged.iter_mut().find(|(_, s)| *s == end) {
                Some(entry) => entry.0 += outcome.probability,
                None => merged.push((outcome.probability, end)),
            }
        }
        assert_eq!(resumed.len(), merged.len());
        for (q, end) in &merged {
            let (p, _) = resumed
                .iter()
                .find(|(_, n)| n.state == *end)
                .expect("the resumed outcome");
            assert!((p - q).abs() < 1e-12);
        }
    }

    /// Unimplemented effects are `Unsupported`; bad strings and illegal choices `Invalid`.
    #[test]
    fn errors_are_classified() {
        let (loaded, nodes) = nodes("hypnosis-gravity");
        let (p1, p2) = turn(&loaded);
        let node = &nodes[0].1;
        for bad in [
            "move",
            "move splash, move protect",
            "switch 9, move protect",
        ] {
            assert!(
                matches!(node.enumerate(bad, &p2, EXACT), Err(NodeError::Invalid(_))),
                "{bad}"
            );
        }
        // Parses, but Gardevoir holds no Mega Stone: the ruleset refuses it.
        assert!(matches!(
            node.enumerate("move hypnosis 1 mega, move fakeout 2", &p2, EXACT),
            Err(NodeError::Invalid(_))
        ));
        assert!(node.enumerate(&p1, &p2, EXACT).is_ok());

        // Metronome's `onHit` is not implemented.
        let team = r#"[{"species": "Clefable", "ability": "Magic Guard", "nature": "Serious",
            "evs": {"hp": 32}, "moves": ["Metronome", "Protect"], "level": 50},
            {"species": "Snorlax", "ability": "Thick Fat", "nature": "Serious",
            "evs": {"hp": 32}, "moves": ["Protect"], "level": 50}]"#;
        let json = format!(
            r#"{{"format": "gen9championsdoublescustomgame",
               "p1": {{"team": {team}, "order": "12"}}, "p2": {{"team": {team}, "order": "12"}}}}"#
        );
        let loaded = load_scenario_str(&json, &scenarios()).unwrap();
        let nodes = scenario_nodes(&loaded, EXACT, false).unwrap();
        let node = &nodes[0].1;
        // The generator leaves unimplemented moves out; naming one is refused as unsupported.
        let choices = node.legal_choices(SideId::One, Pruning::All).unwrap();
        assert!(
            !choices.iter().any(|c| c.contains("metronome")),
            "{choices:?}"
        );
        assert!(choices.contains(&"move protect, move protect".to_owned()));
        let result = node.enumerate(
            "move metronome, move protect",
            "move protect, move protect",
            EXACT,
        );
        assert!(
            matches!(result, Err(NodeError::Unsupported(_))),
            "{result:?}"
        );
        assert!(matches!(
            node.sample(
                "move metronome, move protect",
                "move protect, move protect",
                1
            ),
            Err(NodeError::Unsupported(_))
        ));
    }

    /// The evaluation is the heuristic from side one, negated for side two; named weights
    /// override the heuristic's.
    #[test]
    fn evaluation_sides_and_weights() {
        let (loaded, nodes) = nodes("hypnosis-gravity");
        let (p1, p2) = turn(&loaded);
        let children = nodes[0].1.enumerate(&p1, &p2, EXACT).unwrap();
        let child = &children[0].1;
        let one = child.evaluate(SideId::One, None);
        assert_eq!(one, Heuristic.evaluate(&child.state));
        assert_eq!(child.evaluate(SideId::Two, None), -one);
        assert_ne!(one, 0.0, "Tyranitar asleep");
        let same = weights_by_name(&[]).unwrap();
        assert_eq!(child.evaluate(SideId::One, Some(&same)), one);
        let heavier = weights_by_name(&[("sleep".into(), -90.0)]).unwrap();
        assert!(child.evaluate(SideId::One, Some(&heavier)) > one);
        assert!(weights_by_name(&[("nonsense".into(), 1.0)]).is_err());
        let features = child.features();
        let dot: f32 = features
            .iter()
            .zip(Heuristic::WEIGHTS)
            .map(|(f, w)| f * w)
            .sum();
        assert!((dot - one).abs() < 1e-3);
        let party = child.party(SideId::Two);
        assert!(party
            .iter()
            .any(|m| m.name == "Tyranitar" && m.status == "slp"));
        assert_eq!(party.iter().filter(|m| m.slot.is_some()).count(), 2);
    }

    #[test]
    fn roll_and_pruning_names() {
        assert_eq!(parse_rolls("full", None).unwrap(), RollMode::Full);
        assert_eq!(
            parse_rolls("pessimistic", Some(SideId::Two)).unwrap(),
            RollMode::Pessimistic(SideId::Two)
        );
        assert!(parse_rolls("pessimistic", None).is_err());
        assert!(parse_rolls("most", None).is_err());
        assert_eq!(parse_pruning("sensible").unwrap(), Pruning::Sensible);
        assert!(parse_pruning("some").is_err());
    }

    /// `nash_turn` is `lab-plan --solve nash`: on `uturn-pause` (median rolls) a 7×3 matrix
    /// with value +142.3 from p1.
    #[test]
    fn nash_turn_matches_lab_plan() {
        let (_, nodes) = nodes("uturn-pause");
        let node = &nodes[0].1;
        let mut config = Config::new(node.ruleset, SideId::One);
        config.rolls = RollMode::Median;
        config.threads = 1;
        let nash = node.nash_turn(config, &Heuristic).unwrap();
        assert_eq!(nash.ours.len(), 7);
        assert_eq!(nash.theirs.len(), 3);
        assert_eq!(
            (nash.analysis.matrix.rows, nash.analysis.matrix.cols),
            (7, 3)
        );
        assert!(
            (nash.analysis.equilibrium.value - 142.3).abs() < 0.1,
            "{}",
            nash.analysis.equilibrium.value
        );
        assert!(nash.ours.contains(&"move uturn 1, move harden".to_owned()));
    }
}
