//! Item handlers (Showdown `data/items.ts`; the Champions mod overrides no callback of these
//! items) for the items listed in `support`.
//!
//! Each function is one Showdown event; `moves.rs`, `battle.rs`, `order.rs`, `residual.rs` and
//! `mod.rs` call it where Showdown runs that event. An item not handled here gets the event's
//! neutral result. Items are read straight from the holder: Klutz holding an item is refused
//! by `support` (Showdown's `ignoringItem`, work plan F17), so no handler here checks it.

use crate::dex::{abilities, items, ItemId, MoveData, Type};
use crate::state::{Pokemon, SlotRef};

use super::abilities::{Handler, SUB_ITEM};
use super::battle::Battle;

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
    if b.item(user) == items::LIFE_ORB {
        out.push(Handler::of(b, user, 0, SUB_ITEM, 5324));
    }
    if let Some(ty) = resist_berry(b.item(target)) {
        let applies = data.move_type == ty && (ty == Type::Normal || type_mod > 0);
        if applies {
            let handler = Handler::of(b, target, 0, SUB_ITEM, crate::damage::MOD_HALF);
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
