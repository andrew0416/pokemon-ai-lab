//! Mega Evolution as a queued action (Showdown `megaEvo`, order 104: after switches, before
//! every move, in Speed order among themselves) and its effect (`runMegaEvo`).
//!
//! `runMegaEvo` is `formeChange(megaSpecies, item, permanent)` followed by clearing
//! `canMegaEvo` on the whole side and the `AfterMega` event:
//!
//! 1. `setSpecies`: species, types and stats become the Mega forme's (stats from the same
//!    nature and SP, `spreadModify`); `updateMaxHp` keeps the HP lost so far.
//! 2. The ability becomes the Mega forme's first ability, permanently (`baseAbility` too).
//!    `setAbility(..., isFromFormeChange)` runs the old ability's `End` and the new one's
//!    `Start`, so Sand Stream sets sand (or fails against the same weather) and Intimidate
//!    fires again.
//! 3. The side's once-per-battle Mega budget is spent (`gimmicks_used`).
//! 4. `AfterMega` has handlers only in items and abilities the engine does not support on the
//!    field (Eject Pack, Mirror Herb, White Herb, Opportunist-style deferred boosts), so
//!    there is nothing to run yet; it is the hook point when they arrive.
//!
//! Eligibility is decided by the ruleset ([`crate::rules::Ruleset::available_gimmicks`]) from
//! species and held item, exactly as Champions' `canMegaEvo` (`item.megaStone[species.name]`).
//! It is deliberately independent of item suppression: `runMegaEvo` reads `getItem()`, so
//! Magic Room, Embargo and Klutz never block it (DESIGN.md).
//!
//! Gen 9 recomputes every queued action's Speed before the next move runs, so the Mega forme's
//! Speed decides the rest of the turn; the engine re-keys the queue at every stage anyway. The
//! one place that differs is unreachable in practice: Showdown skips the re-sort when the next
//! queued action is another `megaEvo` (not a move), so two Mega Evolutions in one turn keep
//! the order computed before a switch-in changed Speeds.

use crate::dex::SpeciesId;
use crate::gimmick::{mega_evolution, Gimmick};
use crate::instruction::Instruction;
use crate::state::{Pokemon, SlotRef};

use super::battle::{cured_on_update, Battle};
use super::support::ability_supported_on_field;
use super::switching::{end_ability, start_ability, switch_in_supported};
use super::TurnError;

/// The Mega forme `mon` evolves into, if the turn engine can simulate the change: the forme
/// must exist for this species and item, have no species callbacks, and its ability must be
/// implemented both on the field and when it starts.
pub(crate) fn mega_target(mon: &Pokemon) -> Result<SpeciesId, String> {
    let name = mon.species.data().name;
    let Some(mega) = mega_evolution(mon.species, mon.item) else {
        return Err(format!(
            "{name} holding {} has no Mega Evolution",
            mon.item.data().name
        ));
    };
    let data = mega.data();
    if !data.handlers.is_empty() {
        return Err(format!(
            "{}: species callbacks {:?}",
            data.name, data.handlers
        ));
    }
    let ability = data.abilities[0];
    if !ability_supported_on_field(ability) || !switch_in_supported(ability) {
        return Err(format!(
            "{}: ability {} ({:?})",
            data.name,
            ability.data().name,
            ability.data().handlers
        ));
    }
    Ok(mega)
}

/// Showdown `runMegaEvo` for the Pokémon at `slot`. Nothing happens if the slot is empty or
/// its Pokémon fainted before this action (Showdown would then `formeChange` a fainted
/// Pokémon, but a `megaEvo` action can only be preceded by switches, which deal no damage).
pub(crate) fn run_mega_evo<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
) -> Result<(), TurnError> {
    let Some(pokemon) = b.alive(slot) else {
        return Ok(());
    };
    let mon = b.mon(pokemon);
    let mega = mega_target(mon).map_err(|why| b.unsupported(why))?;
    let old = mon.forme();
    let new = mon.forme_as(mega);
    // The Update after this action would cure the status with the new ability (a sleeping
    // Mewtwo becoming Mewtwo-Mega-Y with Insomnia); no Update event yet, see `cured_on_update`.
    if cured_on_update(new.ability, mon.status) {
        return Err(b.unsupported(format!(
            "{}: {} would cure {:?} on the next Update",
            mega.data().name,
            new.ability.data().name,
            mon.status
        )));
    }
    let hp = mon.hp;
    let new_hp = new.hp_after(old.max_hp, hp);
    // setAbility → the old ability's `End` (Flash Fire drops its volatile).
    end_ability(b, slot, old.ability)?;
    b.apply(Instruction::SetForme {
        target: pokemon,
        old,
        new,
    });
    if new_hp != hp {
        // updateMaxHp adjusts HP silently, outside `damage`/`heal`.
        let amount = hp - new_hp;
        if amount > 0 {
            b.apply(Instruction::Damage {
                target: pokemon,
                amount,
            });
        } else {
            b.apply(Instruction::Heal {
                target: pokemon,
                amount: -amount,
            });
        }
    }
    b.apply(Instruction::UseGimmick {
        side: slot.side,
        gimmick: Gimmick::Mega,
    });
    // setAbility → the new ability's `Start`.
    start_ability(b, slot, new.ability)?;
    // AfterMega: no supported handler.
    Ok(())
}
