//! P7d capability for a first-hit boundary, not general mid-move suspension.
//! Only the exact Full factored driver enables it. Callback-bearing/newly registered moves
//! stay on the original path; supported mechanics are not automatically checkpoint-capable.
//! The cut precedes primary hit/damage, so its totals are zero and it carries no HpMark,
//! Pokemon snapshots, lazy spans or symbolic dealt-damage dependency across group lifetimes.
use super::battle::HistoryReaders;
use crate::dex::{MoveCategory, MoveData, MoveFlags, SelfDestruct, SelfSwitch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Policy {
    Disabled,
    ExactFull,
}
impl Policy {
    pub(super) fn for_options(options: super::FactoredOptions) -> Self {
        if options.rolls == super::RollMode::Full && options.max_support.is_none() {
            Self::ExactFull
        } else {
            Self::Disabled
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Direct,
    Nested,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Context {
    pub history_readers: HistoryReaders,
    pub suppression: bool,
    pub move_self_switch: bool,
}

/// Data-only effects continue through the original suffix. Arbitrary callback stacks do not.
/// Exhaustive destructuring makes a new dex field a review obligation; no move-name list.
pub(super) fn data_capable(data: &MoveData) -> bool {
    let MoveData {
        id: _,
        name: _,
        num: _,
        nonstandard: _,
        move_type: _,
        category,
        base_power: _,
        accuracy: _,
        pp: _,
        priority: _,
        target: _,
        non_ghost_target: _,
        flags,
        crit_ratio: _,
        will_crit: _,
        multihit,
        multiaccuracy,
        drain: _,
        recoil: _,
        heal: _,
        fixed_damage: _,
        ohko: _,
        ignore_immunity: _,
        status: _,
        volatile_status: _,
        side_condition: _,
        slot_condition: _,
        pseudo_weather: _,
        weather: _,
        terrain: _,
        boosts: _,
        self_effect: _,
        self_boost: _,
        secondaries: _,
        condition_duration: _,
        condition_counter_max: _,
        condition_locks_move: _,
        condition_no_invulnerability: _,
        condition_blocks_crits: _,
        self_switch,
        selfdestruct,
        override_offensive_pokemon_target: _,
        override_offensive_stat: _,
        override_defensive_stat: _,
        z_move: _,
        max_move_power: _,
        force_switch: _,
        breaks_protect: _,
        stalling_move: _,
        thaws_target: _,
        tracks_target: _,
        smart_target,
        sleep_usable: _,
        steals_boosts: _,
        calls_move,
        has_crash_damage: _,
        mind_blown_recoil: _,
        struggle_recoil: _,
        chloroblast_recoil: _,
        ignore_ability: _,
        ignore_evasion: _,
        ignore_defensive: _,
        ignore_offensive: _,
        ignore_negative_offensive: _,
        ignore_positive_defensive: _,
        has_sheer_force_boost: _,
        force_stab: _,
        no_pp_boosts: _,
        is_z,
        is_max,
        event_orders,
        handlers,
    } = data;
    *category != MoveCategory::Status
        && handlers.is_empty()
        && event_orders.is_empty()
        && multihit.is_none()
        && !*multiaccuracy
        && !*smart_target
        && !*calls_move
        && *selfdestruct == SelfDestruct::No
        && *self_switch == SelfSwitch::No
        && !*is_z
        && !*is_max
        && !flags.contains(MoveFlags::FUTUREMOVE)
        && !flags.contains(MoveFlags::DANCE)
}

#[cfg(test)]
mod observer {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Counts {
        pub eligible: usize,
        pub captures: usize,
        pub resumes: usize,
        pub prefix_entries: usize,
        pub fallbacks: usize,
    }
    thread_local! {
        pub(super) static COUNTS: Cell<Counts> = const { Cell::new(Counts { eligible:0,captures:0,resumes:0,prefix_entries:0,fallbacks:0 }) };
        pub(super) static DISABLED: Cell<bool> = const { Cell::new(false) };
    }
    pub(super) fn change(f: impl FnOnce(&mut Counts)) {
        COUNTS.with(|c| {
            let mut n = c.get();
            f(&mut n);
            c.set(n);
        });
    }
    pub(crate) struct TestDisableGuard {
        previous: bool,
    }
    impl TestDisableGuard {
        pub(crate) fn new(disabled: bool) -> Self {
            Self {
                previous: DISABLED.with(|c| c.replace(disabled)),
            }
        }
    }
    impl Drop for TestDisableGuard {
        fn drop(&mut self) {
            DISABLED.with(|c| c.set(self.previous));
        }
    }
    pub(crate) fn test_observer_reset() {
        COUNTS.with(|c| c.set(Counts::default()));
    }
    pub(crate) fn test_observer_snapshot() -> Counts {
        COUNTS.with(Cell::get)
    }
}
#[cfg(test)]
pub(super) use observer::{test_observer_reset, test_observer_snapshot, TestDisableGuard};
#[inline]
pub(super) fn enabled() -> bool {
    #[cfg(test)]
    {
        return !observer::DISABLED.with(std::cell::Cell::get);
    }
    #[cfg(not(test))]
    {
        true
    }
}
#[inline]
pub(super) fn prefix_entry() {
    #[cfg(test)]
    observer::change(|c| c.prefix_entries += 1);
}
#[inline]
pub(super) fn eligible() {
    #[cfg(test)]
    observer::change(|c| c.eligible += 1);
}
#[inline]
pub(super) fn captured() {
    #[cfg(test)]
    observer::change(|c| c.captures += 1);
}
#[inline]
pub(super) fn resumed() {
    #[cfg(test)]
    observer::change(|c| c.resumes += 1);
}
#[inline]
pub(super) fn fallback() {
    #[cfg(test)]
    observer::change(|c| c.fallbacks += 1);
}
