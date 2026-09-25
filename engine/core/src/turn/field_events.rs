//! Events a change of the field runs (work plan O102): Showdown `field.setTerrain` /
//! `clearTerrain` end with `eachEvent('TerrainChange')`, `setWeather` / `clearWeather` with
//! `eachEvent('WeatherChange')`, and a residual expiry ends the effect through the same
//! `clearTerrain` / `clearWeather`. `eachEvent` visits every active Pokémon in Speed order
//! (ties shuffled).
//!
//! Implemented handlers: the four Seeds' `onTerrainChange` (O92), Quark Drive's
//! `onTerrainChange` and Protosynthesis's `onWeatherChange` (O72), Ice Face's `onWeatherChange`
//! and Mimicry's `onTerrainChange` (F19, `forme.rs`). The other `onWeatherChange` (Forecast,
//! Flower Gift) holders are refused on the
//! field and at switch-in, which a test below pins, so these events cannot meet an
//! unimplemented handler. Every implemented handler only changes its own holder, so the Speed
//! order (and its random tie-breaks) cannot change the outcome and is not drawn.

use crate::dex::{abilities, items, ItemId};
use crate::field::Terrain;
use crate::state::SlotRef;

use super::battle::Battle;

/// The terrain a Seed reacts to (`this.field.isTerrain(...)`), if `item` is one.
pub(crate) fn seed_terrain(item: ItemId) -> Option<Terrain> {
    match item {
        i if i == items::ELECTRIC_SEED => Some(Terrain::Electric),
        i if i == items::GRASSY_SEED => Some(Terrain::Grassy),
        i if i == items::MISTY_SEED => Some(Terrain::Misty),
        i if i == items::PSYCHIC_SEED => Some(Terrain::Psychic),
        _ => None,
    }
}

/// A Seed's `onStart` / `onTerrainChange` for its holder in `slot`: if the terrain is the
/// seed's (`isTerrain`, grounded or not; no `TryTerrain` handler exists), `useItem` (Def or SpD
/// +1, consumed). `onStart`'s `!pokemon.ignoringItem()` holds: Klutz holders are refused.
pub(crate) fn seed_check<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(terrain) = seed_terrain(b.item(slot)) else {
        return;
    };
    if b.terrain() == terrain {
        super::items::use_boost_item(b, slot);
    }
}

/// Showdown `eachEvent('TerrainChange')`: per Pokémon, Quark Drive's or Mimicry's
/// `onTerrainChange` (an ability, sub-order 7) before its Seed's (an item, 8), which matters for
/// the same holder: the Seed's boost comes after Quark Drive picked its best stat.
pub(crate) fn terrain_changed<const N: usize>(b: &mut Battle<'_, N>) {
    for slot in b.all_alive() {
        if b.ability(slot) == abilities::QUARK_DRIVE {
            super::abilities::paradox_change(b, slot);
        }
        // Mimicry (an ability too; a Pokémon has one).
        super::forme::terrain_changed(b, slot);
        seed_check(b, slot);
    }
}

/// Showdown `eachEvent('WeatherChange')`: Protosynthesis's and Ice Face's `onWeatherChange`
/// (`forme::weather_changed`).
pub(crate) fn weather_changed<const N: usize>(b: &mut Battle<'_, N>) {
    for slot in b.all_alive() {
        if b.ability(slot) == abilities::PROTOSYNTHESIS {
            super::abilities::paradox_change(b, slot);
        }
        super::forme::weather_changed(b, slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::{AbilityId, ItemId, SpeciesId};
    use crate::turn::support::{ability_supported_on_field, item_supported_on_field};
    use crate::turn::switching::switch_in_supported;

    fn reacts(handlers: &[&str]) -> bool {
        handlers.iter().any(|h| {
            let event = h.strip_prefix("on").unwrap_or(h);
            let event = ["Any", "Ally", "Foe", "Source"]
                .iter()
                .find_map(|p| event.strip_prefix(p))
                .unwrap_or(event);
            event == "TerrainChange" || event == "WeatherChange"
        })
    }

    /// Every ability, item and species with a TerrainChange / WeatherChange handler is either
    /// implemented here (the Seeds) or kept off the field.
    #[test]
    fn change_handlers_are_implemented_or_refused() {
        for id in ItemId::all() {
            if reacts(id.data().handlers) && seed_terrain(id).is_none() {
                assert!(!item_supported_on_field(id), "{id:?}");
            }
        }
        for id in AbilityId::all() {
            let implemented = id == abilities::PROTOSYNTHESIS
                || id == abilities::QUARK_DRIVE
                || id == abilities::ICE_FACE
                || id == abilities::MIMICRY;
            if reacts(id.data().handlers) && !implemented {
                assert!(
                    !ability_supported_on_field(id) && !switch_in_supported(id),
                    "{id:?}"
                );
            }
        }
        for id in SpeciesId::all() {
            assert!(!reacts(id.data().handlers), "{id:?}");
        }
    }

    #[test]
    fn seeds_match_the_dex() {
        for item in [
            items::ELECTRIC_SEED,
            items::GRASSY_SEED,
            items::MISTY_SEED,
            items::PSYCHIC_SEED,
        ] {
            let data = item.data();
            assert_eq!(data.handlers, ["onStart", "onTerrainChange"], "{item:?}");
            assert!(
                data.event_orders.contains(&("onSwitchInPriority", -1)),
                "{item:?}"
            );
        }
    }
}
