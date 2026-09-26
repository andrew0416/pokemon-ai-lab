//! The choices a side can make in a turn (WORKPLAN roadmap 7): what [`super::check_side`]
//! accepts, generated rather than validated, for the search.

use crate::action::{JointAction, SlotAction};
use crate::dex::moves;
use crate::gimmick::Gimmick;
use crate::rules::Ruleset;
use crate::state::{SideId, SlotRef, State};

use super::lock::{self, Locked, RECHARGE_INDEX, STRUGGLE_INDEX};
use super::{check_side, disabled, support, takes_target, valid_target_loc};

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
        if let Ok(normalized) = check_side(state, ruleset, side, &action) {
            if !out.contains(&normalized) {
                out.push(normalized);
            }
        }
    }
    out
}

/// The base choices of one slot (no gimmicks; the ruleset adds those).
fn slot_candidates<const N: usize>(state: &State<N>, slot: SlotRef) -> Vec<SlotAction> {
    let Some(mon) = state.active(slot).filter(|p| p.hp > 0) else {
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
        let data = id.data();
        if takes_target(N, data.target) {
            let n = N as i8;
            for loc in (-n..=n).filter(|&loc| loc != 0) {
                if valid_target_loc(N, slot, loc, data.target) {
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
        if member.hp > 0 && !member.species.is_none() && !active {
            out.push(SlotAction::Switch { party_index: party });
        }
    }
    out
}
