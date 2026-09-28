//! Load errors. Every error names the side and team member it came from.

use std::fmt;
use std::path::PathBuf;

use lab_engine::state::SideId;
use lab_engine::stats::StatPointError;
use lab_engine::turn::TurnError;

use crate::canonical::CanonicalError;
use crate::switch_in::SwitchInError;

#[derive(Debug)]
pub enum LoadError {
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    Json {
        what: String,
        error: serde_json::Error,
    },
    /// Only formats whose team preview and slot count the loader reproduces are accepted.
    UnsupportedFormat(String),
    /// A scenario feature the loader refuses instead of silently skipping.
    Unsupported {
        field: &'static str,
        reason: &'static str,
    },
    Team {
        side: SideId,
        problem: TeamProblem,
    },
    Set {
        side: SideId,
        /// 0-based position in the team JSON.
        index: usize,
        name: String,
        problem: SetProblem,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum TeamProblem {
    Empty,
    TooLarge(usize),
    /// Canonical states key Pokémon by name, so names must be unique per side.
    DuplicateName(String),
    Order {
        order: String,
        reason: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum SetProblem {
    UnknownSpecies(String),
    UnknownItem(String),
    UnknownAbility(String),
    MissingAbility,
    UnknownNature(String),
    MissingNature,
    UnknownMove(String),
    DuplicateMove(String),
    NoMoves,
    TooManyMoves(usize),
    StatPoints(StatPointError),
    InvalidIv {
        stat: &'static str,
        value: u8,
    },
    UnknownTeraType(String),
    /// A temporary in-battle forme as the set's species (`lab_engine::turn::temporary_forme_base`):
    /// Showdown would keep it as the base species, which the engine's state cannot tell apart
    /// from the forme reached in battle.
    TemporaryForme(String),
}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            LoadError::Json { what, error } => write!(f, "{what}: {error}"),
            LoadError::UnsupportedFormat(format) => write!(
                f,
                "unsupported format {format:?}: only {} (all members brought) and {} (team \
                 preview picks 4) are loaded; order = team preview choice",
                crate::DOUBLES_FORMAT,
                crate::VGC_FORMAT
            ),
            LoadError::Unsupported { field, reason } => {
                write!(f, "scenario field {field:?} is not supported: {reason}")
            }
            LoadError::Team { side, problem } => {
                write!(f, "{} team: {problem}", side_name(*side))
            }
            LoadError::Set {
                side,
                index,
                name,
                problem,
            } => write!(f, "{} team[{index}] {name:?}: {problem}", side_name(*side)),
        }
    }
}

impl fmt::Display for TeamProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TeamProblem::Empty => write!(f, "no Pokémon"),
            TeamProblem::TooLarge(n) => write!(f, "{n} Pokémon (at most 6)"),
            TeamProblem::DuplicateName(name) => write!(
                f,
                "duplicate name {name:?}: canonical states key Pokémon by name"
            ),
            TeamProblem::Order { order, reason } => write!(f, "order {order:?}: {reason}"),
        }
    }
}

impl fmt::Display for SetProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetProblem::UnknownSpecies(s) => write!(f, "unknown species {s:?}"),
            SetProblem::UnknownItem(s) => write!(f, "unknown item {s:?}"),
            SetProblem::UnknownAbility(s) => write!(f, "unknown ability {s:?}"),
            SetProblem::MissingAbility => write!(f, "no ability given"),
            SetProblem::UnknownNature(s) => write!(f, "unknown nature {s:?}"),
            SetProblem::MissingNature => write!(f, "no nature given"),
            SetProblem::UnknownMove(s) => write!(f, "unknown move {s:?}"),
            SetProblem::DuplicateMove(s) => write!(f, "move {s:?} appears twice"),
            SetProblem::NoMoves => write!(f, "no moves"),
            SetProblem::TooManyMoves(n) => write!(f, "{n} moves (at most 4)"),
            SetProblem::StatPoints(StatPointError::PerStat { stat, value }) => {
                write!(f, "{value} SP in {stat:?} (at most 32 per stat)")
            }
            SetProblem::StatPoints(StatPointError::Total { value }) => {
                write!(f, "{value} SP in total (at most 66)")
            }
            SetProblem::InvalidIv { stat, value } => {
                write!(f, "IV {value} in {stat} (at most 31)")
            }
            SetProblem::UnknownTeraType(s) => write!(f, "unknown Tera type {s:?}"),
            SetProblem::TemporaryForme(s) => {
                write!(
                    f,
                    "{s:?} is a temporary in-battle forme, not a set's species"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Io { error, .. } => Some(error),
            LoadError::Json { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Why the positions of a scenario (or a decision run from one) could not be produced
/// (`scenario_positions*`, `run_decision_mid_turn*`). The message is the text these functions
/// returned as a `String` before (board T1); the variant says whose fault it is, so callers
/// (the search's `Node`, `lab-check`) do not read the text to tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioError {
    /// Something the scenario needs that the engine does not implement: the turn engine's
    /// `TurnError::Unsupported`, a switch-in handler, a slot count, a state with no canonical
    /// form yet.
    Unsupported(String),
    /// Anything else: a choice that does not parse or is not legal in a replayed position, a
    /// pinned state no position has, a patch that cannot be applied, a turn that pauses for a
    /// mid-turn switch nobody gave.
    Invalid(String),
}

impl ScenarioError {
    /// The message.
    pub fn message(&self) -> &str {
        match self {
            ScenarioError::Unsupported(m) | ScenarioError::Invalid(m) => m,
        }
    }

    /// Whether the engine does not implement something the scenario needs.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, ScenarioError::Unsupported(_))
    }

    /// The same error with `suffix` after its message.
    pub fn suffixed(self, suffix: &str) -> Self {
        match self {
            ScenarioError::Unsupported(m) => ScenarioError::Unsupported(m + suffix),
            ScenarioError::Invalid(m) => ScenarioError::Invalid(m + suffix),
        }
    }

    /// The same error with `prefix` before its message (`"setup turn 2: …"`).
    pub fn context(self, prefix: impl fmt::Display) -> Self {
        match self {
            ScenarioError::Unsupported(m) => ScenarioError::Unsupported(format!("{prefix}: {m}")),
            ScenarioError::Invalid(m) => ScenarioError::Invalid(format!("{prefix}: {m}")),
        }
    }
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ScenarioError {}

impl From<String> for ScenarioError {
    fn from(message: String) -> Self {
        ScenarioError::Invalid(message)
    }
}

impl From<&str> for ScenarioError {
    fn from(message: &str) -> Self {
        ScenarioError::Invalid(message.to_owned())
    }
}

/// For callers that only want the text (`?` in a function returning `Result<_, String>`).
impl From<ScenarioError> for String {
    fn from(e: ScenarioError) -> Self {
        match e {
            ScenarioError::Unsupported(m) | ScenarioError::Invalid(m) => m,
        }
    }
}

impl From<TurnError> for ScenarioError {
    fn from(e: TurnError) -> Self {
        match e {
            TurnError::Unsupported(_) => ScenarioError::Unsupported(e.to_string()),
            other => ScenarioError::Invalid(other.to_string()),
        }
    }
}

impl From<SwitchInError> for ScenarioError {
    fn from(e: SwitchInError) -> Self {
        match e {
            SwitchInError::NotInitial { .. } => ScenarioError::Invalid(e.to_string()),
            SwitchInError::UnsupportedSlotCount(_)
            | SwitchInError::UnsupportedAbility { .. }
            | SwitchInError::UnsupportedItem { .. }
            | SwitchInError::UnsupportedSpecies { .. }
            | SwitchInError::Unsupported { .. } => ScenarioError::Unsupported(e.to_string()),
        }
    }
}

impl From<CanonicalError> for ScenarioError {
    fn from(e: CanonicalError) -> Self {
        match e {
            CanonicalError::Unrepresentable { .. } => ScenarioError::Unsupported(e.to_string()),
            other => ScenarioError::Invalid(other.to_string()),
        }
    }
}
