//! Per-slot actions. A side's decision is one [`SlotAction`] per active slot; search can keep
//! statistics per slot (factored) instead of per joint action.
//!
//! Whether an action is allowed depends on the ruleset; see [`crate::rules::Ruleset`].

pub use crate::gimmick::Gimmick;

/// Showdown target convention: positive = foe position (1-based), negative = ally position,
/// 0 = no explicit target (self, spread, field, or singles).
pub type TargetLoc = i8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SlotAction {
    /// Empty or fainted slot with nothing to do.
    #[default]
    Pass,
    Move {
        index: u8,
        target: TargetLoc,
        gimmick: Gimmick,
    },
    Switch {
        party_index: u8,
    },
}

impl SlotAction {
    /// The activation this action requests ([`Gimmick::None`] unless it is a move).
    pub const fn gimmick(self) -> Gimmick {
        match self {
            SlotAction::Move { gimmick, .. } => gimmick,
            _ => Gimmick::None,
        }
    }

    /// The same action with `gimmick`; non-move actions are returned unchanged.
    pub const fn with_gimmick(self, gimmick: Gimmick) -> SlotAction {
        match self {
            SlotAction::Move { index, target, .. } => SlotAction::Move {
                index,
                target,
                gimmick,
            },
            other => other,
        }
    }
}

pub type JointAction<const N: usize> = [SlotAction; N];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_action_stays_within_four_bytes() {
        assert!(std::mem::size_of::<SlotAction>() <= 4);
    }
}
