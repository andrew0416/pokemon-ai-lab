//! Forced choices (WORKPLAN F9): Showdown's `LockMove` event. A Pokémon locked into a move
//! (Outrage's `lockedmove`) or into recharging (`mustrecharge`) cannot choose anything else;
//! Showdown replaces whatever it chose by the locked move, charges no PP for it, and forbids
//! switching (`trapped`). Encore is not a lock: it disables the other moves (`DisableMove`) and
//! overrides a move already chosen this turn (`OverrideAction`).

use crate::dex::{moves, MoveId};
use crate::state::{Pokemon, SlotRef, State};
use crate::volatile::Volatile;

/// Showdown `getLockedMove()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locked {
    Move(MoveId),
    /// The `recharge` pseudo-move: the turn is spent recharging.
    Recharge,
}

/// The move slot index that stands for the `recharge` pseudo-move in an action.
pub const RECHARGE_INDEX: u8 = u8::MAX;

/// The move slot index that stands for Struggle in an action: the only choice of a Pokémon
/// without a usable move (Showdown `getMoves()` empty: every move out of PP or disabled). A
/// choice of the first move (`move 1`) is read as Struggle then, as Showdown does.
pub const STRUGGLE_INDEX: u8 = u8::MAX - 1;

/// The move an action's move index names: the move in that slot, Struggle, or none for the
/// `recharge` pseudo-move.
pub(crate) fn action_move_id(mon: &Pokemon, index: u8) -> MoveId {
    match index {
        RECHARGE_INDEX => MoveId::NONE,
        STRUGGLE_INDEX => moves::STRUGGLE,
        _ => mon.moves[index as usize].id,
    }
}

/// What the Pokémon at `slot` is locked into, if anything.
pub fn locked_move<const N: usize>(state: &State<N>, slot: SlotRef) -> Option<Locked> {
    let volatiles = &state.slot(slot).volatiles;
    if volatiles.has(Volatile::MustRecharge) {
        return Some(Locked::Recharge);
    }
    let locked = volatiles.get(Volatile::LockedMove);
    if locked.active {
        return Some(Locked::Move(locked.mv));
    }
    None
}
