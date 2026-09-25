//! The `Update` event (WORKPLAN F14) and the berries eaten on it.
//!
//! Showdown runs `eachEvent('Update')` after every action (`runAction`), after the damage of a
//! hit (`hitStepMoveHitLoop`), after the weather's residual damage, before a healthy Pokémon
//! switches out, and after a batch of switch-ins. Every active Pokémon is visited in stored
//! Speed order with ties shuffled; none of the implemented listeners touches another Pokémon
//! (Starf Berry's random stat is drawn independently per eater), so the order cannot change
//! the outcome and no random draw is spent on it.
//!
//! Listeners implemented here: berries with `onUpdate` (Sitrus, Oran, the five Figy-type
//! berries, the five pinch stat berries, Lansat, Starf, Lum and the six one-status berries,
//! Leppa) and Lum's `onAfterSetStatus`. [`eat_item`] also runs the `onEat` of the berries
//! eaten elsewhere (Kee, Maranga, Jaboca, Rowap, Micle, Custap, Enigma). Ability `onUpdate` handlers are either unreachable by construction
//! (`cured_on_update`) or refused (Trace still seeking, Disguise, ...).

use crate::dex::{abilities, items, ItemId, Stat, NO_BOOSTS};
use crate::instruction::Instruction;
use crate::state::{Pokemon, PokemonRef, SlotRef, Status};
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
    // Persim cures confusion, which the engine has no volatile for: it never fires.
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

/// Showdown `eachEvent('Update')`.
pub(crate) fn update_event<const N: usize>(b: &mut Battle<'_, N>) -> Result<(), TurnError> {
    let mut actives: Vec<(SlotRef, i32)> = b
        .all_alive()
        .into_iter()
        .map(|s| (s, b.action_speed(s)))
        .collect();
    actives.sort_by_key(|&(_, speed)| std::cmp::Reverse(speed));
    for (slot, _) in actives {
        if b.alive(slot).is_none() {
            continue;
        }
        if item_wants_eating(b, slot) {
            eat_item(b, slot);
        }
    }
    Ok(())
}

/// The berry's `onUpdate` condition.
fn item_wants_eating<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    let Some(mon) = b.slot_mon(slot) else {
        return false;
    };
    let item = mon.item;
    let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
    // `pokemon.hp <= pokemon.maxhp / 2` and `/ 4`, in integers.
    let half = 2 * hp <= max_hp;
    // Gluttony's `abilityState.gluttony` is set on switch-in and on damage: always set here.
    let pinch = 4 * hp <= max_hp || (half && mon.ability == abilities::GLUTTONY);
    if item == items::SITRUS_BERRY || item == items::ORAN_BERRY {
        half
    } else if FIGY_BERRIES.iter().any(|&(i, _)| i == item)
        || STAT_BERRIES.iter().any(|&(i, _)| i == item)
        || item == items::LANSAT_BERRY
        || item == items::STARF_BERRY
    {
        pinch
    } else if item == items::LUM_BERRY {
        // `pokemon.status || pokemon.volatiles['confusion']`.
        mon.status != Status::None
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
    let max_hp = f64::from(mon.max_hp);
    let status = mon.status;
    if item == items::SITRUS_BERRY {
        b.heal(slot, max_hp / 4.0);
    } else if item == items::ORAN_BERRY {
        b.heal(slot, 10.0);
    } else if FIGY_BERRIES.iter().any(|&(i, _)| i == item) {
        // The confusing case is refused before the turn (`berry_problem`).
        b.heal(slot, max_hp / 3.0);
    } else if let Some(&(_, index)) = STAT_BERRIES.iter().find(|&&(i, _)| i == item) {
        let mut up = NO_BOOSTS;
        up[index] = 1;
        b.boost_by(slot, &up, Some(slot), BoostEffect::Item(item));
    } else if item == items::LUM_BERRY {
        b.cure_status(pokemon);
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
            let new = (slot_move.pp + 10).min(max);
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
    consume(b, pokemon)
}

/// The end of `eatItem`: `lastItem = item; item = ''` (at any HP, unlike `useItem`).
fn consume<const N: usize>(b: &mut Battle<'_, N>, pokemon: PokemonRef) -> bool {
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
    true
}

/// `onAfterSetStatus` handlers: Lum Berry is eaten the moment a status lands.
pub(crate) fn after_set_status<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.item(slot) == items::LUM_BERRY {
        eat_item(b, slot);
    }
}
