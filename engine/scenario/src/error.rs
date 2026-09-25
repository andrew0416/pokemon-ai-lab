//! Load errors. Every error names the side and team member it came from.

use std::fmt;
use std::path::PathBuf;

use lab_engine::state::SideId;
use lab_engine::stats::StatPointError;

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
    /// The Champions stat formula in `lab_engine::stats` is the level-50 one.
    UnsupportedLevel(u8),
    StatPoints(StatPointError),
    InvalidIv {
        stat: &'static str,
        value: u8,
    },
    UnknownGender(String),
    UnknownTeraType(String),
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
                "unsupported format {format:?}: only {} is loaded (all members brought, \
                 order = team preview choice)",
                crate::DOUBLES_FORMAT
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
            SetProblem::UnsupportedLevel(level) => {
                write!(f, "level {level}: Champions stats are computed at level 50")
            }
            SetProblem::StatPoints(StatPointError::PerStat { stat, value }) => {
                write!(f, "{value} SP in {stat:?} (at most 32 per stat)")
            }
            SetProblem::StatPoints(StatPointError::Total { value }) => {
                write!(f, "{value} SP in total (at most 66)")
            }
            SetProblem::InvalidIv { stat, value } => {
                write!(f, "IV {value} in {stat} (at most 31)")
            }
            SetProblem::UnknownGender(s) => write!(f, "unknown gender {s:?} (M, F or N)"),
            SetProblem::UnknownTeraType(s) => write!(f, "unknown Tera type {s:?}"),
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
