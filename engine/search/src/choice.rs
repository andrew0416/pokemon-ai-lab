//! A side's choice at any decision, and its Showdown choice-string form.

use lab_engine::action::{JointAction, SlotAction};
use lab_engine::gimmick::Gimmick;
use lab_engine::state::{SideId, SlotRef, State};
use lab_engine::turn::{locked_move, Locked, RECHARGE_INDEX, STRUGGLE_INDEX};

/// One side's choice at a decision (see [`crate::game::Decision`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Choice<const N: usize> {
    /// A turn: one action per active slot, in the form the turn runs it.
    Turn(JointAction<N>),
    /// A replacement or mid-turn switch decision: the party index entering each slot;
    /// `[None; N]` when the side is not asked (it waits).
    Switches([Option<u8>; N]),
}

impl<const N: usize> Choice<N> {
    pub const WAIT: Choice<N> = Choice::Switches([None; N]);
}

/// `action` as Showdown's choice string against `order`, the side's current party order
/// (what `switch N` counts; `lab_scenario::PartyOrder`): `move hypervoice 1, move protect`,
/// `switch 3`, `pass`, `move 2 -1 mega`. Moves are written by id; the locked pseudo-moves as
/// `move recharge` and `move struggle`; in doubles a locked move aimed at no location (Outrage,
/// Uproar, the recharge turn) with target 1, which Showdown requires and ignores
/// (`move outrage 1`, `move recharge 1`); a lock onto a move the Pokémon does not know (one Copycat
/// called) as `move 1`, the only move of Showdown's request, whose index form takes no target.
/// The inverse of `lab_scenario::parse_choice` for actions in their normalized form.
pub fn format_choice<const N: usize>(
    state: &State<N>,
    side: SideId,
    order: &[u8],
    action: &JointAction<N>,
) -> String {
    let parts: Vec<String> = action
        .iter()
        .enumerate()
        .map(|(i, slot_action)| {
            let slot = SlotRef {
                side,
                slot: i as u8,
            };
            format_slot_action(state, slot, order, *slot_action)
        })
        .collect();
    parts.join(", ")
}

fn format_slot_action<const N: usize>(
    state: &State<N>,
    slot: SlotRef,
    order: &[u8],
    action: SlotAction,
) -> String {
    match action {
        SlotAction::Pass => "pass".to_owned(),
        SlotAction::Switch { party_index } => format!("switch {}", position(order, party_index)),
        SlotAction::Move {
            index,
            target,
            gimmick,
        } => {
            let unknown_lock = match (locked_move(state, slot), state.active(slot)) {
                (Some(Locked::Move(id) | Locked::TwoTurn { id, .. }), Some(mon)) => {
                    !mon.moves.iter().any(|m| m.id == id)
                }
                _ => false,
            };
            if unknown_lock {
                return "move 1".to_owned();
            }
            // Showdown's `chooseMove` takes a named move's target type from the request, and a
            // locked Pokémon's request (`getMoves(lockedMove)`) gives none, so it counts as
            // `normal`: in doubles the choice needs a target, which the lock then ignores
            // (`lastMoveTargetLoc`). A locked move aimed at no location (Outrage, Petal Dance,
            // Thrash, Raging Fury, Uproar, the recharge turn, and the second turn of a two-turn
            // move that targets no location: Geomancy, Razor Wind) is written with target 1, as
            // the hand-written fixtures do (`outrage-lock`, `hyper-beam-recharge`,
            // `va-geomancy-charge-lock`).
            let locked_untargeted = N >= 2
                && target == 0
                && matches!(
                    locked_move(state, slot),
                    Some(Locked::Move(_) | Locked::Recharge | Locked::TwoTurn { .. })
                );
            let mut text = if index == RECHARGE_INDEX {
                "move recharge".to_owned()
            } else if index == STRUGGLE_INDEX {
                "move struggle".to_owned()
            } else {
                let name = state
                    .active(slot)
                    .and_then(|mon| mon.moves.get(index as usize))
                    .map(|m| m.id.id())
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| (index + 1).to_string());
                format!("move {name}")
            };
            if target != 0 {
                text.push_str(&format!(" {target}"));
            } else if locked_untargeted {
                text.push_str(" 1");
            }
            match gimmick {
                Gimmick::None => {}
                Gimmick::Mega => text.push_str(" mega"),
                Gimmick::Tera => text.push_str(" terastallize"),
                Gimmick::Dynamax => text.push_str(" dynamax"),
                Gimmick::ZMove => text.push_str(" zmove"),
                Gimmick::UltraBurst => text.push_str(" ultra"),
            }
            text
        }
    }
}

/// A replacement or mid-turn switch choice as Showdown writes it, one entry per slot the
/// decision asks about (`slots`, in slot order; `lab_scenario::parse_replacement` fills the
/// empty slots, `parse_mid_turn` the flagged ones): `switch 3`, `switch 3, pass`. Empty when
/// the side is not asked.
pub fn format_switches<const N: usize>(
    order: &[u8],
    slots: &[usize],
    choice: &[Option<u8>; N],
) -> String {
    let parts: Vec<String> = slots
        .iter()
        .map(|&i| match choice[i] {
            Some(party) => format!("switch {}", position(order, party)),
            None => "pass".to_owned(),
        })
        .collect();
    parts.join(", ")
}

/// Showdown's 1-based position of `party_index` in `order`.
fn position(order: &[u8], party_index: u8) -> String {
    match order.iter().position(|&p| p == party_index) {
        Some(i) => (i + 1).to_string(),
        None => format!("?{party_index}"),
    }
}
