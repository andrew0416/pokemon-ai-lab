//! The game the solver plays: which decision a position asks for, each side's legal choices,
//! and the exact outcome distribution of a pair of choices. Everything is delegated to
//! `lab_engine::turn`, so a choice the solver considers is exactly one the turn engine
//! accepts.

use lab_engine::action::SlotAction;
use lab_engine::dex::{moves, MoveCategory, MoveTarget};
use lab_engine::field::SlotCondition;
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, SideId, SlotRef, State, SwitchFlag};
use lab_engine::turn::{
    enumerate_replacements, enumerate_turn, legal_joint_actions, resume_turn, side_must_replace,
    side_must_switch, Suspension, TurnError,
};

use crate::choice::Choice;

/// What a position asks the sides for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The battle is over.
    Over(BattleResult),
    /// A turn stopped for a mid-turn switch (`Outcome::suspension`): the sides with a flagged
    /// slot choose ([`side_must_switch`]), the turn continues with `resume_turn`.
    MidTurn,
    /// Fainted Pokémon must be replaced before the next turn ([`side_must_replace`]).
    Replacement,
    /// Both sides choose a turn's actions.
    Turn,
}

/// Which legal choices the solver considers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pruning {
    /// Every legal choice.
    All,
    /// Legal choices minus damaging moves aimed at an ally (Pollen Puff and moves that can
    /// only target an ally excepted). Status moves keep their ally targets (Heal Pulse,
    /// Coaching, Skill Swap, ...).
    #[default]
    Sensible,
}

/// The decision `state` asks for. `suspension` is the one attached to the outcome that led to
/// `state` (a suspended turn), if any.
pub fn decision<const N: usize>(
    state: &State<N>,
    suspension: Option<&Suspension>,
) -> Result<Decision, TurnError> {
    if state.result.is_over() {
        return Ok(Decision::Over(state.result));
    }
    let pending = [SideId::One, SideId::Two]
        .into_iter()
        .any(|side| side_must_switch(state, side));
    match (suspension, pending) {
        (Some(_), true) => Ok(Decision::MidTurn),
        (Some(_), false) => Err(TurnError::InvalidChoice {
            side: SideId::One,
            slot: 0,
            reason: "a suspension without a slot that must switch".into(),
        }),
        (None, true) => Err(TurnError::InvalidChoice {
            side: SideId::One,
            slot: 0,
            reason: "a mid-turn switch is pending but no suspension was given".into(),
        }),
        (None, false) => {
            if [SideId::One, SideId::Two]
                .into_iter()
                .any(|side| side_must_replace(state, side))
            {
                Ok(Decision::Replacement)
            } else {
                Ok(Decision::Turn)
            }
        }
    }
}

/// The choices `side` may make at `decision`. A side that a replacement or mid-turn decision
/// does not ask has the single choice [`Choice::WAIT`]. Empty only when the battle is over.
pub fn legal_choices<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    decision: Decision,
    side: SideId,
    pruning: Pruning,
) -> Vec<Choice<N>> {
    match decision {
        Decision::Over(_) => Vec::new(),
        Decision::Turn => legal_joint_actions(state, ruleset, side)
            .into_iter()
            .filter(|action| pruning == Pruning::All || sensible(state, side, action))
            .map(Choice::Turn)
            .collect(),
        Decision::Replacement => {
            if !side_must_replace(state, side) {
                return vec![Choice::WAIT];
            }
            let slots = asked_slots(state, decision, side);
            let bench = bench(state, side);
            let pools: Vec<Vec<u8>> = slots.iter().map(|_| bench.clone()).collect();
            let required = slots.len().min(bench.len());
            assignments::<N>(&slots, &pools, required)
                .into_iter()
                .map(Choice::Switches)
                .collect()
        }
        Decision::MidTurn => {
            if !side_must_switch(state, side) {
                return vec![Choice::WAIT];
            }
            // Mirrors `turn::check_mid_turn_switches`: Revival Blessing's slot takes a fainted
            // party member, the others a bench member.
            let s = state.side(side);
            let slots = asked_slots(state, decision, side);
            let reviving = |i: usize| {
                s.slot_conditions[i][SlotCondition::RevivalBlessing as usize].is_active()
            };
            let bench = bench(state, side);
            let fainted: Vec<u8> = (0..s.party.len() as u8)
                .filter(|&i| s.party[i as usize].hp == 0 && !s.party[i as usize].species.is_none())
                .collect();
            let pools: Vec<Vec<u8>> = slots
                .iter()
                .map(|&i| {
                    if reviving(i) {
                        fainted.clone()
                    } else {
                        bench.clone()
                    }
                })
                .collect();
            let reviving_count = slots.iter().filter(|&&i| reviving(i)).count();
            let required = slots
                .iter()
                .filter(|&&i| {
                    if reviving(i) {
                        !fainted.is_empty()
                    } else {
                        true
                    }
                })
                .count()
                .min(bench.len() + reviving_count);
            assignments::<N>(&slots, &pools, required)
                .into_iter()
                .map(Choice::Switches)
                .collect()
        }
    }
}

/// The slots a replacement or mid-turn decision asks `side` about, in slot order (for
/// [`crate::format_switches`]).
pub fn asked_slots<const N: usize>(
    state: &State<N>,
    decision: Decision,
    side: SideId,
) -> Vec<usize> {
    let s = state.side(side);
    match decision {
        Decision::Replacement if side_must_replace(state, side) => (0..N)
            .filter(|&i| s.slots[i].party_index.is_none())
            .collect(),
        Decision::MidTurn if side_must_switch(state, side) => (0..N)
            .filter(|&i| {
                s.slots[i].switch_flag != SwitchFlag::None && s.slots[i].party_index.is_some()
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Every outcome of the pair of choices at `decision`; `state` is left unchanged.
pub fn transitions<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    decision: Decision,
    suspension: Option<&Suspension>,
    choices: [Choice<N>; 2],
) -> Result<Vec<Outcome>, TurnError> {
    let mismatch = || TurnError::InvalidChoice {
        side: SideId::One,
        slot: 0,
        reason: format!("{choices:?} do not fit {decision:?}"),
    };
    match decision {
        Decision::Over(_) => Err(TurnError::BattleOver),
        Decision::Turn => match choices {
            [Choice::Turn(a), Choice::Turn(b)] => enumerate_turn(state, ruleset, [a, b]),
            _ => Err(mismatch()),
        },
        Decision::Replacement => match choices {
            [Choice::Switches(a), Choice::Switches(b)] => enumerate_replacements(state, [a, b]),
            _ => Err(mismatch()),
        },
        Decision::MidTurn => match (choices, suspension) {
            ([Choice::Switches(a), Choice::Switches(b)], Some(suspension)) => {
                resume_turn(state, suspension, [a, b])
            }
            _ => Err(mismatch()),
        },
    }
}

/// Healthy party members not on the field.
fn bench<const N: usize>(state: &State<N>, side: SideId) -> Vec<u8> {
    let s = state.side(side);
    (0..s.party.len() as u8)
        .filter(|&i| {
            s.party[i as usize].hp > 0 && !s.slots.iter().any(|slot| slot.party_index == Some(i))
        })
        .collect()
}

/// Every way to fill exactly `required` of `slots` with distinct members of their pools
/// (`pools[k]` belongs to `slots[k]`), as per-slot party indices.
fn assignments<const N: usize>(
    slots: &[usize],
    pools: &[Vec<u8>],
    required: usize,
) -> Vec<[Option<u8>; N]> {
    #[allow(clippy::too_many_arguments)]
    fn go<const N: usize>(
        k: usize,
        slots: &[usize],
        pools: &[Vec<u8>],
        required: usize,
        used: usize,
        current: &mut [Option<u8>; N],
        out: &mut Vec<[Option<u8>; N]>,
    ) {
        if k == slots.len() {
            if used == required {
                out.push(*current);
            }
            return;
        }
        // Leave the slot empty (only useful when the pools cannot fill every slot).
        go(k + 1, slots, pools, required, used, current, out);
        for &party in &pools[k] {
            if current.contains(&Some(party)) {
                continue;
            }
            current[slots[k]] = Some(party);
            go(k + 1, slots, pools, required, used + 1, current, out);
            current[slots[k]] = None;
        }
    }
    let mut out = Vec::new();
    go::<N>(0, slots, pools, required, 0, &mut [None; N], &mut out);
    out
}

/// [`Pruning::Sensible`]: no damaging move at an ally unless the move heals it (Pollen Puff)
/// or can target nothing else.
fn sensible<const N: usize>(state: &State<N>, side: SideId, action: &[SlotAction; N]) -> bool {
    action.iter().enumerate().all(|(i, slot_action)| {
        let SlotAction::Move { index, target, .. } = *slot_action else {
            return true;
        };
        if target >= 0 {
            return true;
        }
        let Some(mon) = state.active(SlotRef {
            side,
            slot: i as u8,
        }) else {
            return true;
        };
        let Some(id) = mon.moves.get(index as usize).map(|m| m.id) else {
            return true;
        };
        let data = id.data();
        data.category == MoveCategory::Status
            || id == moves::POLLEN_PUFF
            || matches!(
                data.target,
                MoveTarget::AdjacentAlly | MoveTarget::AdjacentAllyOrSelf
            )
    })
}
