//! Python bindings of lab-engine (`import lab_engine`): load an oracle scenario, list each
//! side's legal choices as Showdown choice strings, get a step's exact outcome distribution or
//! one sampled outcome, read a position as canonical JSON, evaluate it, and solve the one-turn
//! matrix game. The logic lives in `lab_search::node` (pure Rust, unit-tested there); this file
//! only converts arguments, errors and results. `engine/py/README.md` documents the API.
//!
//! Doubles only for now: `format` arguments take `"doubles"`, and positions dispatch through
//! [`AnyNode`] so a `Singles` variant (`Node<1>`) can be added without changing the Python API.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyNotImplementedError, PyOSError, PyValueError};
use pyo3::prelude::*;

// `::` 필수: `#[pymodule] fn lab_engine`이 같은 이름의 모듈을 만들어 의존 크레이트를 가린다.
use ::lab_engine::eval::{Heuristic, FEATURE_COUNT, FEATURE_NAMES};
use ::lab_engine::state::{BattleResult, SideId};
use ::lab_engine::turn::EnumerateOptions;
use lab_scenario::canonical::format_ruleset;
use lab_scenario::{
    apply_patch, canonical_json_hidden, load_scenario_file, load_scenario_str,
    state_from_canonical, LoadError, LoadedScenario, PatchJson, ScenarioError, ScenarioMeta,
};
use lab_search::nash::{self as rm, Matrix};
use lab_search::node::{
    decision_name, parse_pruning, parse_rolls, scenario_nodes, weights_by_name, NashTurn as RsNash,
    Node, NodeError,
};
use lab_search::Config;

create_exception!(
    lab_engine,
    EngineError,
    PyException,
    "Base class of the engine's own errors (bad arguments raise ValueError)."
);
create_exception!(
    lab_engine,
    Unsupported,
    EngineError,
    "The engine does not implement something the step needs (a move, ability, item or state). Search callers skip the pair, as lab-plan does."
);

fn py_err(e: NodeError) -> PyErr {
    match e {
        NodeError::Invalid(why) => PyValueError::new_err(why),
        NodeError::Unsupported(why) => Unsupported::new_err(why),
    }
}

fn scenario_err(e: ScenarioError) -> PyErr {
    match e {
        ScenarioError::Unsupported(why) => Unsupported::new_err(why),
        ScenarioError::Invalid(why) => PyValueError::new_err(why),
    }
}

/// A JSON argument: a dict (or any `json.dumps`-able value) or the JSON text itself.
fn json_arg(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let text = match value.extract::<String>() {
        Ok(text) => text,
        Err(_) => value
            .py()
            .import("json")?
            .call_method1("dumps", (value,))?
            .extract::<String>()?,
    };
    serde_json::from_str(&text).map_err(|e| PyValueError::new_err(format!("bad JSON: {e}")))
}

/// A position rebuilt from canonical JSON on `base` (a state of the same battle).
fn node_from_canonical(
    base: &::lab_engine::Doubles,
    meta: &Arc<ScenarioMeta>,
    value: &serde_json::Value,
) -> PyResult<Node<2>> {
    let rebuilt = state_from_canonical(base, meta, value).map_err(scenario_err)?;
    let ruleset = format_ruleset(&meta.format).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(Node {
        state: rebuilt.state,
        order: rebuilt.order,
        suspension: None,
        meta: Arc::clone(meta),
        ruleset,
    })
}

fn load_err(e: LoadError) -> PyErr {
    match e {
        LoadError::Io { .. } => PyOSError::new_err(e.to_string()),
        other => PyValueError::new_err(other.to_string()),
    }
}

/// The `format` argument: the only one the loader supports is `"doubles"`.
fn check_format(format: &str) -> PyResult<()> {
    match format {
        "doubles" => Ok(()),
        "singles" => Err(PyNotImplementedError::new_err(
            "singles scenarios are not supported yet (the loader builds doubles states only)",
        )),
        other => Err(PyValueError::new_err(format!(
            "unknown format {other:?}: \"doubles\" (\"singles\" later)"
        ))),
    }
}

/// A side: `0` / `1` or `"p1"` / `"p2"`.
#[derive(FromPyObject)]
enum SideArg {
    Index(i64),
    Name(String),
}

impl SideArg {
    fn side(&self) -> PyResult<SideId> {
        match self {
            SideArg::Index(0) => Ok(SideId::One),
            SideArg::Index(1) => Ok(SideId::Two),
            SideArg::Name(n) if n == "p1" => Ok(SideId::One),
            SideArg::Name(n) if n == "p2" => Ok(SideId::Two),
            SideArg::Index(i) => Err(PyValueError::new_err(format!("side {i}: 0 or 1"))),
            SideArg::Name(n) => Err(PyValueError::new_err(format!(
                "side {n:?}: \"p1\" or \"p2\""
            ))),
        }
    }
}

/// Evaluation weights: a dict `{feature name: weight}` (unnamed features keep the heuristic's)
/// or a list of all `FEATURE_NAMES` weights in order.
#[derive(FromPyObject)]
enum WeightsArg {
    Named(HashMap<String, f32>),
    List(Vec<f32>),
}

fn weights(arg: Option<WeightsArg>) -> PyResult<Option<[f32; FEATURE_COUNT]>> {
    match arg {
        None => Ok(None),
        Some(WeightsArg::Named(map)) => {
            let named: Vec<(String, f32)> = map.into_iter().collect();
            Ok(Some(weights_by_name(&named).map_err(py_err)?))
        }
        Some(WeightsArg::List(list)) => {
            let n = list.len();
            let array: [f32; FEATURE_COUNT] = list.try_into().map_err(|_| {
                PyValueError::new_err(format!(
                    "{n} weights given, {FEATURE_COUNT} needed (FEATURE_NAMES order)"
                ))
            })?;
            Ok(Some(array))
        }
    }
}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

/// A position of any supported format (doubles today).
#[derive(Clone)]
enum AnyNode {
    Doubles(Node<2>),
}

impl From<Node<2>> for AnyNode {
    fn from(node: Node<2>) -> Self {
        AnyNode::Doubles(node)
    }
}

/// Runs `$body` with `$n` bound to the concrete `Node<N>` inside an [`AnyNode`].
macro_rules! with_node {
    ($any:expr, $n:ident => $body:expr) => {
        match $any {
            AnyNode::Doubles($n) => $body,
        }
    };
}

impl AnyNode {
    fn format(&self) -> &'static str {
        match self {
            AnyNode::Doubles(_) => "doubles",
        }
    }
}

/// Library version.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Number of active slots per side for each supported format.
#[pyfunction]
fn slots(format: &str) -> PyResult<usize> {
    match format {
        "singles" => Ok(::lab_engine::Singles::SLOTS),
        "doubles" => Ok(::lab_engine::Doubles::SLOTS),
        other => Err(PyValueError::new_err(format!("unknown format: {other}"))),
    }
}

/// Loads an oracle scenario (`engine/oracle/scenarios/*.json` format): a path (str or
/// os.PathLike), or the JSON text itself (a str starting with `{`), whose relative team paths
/// resolve against `base_dir` (default: the current directory).
#[pyfunction]
#[pyo3(signature = (source, format = "doubles", base_dir = None))]
fn load_scenario(
    source: &Bound<'_, PyAny>,
    format: &str,
    base_dir: Option<PathBuf>,
) -> PyResult<Scenario> {
    check_format(format)?;
    let loaded = match source.extract::<String>() {
        Ok(text) if looks_like_json(&text) => {
            let base = base_dir.unwrap_or_else(|| PathBuf::from("."));
            load_scenario_str(&text, &base)
        }
        Ok(path) => load_scenario_file(path),
        Err(_) => load_scenario_file(source.extract::<PathBuf>()?),
    }
    .map_err(load_err)?;
    Ok(Scenario {
        loaded: Arc::new(loaded),
    })
}

/// Scenario JSON text rather than a path: an object, after an optional byte order mark.
fn looks_like_json(text: &str) -> bool {
    let text = text.trim_start_matches('\u{feff}');
    text.trim_start().starts_with('{')
}

/// Solves a zero-sum matrix game (rows maximize) by regret matching plus: the solver behind
/// `lab-plan --solve nash`. Stops after `iterations` or once the exploitability is at most
/// `tol`.
#[pyfunction]
#[pyo3(signature = (matrix, iterations = 20000, tol = 0.01))]
fn nash(
    py: Python<'_>,
    matrix: Vec<Vec<f32>>,
    iterations: usize,
    tol: f32,
) -> PyResult<Equilibrium> {
    let rows = matrix.len();
    let cols = matrix.first().map_or(0, Vec::len);
    if rows == 0 || cols == 0 {
        return Err(PyValueError::new_err("an empty matrix"));
    }
    if matrix.iter().any(|row| row.len() != cols) {
        return Err(PyValueError::new_err("the rows have different lengths"));
    }
    let game = Matrix::new(rows, cols, matrix.into_iter().flatten().collect());
    let eq = py.detach(|| rm::solve(&game, iterations, tol));
    Ok(Equilibrium {
        rows: eq.rows,
        cols: eq.cols,
        value: eq.value,
        exploitability: eq.exploitability,
        iterations: eq.iterations,
        maximin: game.maximin(),
    })
}

/// A loaded scenario: two teams, their team preview orders, setup turns, a patch and the turn
/// to check. `positions()` gives the positions its decision is made in.
#[pyclass(frozen, module = "lab_engine")]
struct Scenario {
    loaded: Arc<LoadedScenario>,
}

#[pymethods]
impl Scenario {
    /// The scenario's `description`.
    #[getter]
    fn description(&self) -> &str {
        &self.loaded.meta.description
    }

    /// `"doubles"`.
    #[getter]
    fn format(&self) -> &'static str {
        "doubles"
    }

    /// The Showdown format id (`gen9championsdoublescustomgame`, `gen9championsvgc2026regmc`).
    #[getter]
    fn showdown_format(&self) -> &str {
        &self.loaded.meta.format
    }

    /// The scenario's `turn` choices `(p1, p2)`, if any.
    #[getter]
    fn turn(&self) -> Option<(String, String)> {
        self.loaded
            .meta
            .turn
            .as_ref()
            .map(|t| (t.p1.clone(), t.p2.clone()))
    }

    /// The scenario's `midTurn` choices `(p1 list, p2 list)`.
    #[getter]
    fn mid_turn(&self) -> (Vec<String>, Vec<String>) {
        (
            self.loaded.mid_turn[0].clone(),
            self.loaded.mid_turn[1].clone(),
        )
    }

    /// The `setupTurns` as `(p1, p2)` choice pairs (mid-turn choices omitted).
    #[getter]
    fn setup_turns(&self) -> Vec<(String, String)> {
        self.loaded
            .setup_turns
            .iter()
            .map(|t| (t.p1.clone(), t.p2.clone()))
            .collect()
    }

    /// `side`'s display names in party (team preview) order.
    fn names(&self, side: SideArg) -> PyResult<Vec<String>> {
        let side = side.side()?;
        Ok(self.loaded.meta.sides[side.index()]
            .members
            .iter()
            .map(|m| m.name.clone())
            .collect())
    }

    /// The positions the scenario's decision is made in, as `(probability, Position)`: every
    /// outcome of the leads' switch-ins and the setup turns (replayed with `setup_rolls`:
    /// `full` is exact, `median`/`extremes`/`quartiles` approximate), then the patch.
    /// `lenient=True` drops replayed branches in which a setup turn's choices are illegal
    /// (`lab-plan --setup-lenient`).
    #[pyo3(signature = (setup_rolls = "full", lenient = false))]
    fn positions(
        &self,
        py: Python<'_>,
        setup_rolls: &str,
        lenient: bool,
    ) -> PyResult<Vec<(f64, Position)>> {
        let rolls = parse_rolls(setup_rolls, None).map_err(py_err)?;
        let loaded = Arc::clone(&self.loaded);
        let nodes = py
            .detach(move || scenario_nodes(&loaded, EnumerateOptions { rolls }, lenient))
            .map_err(py_err)?;
        Ok(nodes
            .into_iter()
            .map(|(p, node)| (p, Position { node: node.into() }))
            .collect())
    }

    /// A position built from canonical JSON (schema 1; a dict or the JSON text), as
    /// `Position.state_json()` writes it, with or without its `x-hidden` member: the scenario's
    /// teams supply what the canonical form leaves out (level, nature, Stat Points, move
    /// order). Hidden state the canonical form does not carry takes the defaults
    /// `engine/scenario/src/from_canonical.rs` lists; what has none (a transformed Pokémon, a
    /// pending future move, volatiles such as Leech Seed whose source is not canonical, a
    /// mid-turn switch) raises `Unsupported` unless `x-hidden` gives it. Genders the teams leave
    /// to chance stay undecided (`Position.from_canonical` on a position keeps its genders).
    #[allow(clippy::wrong_self_convention)] // the Python API name
    fn from_canonical(&self, state: &Bound<'_, PyAny>) -> PyResult<Position> {
        let value = json_arg(state)?;
        let meta = Arc::new(self.loaded.meta.clone());
        let node = node_from_canonical(&self.loaded.state, &meta, &value)?;
        Ok(Position { node: node.into() })
    }

    fn __repr__(&self) -> String {
        let names = |side: usize| -> String {
            self.loaded.meta.sides[side]
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "<Scenario {} ({}) p1: {} | p2: {}>",
            self.loaded.meta.format,
            self.format(),
            names(0),
            names(1)
        )
    }
}

/// A decision point of a battle: the state, both sides' Showdown party orders, and the
/// suspended turn when a mid-turn switch is pending. Immutable: steps return new positions.
#[pyclass(frozen, module = "lab_engine")]
struct Position {
    node: AnyNode,
}

#[pymethods]
impl Position {
    /// `"doubles"`.
    #[getter]
    fn format(&self) -> &'static str {
        self.node.format()
    }

    /// Showdown's turn counter (1 at the first decision).
    #[getter]
    fn turn(&self) -> u16 {
        with_node!(&self.node, n => n.state.turn)
    }

    /// What the position asks for: `"turn"`, `"replace"` (fainted Pokémon to replace),
    /// `"mid_turn_switch"` (a suspended turn: U-turn, Eject Button, ...) or `"finished"`.
    fn decision(&self) -> PyResult<&'static str> {
        with_node!(&self.node, n => n.decision().map(decision_name).map_err(py_err))
    }

    /// `None` while the battle goes on, else `"p1"`, `"p2"` or `"tie"`.
    fn winner(&self) -> Option<&'static str> {
        with_node!(&self.node, n => n.result()).map(|r| match r {
            BattleResult::Win(side) => side_name(side),
            _ => "tie",
        })
    }

    /// `side`'s legal choices as Showdown choice strings (`"move hypervoice, move protect"`,
    /// `"move 2 -1 mega"`, `"switch 3"`), the form `lab-plan` prints and `enumerate` accepts.
    /// A side the decision does not ask has the one choice `""`. `pruning="sensible"` drops
    /// damaging moves aimed at the ally (lab-plan's default). Moves the engine does not
    /// implement are left out.
    #[pyo3(signature = (side, pruning = "all"))]
    fn legal_choices(&self, side: SideArg, pruning: &str) -> PyResult<Vec<String>> {
        let side = side.side()?;
        let pruning = parse_pruning(pruning).map_err(py_err)?;
        with_node!(&self.node, n => n.legal_choices(side, pruning).map_err(py_err))
    }

    /// Every outcome of the two choice strings at this decision, as `(probability, Position)`
    /// with probabilities summing to 1. `rolls`: `full` (exact), `extremes`, `quartiles`,
    /// `median`, `pessimistic-p1`, `pessimistic-p2`. Raises `Unsupported` when the engine does
    /// not implement something the step needs, `ValueError` for bad or illegal choices.
    #[pyo3(signature = (p1, p2, rolls = "full"))]
    fn enumerate(
        &self,
        py: Python<'_>,
        p1: &str,
        p2: &str,
        rolls: &str,
    ) -> PyResult<Vec<(f64, Position)>> {
        let rolls = parse_rolls(rolls, None).map_err(py_err)?;
        let options = EnumerateOptions { rolls };
        with_node!(&self.node, n => {
            let children = py
                .detach(|| n.enumerate(p1, p2, options))
                .map_err(py_err)?;
            Ok(children
                .into_iter()
                .map(|(p, child)| (p, Position { node: child.into() }))
                .collect())
        })
    }

    /// One outcome of the two choice strings drawn with exact chance; the same `seed` gives
    /// the same position.
    fn sample(&self, py: Python<'_>, p1: &str, p2: &str, seed: u64) -> PyResult<Position> {
        with_node!(&self.node, n => {
            let child = py.detach(|| n.sample(p1, p2, seed)).map_err(py_err)?;
            Ok(Position { node: child.into() })
        })
    }

    /// The state as the oracle's canonical JSON (schema 1, `engine/oracle/canonical.cjs`).
    /// `hidden=True` appends the engine's `x-hidden` member: the state the canonical form
    /// leaves out (party order, hazard order, volatile payloads, ...), so that
    /// `from_canonical` gives back this exact position; the rest of the string is unchanged.
    #[pyo3(signature = (hidden = false))]
    fn state_json(&self, hidden: bool) -> PyResult<String> {
        if !hidden {
            return with_node!(&self.node, n => n.state_json().map_err(py_err));
        }
        with_node!(&self.node, n => {
            canonical_json_hidden(&n.state, &n.meta, Some(&n.order)).map_err(scenario_err)
        })
    }

    /// Another position of the same battle built from canonical JSON (a dict or the JSON
    /// text, `x-hidden` optional; see `Scenario.from_canonical`): this position supplies the
    /// set data, genders included. Edit `state_json(hidden=True)` and put it back to change a
    /// position by hand. A suspended turn is not carried.
    #[allow(clippy::wrong_self_convention)] // the Python API name
    fn from_canonical(&self, state: &Bound<'_, PyAny>) -> PyResult<Position> {
        let value = json_arg(state)?;
        with_node!(&self.node, n => {
            let node = node_from_canonical(&n.state, &n.meta, &value)?;
            Ok(Position { node: node.into() })
        })
    }

    /// This position with an oracle `patch` applied (the scenario files' `patch` format:
    /// `{"p1": {name: {"hp", "status", "statusTime", "boosts", "item"}}, "p2": ..., "sides":
    /// {"p1": {"tailwind": 3}}, "field": {"weather", "weatherDuration", "terrain",
    /// "terrainDuration", "pseudoWeather": {"trickroom": 5}}}`; a dict or the JSON text).
    fn with_patch(&self, patch: &Bound<'_, PyAny>) -> PyResult<Position> {
        let value = json_arg(patch)?;
        let patch: PatchJson = serde_json::from_value(value)
            .map_err(|e| PyValueError::new_err(format!("patch: {e}")))?;
        with_node!(&self.node, n => {
            if n.suspension.is_some() {
                return Err(PyValueError::new_err("a suspended turn cannot be patched"));
            }
            let mut node = n.clone();
            apply_patch(&mut node.state, &node.meta, &patch).map_err(PyValueError::new_err)?;
            Ok(Position { node: node.into() })
        })
    }

    /// The heuristic evaluation from `side`'s point of view (HP-bar units, 100 = one full
    /// bar; positive = `side` ahead), or a linear one with `weights` (a dict by feature name,
    /// unnamed ones keeping the heuristic's, or a list in `FEATURE_NAMES` order). A finished
    /// battle is still scored by material; check `winner()`.
    #[pyo3(signature = (side = SideArg::Index(0), weights = None))]
    fn evaluate(&self, side: SideArg, weights: Option<WeightsArg>) -> PyResult<f32> {
        let side = side.side()?;
        let w = self::weights(weights)?;
        Ok(with_node!(&self.node, n => n.evaluate(side, w.as_ref())))
    }

    /// The evaluation features (`FEATURE_NAMES` order), p1's counts minus p2's.
    fn features(&self) -> Vec<f32> {
        with_node!(&self.node, n => n.features().to_vec())
    }

    /// `side`'s party in team preview order, one dict per member: `name`, `species`, `hp`,
    /// `max_hp`, `status` (Showdown id, `""` when healthy), `item`, `ability`, `moves`
    /// (`[(id, pp)]`) and `slot` (active slot index or `None`).
    fn party<'py>(
        &self,
        py: Python<'py>,
        side: SideArg,
    ) -> PyResult<Vec<Bound<'py, pyo3::types::PyDict>>> {
        let side = side.side()?;
        let members = with_node!(&self.node, n => n.party(side));
        members
            .into_iter()
            .map(|m| {
                let d = pyo3::types::PyDict::new(py);
                d.set_item("name", m.name)?;
                d.set_item("species", m.species)?;
                d.set_item("hp", m.hp)?;
                d.set_item("max_hp", m.max_hp)?;
                d.set_item("status", m.status)?;
                d.set_item("item", m.item)?;
                d.set_item("ability", m.ability)?;
                d.set_item("moves", m.moves)?;
                d.set_item("slot", m.slot)?;
                Ok(d)
            })
            .collect()
    }

    /// `side`'s HP as `[(name, hp, max_hp)]` in team preview order.
    fn hp(&self, side: SideArg) -> PyResult<Vec<(String, i16, i16)>> {
        let side = side.side()?;
        Ok(with_node!(&self.node, n => n.party(side))
            .into_iter()
            .map(|m| (m.name, m.hp, m.max_hp))
            .collect())
    }

    /// The names in `side`'s active slots (`None` for an empty slot).
    fn active(&self, side: SideArg) -> PyResult<Vec<Option<String>>> {
        let side = side.side()?;
        Ok(with_node!(&self.node, n => {
            let meta = &n.meta.sides[side.index()];
            n.state
                .side(side)
                .slots
                .iter()
                .map(|slot| {
                    slot.party_index
                        .and_then(|p| meta.name(p))
                        .map(str::to_owned)
                })
                .collect()
        }))
    }

    /// `side`'s names in Showdown's current party order: `switch N` names entry `N - 1`.
    fn switch_order(&self, side: SideArg) -> PyResult<Vec<String>> {
        let side = side.side()?;
        Ok(with_node!(&self.node, n => {
            let meta = &n.meta.sides[side.index()];
            n.order[side.index()]
                .iter()
                .map(|&p| meta.name(p).unwrap_or("?").to_owned())
                .collect()
        }))
    }

    /// The one-turn matrix game from `side`'s point of view (`lab-plan --solve nash`): every
    /// pair of the two sides' choices valued after the exact chance node (`rolls`, default
    /// median) by the heuristic (or `weights`), `depth` turns deep, then solved by regret
    /// matching. Pairs that reach unimplemented effects are dropped and listed in
    /// `unsupported`. `threads=0` uses every core.
    #[pyo3(signature = (side = SideArg::Index(0), rolls = "median", depth = 1, pruning = "sensible", threads = 0, weights = None))]
    #[allow(clippy::too_many_arguments)] // Python keyword arguments
    fn nash_turn(
        &self,
        py: Python<'_>,
        side: SideArg,
        rolls: &str,
        depth: u32,
        pruning: &str,
        threads: usize,
        weights: Option<WeightsArg>,
    ) -> PyResult<NashTurn> {
        let us = side.side()?;
        let w = self::weights(weights)?;
        with_node!(&self.node, n => {
            let mut config = Config::new(n.ruleset, us);
            config.rolls = parse_rolls(rolls, Some(us)).map_err(py_err)?;
            config.pruning = parse_pruning(pruning).map_err(py_err)?;
            config.depth = depth.max(1);
            config.threads = threads;
            let result = py.detach(|| match w {
                Some(weights) => n.nash_turn(config, &::lab_engine::eval::Weighted { weights }),
                None => n.nash_turn(config, &Heuristic),
            });
            Ok(NashTurn::new(result.map_err(py_err)?, us))
        })
    }

    fn __eq__(&self, other: &Bound<'_, Position>) -> bool {
        match (&self.node, &other.get().node) {
            (AnyNode::Doubles(a), AnyNode::Doubles(b)) => a == b,
        }
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        with_node!(&self.node, n => n.hash(&mut h));
        h.finish()
    }

    fn __repr__(&self) -> String {
        with_node!(&self.node, n => {
            let decision = n.decision().map(decision_name).unwrap_or("invalid");
            let side = |s: SideId| -> String {
                n.party(s)
                    .iter()
                    .filter(|m| m.slot.is_some())
                    .map(|m| {
                        format!(
                            "{} {:.0}%",
                            m.name,
                            100.0 * f32::from(m.hp.max(0)) / f32::from(m.max_hp.max(1))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "<Position {} turn {} {}: p1 {} | p2 {}>",
                self.node.format(),
                n.state.turn,
                decision,
                side(SideId::One),
                side(SideId::Two)
            )
        })
    }
}

/// A mixed-strategy solution of a matrix game (`nash`).
#[pyclass(frozen, get_all, module = "lab_engine")]
struct Equilibrium {
    /// The row player's strategy, one probability per row.
    rows: Vec<f32>,
    /// The column player's strategy.
    cols: Vec<f32>,
    /// The row player's expected payoff under the two strategies.
    value: f32,
    /// Both players' best deviation gains summed (0 at an exact equilibrium).
    exploitability: f32,
    iterations: usize,
    /// The pure maximin `(row, value)`.
    maximin: (usize, f32),
}

#[pymethods]
impl Equilibrium {
    fn __repr__(&self) -> String {
        format!(
            "<Equilibrium {}x{} value {:+.3} exploitability {:.4} after {} iterations>",
            self.rows.len(),
            self.cols.len(),
            self.value,
            self.exploitability,
            self.iterations
        )
    }
}

/// `Position.nash_turn`'s result: the choice lists, the payoff matrix from `side`'s point of
/// view and the equilibrium.
#[pyclass(frozen, get_all, module = "lab_engine")]
struct NashTurn {
    /// The side the values are for (`"p1"` / `"p2"`).
    side: &'static str,
    /// The decision solved (`"turn"`, `"replace"`, `"mid_turn_switch"`).
    decision: &'static str,
    /// Our choices (matrix rows) and theirs (columns) as choice strings.
    ours: Vec<String>,
    theirs: Vec<String>,
    /// `len(ours) × len(theirs)` payoffs from `side`'s point of view.
    matrix: Vec<Vec<f32>>,
    /// Equilibrium strategies (one probability per row / column).
    rows: Vec<f32>,
    cols: Vec<f32>,
    value: f32,
    exploitability: f32,
    iterations: usize,
    /// The pure maximin `(row, value)` of the matrix.
    maximin: (usize, f32),
    /// Effects the engine refused in some cell; those columns and then rows were dropped.
    unsupported: Vec<String>,
    omitted_ours: usize,
    omitted_theirs: usize,
    /// Search nodes and turn enumerations used, and the wall time in seconds.
    nodes: u64,
    turns: u64,
    elapsed: f64,
}

impl NashTurn {
    fn new<const N: usize>(result: RsNash<N>, us: SideId) -> NashTurn {
        let a = result.analysis;
        let matrix = (0..a.matrix.rows)
            .map(|r| (0..a.matrix.cols).map(|c| a.matrix.at(r, c)).collect())
            .collect();
        NashTurn {
            side: side_name(us),
            decision: decision_name(a.decision),
            ours: result.ours,
            theirs: result.theirs,
            matrix,
            rows: a.equilibrium.rows,
            cols: a.equilibrium.cols,
            value: a.equilibrium.value,
            exploitability: a.equilibrium.exploitability,
            iterations: a.equilibrium.iterations,
            maximin: a.maximin,
            unsupported: a.unsupported,
            omitted_ours: a.omitted_ours,
            omitted_theirs: a.omitted_theirs,
            nodes: a.nodes,
            turns: a.turns,
            elapsed: a.elapsed.as_secs_f64(),
        }
    }
}

#[pymethods]
impl NashTurn {
    /// Our choices with equilibrium probability at least `min`, most likely first.
    #[pyo3(signature = (min = 0.01))]
    fn our_strategy(&self, min: f32) -> Vec<(String, f32)> {
        support(&self.ours, &self.rows, min)
    }

    /// Their choices with equilibrium probability at least `min`, most likely first.
    #[pyo3(signature = (min = 0.01))]
    fn their_strategy(&self, min: f32) -> Vec<(String, f32)> {
        support(&self.theirs, &self.cols, min)
    }

    fn __repr__(&self) -> String {
        format!(
            "<NashTurn {} {} {}x{} value {:+.1} exploitability {:.3}>",
            self.side,
            self.decision,
            self.ours.len(),
            self.theirs.len(),
            self.value,
            self.exploitability
        )
    }
}

fn support(choices: &[String], probabilities: &[f32], min: f32) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = choices
        .iter()
        .zip(probabilities)
        .filter(|(_, &p)| p >= min)
        .map(|(c, &p)| (c.clone(), p))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[pymodule]
fn lab_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(slots, m)?)?;
    m.add_function(wrap_pyfunction!(load_scenario, m)?)?;
    m.add_function(wrap_pyfunction!(nash, m)?)?;
    m.add_class::<Scenario>()?;
    m.add_class::<Position>()?;
    m.add_class::<NashTurn>()?;
    m.add_class::<Equilibrium>()?;
    m.add("EngineError", m.py().get_type::<EngineError>())?;
    m.add("Unsupported", m.py().get_type::<Unsupported>())?;
    m.add("FEATURE_NAMES", FEATURE_NAMES.to_vec())?;
    m.add("HEURISTIC_WEIGHTS", Heuristic::WEIGHTS.to_vec())?;
    Ok(())
}
