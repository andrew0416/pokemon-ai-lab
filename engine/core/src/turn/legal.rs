//! The choices a side can make in a turn (WORKPLAN roadmap 7): what [`super::check_side`]
//! accepts, generated rather than validated, for the search.

use crate::action::{JointAction, SlotAction};
use crate::dex::moves;
use crate::gimmick::Gimmick;
use crate::rules::{repeated_gimmick, Ruleset};
use crate::state::{SideId, SlotRef, State};

use super::lock::{self, Locked, RECHARGE_INDEX, STRUGGLE_INDEX};
use super::{check_slot, choice_target, disabled, support, takes_target, valid_target_loc};

/// Every joint action `side` may choose in `state` under `ruleset`, in the form the turn runs
/// it (a locked Pokémon has its one forced action; Struggle when no move is usable; every
/// target location a targeted move can name, including empty foe slots, which the move
/// retargets as Showdown does; Mega Evolution variants from the ruleset). Slot 0 varies
/// fastest. Empty when the side has no legal choice (the battle is over, a replacement or
/// mid-turn switch is pending). Whether the state itself is supported (`support::check_state`)
/// is left to `enumerate_turn`.
pub fn legal_joint_actions<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    side: SideId,
) -> Vec<JointAction<N>> {
    if state.result.is_over() {
        return Vec::new();
    }
    // What `Ruleset::joint_actions` would combine, each slot's candidates checked once on their
    // own (`validate_slot_action` + `check_slot`, what `check_side` runs per slot) instead of
    // once per joint action (board P2a: `check_side` was 63 % of this function). Only the
    // cross-slot rules remain per joint action: one activation mode per turn and two slots
    // switching to the same party member. The joint actions come out in the same order as
    // `check_side` over `Ruleset::joint_actions` (slot 0 fastest; a slot's failing candidates
    // are skipped, which keeps the others' relative order).
    //
    // Duplicates (two candidates the turn runs the same way) are dropped per slot, keeping the
    // first (board P2b: the quadratic de-duplication of the joint actions was 30 %): a joint
    // action's first occurrence in the slot-0-fastest order is the combination of its slots'
    // first occurrences, so the order of the distinct joint actions is that of the combinations
    // of the per-slot distinct actions. The cross-slot rules read the normalized actions,
    // which carry the candidates' gimmicks and switch targets (a normalization only rewrites a
    // move choice without a gimmick into the locked move or Struggle), so every candidate
    // with the same normalized action passes or fails them alike.
    let checked: [Vec<SlotAction>; N] = std::array::from_fn(|i| {
        let slot = SlotRef {
            side,
            slot: i as u8,
        };
        let available = ruleset.available_gimmicks(state, slot);
        let mut out = Vec::new();
        let mut check = |action: SlotAction| {
            if ruleset.validate_slot_action(state, slot, action).is_ok() {
                if let Ok(normalized) = check_slot(state, side, i as u8, action) {
                    debug_assert_eq!(normalized.gimmick(), action.gimmick());
                    if !out.contains(&normalized) {
                        out.push(normalized);
                    }
                }
            }
        };
        for base in slot_candidates(state, slot) {
            let base = base.with_gimmick(Gimmick::None);
            if ruleset.validate_slot_action(state, slot, base).is_err() {
                continue;
            }
            check(base);
            if let SlotAction::Move { .. } = base {
                for gimmick in available.iter() {
                    check(base.with_gimmick(gimmick));
                }
            }
        }
        out
    });
    if checked.iter().any(Vec::is_empty) {
        return Vec::new();
    }
    let mut out: Vec<JointAction<N>> = Vec::with_capacity(checked.iter().map(Vec::len).product());
    let mut cursor = [0usize; N];
    loop {
        let action: JointAction<N> = std::array::from_fn(|i| checked[i][cursor[i]]);
        if repeated_gimmick(&action).is_none() && !switches_twice(&action) {
            out.push(action);
        }
        let mut i = 0;
        loop {
            if i == N {
                return out;
            }
            cursor[i] += 1;
            if cursor[i] < checked[i].len() {
                break;
            }
            cursor[i] = 0;
            i += 1;
        }
    }
}

/// [`legal_joint_actions`] as it was before board P2a: `check_side` over every joint action
/// of `Ruleset::joint_actions`, duplicates dropped. The reference the tests hold the fast
/// generation to (same actions, same order); not for use.
#[doc(hidden)]
pub fn legal_joint_actions_reference<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    side: SideId,
) -> Vec<JointAction<N>> {
    if state.result.is_over() {
        return Vec::new();
    }
    let candidates: [Vec<SlotAction>; N] = std::array::from_fn(|i| {
        slot_candidates(
            state,
            SlotRef {
                side,
                slot: i as u8,
            },
        )
    });
    let refs: [&[SlotAction]; N] = std::array::from_fn(|i| candidates[i].as_slice());
    let mut raw = Vec::new();
    ruleset.joint_actions(state, side, refs, &mut raw);
    let mut out: Vec<JointAction<N>> = Vec::with_capacity(raw.len());
    for action in raw {
        if let Ok(normalized) = super::check_side(state, ruleset, side, &action) {
            if !out.contains(&normalized) {
                out.push(normalized);
            }
        }
    }
    out
}

/// Whether two slots of `action` switch to the same party member (`check_side`).
fn switches_twice<const N: usize>(action: &JointAction<N>) -> bool {
    action.iter().enumerate().any(|(i, a)| {
        matches!(a, SlotAction::Switch { party_index } if action[..i]
            .iter()
            .any(|b| matches!(b, SlotAction::Switch { party_index: q } if q == party_index)))
    })
}

/// The base choices of one slot (no gimmicks; the ruleset adds those).
fn slot_candidates<const N: usize>(state: &State<N>, slot: SlotRef) -> Vec<SlotAction> {
    let Some(mon) = state.active(slot).filter(|p| p.is_alive()) else {
        return vec![SlotAction::Pass];
    };
    // A commanding Tatsugiri (Commander) can only pass.
    if state
        .slot(slot)
        .volatiles
        .has(crate::volatile::Volatile::Commanding)
    {
        return vec![SlotAction::Pass];
    }
    if let Some(locked) = lock::locked_move(state, slot) {
        let (index, target) = match locked {
            Locked::Recharge => (RECHARGE_INDEX, 0),
            Locked::Move(id) => (
                mon.moves.iter().position(|m| m.id == id).unwrap_or(0) as u8,
                0,
            ),
            Locked::TwoTurn { id, target } => (
                mon.moves.iter().position(|m| m.id == id).unwrap_or(0) as u8,
                target,
            ),
        };
        return vec![SlotAction::Move {
            index,
            target,
            gimmick: Gimmick::None,
        }];
    }
    let mut out = Vec::new();
    let mut usable = false;
    for (index, slot_move) in mon.moves.iter().enumerate() {
        let id = slot_move.id;
        if id.is_none()
            || slot_move.pp == 0
            || disabled(state, slot, id).is_some()
            || support::move_unsupported(id).is_some()
        {
            continue;
        }
        usable = true;
        let target_type = choice_target(mon, id);
        if takes_target(N, target_type) {
            let n = N as i8;
            for loc in (-n..=n).filter(|&loc| loc != 0) {
                if valid_target_loc(N, slot, loc, target_type) {
                    out.push(SlotAction::Move {
                        index: index as u8,
                        target: loc,
                        gimmick: Gimmick::None,
                    });
                }
            }
        } else {
            out.push(SlotAction::Move {
                index: index as u8,
                target: 0,
                gimmick: Gimmick::None,
            });
        }
    }
    if !usable && support::move_unsupported(moves::STRUGGLE).is_none() {
        out.push(SlotAction::Move {
            index: STRUGGLE_INDEX,
            target: 0,
            gimmick: Gimmick::None,
        });
    }
    let s = state.side(slot.side);
    for party in 0..s.party.len() as u8 {
        let member = &s.party[party as usize];
        let active = s.slots.iter().any(|x| x.party_index == Some(party));
        if member.is_alive() && !member.species.is_none() && !active {
            out.push(SlotAction::Switch { party_index: party });
        }
    }
    out
}
