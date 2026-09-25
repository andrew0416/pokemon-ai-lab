//! lab-engine core.
//!
//! One rules engine for singles and doubles: the number of active slots per side is the
//! const generic `N` (`State<1>` = singles, `State<2>` = doubles), so each format gets its
//! own monomorphized code and singles pays nothing for doubles support.
//!
//! Search works by make/unmake: a turn produces weighted outcomes, each a list of reversible
//! [`instruction::Instruction`]s that are applied on the way down and reversed on the way up.

pub mod action;
pub mod eval;
pub mod field;
pub mod instruction;
pub mod state;

pub type Singles = state::State<1>;
pub type Doubles = state::State<2>;
