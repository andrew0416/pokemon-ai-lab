//! Per-slot actions. A side's decision is one [`SlotAction`] per active slot; search can keep
//! statistics per slot (factored) instead of per joint action.

/// Showdown target convention: positive = foe position (1-based), negative = ally position,
/// 0 = no explicit target (self, spread, field, or singles).
pub type TargetLoc = i8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Gimmick {
    #[default]
    None,
    Mega,
    Tera,
}

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

pub type JointAction<const N: usize> = [SlotAction; N];
