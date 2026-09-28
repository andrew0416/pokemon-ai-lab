//! Ruleset capabilities: what a format enables, as opposed to what the engine can represent.
//!
//! The engine keeps every activation mode in [`Gimmick`]; a [`Ruleset`] decides which ones a
//! format allows. Champions M-C enables only Mega Evolution. The other modes are disabled
//! here, not removed, so a future format turns them on by changing the ruleset alone.
//!
//! The same checks back both legal-action generation ([`Ruleset::joint_actions`]) and the
//! defensive validation of externally supplied actions ([`Ruleset::validate_joint_action`]),
//! so generated actions always validate.

use std::fmt;

use crate::action::{JointAction, SlotAction, TargetLoc};
use crate::gimmick::{Gimmick, GimmickSet};
use crate::state::{SideId, SlotRef, State, PARTY_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ruleset {
    /// Activation modes the format enables.
    pub gimmicks: GimmickSet,
}

impl Ruleset {
    /// Pokémon Champions Regulation M-C (doubles `gen9championsvgc2026regmc`, singles
    /// `gen9championsbssregmc`): Mega Evolution only.
    pub const CHAMPIONS_MC: Ruleset = Ruleset {
        gimmicks: GimmickSet::MEGA,
    };

    /// No activation modes at all.
    pub const NO_GIMMICKS: Ruleset = Ruleset {
        gimmicks: GimmickSet::EMPTY,
    };

    /// [`Gimmick::None`] is always allowed.
    pub const fn allows(self, gimmick: Gimmick) -> bool {
        gimmick.is_none() || self.gimmicks.contains(gimmick)
    }

    /// This ruleset with `gimmick` also enabled.
    pub const fn enabling(self, gimmick: Gimmick) -> Ruleset {
        Ruleset {
            gimmicks: self.gimmicks.with(gimmick),
        }
    }

    /// Activation modes the Pokémon at `slot` may choose now: enabled by the ruleset,
    /// supported by the Pokémon, and not yet spent by its side. Empty for an empty or
    /// out-of-range slot.
    ///
    /// Deliberately independent of item suppression: Showdown's `canMegaEvo`/`runMegaEvo`
    /// read `pokemon.getItem()` directly, so Magic Room, Embargo and Klutz never block Mega
    /// Evolution. A future Magic Room must not gate this on `ignoringItem`.
    pub fn available_gimmicks<const N: usize>(
        &self,
        state: &State<N>,
        slot: SlotRef,
    ) -> GimmickSet {
        let side = state.side(slot.side);
        let Some(pokemon) = side
            .slots
            .get(slot.slot as usize)
            .and_then(|s| s.party_index)
            .map(|i| &side.party[i as usize])
        else {
            return GimmickSet::EMPTY;
        };
        self.gimmicks
            .intersection(pokemon.gimmicks)
            .difference(side.gimmicks_used)
    }

    /// Checks one slot's action on its own. Covers the ruleset, per-side usage, the
    /// Pokémon's eligibility and index bounds; move-specific rules (PP, Disable, valid
    /// targets for the move, Z/Max move conversion) belong to the turn engine.
    pub fn validate_slot_action<const N: usize>(
        &self,
        state: &State<N>,
        slot: SlotRef,
        action: SlotAction,
    ) -> Result<(), ActionError> {
        let s = slot.slot;
        if s as usize >= N {
            return Err(ActionError::SlotOutOfRange { slot: s });
        }
        match action {
            SlotAction::Pass => Ok(()),
            SlotAction::Switch { party_index } => {
                if party_index as usize >= PARTY_SIZE {
                    return Err(ActionError::PartyIndexOutOfRange {
                        slot: s,
                        party_index,
                    });
                }
                // A trapped Pokémon cannot choose to switch (Shadow Tag, Arena Trap, Magnet
                // Pull; `turn::trapped`).
                if crate::turn::trapped(state, slot) {
                    return Err(ActionError::Trapped { slot: s });
                }
                Ok(())
            }
            SlotAction::Move {
                index,
                target,
                gimmick,
            } => {
                if state.slot(slot).party_index.is_none() {
                    return Err(ActionError::EmptySlot { slot: s });
                }
                // `STRUGGLE_INDEX` stands for Struggle (the turn engine checks it is the only choice),
                // `RECHARGE_INDEX` for the `recharge` pseudo-move of a Pokémon that must recharge
                // (what `legal_joint_actions` offers it; board B38).
                let recharging = index == crate::turn::RECHARGE_INDEX
                    && crate::turn::locked_move(state, slot) == Some(crate::turn::Locked::Recharge);
                if index >= 4 && index != crate::turn::STRUGGLE_INDEX && !recharging {
                    return Err(ActionError::MoveIndexOutOfRange { slot: s, index });
                }
                if target.unsigned_abs() as usize > N {
                    return Err(ActionError::TargetOutOfRange { slot: s, target });
                }
                self.check_gimmick(state, slot, gimmick)
            }
        }
    }

    fn check_gimmick<const N: usize>(
        &self,
        state: &State<N>,
        slot: SlotRef,
        gimmick: Gimmick,
    ) -> Result<(), ActionError> {
        let s = slot.slot;
        if gimmick.is_none() {
            return Ok(());
        }
        if !self.allows(gimmick) {
            return Err(ActionError::GimmickDisabled { slot: s, gimmick });
        }
        if state.side(slot.side).gimmicks_used.contains(gimmick) {
            return Err(ActionError::GimmickAlreadyUsed { slot: s, gimmick });
        }
        if !self.available_gimmicks(state, slot).contains(gimmick) {
            return Err(ActionError::GimmickUnavailable { slot: s, gimmick });
        }
        Ok(())
    }

    /// Checks a whole side decision: every slot on its own, then that no activation mode is
    /// requested by two slots in the same turn (each kind is once per battle per side).
    pub fn validate_joint_action<const N: usize>(
        &self,
        state: &State<N>,
        side: SideId,
        action: &JointAction<N>,
    ) -> Result<(), ActionError> {
        for (i, &slot_action) in action.iter().enumerate() {
            let slot = SlotRef {
                side,
                slot: i as u8,
            };
            self.validate_slot_action(state, slot, slot_action)?;
        }
        match repeated_gimmick(action) {
            Some((slot, gimmick)) => Err(ActionError::GimmickTwiceInTurn { slot, gimmick }),
            None => Ok(()),
        }
    }

    /// Appends every rule-legal joint action for `side` built from per-slot base candidates.
    ///
    /// Candidates come from the (future) move/switch generator and carry no gimmick; any
    /// gimmick they do carry is ignored. Each valid move candidate is emitted once plain and
    /// once per activation mode in [`Ruleset::available_gimmicks`]; joint actions that
    /// request one mode from two slots are dropped. Slot 0 varies fastest.
    pub fn joint_actions<const N: usize>(
        &self,
        state: &State<N>,
        side: SideId,
        candidates: [&[SlotAction]; N],
        out: &mut Vec<JointAction<N>>,
    ) {
        let expanded: [Vec<SlotAction>; N] = std::array::from_fn(|i| {
            let slot = SlotRef {
                side,
                slot: i as u8,
            };
            let available = self.available_gimmicks(state, slot);
            let mut actions = Vec::with_capacity(candidates[i].len());
            for &base in candidates[i] {
                let base = base.with_gimmick(Gimmick::None);
                if self.validate_slot_action(state, slot, base).is_err() {
                    continue;
                }
                actions.push(base);
                if let SlotAction::Move { .. } = base {
                    actions.extend(available.iter().map(|g| base.with_gimmick(g)));
                }
            }
            actions
        });
        if expanded.iter().any(Vec::is_empty) {
            return;
        }

        let mut cursor = [0usize; N];
        loop {
            let action: JointAction<N> = std::array::from_fn(|i| expanded[i][cursor[i]]);
            if repeated_gimmick(&action).is_none() {
                out.push(action);
            }
            let mut i = 0;
            loop {
                if i == N {
                    return;
                }
                cursor[i] += 1;
                if cursor[i] < expanded[i].len() {
                    break;
                }
                cursor[i] = 0;
                i += 1;
            }
        }
    }
}

/// The first slot that requests an activation mode an earlier slot already requested.
pub(crate) fn repeated_gimmick<const N: usize>(action: &JointAction<N>) -> Option<(u8, Gimmick)> {
    let mut requested = GimmickSet::EMPTY;
    for (i, slot_action) in action.iter().enumerate() {
        let gimmick = slot_action.gimmick();
        if requested.contains(gimmick) {
            return Some((i as u8, gimmick));
        }
        requested = requested.with(gimmick);
    }
    None
}

/// Why an action was rejected. `slot` is the side-relative active slot index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionError {
    SlotOutOfRange {
        slot: u8,
    },
    /// A move from a slot with no active Pokémon.
    EmptySlot {
        slot: u8,
    },
    MoveIndexOutOfRange {
        slot: u8,
        index: u8,
    },
    TargetOutOfRange {
        slot: u8,
        target: TargetLoc,
    },
    PartyIndexOutOfRange {
        slot: u8,
        party_index: u8,
    },
    /// The ruleset does not enable this activation mode.
    GimmickDisabled {
        slot: u8,
        gimmick: Gimmick,
    },
    /// The side already spent this mode's once-per-battle budget.
    GimmickAlreadyUsed {
        slot: u8,
        gimmick: Gimmick,
    },
    /// The Pokémon cannot use this mode (no Mega Stone, no Tera type, ...).
    GimmickUnavailable {
        slot: u8,
        gimmick: Gimmick,
    },
    /// Another slot already requested this mode this turn.
    GimmickTwiceInTurn {
        slot: u8,
        gimmick: Gimmick,
    },
    /// The Pokémon is trapped (a foe's Shadow Tag, Arena Trap or Magnet Pull) and cannot
    /// choose to switch.
    Trapped {
        slot: u8,
    },
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::SlotOutOfRange { slot } => write!(f, "slot {slot}: no such slot"),
            ActionError::EmptySlot { slot } => write!(f, "slot {slot}: no active Pokémon"),
            ActionError::MoveIndexOutOfRange { slot, index } => {
                write!(f, "slot {slot}: move index {index} out of range")
            }
            ActionError::TargetOutOfRange { slot, target } => {
                write!(f, "slot {slot}: target {target} out of range")
            }
            ActionError::PartyIndexOutOfRange { slot, party_index } => {
                write!(f, "slot {slot}: party index {party_index} out of range")
            }
            ActionError::GimmickDisabled { slot, gimmick } => {
                write!(f, "slot {slot}: {gimmick:?} is disabled by the ruleset")
            }
            ActionError::GimmickAlreadyUsed { slot, gimmick } => {
                write!(f, "slot {slot}: {gimmick:?} was already used this battle")
            }
            ActionError::GimmickUnavailable { slot, gimmick } => {
                write!(f, "slot {slot}: this Pokémon cannot use {gimmick:?}")
            }
            ActionError::GimmickTwiceInTurn { slot, gimmick } => {
                write!(
                    f,
                    "slot {slot}: {gimmick:?} requested by two slots this turn"
                )
            }
            ActionError::Trapped { slot } => {
                write!(f, "slot {slot}: the Pokémon is trapped and cannot switch")
            }
        }
    }
}

impl std::error::Error for ActionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::SpeciesId;
    use crate::gimmick::DynamaxState;
    use crate::instruction::Instruction;
    use crate::state::Slot;

    const ONE_0: SlotRef = SlotRef {
        side: SideId::One,
        slot: 0,
    };

    /// Doubles with two leads per side. Every Pokémon is eligible for every activation mode,
    /// so only the ruleset and per-side usage restrict them.
    fn doubles() -> State<2> {
        let mut state = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, pokemon) in state.side_mut(side).party.iter_mut().enumerate() {
                pokemon.species = SpeciesId(i as u16 + 1);
                pokemon.max_hp = 200;
                pokemon.hp = 200;
                pokemon.gimmicks = GimmickSet::ALL;
            }
            for slot in 0..2u8 {
                state.side_mut(side).slots[slot as usize].party_index = Some(slot);
            }
        }
        state
    }

    fn mv(index: u8, target: TargetLoc, gimmick: Gimmick) -> SlotAction {
        SlotAction::Move {
            index,
            target,
            gimmick,
        }
    }

    fn plain(index: u8, target: TargetLoc) -> SlotAction {
        mv(index, target, Gimmick::None)
    }

    fn generate(ruleset: Ruleset, state: &State<2>, side: SideId) -> Vec<JointAction<2>> {
        let slot0 = [
            plain(0, 1),
            plain(1, 2),
            SlotAction::Switch { party_index: 2 },
        ];
        let slot1 = [plain(0, 1)];
        let mut out = Vec::new();
        ruleset.joint_actions(state, side, [&slot0, &slot1], &mut out);
        out
    }

    #[test]
    fn champions_mc_allows_only_mega() {
        let mc = Ruleset::CHAMPIONS_MC;
        let state = doubles();
        assert!(mc.allows(Gimmick::None) && mc.allows(Gimmick::Mega));
        assert_eq!(mc.available_gimmicks(&state, ONE_0), GimmickSet::MEGA);

        assert_eq!(
            mc.validate_slot_action(&state, ONE_0, mv(0, 1, Gimmick::Mega)),
            Ok(())
        );
        for gimmick in [
            Gimmick::UltraBurst,
            Gimmick::ZMove,
            Gimmick::Dynamax,
            Gimmick::Tera,
        ] {
            assert!(!mc.allows(gimmick));
            assert_eq!(
                mc.validate_slot_action(&state, ONE_0, mv(0, 1, gimmick)),
                Err(ActionError::GimmickDisabled { slot: 0, gimmick })
            );
            // Externally supplied joint actions are rejected too, from either slot.
            assert_eq!(
                mc.validate_joint_action(&state, SideId::Two, &[plain(0, 1), mv(1, 2, gimmick)]),
                Err(ActionError::GimmickDisabled { slot: 1, gimmick })
            );
        }
    }

    #[test]
    fn champions_mc_generation_emits_only_mega() {
        let mc = Ruleset::CHAMPIONS_MC;
        let state = doubles();
        let actions = generate(mc, &state, SideId::One);
        // Slot 0: 2 moves x {plain, Mega} + switch = 5; slot 1: 1 move x {plain, Mega} = 2.
        // 10 combinations minus the 2 where both slots Mega Evolve.
        assert_eq!(actions.len(), 8);
        for action in &actions {
            assert!(action
                .iter()
                .all(|a| matches!(a.gimmick(), Gimmick::None | Gimmick::Mega)));
            assert_eq!(
                mc.validate_joint_action(&state, SideId::One, action),
                Ok(())
            );
        }
        // Gimmicks already present on candidates are ignored, not trusted.
        let mut out = Vec::new();
        let tera = [mv(0, 1, Gimmick::Tera)];
        mc.joint_actions(&state, SideId::One, [&tera, &tera], &mut out);
        assert!(out.iter().flatten().all(|a| a.gimmick() != Gimmick::Tera));

        // No gimmick at all when the ruleset enables none.
        let none = generate(Ruleset::NO_GIMMICKS, &state, SideId::One);
        assert_eq!(none.len(), 3);
        assert!(none.iter().flatten().all(|a| a.gimmick().is_none()));
    }

    #[test]
    fn future_capabilities_can_be_enabled() {
        let state = doubles();
        let future = Ruleset::CHAMPIONS_MC
            .enabling(Gimmick::Tera)
            .enabling(Gimmick::Dynamax);
        assert_eq!(
            future.validate_slot_action(&state, ONE_0, mv(0, 1, Gimmick::Tera)),
            Ok(())
        );
        // Different modes may be activated by different slots in the same turn.
        assert_eq!(
            future.validate_joint_action(
                &state,
                SideId::One,
                &[mv(0, 1, Gimmick::Mega), mv(0, 2, Gimmick::Dynamax)]
            ),
            Ok(())
        );
        assert_eq!(
            future.validate_slot_action(&state, ONE_0, mv(0, 1, Gimmick::ZMove)),
            Err(ActionError::GimmickDisabled {
                slot: 0,
                gimmick: Gimmick::ZMove
            })
        );

        let everything = Ruleset {
            gimmicks: GimmickSet::ALL,
        };
        for gimmick in Gimmick::ACTIVATIONS {
            assert!(everything.allows(gimmick));
        }
        let mut out = Vec::new();
        let one_move = [plain(0, 1)];
        let pass = [SlotAction::Pass];
        everything.joint_actions(&state, SideId::One, [&one_move, &pass], &mut out);
        assert_eq!(out.len(), 1 + Gimmick::ACTIVATIONS.len());

        // Enabling a mode does not make every Pokémon eligible for it.
        let mut no_tera = state.clone();
        no_tera.side_mut(SideId::One).party[0].gimmicks = GimmickSet::MEGA;
        assert_eq!(
            future.validate_slot_action(&no_tera, ONE_0, mv(0, 1, Gimmick::Tera)),
            Err(ActionError::GimmickUnavailable {
                slot: 0,
                gimmick: Gimmick::Tera
            })
        );
    }

    #[test]
    fn once_per_battle_usage_is_per_side() {
        let mc = Ruleset::CHAMPIONS_MC;
        let mut state = doubles();
        let original = state.clone();
        let spend = [Instruction::UseGimmick {
            side: SideId::One,
            gimmick: Gimmick::Mega,
        }];
        state.apply(&spend);

        let mega = [mv(0, 1, Gimmick::Mega), plain(0, 1)];
        assert_eq!(
            mc.validate_joint_action(&state, SideId::One, &mega),
            Err(ActionError::GimmickAlreadyUsed {
                slot: 0,
                gimmick: Gimmick::Mega
            })
        );
        assert_eq!(mc.validate_joint_action(&state, SideId::Two, &mega), Ok(()));
        assert!(mc.available_gimmicks(&state, ONE_0).is_empty());
        assert_eq!(generate(mc, &state, SideId::One).len(), 3);
        assert_eq!(generate(mc, &state, SideId::Two).len(), 8);

        // Each mode has its own budget: spending Mega leaves Tera available.
        let future = mc.enabling(Gimmick::Tera);
        assert_eq!(
            future.available_gimmicks(&state, ONE_0),
            GimmickSet::of(Gimmick::Tera)
        );

        state.reverse(&spend);
        assert_eq!(state, original);
        assert_eq!(mc.validate_joint_action(&state, SideId::One, &mega), Ok(()));
    }

    #[test]
    fn doubles_joint_action_validation() {
        let mc = Ruleset::CHAMPIONS_MC;
        let mut state = doubles();
        let side = SideId::One;

        assert_eq!(
            mc.validate_joint_action(
                &state,
                side,
                &[mv(0, 1, Gimmick::Mega), mv(1, -1, Gimmick::Mega)]
            ),
            Err(ActionError::GimmickTwiceInTurn {
                slot: 1,
                gimmick: Gimmick::Mega
            })
        );
        assert_eq!(
            mc.validate_joint_action(&state, side, &[plain(0, 2), mv(1, 1, Gimmick::Mega)]),
            Ok(())
        );
        assert_eq!(
            mc.validate_joint_action(
                &state,
                side,
                &[SlotAction::Switch { party_index: 3 }, SlotAction::Pass]
            ),
            Ok(())
        );
        assert_eq!(
            mc.validate_joint_action(&state, side, &[plain(0, 3), plain(0, 1)]),
            Err(ActionError::TargetOutOfRange { slot: 0, target: 3 })
        );
        assert_eq!(
            mc.validate_joint_action(&state, side, &[plain(0, 1), plain(4, 1)]),
            Err(ActionError::MoveIndexOutOfRange { slot: 1, index: 4 })
        );
        assert_eq!(
            mc.validate_joint_action(
                &state,
                side,
                &[SlotAction::Switch { party_index: 6 }, plain(0, 1)]
            ),
            Err(ActionError::PartyIndexOutOfRange {
                slot: 0,
                party_index: 6
            })
        );
        assert_eq!(
            mc.validate_slot_action(&state, SlotRef { side, slot: 2 }, plain(0, 1)),
            Err(ActionError::SlotOutOfRange { slot: 2 })
        );

        state.side_mut(side).slots[1] = Slot::default();
        assert_eq!(
            mc.validate_joint_action(&state, side, &[plain(0, 1), mv(0, 1, Gimmick::Mega)]),
            Err(ActionError::EmptySlot { slot: 1 })
        );
        assert!(mc
            .available_gimmicks(&state, SlotRef { side, slot: 1 })
            .is_empty());
    }

    #[test]
    fn singles_share_the_same_rules() {
        let mut state = State::<1>::default();
        let pokemon = &mut state.side_mut(SideId::One).party[0];
        pokemon.hp = 100;
        pokemon.gimmicks = GimmickSet::ALL;
        state.side_mut(SideId::One).slots[0].party_index = Some(0);

        let mc = Ruleset::CHAMPIONS_MC;
        assert_eq!(
            mc.validate_joint_action(&state, SideId::One, &[mv(0, 1, Gimmick::Mega)]),
            Ok(())
        );
        assert_eq!(
            mc.validate_joint_action(&state, SideId::One, &[mv(0, 1, Gimmick::Dynamax)]),
            Err(ActionError::GimmickDisabled {
                slot: 0,
                gimmick: Gimmick::Dynamax
            })
        );
        assert_eq!(
            mc.validate_joint_action(&state, SideId::One, &[plain(0, 2)]),
            Err(ActionError::TargetOutOfRange { slot: 0, target: 2 })
        );
    }

    #[test]
    fn dynamax_state_resets_on_switch() {
        let mut state = doubles();
        state.slot_mut(ONE_0).dynamax = DynamaxState::Gigantamax { turns: 3 };
        let previous = state.slot(ONE_0).clone();
        let switch = [Instruction::Switch {
            slot: ONE_0,
            previous: Box::new(previous),
            party_index: Some(2),
        }];
        state.apply(&switch);
        assert!(!state.slot(ONE_0).dynamax.is_active());
        state.reverse(&switch);
        assert_eq!(
            state.slot(ONE_0).dynamax,
            DynamaxState::Gigantamax { turns: 3 }
        );
    }
}
