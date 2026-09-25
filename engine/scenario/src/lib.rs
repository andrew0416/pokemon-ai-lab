//! Oracle scenario/team JSON → `lab_engine::State<2>` plus sidecar identity data.
//!
//! This crate is the only place serde touches the engine. It runs once per scenario, off the
//! search hot path; the `State` it produces holds no strings or JSON values. Names and other
//! set data the rules do not need live in [`meta::ScenarioMeta`] beside it.
//!
//! Scope: initial states only. A scenario is two teams, their team preview order and the
//! decision to check. The loaded state is the one right after leads are placed and **before
//! switch-in effects**. [`switch_in::initial_outcomes`] expands it into the weighted states
//! after the leads' start effects (the implemented subset: Trace, Sand Stream, Grassy Surge;
//! anything else that could act is rejected); one of those is Showdown's `before` snapshot.
//! [`canonical::canonical_json`] writes a state in the oracle's canonical form (schema 1).
//! Scenario features that need rules the engine does not have (`setupTurns`, `patch`) are
//! rejected, never skipped.

pub mod canonical;
pub mod error;
pub mod json;
pub mod meta;
pub mod switch_in;
pub mod team;

use std::path::Path;

use serde_json::Value;

use lab_engine::state::{SideId, State};
use lab_engine::Doubles;

pub use canonical::{canonical_json, canonical_value, CanonicalError};
pub use error::{LoadError, SetProblem, TeamProblem};
pub use json::{ScenarioJson, TeamSet};
pub use meta::{MemberMeta, ScenarioMeta, SideMeta};
pub use switch_in::{expand_switch_ins, initial_outcomes, InitialOutcome, SwitchInError};
pub use team::{build_pokemon, build_side};

/// The oracle's doubles format: Champions mechanics, every member brought, level 50.
pub const DOUBLES_FORMAT: &str = "gen9championsdoublescustomgame";

/// Showdown's `battle.turn` at the first decision after team preview.
const FIRST_TURN: u16 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedScenario {
    pub state: Doubles,
    pub meta: ScenarioMeta,
}

/// Loads a scenario file; team paths resolve relative to its directory.
pub fn load_scenario_file(path: impl AsRef<Path>) -> Result<LoadedScenario, LoadError> {
    let path = path.as_ref();
    let text = read_text(path)?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    load_scenario_str(&text, base_dir)
}

/// Loads scenario JSON; team paths resolve relative to `base_dir`.
pub fn load_scenario_str(json: &str, base_dir: &Path) -> Result<LoadedScenario, LoadError> {
    let scenario: ScenarioJson =
        serde_json::from_str(strip_bom(json)).map_err(|error| LoadError::Json {
            what: "scenario".into(),
            error,
        })?;

    if scenario.format != DOUBLES_FORMAT {
        return Err(LoadError::UnsupportedFormat(scenario.format));
    }
    if !is_empty(scenario.setup_turns.as_ref()) {
        return Err(LoadError::Unsupported {
            field: "setupTurns",
            reason: "replaying turns needs the turn engine; describe the position as an \
                     initial state instead",
        });
    }
    if !is_empty(scenario.patch.as_ref()) {
        return Err(LoadError::Unsupported {
            field: "patch",
            reason: "HP, status, boost, side and field patches are not applied yet; \
                     loading without them would silently change the position",
        });
    }

    let mut state = Doubles::default();
    let mut sides: [SideMeta; 2] = Default::default();
    for (side, spec) in [(SideId::One, &scenario.p1), (SideId::Two, &scenario.p2)] {
        let team = resolve_team(side, &spec.team, base_dir)?;
        let (built, meta) = build_side::<2>(side, &team, spec.order.as_deref())?;
        state.sides[side.index()] = built;
        sides[side.index()] = meta;
    }
    state.turn = FIRST_TURN;

    Ok(LoadedScenario {
        state,
        meta: ScenarioMeta {
            format: scenario.format,
            description: scenario.description.unwrap_or_default(),
            sides,
            turn: scenario.turn,
        },
    })
}

/// Parses a team JSON array.
pub fn parse_team(json: &str) -> Result<Vec<TeamSet>, LoadError> {
    serde_json::from_str(strip_bom(json)).map_err(|error| LoadError::Json {
        what: "team".into(),
        error,
    })
}

/// Builds a state from teams and team preview orders directly (no scenario file).
pub fn state_from_teams<const N: usize>(
    teams: [(&[TeamSet], Option<&str>); 2],
) -> Result<(State<N>, [SideMeta; 2]), LoadError> {
    let mut state = State::<N>::default();
    let mut sides: [SideMeta; 2] = Default::default();
    for (side, (team, order)) in [SideId::One, SideId::Two].into_iter().zip(teams) {
        let (built, meta) = build_side::<N>(side, team, order)?;
        state.sides[side.index()] = built;
        sides[side.index()] = meta;
    }
    state.turn = FIRST_TURN;
    Ok((state, sides))
}

fn resolve_team(side: SideId, spec: &Value, base_dir: &Path) -> Result<Vec<TeamSet>, LoadError> {
    let what = format!("{} team", if side == SideId::One { "p1" } else { "p2" });
    match spec {
        Value::String(relative) => {
            let path = base_dir.join(relative);
            serde_json::from_str(strip_bom(&read_text(&path)?)).map_err(|error| LoadError::Json {
                what: format!("{what} {}", path.display()),
                error,
            })
        }
        Value::Array(_) => {
            serde_json::from_value(spec.clone()).map_err(|error| LoadError::Json { what, error })
        }
        _ => Err(LoadError::Unsupported {
            field: "team",
            reason: "expected a path string or an inline team array",
        }),
    }
}

fn read_text(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|error| LoadError::Io {
        path: path.to_owned(),
        error,
    })
}

/// Team files written by PowerShell may start with a byte order mark.
fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Absent, `null`, `[]` and `{}` all mean "nothing to do".
fn is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
        Some(_) => false,
    }
}
