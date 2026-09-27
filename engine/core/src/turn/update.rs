//! The `Update` event (WORKPLAN F14) and the berries eaten on it.
//!
//! Showdown runs `eachEvent('Update')` after every action (`runAction`), after the damage of a
//! hit (`hitStepMoveHitLoop`), after the weather's residual damage, before a healthy Pokémon
//! switches out, and after a batch of switch-ins. Every active Pokémon is visited in stored
//! Speed order with ties shuffled; of the implemented listeners only an item use next to an
//! ally's Symbiosis touches another Pokémon (Starf Berry's random stat is drawn independently
//! per eater), so only such a tie spends a random draw ([`update_event`]).
//!
//! Listeners implemented here: berries with `onUpdate` (Sitrus, Oran, the five Figy-type
//! berries, the five pinch stat berries, Lansat, Starf, Lum, Miracle and the six one-status
//! berries, Leppa) and Lum's `onAfterSetStatus`; the non-berry items Booster Energy, Mental
//! Herb and Berry Juice (used, not eaten); the abilities' `onUpdate` cures run first
//! (`abilities::on_update`: the status cures of `cured_on_update`, Own Tempo's confusion cure;
//! `forme::on_update`: Disguise, Ice Face). Other ability `onUpdate` handlers are refused (Trace
//! still seeking, ...). A berry
//! is eaten only if the `TryEatItem` handlers allow it (`abilities::try_eat_item`). [`eat_item`]
//! also runs the `onEat` of the berries eaten elsewhere (Kee, Maranga, Jaboca, Rowap, Micle,
//! Custap, Enigma).

use crate::dex::{abilities, items, ItemId, Stat, NO_BOOSTS};
use crate::instruction::Instruction;
use crate::state::{Pokemon, PokemonRef, SlotRef, State, Status};
use crate::volatile::Volatile;

use super::battle::{Battle, BoostEffect};
use super::TurnError;

/// The five "pinch" healing berries with the stat whose lowering nature confuses the eater.
const FIGY_BERRIES: [(ItemId, Stat); 5] = [
    (items::FIGY_BERRY, Stat::Atk),
    (items::WIKI_BERRY, Stat::Spa),
    (items::MAGO_BERRY, Stat::Spe),
    (items::AGUAV_BERRY, Stat::Spd),
    (items::IAPAPA_BERRY, Stat::Def),
];

/// The five pinch stat berries with the boost index they raise.
const STAT_BERRIES: [(ItemId, usize); 5] = [
    (items::LIECHI_BERRY, 0),
    (items::GANLON_BERRY, 1),
    (items::SALAC_BERRY, 4),
    (items::PETAYA_BERRY, 2),
    (items::APICOT_BERRY, 3),
];

/// Berries whose `onUpdate` eats them for one status.
const STATUS_BERRIES: [(ItemId, &[Status]); 6] = [
    (items::CHERI_BERRY, &[Status::Paralyze]),
    (items::CHESTO_BERRY, &[Status::Sleep]),
    (items::PECHA_BERRY, &[Status::Poison, Status::Toxic]),
    (items::RAWST_BERRY, &[Status::Burn]),
    (items::ASPEAR_BERRY, &[Status::Freeze]),
    // Persim cures confusion (a volatile: `item_wants_eating` and `eat_item` special-case it).
    (items::PERSIM_BERRY, &[]),
];

/// Why a berry holder cannot be simulated, if it cannot: a Figy-type berry confuses an eater
/// whose nature lowers the berry's stat, and confusion is not implemented.
pub(crate) fn berry_problem(mon: &Pokemon) -> Option<String> {
    let disliked = FIGY_BERRIES
        .iter()
        .find(|&&(item, _)| item == mon.item)
        .map(|&(_, stat)| stat);
    if let Some(stat) = disliked {
        if mon.nature.modifiers().1 == Some(stat) {
            return Some(format!(
                "{}: {} would confuse a {} nature (confusion is not implemented)",
                mon.species.data().name,
                mon.item.data().name,
                mon.nature.name()
            ));
        }
    }
    None
}

/// Showdown `eachEvent('Update')`: the actives are sorted once by `pokemon.speed`
/// (`speedSort(actives, (a, b) => b.speed - a.speed)`, ties shuffled), then each runs its
/// handlers, collected at its own turn (`runEvent('Update', pokemon)`: its state as the earlier
/// ones left it; an item it gains during its turn waits for the next Update).
///
/// Only Symbiosis links two Pokémon's handlers: the item one uses or eats is replaced by its
/// ally's, which the ally then no longer has for its own turn (oracle `w-update-symbiosis-tie`).
/// So a tie is drawn only between Pokémon on a side with an active Symbiosis holder; any other
/// tie cannot change the outcome and keeps slot order.
pub(crate) fn update_event<const N: usize>(b: &mut Battle<'_, N>) -> Result<(), TurnError> {
    // `getAllActive()` also holds a Pokémon at 0 HP whose faint is not processed yet (`fainted` is
    // set by `faintMessages`), and of its Update handlers Fling's condition acts: the user that a
    // reaction to the hit (Innards Out) knocked out before the hit loop's Update
    // ([`super::conditions::fling_update_fainted`]; oracle `rr-fling-innards-out`).
    let mut actives = b.all_alive();
    for slot in State::<N>::slot_refs() {
        if b.occupant(slot).is_some()
            && b.alive(slot).is_none()
            && b.volatile(slot, Volatile::Fling).active
        {
            actives.push(slot);
        }
    }
    // The effective ability: Symbiosis acts through `onAllyAfterUseItem`, a `runEvent` handler
    // skipped while its holder ignores its ability (no implemented Update handler changes that).
    let actives = super::abilities::speed_sorted(b, actives, |b, slot| {
        b.alive_slots(slot.side)
            .into_iter()
            .any(|s| b.ability(s) == abilities::SYMBIOSIS)
    });
    for slot in actives {
        if b.alive(slot).is_none() {
            super::conditions::fling_update_fainted(b, slot);
            continue;
        }
        // Its conditions' `onUpdate` (sub-order 2: Attract, Syrup Bomb, Fling; Attract and Syrup
        // Bomb only remove themselves, Fling throws the item before the item's own handlers
        // below), the ability's (7), the item's (8); a Pokémon has one ability, so the two
        // ability calls never both act.
        super::conditions::attract_update(b, slot);
        super::conditions::syrup_bomb_update(b, slot);
        super::conditions::fling_update(b, slot)?;
        super::abilities::on_update(b, slot);
        super::forme::on_update(b, slot);
        // The item's handlers were collected with the Pokémon's item at the start of its turn
        // in the event: an item Symbiosis gives it after a berry is eaten waits for the next
        // Update.
        let item = b.item(slot);
        if item_wants_eating(b, slot) {
            eat_item(b, slot);
        }
        // Booster Energy's, Mental Herb's and Berry Juice's `onUpdate`: only the collected item's
        // (one Symbiosis passes after another is used waits too).
        if b.item(slot) == item {
            if item == items::BOOSTER_ENERGY {
                super::abilities::booster_energy(b, slot);
            } else if item == items::MENTAL_HERB {
                super::items::mental_herb(b, slot);
            } else if item == items::BERRY_JUICE {
                super::items::berry_juice(b, slot);
            } else if item == items::UTILITY_UMBRELLA {
                super::items::umbrella_update(b, slot);
            }
        }
    }
    Ok(())
}

/// The berry's `onUpdate` condition.
fn item_wants_eating<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    let Some(mon) = b.slot_mon(slot) else {
        return false;
    };
    // The effective item: a suppressed berry (Magic Room, Klutz) is never eaten by `onUpdate`.
    let item = b.item(slot);
    let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
    // `pokemon.hp <= pokemon.maxhp / 2` and `/ 4`, in integers.
    let half = 2 * hp <= max_hp;
    // Gluttony's `abilityState.gluttony` is set on switch-in and on damage: always set here.
    let pinch = 4 * hp <= max_hp || (half && b.ability(slot) == abilities::GLUTTONY);
    if item == items::SITRUS_BERRY || item == items::ORAN_BERRY {
        half
    } else if FIGY_BERRIES.iter().any(|&(i, _)| i == item)
        || STAT_BERRIES.iter().any(|&(i, _)| i == item)
        || item == items::LANSAT_BERRY
        || item == items::STARF_BERRY
    {
        pinch
    } else if item == items::LUM_BERRY || item == items::MIRACLE_BERRY {
        // `pokemon.status || pokemon.volatiles['confusion']` (Miracle Berry: the same, without
        // Lum's `onAfterSetStatus`).
        mon.status != Status::None || b.volatile(slot, Volatile::Confusion).active
    } else if item == items::PERSIM_BERRY {
        b.volatile(slot, Volatile::Confusion).active
    } else if let Some(&(_, cured)) = STATUS_BERRIES.iter().find(|&&(i, _)| i == item) {
        cured.contains(&mon.status)
    } else if item == items::LEPPA_BERRY {
        mon.moves.iter().any(|m| !m.id.is_none() && m.pp == 0)
    } else {
        false
    }
}

/// Showdown `eatItem`: `TryEatItem` (nothing implemented blocks it), the berry's `onEat`, then
/// the berry is gone and remembered as `lastItem`. Returns whether it was eaten. The holder
/// must have HP, except for Jaboca and Rowap Berry (`!this.hp && this.item !== 'jabocaberry'
/// && this.item !== 'rowapberry'`), which a holder fainting from the hit still eats.
pub(crate) fn eat_item<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) -> bool {
    let at_zero_hp = [items::JABOCA_BERRY, items::ROWAP_BERRY].contains(&b.item(slot));
    let holder = if at_zero_hp {
        b.occupant(slot)
    } else {
        b.alive(slot)
    };
    let Some(pokemon) = holder else {
        return false;
    };
    let mon = b.mon(pokemon);
    let item = mon.item;
    if !item.data().is_berry {
        return false;
    }
    // TryEatItem: the eater's and its foes' ability handlers (`abilities::try_eat_item`), and the
    // healing berries' own `onTryEatItem` (Sitrus, Oran, the Figy berries, Enigma: `if
    // (!this.runEvent('TryHeal', pokemon, ...)) return false;`), which Heal Block answers.
    if !super::abilities::try_eat_item(b, slot) {
        return false;
    }
    if item.data().handlers.contains(&"onTryEatItem")
        && b.volatile(slot, Volatile::HealBlock).active
    {
        return false;
    }
    if !berry_on_eat(b, slot, pokemon, item) {
        return false;
    }
    // `runEvent('EatItem')`: Cheek Pouch, Cud Chew, Ripen.
    super::abilities::eat_item_event(b, slot, item, false);
    consume(b, slot, pokemon)
}

/// Showdown `eatItem(true)` (Stuff Cheeks, Teatime) for the holder in `slot`: a held berry
/// (`this.item`, whatever suppresses it) is eaten without `TryEatItem` (Unnerve and the healing
/// berries' Heal Block check do not apply) by an active holder with HP: its `onEat`, then
/// `lastItem` and AfterUseItem. A berry whose `onEat` is empty (the resist berries, Jaboca,
/// Rowap, Custap, Enigma, the effectless ones) is just consumed. Returns whether it was eaten.
/// A holder ignoring its item (Magic Room, Klutz) is unsupported.
pub(crate) fn eat_item_forced<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
) -> Result<bool, TurnError> {
    let Some(pokemon) = b.alive(slot) else {
        return Ok(false);
    };
    let item = b.mon(pokemon).item;
    if !item.data().is_berry {
        return Ok(false);
    }
    if super::items::ignoring_item(b.state, slot) {
        return Err(b.unsupported(format!(
            "{} eaten by force while its holder ignores its item",
            item.data().name
        )));
    }
    let empty_on_eat = super::items::resist_berry(item).is_some()
        || !item.data().handlers.contains(&"onEat")
        || [
            items::JABOCA_BERRY,
            items::ROWAP_BERRY,
            items::CUSTAP_BERRY,
            items::ENIGMA_BERRY,
        ]
        .contains(&item);
    if !empty_on_eat && !berry_on_eat(b, slot, pokemon, item) {
        return Err(b.unsupported(format!("{} eaten by force", item.data().name)));
    }
    Ok(consume(b, slot, pokemon))
}

/// `battle.heal` for a heal whose effect is a berry: the amount is normalized (at least 1,
/// truncated), then `runEvent('TryHeal')` doubles it for a Ripen holder (`chainModify(2)`) and
/// Heal Block stops it (`Battle::heal`).
pub(crate) fn berry_heal<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, amount: f64) {
    let amount = if amount > 0.0 && amount <= 1.0 {
        1.0
    } else {
        amount.trunc()
    };
    let factor = if super::abilities::ripens(b, slot) {
        2.0
    } else {
        1.0
    };
    b.heal(slot, amount * factor);
}

/// The berry's `onEat` for `pokemon` in `slot`: its holder, or the user of Bug Bite / Pluck
/// eating the target's berry (`singleEvent('Eat', item, ..., source, source, move)`). `false` for
/// a berry this does not implement (resist berries and effectless berries have an empty `onEat`
/// but are not eaten through [`eat_item`]).
pub(crate) fn berry_on_eat<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    pokemon: PokemonRef,
    item: ItemId,
) -> bool {
    let mon = b.mon(pokemon);
    let max_hp = f64::from(mon.max_hp);
    let status = mon.status;
    // The heals go through `berry_heal` (Ripen doubles them).
    if item == items::SITRUS_BERRY {
        berry_heal(b, slot, max_hp / 4.0);
    } else if item == items::ORAN_BERRY {
        berry_heal(b, slot, 10.0);
    } else if let Some(&(_, disliked)) = FIGY_BERRIES.iter().find(|&&(i, _)| i == item) {
        // `if (pokemon.getNature().minus === stat) pokemon.addVolatile('confusion');` (a holder
        // with such a nature is refused before the turn: `berry_problem`; a Bug Bite user is not).
        berry_heal(b, slot, max_hp / 3.0);
        if b.mon(pokemon).nature.modifiers().1 == Some(disliked) {
            b.add_volatile(slot, Volatile::Confusion);
        }
    } else if let Some(&(_, index)) = STAT_BERRIES.iter().find(|&&(i, _)| i == item) {
        let mut up = NO_BOOSTS;
        up[index] = 1;
        b.boost_by(slot, &up, Some(slot), BoostEffect::Item(item));
    } else if item == items::LUM_BERRY || item == items::MIRACLE_BERRY {
        // `cureStatus()` then `removeVolatile('confusion')`.
        b.cure_status(pokemon);
        b.remove_volatile(slot, Volatile::Confusion);
    } else if item == items::PERSIM_BERRY {
        b.remove_volatile(slot, Volatile::Confusion);
    } else if let Some(&(_, cured)) = STATUS_BERRIES.iter().find(|&&(i, _)| i == item) {
        if cured.contains(&status) {
            b.cure_status(pokemon);
        }
    } else if item == items::LEPPA_BERRY {
        // The first move at 0 PP (else the first below max) regains 10, capped at its max.
        let moves = b.mon(pokemon).moves;
        let index = moves
            .iter()
            .position(|m| !m.id.is_none() && m.pp == 0)
            .or_else(|| {
                moves
                    .iter()
                    .position(|m| !m.id.is_none() && m.pp < crate::state::champions_max_pp(m.id))
            });
        if let Some(index) = index {
            let slot_move = moves[index];
            let max = crate::state::champions_max_pp(slot_move.id);
            // `const addedPP = pokemon.hasAbility('ripen') ? 20 : 10;`
            let added = if super::abilities::ripens(b, slot) {
                20
            } else {
                10
            };
            let new = (slot_move.pp + added).min(max);
            b.apply(Instruction::SetPp {
                target: pokemon,
                move_index: index as u8,
                old: slot_move.pp,
                new,
            });
        }
    } else if let Some(index) = [(items::KEE_BERRY, 1), (items::MARANGA_BERRY, 3)]
        .iter()
        .find(|&&(i, _)| i == item)
        .map(|&(_, index)| index)
    {
        // `this.boost({def: 1})` / `{spd: 1}` (target and source: the eater).
        let mut up = NO_BOOSTS;
        up[index] = 1;
        b.boost_by(slot, &up, Some(slot), BoostEffect::Item(item));
    } else if item == items::LANSAT_BERRY {
        // `pokemon.addVolatile('focusenergy')` (fails if it is already there: no onRestart).
        b.add_volatile(slot, Volatile::FocusEnergy);
    } else if item == items::STARF_BERRY {
        // `this.sample(stats)` over Atk..Spe below +6, then `this.boost({[stat]: 2})`.
        let boosts = b.state.slot(slot).boosts;
        let stats: Vec<usize> = (0..5).filter(|&i| boosts[i] < 6).collect();
        if !stats.is_empty() {
            let pick = if stats.len() == 1 {
                0
            } else {
                b.rng.uniform(stats.len())
            };
            let mut up = NO_BOOSTS;
            up[stats[pick]] = 2;
            b.boost_by(slot, &up, Some(slot), BoostEffect::Item(item));
        }
    } else if item == items::MICLE_BERRY {
        // `pokemon.addVolatile('micleberry')` (no onRestart: kept if already there).
        b.add_volatile(slot, Volatile::MicleBerry);
    } else if ![
        items::JABOCA_BERRY,
        items::ROWAP_BERRY,
        items::CUSTAP_BERRY,
        items::ENIGMA_BERRY,
    ]
    .contains(&item)
    {
        // Those four have an empty `onEat`: their effect follows the eating in the handler
        // that ate them (damage, +0.1 priority, heal).
        return false;
    }
    true
}

/// The end of `eatItem`: `lastItem = item; item = ''` (at any HP, unlike `useItem`), then
/// `AfterUseItem` (Unburden).
fn consume<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, pokemon: PokemonRef) -> bool {
    let (item, last) = (b.mon(pokemon).item, b.mon(pokemon).last_item);
    if item.is_none() {
        return false;
    }
    b.apply(Instruction::SetLastItem {
        target: pokemon,
        old: last,
        new: item,
    });
    b.apply(Instruction::SetItem {
        target: pokemon,
        old: item,
        new: ItemId::NONE,
    });
    // `this.usedItemThisTurn = true` (Pickup), `this.ateBerry = true` (Belch).
    b.record_used_item(slot);
    b.record_ate_berry(pokemon);
    super::abilities::unburden(b, slot);
    super::abilities::symbiosis(b, slot);
    true
}

/// `onAfterSetStatus` handlers: Lum Berry is eaten the moment a status lands.
pub(crate) fn after_set_status<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.item(slot) == items::LUM_BERRY {
        eat_item(b, slot);
    }
}
