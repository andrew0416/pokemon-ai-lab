//! Item handlers (Showdown `data/items.ts`; the Champions mod overrides no callback of these
//! items) for the items listed in `support`.
//!
//! Each function is one Showdown event; `moves.rs`, `battle.rs`, `order.rs`, `residual.rs` and
//! `mod.rs` call it where Showdown runs that event. An item not handled here gets the event's
//! neutral result. Items are read straight from the holder: Klutz holding an item is refused
//! by `support` (Showdown's `ignoringItem`, work plan F17), so no handler here checks it.

use crate::damage::{MOD_HALF, MOD_ONE_POINT_FIVE};
use crate::dex::{
    abilities, conditions, items, moves, ItemId, MoveCategory, MoveData, MoveId, Secondary, Stat,
    Type, NO_BOOSTS,
};
use crate::state::{Pokemon, SlotRef, State, Status};
use crate::volatile::{Volatile, VolatileState};

use super::abilities::{Handler, SUB_ITEM};
use super::battle::{Battle, DamageSource};

/// The type-resist berries: `onSourceModifyDamage` halves a super-effective hit of one type
/// (Chilan Berry: every Normal hit) after eating the berry; their `onEat` does nothing.
pub(crate) const RESIST_BERRIES: [(ItemId, Type); 18] = [
    (items::OCCA_BERRY, Type::Fire),
    (items::PASSHO_BERRY, Type::Water),
    (items::WACAN_BERRY, Type::Electric),
    (items::RINDO_BERRY, Type::Grass),
    (items::YACHE_BERRY, Type::Ice),
    (items::CHOPLE_BERRY, Type::Fighting),
    (items::KEBIA_BERRY, Type::Poison),
    (items::SHUCA_BERRY, Type::Ground),
    (items::COBA_BERRY, Type::Flying),
    (items::PAYAPA_BERRY, Type::Psychic),
    (items::TANGA_BERRY, Type::Bug),
    (items::CHARTI_BERRY, Type::Rock),
    (items::KASIB_BERRY, Type::Ghost),
    (items::HABAN_BERRY, Type::Dragon),
    (items::COLBUR_BERRY, Type::Dark),
    (items::BABIRI_BERRY, Type::Steel),
    (items::CHILAN_BERRY, Type::Normal),
    (items::ROSELI_BERRY, Type::Fairy),
];

/// The type a resist berry weakens, if `item` is one.
pub(crate) fn resist_berry(item: ItemId) -> Option<Type> {
    RESIST_BERRIES
        .iter()
        .find(|&&(i, _)| i == item)
        .map(|&(_, t)| t)
}

/// Why a Pokémon's item cannot be simulated with its ability, if it cannot. Klutz makes the
/// holder ignore its item (`ignoringItem`) except for `ignoreKlutz` items; no item effect here
/// checks that yet (work plan F17), so a Klutz holder with any other item is refused.
pub(crate) fn held_item_problem(mon: &Pokemon) -> Option<String> {
    if mon.ability == abilities::KLUTZ && !mon.item.is_none() && !mon.item.data().ignore_klutz {
        return Some(format!(
            "{}: Klutz ignoring {} (ignoringItem, work plan F17)",
            mon.species.data().name,
            mon.item.data().name
        ));
    }
    None
}

/// Whether an item's `onStart` does nothing when its holder switches in (Showdown runs item
/// `onStart` handlers in the `SwitchIn` event): the Choice items only remove a `choicelock`
/// the newcomer cannot have yet.
pub(crate) fn inert_start(item: ItemId) -> bool {
    item.data().is_choice
}

// ---- Choice items ---------------------------------------------------------------------------

/// `ModifySpe` factor of the holder's item: Choice Scarf `chainModify(1.5)` (skipped while
/// Dynamaxed, which `support` refuses).
pub(crate) fn speed_modifier(item: ItemId) -> Option<u32> {
    (item == items::CHOICE_SCARF).then_some(MOD_ONE_POINT_FIVE)
}

/// `ModifyAtk` (physical moves) or `ModifySpA` (special moves) handlers of the user's item:
/// Choice Band / Choice Specs `chainModify(1.5)` at priority 1 (not while Dynamaxed).
pub(crate) fn attack_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let item = b.item(user);
    let (boosted, event) = match data.category {
        MoveCategory::Physical => (items::CHOICE_BAND, "onModifyAtkPriority"),
        _ => (items::CHOICE_SPECS, "onModifySpAPriority"),
    };
    if item == boosted {
        let p = super::abilities::priority(item.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ITEM, MOD_ONE_POINT_FIVE));
    }
    out
}

// ---- defensive stat items ----------------------------------------------------------------------

/// `ModifyDef` / `ModifySpD` handlers of the target's item (by the stat the move targets):
/// Assault Vest `onModifySpD` `chainModify(1.5)` (priority 1); Eviolite `onModifyDef` and
/// `onModifySpD` `chainModify(1.5)` (priority 2) when `pokemon.baseSpecies.nfe` (the species
/// itself: no forme change the engine makes turns a Pokémon that can evolve into another
/// species).
pub(crate) fn defense_handlers<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    defense_stat: Stat,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let Some(mon) = b.slot_mon(target) else {
        return out;
    };
    let item = mon.item;
    let event = match defense_stat {
        Stat::Def => "onModifyDefPriority",
        _ => "onModifySpDPriority",
    };
    let applies = match item {
        i if i == items::ASSAULT_VEST => defense_stat == Stat::Spd,
        i if i == items::EVIOLITE => mon.species.data().nfe,
        _ => false,
    };
    if applies {
        let p = super::abilities::priority(item.data().event_orders, event);
        out.push(Handler::of(b, target, p, SUB_ITEM, MOD_ONE_POINT_FIVE));
    }
    out
}

/// The user's item `onModifyMove` (`runEvent('ModifyMove')`, after the move's own): a Choice
/// item adds `choicelock`, whose `onStart` stores the move (`effectState.move`). A lock that
/// is already there is kept (`addVolatile` without `onRestart`).
pub(crate) fn on_modify_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, id: MoveId) {
    if b.item(user).data().is_choice && !b.volatile(user, Volatile::ChoiceLock).active {
        b.set_volatile_state(
            user,
            Volatile::ChoiceLock,
            VolatileState {
                active: true,
                duration: 0,
                counter: id.0,
            },
        );
    }
}

/// `choicelock`'s `onBeforeMove` (priority 0, after paralysis): the lock ends once the item is
/// no longer a Choice item; otherwise another move fails (no PP, no `lastMove`). `false` = the
/// move is not used. The engine only lets a locked Pokémon choose its move
/// ([`disabled_move`]), so the failure needs a lock set later in the turn.
pub(crate) fn before_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> bool {
    let lock = b.volatile(user, Volatile::ChoiceLock);
    if !lock.active {
        return true;
    }
    if !b.item(user).data().is_choice {
        b.remove_volatile(user, Volatile::ChoiceLock);
        return true;
    }
    id.0 == lock.counter
}

/// The item `DisableMove` handlers `endTurn` runs for every active Pokémon: `choicelock`'s
/// `onDisableMove` removes the lock once the item is no longer a Choice item (the disabling
/// itself is [`disabled_move`], read from the state when choices are checked).
pub(crate) fn end_turn_disable_move<const N: usize>(b: &mut Battle<'_, N>) {
    for slot in State::<N>::slot_refs() {
        if b.alive(slot).is_some()
            && b.volatile(slot, Volatile::ChoiceLock).active
            && !b.item(slot).data().is_choice
        {
            b.remove_volatile(slot, Volatile::ChoiceLock);
        }
    }
}

/// Why the Pokémon in `slot` cannot choose `id` because of its item (Showdown `DisableMove`):
/// Assault Vest disables every status move but Me First; `choicelock` disables every other
/// move while the item is a Choice item.
pub(crate) fn disabled_move<const N: usize>(
    state: &State<N>,
    slot: SlotRef,
    id: MoveId,
) -> Option<String> {
    let mon = state.active(slot)?;
    if mon.item == items::ASSAULT_VEST
        && id.data().category == MoveCategory::Status
        && id != moves::ME_FIRST
    {
        return Some(format!(
            "{} cannot use status moves with Assault Vest",
            mon.species.data().name
        ));
    }
    let lock = state.slot(slot).volatiles.get(Volatile::ChoiceLock);
    if lock.active && mon.item.data().is_choice && id.0 != lock.counter {
        return Some(format!(
            "{} is locked into {} by {}",
            mon.species.data().name,
            MoveId(lock.counter).data().name,
            mon.item.data().name
        ));
    }
    None
}

// ---- accuracy, critical hits, flinch, Damage ----------------------------------------------------

/// `ModifyAccuracy` handlers of the user's item (`onSourceModifyAccuracy`, priority -2, only for
/// a numeric accuracy, which is when the engine checks accuracy): Wide Lens 4505/4096; Zoom
/// Lens 4915/4096 when the target has no move action left in the queue
/// (`!this.queue.willMove(target)`).
pub(crate) fn accuracy_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let item = b.item(user);
    let modifier = match item {
        i if i == items::WIDE_LENS => Some(4505),
        i if i == items::ZOOM_LENS => {
            let moves_later = b.occupant(target).is_some_and(|p| b.will_move(p));
            (!moves_later).then_some(4915)
        }
        _ => None,
    };
    if let Some(modifier) = modifier {
        let p =
            super::abilities::priority(item.data().event_orders, "onSourceModifyAccuracyPriority");
        out.push(Handler::of(b, user, p, SUB_ITEM, modifier));
    }
    out
}

/// `ModifyCritRatio` of the user's item: Scope Lens and Razor Claw `return critRatio + 1`.
pub(crate) fn crit_ratio_bonus(item: ItemId) -> i32 {
    i32::from(item == items::SCOPE_LENS || item == items::RAZOR_CLAW)
}

/// King's Rock / Razor Fang `onModifyMove` (priority -1): a non-status move without a flinch
/// secondary gets `{chance: 10, volatileStatus: 'flinch'}` appended to its secondaries.
/// (Serene Grace, priority -2, would double it; it is refused by `support`.)
pub(crate) fn added_secondary(item: ItemId, data: &MoveData) -> Option<Secondary> {
    let flinch_item = item == items::KINGS_ROCK || item == items::RAZOR_FANG;
    let has_flinch = data
        .secondaries
        .iter()
        .any(|s| s.volatile_status == conditions::FLINCH);
    (flinch_item && data.category != MoveCategory::Status && !has_flinch).then_some(Secondary {
        chance: 10,
        status: Status::None,
        volatile_status: conditions::FLINCH,
        boosts: NO_BOOSTS,
        self_boosts: NO_BOOSTS,
    })
}

/// The item `Damage` handlers (priority -40, after Sturdy's -30) for `amount` of damage to the
/// Pokémon in `target`; returns the damage to deal.
/// - Focus Sash: at full HP, a move's damage that would faint leaves 1 HP (`useItem`).
/// - Focus Band: `this.randomChance(1, 10) && damage >= target.hp && effect.effectType ===
///   'Move'` leaves 1 HP. Showdown draws for every damage; drawing only when the rest holds
///   gives the same distribution.
pub(crate) fn on_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    amount: i32,
    source: DamageSource,
) -> i32 {
    let Some(mon) = b.slot_mon(target) else {
        return amount;
    };
    let hp = i32::from(mon.hp);
    if source != DamageSource::Move || amount < hp {
        return amount;
    }
    let survives = match mon.item {
        i if i == items::FOCUS_SASH => mon.hp == mon.max_hp && b.use_item(target),
        i if i == items::FOCUS_BAND => b.rng.chance(1, 10),
        _ => false,
    };
    if survives {
        hp - 1
    } else {
        amount
    }
}

/// Showdown `eatItem` for a held berry: it is consumed and becomes `lastItem`. The events it
/// runs (`UseItem`, `TryEatItem`, `Eat`, `EatItem`, `AfterUseItem`) have no implemented
/// handler: Unnerve, As One, Ripen, Cheek Pouch, Cud Chew and Unburden are refused by
/// `support`, and the resist berries' `onEat` is empty.
fn eat_item<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) -> bool {
    b.use_item(holder)
}

/// `ModifyDamage` handlers of items (`modifyDamage`, after the burn halving): the user's
/// `onModifyDamage` and the target's `onSourceModifyDamage`. `type_mod` is the hit's clamped
/// effectiveness (`getMoveHitData(move).typeMod`).
///
/// - Life Orb: `chainModify([5324, 4096])`.
/// - Expert Belt: `chainModify([4915, 4096])` on a super-effective hit.
/// - Resist berries: a hit of the berry's type (super effective, except for Chilan Berry)
///   eats the berry (`target.eatItem()`), then `chainModify(0.5)`. Showdown eats it inside the
///   handler; no other ModifyDamage handler reads the target's item, so eating it while the
///   handlers are collected gives the same result. A substitute would stop it, but substitutes
///   are refused by `support`.
pub(crate) fn modify_damage_handlers<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    type_mod: i32,
) -> Vec<Handler> {
    let mut out = Vec::new();
    match b.item(user) {
        i if i == items::LIFE_ORB => out.push(Handler::of(b, user, 0, SUB_ITEM, 5324)),
        i if i == items::EXPERT_BELT && type_mod > 0 => {
            out.push(Handler::of(b, user, 0, SUB_ITEM, 4915));
        }
        _ => {}
    }
    if let Some(ty) = resist_berry(b.item(target)) {
        let applies = data.move_type == ty && (ty == Type::Normal || type_mod > 0);
        if applies {
            let handler = Handler::of(b, target, 0, SUB_ITEM, MOD_HALF);
            if eat_item(b, target) {
                out.push(handler);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every resist berry's callbacks are the two implemented ones, and (a Showdown data
    /// coincidence worth pinning) the type it weakens is its Natural Gift type.
    #[test]
    fn resist_berries_match_the_dex() {
        for (item, ty) in RESIST_BERRIES {
            let data = item.data();
            assert_eq!(data.handlers, ["onEat", "onSourceModifyDamage"], "{item:?}");
            assert!(data.is_berry, "{item:?}");
            assert_eq!(data.natural_gift.map(|(_, t)| t), Some(ty), "{item:?}");
            assert!(data.event_orders.is_empty(), "{item:?}");
        }
    }
}
