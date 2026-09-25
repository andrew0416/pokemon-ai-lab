//! Switch actions: Showdown `switchIn` (the old active leaves, its slot state and ability
//! reset) and `runSwitch` (the newcomer's switch-in handlers).
//!
//! Implemented switch-in handlers: weather and terrain setters and Intimidate; abilities whose
//! `onStart` only announces them are accepted as no-ops. A newcomer
//! with any other ability or item that acts on switch-in (or that the turn engine does not
//! implement on the field) makes the turn unsupported.

use crate::dex::{abilities, AbilityId, NO_BOOSTS};
use crate::field::{Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SlotRef, BOOST_COUNT};

use super::battle::{cured_on_update, Battle};
use super::moves::{set_terrain, set_weather};
use super::support::{ability_supported_on_field, item_supported_on_field};
use super::TurnError;

/// What an `onStart`-only ability does on switch-in.
enum StartEffect {
    None,
    Weather(Weather),
    Terrain(Terrain),
    Intimidate,
}

fn start_effect(ability: AbilityId) -> Option<StartEffect> {
    let data = ability.data();
    Some(match ability {
        a if a == abilities::DROUGHT => StartEffect::Weather(Weather::Sun),
        a if a == abilities::DRIZZLE => StartEffect::Weather(Weather::Rain),
        a if a == abilities::SAND_STREAM => StartEffect::Weather(Weather::Sand),
        a if a == abilities::SNOW_WARNING => StartEffect::Weather(Weather::Snow),
        a if a == abilities::ELECTRIC_SURGE => StartEffect::Terrain(Terrain::Electric),
        a if a == abilities::GRASSY_SURGE => StartEffect::Terrain(Terrain::Grassy),
        a if a == abilities::MISTY_SURGE => StartEffect::Terrain(Terrain::Misty),
        a if a == abilities::PSYCHIC_SURGE => StartEffect::Terrain(Terrain::Psychic),
        a if a == abilities::INTIMIDATE => StartEffect::Intimidate,
        // `onStart` only announces the ability.
        a if a == abilities::COMATOSE => StartEffect::None,
        // Implemented on the field and nothing at switch-in.
        _ if !data.handlers.contains(&"onStart") => StartEffect::None,
        _ => return None,
    })
}

/// Whether a Pokémon with this ability can switch in during a turn (its switch-in effect, if
/// any, is implemented).
pub(crate) fn switch_in_supported(ability: AbilityId) -> bool {
    start_effect(ability).is_some()
}

/// Showdown `switchIn` + `runSwitch` for a chosen switch.
pub(crate) fn run_switch<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    party_index: u8,
) -> Result<(), TurnError> {
    let incoming = PokemonRef {
        side: slot.side,
        party: party_index,
    };
    let mon = b.mon(incoming);
    let name = mon.species.data().name;
    if !ability_supported_on_field(mon.ability) || !item_supported_on_field(mon.item) {
        return Err(b.unsupported(format!("{name} switching in with its ability or item")));
    }
    if mon.item.data().handlers.contains(&"onStart") {
        return Err(b.unsupported(format!("{name}: item switch-in effect")));
    }
    let Some(effect) = start_effect(mon.ability) else {
        return Err(b.unsupported(format!(
            "{name}: switch-in ability {}",
            mon.ability.data().name
        )));
    };
    // The Update after the switch would cure the status (see `cured_on_update`).
    if cured_on_update(mon.ability, mon.status) {
        return Err(b.unsupported(format!(
            "{name}: {} would cure its status on Update",
            mon.ability.data().name
        )));
    }

    // The old active leaves: its ability reverts, the slot resets.
    if let Some(outgoing) = b.occupant(slot) {
        let out = b.mon(outgoing);
        if out.ability != out.base_ability {
            let (old, new) = (out.ability, out.base_ability);
            b.apply(Instruction::SetAbility {
                target: outgoing,
                old,
                new,
            });
        }
    }
    let previous = b.state.slot(slot).clone();
    b.apply(Instruction::Switch {
        slot,
        previous,
        party_index: Some(party_index),
    });

    // runSwitch: the newcomer's switch-in handlers.
    match effect {
        StartEffect::None => {}
        StartEffect::Weather(weather) => {
            set_weather(b, slot, weather);
        }
        StartEffect::Terrain(terrain) => {
            set_terrain(b, slot, terrain);
        }
        StartEffect::Intimidate => {
            let mut drop = NO_BOOSTS;
            drop[0] = -1;
            debug_assert_eq!(drop.len(), BOOST_COUNT);
            for foe in b.alive_slots(slot.side.other()) {
                b.boost(foe, &drop);
            }
        }
    }
    Ok(())
}
