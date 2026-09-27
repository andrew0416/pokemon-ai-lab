//! Offline planning on lab-engine (DESIGN.md "탐색의 용도와 정보 모델", roadmap 7).
//!
//! The tool answers "against this known team, from this position, which choice holds up?":
//! both teams are fixed, the engine enumerates every outcome of a turn exactly, and the
//! solver treats the opponent as an adversary (opponent model ①, perfect information) with
//! the dice either averaged ([`Chance::Expect`], win-rate) or also adversarial
//! ([`Chance::Worst`], "최악 난수 보장": a line that wins under every roll).
//!
//! - [`game`]: the decisions a position asks for (a turn, replacements of fainted Pokémon, a
//!   mid-turn switch after U-turn and the like), the legal choices of each side and the exact
//!   outcome distribution of a pair of choices, all on `lab_engine::turn`.
//! - [`solve`]: depth-limited maximin over pure strategies with alpha-beta cutoffs at the
//!   adversary node and Star1 cutoffs at chance nodes; every line of the root is reported with
//!   its value and the reply that holds it there. [`Solver::analyse_mixed`] instead solves the
//!   root turn as a matrix game ([`nash`]): the value when neither side can be read, with a
//!   mixed strategy for each side.
//! - [`choice`]: choices in Showdown's choice-string form (`move hypervoice 1, switch 3`), the
//!   inverse of `lab_scenario::parse_choice`.
//! - `node` (feature `scenario`, on by default): a position as a library value stepped by
//!   choice strings, over the scenario loader; the core of the Python API (`engine/py`).
//!
//! Not here yet: opponent models ② and ③ (belief about our spreads and its update), plan
//! conditions, the spread-grid tables, transposition tables, mixed strategies.

pub mod choice;
pub mod game;
pub mod nash;
#[cfg(feature = "scenario")]
pub mod node;
pub mod solve;

pub use choice::{format_choice, format_switches, Choice};
pub use game::{decision, legal_choices, transitions, Decision, Pruning};
pub use nash::{Equilibrium, Matrix};
pub use solve::{
    Analysis, BestResponse, Chance, ChildValues, Config, DeepAnalysis, DeepLine, DeepMixedAnalysis,
    Line, MixedAnalysis, PlanReport, SearchError, Solver, MIXED_SUPPORT, WIN,
};
