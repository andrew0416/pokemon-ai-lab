//! Callbacks of the conditions moves create (`condition` in `data/moves.ts`): what happens
//! when a volatile starts and when its duration runs out in the residual.

use crate::dex::{moves, MoveCategory, MoveId, Type};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SlotRef, State, Status};
use crate::volatile::{decode_types, encode_types, Volatile, VolatileState};

use super::battle::Battle;
use super::TurnError;

/// Roost's `onType` from the moment the volatile starts: Flying is filtered out of the types
/// (`types.filter(type => type !== 'Flying')`, and `getTypes` gives Normal when nothing is
/// left). Showdown filters on every read; the engine changes the types once and saves the old
/// ones in the volatile's `counter` to restore them when Roost ends. (Its `onStart` only fails
/// for a Terastallized Pokémon, which the engine does not have.)
pub(crate) fn roost_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.occupant(slot) else {
        return;
    };
    let old = b.mon(pokemon).types;
    if !old.contains(&Type::Flying) {
        return;
    }
    let mut new = [Type::None; 2];
    let mut kept = old
        .into_iter()
        .filter(|&t| t != Type::Flying && t != Type::None);
    new[0] = kept.next().unwrap_or(Type::Normal);
    new[1] = kept.next().unwrap_or(Type::None);
    b.apply(Instruction::SetTypes {
        target: pokemon,
        old,
        new,
    });
    let state = b.volatile(slot, Volatile::Roost);
    b.set_volatile_state(
        slot,
        Volatile::Roost,
        VolatileState {
            counter: encode_types(old),
            ..state
        },
    );
}

/// The volatile's `onEnd` when its duration runs out in the residual (Showdown
/// `removeVolatile`: `End` runs while the volatile is still there; the caller removes it).
pub(crate) fn volatile_end<const N: usize>(
    b: &mut Battle<'_, N>,
    pokemon: PokemonRef,
    slot: SlotRef,
    volatile: Volatile,
) -> Result<(), TurnError> {
    let state = b.volatile(slot, volatile);
    match volatile {
        // Roost ends: the types are read without its filter again.
        Volatile::Roost if state.counter != 0 => {
            let old = b.mon(pokemon).types;
            b.apply(Instruction::SetTypes {
                target: pokemon,
                old,
                new: decode_types(state.counter),
            });
        }
        // Yawn: `target.trySetStatus('slp', this.effectState.source)` with the Yawn condition
        // as the effect: Safeguard lets it through, the sleep blocks (abilities, Sweet Veil,
        // Electric and Misty Terrain) apply.
        Volatile::Yawn => {
            b.try_set_status_from(slot, Status::Sleep, None);
        }
        // Perish Song: `target.faint()`.
        Volatile::PerishSong => b.faint(slot),
        // A locked move (Outrage) that ran its course confuses the user (`trueDuration <= 1`);
        // `Battle::remove_volatile` does the same when the move itself ends it.
        Volatile::LockedMove if state.hidden <= 1 => {
            b.add_volatile(slot, Volatile::Confusion);
        }
        _ => {}
    }
    Ok(())
}

/// The volatile's `onStart` when it is added (after `TryAddVolatile`; Encore's, confusion's and
/// a locked move's are in `Battle::add_volatile_from`): it may change the new state, or fail
/// (`false`: the volatile is not added).
pub(crate) fn volatile_start<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    volatile: Volatile,
    new: &mut VolatileState,
) -> bool {
    match volatile {
        // Taunt: `if (target.activeTurns && !this.queue.willMove(target))
        // this.effectState.duration++;` (`activeTurns` is `active_since_turn_start`).
        Volatile::Taunt => {
            if b.active_since_turn_start(target) && b.will_move(target).is_none() {
                new.duration += 1;
            }
            true
        }
        _ => true,
    }
}

/// The user's condition `onBeforeMove` handlers between Gravity (priority 6) and confusion
/// (3): Taunt (5) fails a status move other than Me First. `false` = the move is not used.
pub(crate) fn before_move_after_gravity<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> bool {
    let data = id.data();
    let taunted = b.volatile(user, Volatile::Taunt).active
        && data.category == MoveCategory::Status
        && id != moves::ME_FIRST;
    !taunted
}

/// Why the Pokémon in `slot` cannot choose `id` because of a condition on it (the conditions'
/// `DisableMove` handlers that `endTurn` runs): Taunt disables every status move but Me First.
pub(crate) fn disabled_move<const N: usize>(
    state: &State<N>,
    slot: SlotRef,
    id: MoveId,
) -> Option<String> {
    let volatiles = &state.slot(slot).volatiles;
    let data = id.data();
    if volatiles.has(Volatile::Taunt)
        && data.category == MoveCategory::Status
        && id != moves::ME_FIRST
    {
        return Some(format!("{} is disabled by Taunt", data.name));
    }
    None
}
