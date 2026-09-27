//! Item handlers (Showdown `data/items.ts`; of these items the Champions mod only overrides
//! Eject Button's `onAfterMoveSecondary`) for the items listed in `support`.
//!
//! Each function is one Showdown event; `moves.rs`, `battle.rs`, `order.rs`, `residual.rs` and
//! `mod.rs` call it where Showdown runs that event. An item not handled here gets the event's
//! neutral result. Handlers read the effective item, [`Battle::item`]: `NONE` while Showdown's
//! `ignoringItem` holds (Magic Room, or Klutz with an item that is not `ignoreKlutz`; work plan
//! F17), when the runEvent loop skips item handlers. What reads `pokemon.item` itself (Knock
//! Off, Trick, Acrobatics, Unburden, ...) uses [`Battle::raw_item`].

use crate::damage::{MOD_DOUBLE, MOD_HALF, MOD_ONE_POINT_FIVE};
use crate::dex::{
    abilities, conditions, items, moves, species, ItemId, MoveCategory, MoveData, MoveFlags,
    MoveId, Secondary, SpeciesId, Stat, Type, TypeImmunities, NO_BOOSTS,
};
use crate::field::{FieldEffect, Weather};
use crate::instruction::Instruction;
use crate::state::{
    MoveResult, Pokemon, PokemonRef, SlotRef, State, Status, SwitchFlag, BOOST_COUNT,
};
use crate::volatile::{Volatile, VolatileState};

use super::abilities::{Handler, SUB_ABILITY, SUB_CONDITION, SUB_ITEM};
use super::battle::{Battle, BoostEffect, DamageSource};
use super::order::ORDER_DEFAULT;
use super::TurnError;

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

/// Showdown `pokemon.ignoringItem()` (WORKPLAN F17): under Magic Room, or with Klutz and an
/// item that is not `ignoreKlutz`. While it holds, item handlers do not run and `hasItem` is
/// false ([`Battle::item`] is `NONE`); `pokemon.item` itself is unaffected (Knock Off, Trick,
/// Acrobatics, Unburden, Mega Evolution: [`Battle::raw_item`]). Embargo and the Primal Orbs are
/// not implemented. Klutz counts while it acts (`hasAbility('klutz')`: not under Gastro Acid or
/// Neutralizing Gas); an `ignoreKlutz` item (Ability Shield) never asks, so the two checks do
/// not recurse. A Klutz holder with no item ignores its item too (`!getItem().ignoreKlutz` of
/// the empty item): only a `singleEvent` for an item it does not hold can tell (Bug Bite's
/// `Eat` of the stolen berry).
pub(crate) fn ignoring_item<const N: usize>(state: &State<N>, slot: SlotRef) -> bool {
    let Some(mon) = state.active(slot) else {
        return false;
    };
    // The cheap ability comparison before the item's dex entry (this runs for every item read).
    state.field[FieldEffect::MagicRoom as usize].is_active()
        || (mon.ability == abilities::KLUTZ
            && !mon.item.data().ignore_klutz
            && !super::abilities::ignoring_ability(state, slot))
}

/// Whether an item's `onStart` does nothing when its holder switches in (Showdown runs item
/// `onStart` handlers in the `SwitchIn` event): the Choice items only remove a `choicelock`
/// the newcomer cannot have yet; Air Balloon only announces itself; Utility Umbrella's returns
/// for a holder that does not ignore its item, and `fieldEvent('SwitchIn')`'s `singleEvent`
/// skips it for one that does (only `setItem`'s Start, which is exempt, reaches such a holder:
/// [`umbrella_start`]).
pub(crate) fn inert_start(item: ItemId) -> bool {
    item.data().is_choice || item == items::AIR_BALLOON || item == items::UTILITY_UMBRELLA
}

// ---- Utility Umbrella's WeatherChange ----------------------------------------------------------

/// The weather Utility Umbrella's handlers react to: `['sunnyday', 'raindance', 'desolateland',
/// 'primordialsea'].includes(this.field.effectiveWeather())` (Air Lock and Cloud Nine suppress
/// it).
fn umbrella_weather<const N: usize>(b: &Battle<'_, N>) -> bool {
    matches!(
        b.effective_weather(),
        Weather::Sun | Weather::Rain | Weather::HarshSun | Weather::HeavyRain
    )
}

/// Utility Umbrella's `onEnd` on the Pokémon in `slot`, which just lost `item` (`takeItem`, or
/// `setItem` with another item or none): `singleEvent('End')` is skipped while the Pokémon
/// ignores its item as it is now (Magic Room; Klutz, unless the new item ignores it); otherwise,
/// in sun or rain, `runEvent('WeatherChange', pokemon, pokemon, item)` on it alone
/// ([`field_events::weather_changed_at`]): Forecast and Flower Gift now see the weather. Returns
/// whether the handler ran (it then marks its item state `inactive`, which matters only for an
/// umbrella given back after `takeItem`: [`Battle::umbrella_inactive`]).
///
/// Magic Room's `onFieldStart` and Klutz's `onStart` call every held item's End too, but their
/// holders then ignore their items (Magic Room is already in `pseudoWeather`), so it never runs
/// there.
///
/// [`field_events::weather_changed_at`]: super::field_events::weather_changed_at
/// [`Battle::umbrella_inactive`]: super::battle::Battle::umbrella_inactive
pub(crate) fn umbrella_end<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    item: ItemId,
) -> bool {
    if item != items::UTILITY_UMBRELLA || ignoring_item(b.state, slot) {
        return false;
    }
    if umbrella_weather(b) {
        super::field_events::weather_changed_at(b, slot);
    }
    true
}

/// Utility Umbrella's `onStart` for its new holder in `slot` (`setItem`'s Start, which runs
/// even while the holder ignores its item): `if (!pokemon.ignoringItem()) return;` — only a
/// holder that ignores it (Klutz, Magic Room) runs WeatherChange, in sun or rain, and then sees
/// the weather. Any other holder keeps its forme until the weather changes.
pub(crate) fn umbrella_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if ignoring_item(b.state, slot) && umbrella_weather(b) {
        super::field_events::weather_changed_at(b, slot);
    }
}

/// Utility Umbrella's `onUpdate` for its holder in `slot` (an item handler: skipped while the
/// holder ignores it): `if (!this.effectState.inactive) return; this.effectState.inactive =
/// false;` then WeatherChange in sun or rain, where the holder, under its umbrella again, loses
/// the weather's forme. Its state is `inactive` only after its End ran in a `takeItem` and the
/// umbrella came back silently (a failed Trick, a thief that cannot hold it).
pub(crate) fn umbrella_update<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(pokemon) = b.occupant(slot) else {
        return;
    };
    let marked = b.umbrella_inactive.len();
    b.umbrella_inactive.retain(|&p| p != pokemon);
    if b.umbrella_inactive.len() != marked && umbrella_weather(b) {
        super::field_events::weather_changed_at(b, slot);
    }
}

/// `setItem` gives `pokemon` a new item state: an `inactive` mark on the old one is gone.
pub(crate) fn fresh_item_state<const N: usize>(b: &mut Battle<'_, N>, pokemon: PokemonRef) {
    b.umbrella_inactive.retain(|&p| p != pokemon);
}

impl<const N: usize> Battle<'_, N> {
    /// Showdown `pokemon.effectiveWeather()` for the Pokémon in `slot`: the field's
    /// [`Battle::effective_weather`], except that Utility Umbrella hides sun and rain (and
    /// their primal forms) from its holder. Sandstorm and snow are unaffected. Every
    /// per-Pokémon weather effect reads this — Chlorophyll / Swift Swim, Solar Power, Rain Dish,
    /// Dry Skin, Hydration, Leaf Guard — except where the reading effect is a move or a weather
    /// (the damage modifier and sand / snow defense, sun's freeze immunity, the move handlers:
    /// Weather Ball, Thunder, Hurricane, Morning Sun...), which read [`Battle::move_weather`]
    /// (Mega Sol); effects that read the field (`field.isWeather`: Sand Rush, Slush Rush,
    /// Blizzard, Aurora Veil, Shore Up, sandstorm damage) read [`Battle::effective_weather`].
    pub fn weather_for(&self, slot: SlotRef) -> Weather {
        let weather = self.effective_weather();
        let hidden = matches!(
            weather,
            Weather::Sun | Weather::Rain | Weather::HarshSun | Weather::HeavyRain
        ) && self.item(slot) == items::UTILITY_UMBRELLA;
        if hidden {
            Weather::None
        } else {
            weather
        }
    }

    /// Showdown `holder.effectiveWeather()` when the reading effect (`this.battle.effect`) is a
    /// move or a weather: `if (this.battle.activePokemon?.hasAbility('megasol') && ...) return
    /// 'sunnyday';` — while a Pokémon with Mega Sol (as it acts: not suppressed, still active)
    /// is using a move, everyone's weather is sun for it, whatever the field's (even none or a
    /// suppressed one) and before Utility Umbrella's check. Electro Shot's own charge check
    /// (`sourceEffect.id !== 'electroshot'`) reads [`Battle::weather_for`] instead.
    ///
    /// Showdown's `activePokemon` lasts until `runAction`'s `clearActiveMove()`, after the
    /// action's phazing step, and after a Dancer copy it is the last dancer; so does the
    /// engine's `active_move` (Opus DD unit B26: `drag_outs` clears it). Nothing reads the
    /// weather through Mega Sol in that window (Opus BB unit B25): only a Move's or a Weather's
    /// handler does, and a dragged-in Pokémon's switch-in handlers belong to its ability, item
    /// and side conditions (oracle `bb-mega-sol-roar-forecast`: Forecast's `onStart` sees the
    /// rain).
    pub fn move_weather(&self, holder: SlotRef) -> Weather {
        let mega_sol = self.active_move.is_some_and(|m| {
            self.occupant(m.user) == Some(m.pokemon) && self.ability(m.user) == abilities::MEGA_SOL
        });
        if mega_sol {
            Weather::Sun
        } else {
            self.weather_for(holder)
        }
    }

    /// Showdown `pokemon.getWeight()`: `runEvent('ModifyWeight', pokemon, null, null,
    /// pokemon.weighthg)`, then at least 1 hg. `weighthg` is the current forme's weight
    /// (`setSpecies`), less Autotomize's reductions since then ([`Pokemon::weight_hg`]).
    /// Handlers by priority: Heavy Metal (1) doubles it; then, at priority 0, Light Metal
    /// (ability) and Float Stone (item) each halve it with truncation. Heavy Metal and Light
    /// Metal are breakable: a move that ignores abilities skips the target's, not its user's
    /// own ([`Battle::ability_unless_broken`]); a suppressed Float Stone does nothing. Read by
    /// Low Kick, Grass Knot, Heavy Slam and Heat Crash.
    pub(crate) fn weight(&self, slot: SlotRef) -> i32 {
        let Some(mon) = self.slot_mon(slot) else {
            return 1;
        };
        let mut weight = mon.weight_hg();
        let ability = self.ability_unless_broken(slot);
        if ability == abilities::HEAVY_METAL {
            weight *= 2;
        }
        if ability == abilities::LIGHT_METAL {
            weight /= 2;
        }
        if self.item(slot) == items::FLOAT_STONE {
            weight /= 2;
        }
        weight.max(1)
    }

    /// `setSpecies`'s `this.weighthg = species.weighthg` for `pokemon`: Autotomize's reductions
    /// end (a forme change, leaving the field).
    pub(crate) fn reset_autotomize(&mut self, pokemon: PokemonRef) {
        let old = self.mon(pokemon).autotomized;
        if old != 0 {
            self.apply(Instruction::SetAutotomized {
                target: pokemon,
                old,
                new: 0,
            });
        }
    }
}

/// Whether `handler`, one of the item's handlers that can fire around a switch-in, is
/// implemented: an inert `onStart` ([`inert_start`]); the `onStart` of an item
/// [`switch_in_priority`] schedules (Seeds, Room Service); a Seed's `onTerrainChange`
/// (`field_events`); White Herb's and Mirror Herb's `onAnySwitchIn` ([`any_switch_in_priority`];
/// White Herb's `onStart` only runs from its own handlers, as `onAnySwitchIn` replaces it as the
/// switch-in callback, or from `setItem`'s Start: [`white_herb_start`]).
pub(crate) fn start_handler_implemented(item: ItemId, handler: &str) -> bool {
    match handler {
        "onStart" => {
            inert_start(item) || switch_in_priority(item).is_some() || item == items::WHITE_HERB
        }
        "onTerrainChange" => super::field_events::seed_terrain(item).is_some(),
        // White Herb, Mirror Herb, Eject Pack.
        "onAnySwitchIn" => any_switch_in_priority(item).is_some(),
        // Ability Shield: the only `setAbility` of a switch-in is the holder's own Trace, which
        // does not seek with an effective shield (`switching::trace`) and, under Magic Room,
        // sets the ability past the skipped item (a later copy of a Trace still seeking asks
        // the shield: `switching::trace_copy`); a forme change skips the event.
        "onSetAbility" => item == items::ABILITY_SHIELD,
        _ => false,
    }
}

/// `onSwitchInPriority` of an item whose `onStart` acts when its holder switches in (it runs
/// in the batched `fieldEvent('SwitchIn')`, after the abilities' priority-0 handlers): the
/// Seeds and Room Service (-1), Booster Energy, Metronome (0).
pub(crate) fn switch_in_priority(item: ItemId) -> Option<i32> {
    let acts = super::field_events::seed_terrain(item).is_some()
        || item == items::ROOM_SERVICE
        || item == items::BOOSTER_ENERGY
        || item == items::METRONOME;
    acts.then(|| super::abilities::priority(item.data().event_orders, "onSwitchInPriority"))
}

/// `onAnySwitchInPriority` of an item whose `onAnySwitchIn` runs for every switch-in batch,
/// held by any active Pokémon: White Herb (-2), Mirror Herb (-3), Eject Pack (-4).
pub(crate) fn any_switch_in_priority(item: ItemId) -> Option<i32> {
    [items::WHITE_HERB, items::MIRROR_HERB, items::EJECT_PACK]
        .contains(&item)
        .then(|| super::abilities::priority(item.data().event_orders, "onAnySwitchInPriority"))
}

/// The switch-in handler [`switch_in_priority`] or [`any_switch_in_priority`] scheduled for the
/// holder in `slot`, run with the item it held when the handlers were collected (Showdown calls
/// the collected callback: a consumed item makes its `useItem` fail).
/// - Seeds: [`super::field_events::seed_check`].
/// - Room Service `onStart`: `this.field.getPseudoWeather('trickroom')` uses it (Speed -1).
/// - White Herb / Mirror Herb `onAnySwitchIn`: [`white_herb`] / [`mirror_herb_use`] (the
///   event's target is the holder: `singleEvent('SwitchIn', ..., effectHolder)`).
/// - Eject Pack `onAnySwitchIn`: [`eject_pack_use`] (an Intimidate or Sticky Web earlier in the
///   same batch set its flag).
/// - Metronome `onStart`: [`metronome_start`].
pub(crate) fn switch_in_item<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, item: ItemId) {
    if b.item(slot) != item {
        return;
    }
    match item {
        i if super::field_events::seed_terrain(i).is_some() => {
            super::field_events::seed_check(b, slot);
        }
        i if i == items::ROOM_SERVICE => {
            if b.field_active(FieldEffect::TrickRoom) {
                use_boost_item(b, slot);
            }
        }
        i if i == items::WHITE_HERB => white_herb(b, slot),
        i if i == items::MIRROR_HERB => mirror_herb_use(b, slot, slot),
        i if i == items::EJECT_PACK => eject_pack_use(b, slot),
        i if i == items::METRONOME => metronome_start(b, slot),
        // Booster Energy's `onStart`: `started = true`, then its `onUpdate`.
        i if i == items::BOOSTER_ENERGY => super::abilities::booster_energy(b, slot),
        _ => {}
    }
}

// ---- stage changes: White Herb, Mirror Herb, Adrenaline Orb, Room Service ----------------------

/// White Herb's `onStart`, which its `onAnySwitchIn`, `onAnyAfterMega`, `onAnyAfterMove` and
/// `onResidual` all call for the holder in `slot`: if a stage is negative, `useItem` (no
/// `boosts`; `onUse` sets every negative stage to 0 with `setBoost`, which runs no boost
/// event), consumed.
pub(crate) fn white_herb<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.item(slot) != items::WHITE_HERB {
        return;
    }
    white_herb_start(b, slot);
}

/// White Herb's `onStart` itself for the holder in `slot`, which `setItem`'s `Start` runs even
/// while the holder ignores its item (Klutz, Magic Room): with a negative stage the herb is used
/// (`useItem`), and its `onUse` (the reset) is a `singleEvent('Use')`, which the suppression
/// skips; the negative stages then stay.
pub(crate) fn white_herb_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.raw_item(slot) != items::WHITE_HERB || b.alive(slot).is_none() {
        return;
    }
    let boosts = b.state.slot(slot).boosts;
    if !boosts.iter().any(|&stage| stage < 0) {
        return;
    }
    if !ignoring_item(b.state, slot) {
        for (stat, &stage) in boosts.iter().enumerate() {
            if stage < 0 {
                b.apply(Instruction::Boost {
                    target: slot,
                    stat: stat as u8,
                    amount: -stage,
                });
            }
        }
    }
    b.use_item(slot);
}

/// Mirror Herb's `onAnySwitchIn` / `onAnyAfterMega` / `onAnyAfterMove` / `onResidual` for the
/// holder in `slot`: once `ready` (it copied a raise), `useItem` (no `boosts`; `onUse`:
/// `this.boost(this.effectState.boosts, pokemon)`, whose source is the event's target,
/// `event_target`), consumed. The copied raises are forgotten when the item goes (`onEnd`).
pub(crate) fn mirror_herb_use<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    event_target: SlotRef,
) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let Some(index) = b.mirror_herb.iter().position(|(p, _)| *p == pokemon) else {
        return;
    };
    let (_, boosts) = b.mirror_herb.remove(index);
    if b.item(slot) != items::MIRROR_HERB {
        return;
    }
    b.boost_by(
        slot,
        &boosts,
        Some(event_target),
        BoostEffect::Item(items::MIRROR_HERB),
    );
    b.use_item(slot);
}

/// The item `AfterBoost` handlers after `target` got `boost` (after TryBoost) from `effect`
/// (Showdown `runEvent('AfterBoost', target, source, effect, boost)`; they run after the
/// target's ability, Rattled):
/// - Adrenaline Orb (the target's): `if (target.boosts['spe'] === 6 || boost.atk === 0)
///   return; if (effect.name === 'Intimidate') target.useItem();` (Speed +1). An Attack stage
///   capped to 0 stops it (`atk_capped_to_zero`); one a TryBoost handler deleted does not.
/// - Mirror Herb (`onFoeAfterBoost`, the target's active foes'): unless the effect is
///   Opportunist or Mirror Herb, every positive stage is added to the holder's copied raises
///   ([`Battle::mirror_herb`]), used at the next trigger ([`mirror_herb_use`]).
/// - Eject Pack (the target's): `if (this.effectState.eject || this.activeMove?.id ===
///   'partingshot') return;` then any negative stage sets `effectState.eject`
///   ([`Volatile::EjectPack`]), which its triggers act on ([`eject_pack_use`]). A stage capped
///   to 0 (already at -6) or deleted by a TryBoost handler does not count.
///
/// Adrenaline Orb's own boost, Mirror Herb's accumulation and Eject Pack's flag commute, so
/// their Speed order is moot.
pub(crate) fn after_boost<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    boost: &[i8; BOOST_COUNT],
    effect: BoostEffect,
    atk_capped_to_zero: bool,
) {
    let raised = boost.iter().any(|&stage| stage > 0);
    let copied = effect != BoostEffect::Item(items::MIRROR_HERB)
        && effect != BoostEffect::Ability(abilities::OPPORTUNIST);
    if raised && copied {
        for foe in b.alive_slots(target.side.other()) {
            if b.item(foe) != items::MIRROR_HERB {
                continue;
            }
            let pokemon = b.alive(foe).expect("alive");
            let index = match b.mirror_herb.iter().position(|(p, _)| *p == pokemon) {
                Some(i) => i,
                None => {
                    b.mirror_herb.push((pokemon, NO_BOOSTS));
                    b.mirror_herb.len() - 1
                }
            };
            for (total, &stage) in b.mirror_herb[index].1.iter_mut().zip(boost) {
                if stage > 0 {
                    *total += stage;
                }
            }
        }
    }
    if effect == BoostEffect::Ability(abilities::INTIMIDATE)
        && b.item(target) == items::ADRENALINE_ORB
        && b.alive(target).is_some()
        && b.state.slot(target).boosts[4] != 6
        && !atk_capped_to_zero
    {
        use_boost_item(b, target);
    }
    if b.item(target) == items::EJECT_PACK
        && b.alive(target).is_some()
        && boost.iter().any(|&stage| stage < 0)
        && !b.active_move.is_some_and(|m| m.id == moves::PARTING_SHOT)
    {
        b.set_volatile_state(
            target,
            Volatile::EjectPack,
            VolatileState {
                active: true,
                ..VolatileState::NONE
            },
        );
    }
}

/// `runEvent('AfterMove', user)` for the items' `onAnyAfterMove` (White Herb, Mirror Herb,
/// Eject Pack), held by any active Pokémon. Showdown collects `onAny` handlers only while the
/// user is still active (not yet processed as fainted); every one of them acts on its own
/// holder, and only Eject Packs depend on each other ([`eject_packs`]).
pub(crate) fn any_after_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) {
    for slot in b.all_alive() {
        white_herb(b, slot);
        mirror_herb_use(b, slot, user);
    }
    eject_packs(b);
}

/// `runEvent('AfterMega', pokemon)` for the items' `onAnyAfterMega` (White Herb, Mirror Herb,
/// Eject Pack: an Intimidate the new forme brings).
pub(crate) fn any_after_mega<const N: usize>(b: &mut Battle<'_, N>, pokemon: SlotRef) {
    for slot in b.all_alive() {
        white_herb(b, slot);
        mirror_herb_use(b, slot, pokemon);
    }
    eject_packs(b);
}

// ---- Eject Pack -----------------------------------------------------------------------------

/// Whether the Pokémon in `slot` holds an Eject Pack whose `eject` flag is set (a stat of it was
/// lowered: [`after_boost`]) and whose handlers run (not suppressed).
fn eject_pending<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    b.item(slot) == items::EJECT_PACK && b.volatile(slot, Volatile::EjectPack).active
}

/// The Eject Packs' `onAnyAfterMove` / `onAnyAfterMega` handlers of every active holder, in
/// Showdown's handler order (`pokemon.speed`, [`Battle::event_speed`]; equal Speeds shuffled).
/// Only holders with the flag act, and the first that switches wins: the next one's `onUseItem`
/// sees its `switchFlag === true` and fails, keeping its flag for a later trigger (the
/// switch-in of the first one's replacement).
fn eject_packs<const N: usize>(b: &mut Battle<'_, N>) {
    let pending: Vec<SlotRef> = b
        .all_alive()
        .into_iter()
        .filter(|&s| eject_pending(b, s))
        .collect();
    if pending.is_empty() {
        return;
    }
    for slot in super::abilities::speed_sorted(b, pending, |_, _| true) {
        eject_pack_use(b, slot);
    }
}

/// Eject Pack's trigger (`onAnySwitchIn`, `onAnyAfterMega`, `onAnyAfterMove`, `onResidual`) for
/// the holder in `slot`: `if (!this.effectState.eject) return; target.useItem();`. `useItem`
/// needs a holder with HP, and the pack's `onUseItem` refuses while its side cannot switch
/// (`canSwitch`: no healthy bench member), while the holder commands or is commanded
/// (Commander), or while any active Pokémon has `switchFlag === true` (an Eject Button's,
/// Emergency Exit's or another Eject Pack's); the flag then stays. Otherwise `onUse` sets the
/// holder's `switchFlag = true` and the pack is used up (its flag goes with the item state,
/// `Battle::use_item`). The Showdown version here has no once-per-turn limit beyond that.
pub(crate) fn eject_pack_use<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if !eject_pending(b, slot) || b.alive(slot).is_none() {
        return;
    }
    // `getAllActive()` also holds a Pokémon at 0 HP whose faint is not processed: a user its
    // own recoil knocked out after Emergency Exit flagged it keeps the pack at AfterMove (oracle
    // `dd-emergency-exit-recoil-eject-pack`).
    if super::residual::bench(b, slot.side).next().is_none()
        || b.volatile(slot, Volatile::Commanding).active
        || b.volatile(slot, Volatile::Commanded).active
        || b.any_active_switch_flag_true()
    {
        return;
    }
    b.set_switch_flag(slot, SwitchFlag::Effect);
    b.use_item(slot);
}

// ---- Metronome ------------------------------------------------------------------------------

/// The `metronome` condition's `onModifyDamage` factors by `min(numConsecutive, 5)`.
const METRONOME_MODIFIERS: [u32; 6] = [4096, 4915, 5734, 6553, 7372, 8192];

/// Metronome's `onStart` (its holder switches in, or gets the item: `setItem`'s Start):
/// `pokemon.addVolatile('metronome')`, whose own `onStart` sets `lastMove = ''` and
/// `numConsecutive = 0`; nothing if the condition is there already (no `onRestart`).
pub(crate) fn metronome_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if b.alive(slot).is_none() || b.volatile(slot, Volatile::Metronome).active {
        return;
    }
    b.set_volatile_state(
        slot,
        Volatile::Metronome,
        VolatileState {
            active: true,
            ..VolatileState::NONE
        },
    );
}

/// The `metronome` condition's `onTryMove` (priority -2, after every implemented TryMove
/// handler: only for a move none of them stopped; a charging turn stops before it) for the
/// user in `user` of the move `id`:
/// - without the item (`hasItem`: a suppressed one does not count) the condition goes;
/// - a move that calls another (`callsMove`: Sleep Talk) changes nothing; the called move
///   counts when it runs;
/// - the same move as `lastMove` after a successful move last turn (`moveLastTurnResult`) adds
///   one; otherwise, while `twoturnmove` is up (the attacking turn of a charging move), a new
///   move starts at 1 and the same move adds one; otherwise the count is 0;
/// - the move becomes `lastMove`.
pub(crate) fn metronome_try_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, id: MoveId) {
    let state = b.volatile(user, Volatile::Metronome);
    if !state.active {
        return;
    }
    if b.item(user) != items::METRONOME {
        b.remove_volatile(user, Volatile::Metronome);
        return;
    }
    if id.data().calls_move {
        return;
    }
    let succeeded = b.slot_history(user).move_last_turn_result == MoveResult::Succeeded;
    let count = if state.mv == id && succeeded {
        state.counter + 1
    } else if b.volatile(user, Volatile::TwoTurnMove).active {
        if state.mv != id {
            1
        } else {
            state.counter + 1
        }
    } else {
        0
    };
    // Only `min(numConsecutive, 5)` is read: higher counts behave alike and merge.
    b.set_volatile_state(
        user,
        Volatile::Metronome,
        VolatileState {
            counter: count.min(5),
            mv: id,
            ..state
        },
    );
}

/// `runEvent('PseudoWeatherChange')` after a new pseudo-weather starts (`addPseudoWeather`, not
/// a restart): Room Service's `onAnyPseudoWeatherChange` on every active holder uses the item
/// (`pokemon.useItem(pokemon)`, Speed -1) while Trick Room is up, whichever pseudo-weather
/// started.
pub(crate) fn pseudo_weather_change<const N: usize>(b: &mut Battle<'_, N>) {
    if !b.field_active(FieldEffect::TrickRoom) {
        return;
    }
    for slot in b.all_alive() {
        if b.item(slot) == items::ROOM_SERVICE {
            use_boost_item(b, slot);
        }
    }
}

/// The end of a stage: Mirror Herb's copied raises live on the item across events in
/// Showdown (until its next trigger, possibly turns later); the engine does not carry them
/// past a stage, so a stage that ends with a living holder still `ready` is refused.
pub(crate) fn stage_end_check<const N: usize>(b: &Battle<'_, N>) -> Result<(), TurnError> {
    // Utility Umbrella's `inactive` mark waits for the holder's next `onUpdate`, which the
    // engine only runs within the stage (the Update after the action always comes first).
    for &pokemon in &b.umbrella_inactive {
        let mon = b.mon(pokemon);
        if mon.hp > 0 && mon.item == items::UTILITY_UMBRELLA {
            return Err(b.unsupported(format!(
                "{}: Utility Umbrella's `inactive` item state past the end of a stage (its \
                 onUpdate has not run)",
                mon.species.data().name
            )));
        }
    }
    for &(pokemon, _) in &b.mirror_herb {
        let mon = b.mon(pokemon);
        if mon.hp > 0 && mon.item == items::MIRROR_HERB {
            return Err(b.unsupported(format!(
                "{}: Mirror Herb keeps copied boosts past the end of a stage (its effectState \
                 persists until the next trigger)",
                mon.species.data().name
            )));
        }
    }
    Ok(())
}

// ---- Speed, grounding, effectiveness, action order --------------------------------------------

/// `ModifySpe` factor of `holder`'s effective `item` ([`Battle::item`]: none under Magic Room,
/// and under Klutz unless the item is `ignoreKlutz`): Choice Scarf `chainModify(1.5)` (skipped
/// while Dynamaxed, which `support` refuses); Iron Ball, Macho Brace and the six Power items
/// `chainModify(0.5)` (Macho Brace and the Power items ignore Klutz); Quick Powder
/// `chainModify(2)` for an untransformed Ditto (`pokemon.species.name === 'Ditto' &&
/// !pokemon.transformed`).
pub(crate) fn speed_modifier(item: ItemId, holder: &Pokemon) -> Option<u32> {
    const HALVING: [ItemId; 8] = [
        items::IRON_BALL,
        items::MACHO_BRACE,
        items::POWER_ANKLET,
        items::POWER_BAND,
        items::POWER_BELT,
        items::POWER_BRACER,
        items::POWER_LENS,
        items::POWER_WEIGHT,
    ];
    match item {
        i if i == items::CHOICE_SCARF => Some(MOD_ONE_POINT_FIVE),
        i if HALVING.contains(&i) => Some(MOD_HALF),
        i if i == items::QUICK_POWDER
            && holder.species == species::DITTO
            && holder.transformed.is_none() =>
        {
            Some(MOD_DOUBLE)
        }
        _ => None,
    }
}

/// `isGrounded`: Iron Ball grounds its holder, checked before the Flying type.
pub(crate) fn grounds(item: ItemId) -> bool {
    item == items::IRON_BALL
}

/// `isGrounded`: Air Balloon lifts its holder, checked last (after Levitate).
pub(crate) fn lifts(item: ItemId) -> bool {
    item == items::AIR_BALLOON
}

/// The target's item `onEffectiveness` for one of its types (`runEffectiveness`, after the
/// move's own handler), given the type's effectiveness so far: Iron Ball returns 0 for a
/// Ground move against every type of a Flying holder, unless Gravity is up (Ingrain and Smack
/// Down are not implemented).
pub(crate) fn on_effectiveness<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    move_type: Type,
    type_mod: i32,
) -> i32 {
    let grounded_flyer = b.item(target) == items::IRON_BALL
        && !b.field_active(FieldEffect::Gravity)
        && move_type == Type::Ground
        && b.has_type(target, Type::Flying);
    if grounded_flyer {
        0
    } else {
        type_mod
    }
}

/// The constant `onFractionalPriority` of an item, in tenths: Lagging Tail and Full Incense
/// `-0.1` (the dex's value; 0 for any other item).
pub(crate) fn constant_fractional_tenths(item: ItemId) -> i8 {
    if item == items::LAGGING_TAIL || item == items::FULL_INCENSE {
        item.data().fractional_priority_tenths
    } else {
        0
    }
}

/// The deterministic handlers of `runEvent('FractionalPriority')` for a move action of `id`, in
/// tenths: the constants (priority 0) — the ability's (Stall, sub-order 7) and then the item's
/// (sub-order 8) replace the value, so the item's wins — then Mycelium Might (priority -1: `if
/// (move.category === 'Status') return -0.1;`). The random handlers run after them when the
/// actions are queued: Quick Draw (-1, `abilities::quick_draw`), Quick Claw and Custap Berry
/// (-2, [`quick_claw`], [`custap`]).
pub(crate) fn fractional_priority_tenths<const N: usize>(
    state: &State<N>,
    slot: SlotRef,
    id: MoveId,
) -> i8 {
    let Some(mon) = state.active(slot) else {
        return 0;
    };
    let item = if ignoring_item(state, slot) {
        ItemId::NONE
    } else {
        mon.item
    };
    let ability = super::abilities::effective_ability(state, slot);
    if ability == abilities::MYCELIUM_MIGHT && action_category(id) == MoveCategory::Status {
        return -1;
    }
    match constant_fractional_tenths(item) {
        0 => super::order::fractional_priority_tenths(ability),
        item => item,
    }
}

/// The category of an action's move (`action.move.category`: the dex's; the `recharge`
/// pseudo-move, `MoveId::NONE` in an action, is a status move).
pub(crate) fn action_category(id: MoveId) -> MoveCategory {
    if id.is_none() {
        MoveCategory::Status
    } else {
        id.data().category
    }
}

/// Mycelium Might's exception in Quick Claw's and Custap Berry's `onFractionalPriority`: `if
/// (move.category === 'Status' && pokemon.hasAbility('myceliummight')) return;`.
fn mycelium_status<const N: usize>(b: &Battle<'_, N>, slot: SlotRef, id: MoveId) -> bool {
    action_category(id) == MoveCategory::Status && b.ability(slot) == abilities::MYCELIUM_MIGHT
}

/// Quick Claw's `onFractionalPriority` (priority -2, after the constants and Quick Draw) for a
/// move action of `pokemon` using `id` whose fractional priority is `current` tenths:
/// `priority <= 0 && this.randomChance(1, 5)` makes it +0.1 (not for a Mycelium Might holder's
/// status move). Showdown draws it when the turn's actions are queued (`resolveAction`).
pub(crate) fn quick_claw<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    pokemon: PokemonRef,
    current: i8,
    id: MoveId,
) -> Option<i8> {
    (b.occupant(slot) == Some(pokemon)
        && b.item(slot) == items::QUICK_CLAW
        && !mycelium_status(b, slot, id)
        && current <= 0
        && b.rng.chance(1, 5))
    .then_some(1)
}

/// Custap Berry's `onFractionalPriority` (priority -2, like Quick Claw, which a holder of it
/// cannot also hold) for a move action of the Pokémon in `slot` using `id` whose fractional
/// priority is `current` tenths: `priority <= 0` and the holder at 1/4 of its max HP or less
/// (1/2 with Gluttony) eats the berry (`eatItem`, empty `onEat`) and makes it +0.1 (not for a
/// Mycelium Might holder's status move). Showdown runs it when the turn's actions are queued
/// (`resolveAction`), so the berry is gone before the first action.
pub(crate) fn custap<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    pokemon: PokemonRef,
    current: i8,
    id: MoveId,
) -> Option<i8> {
    if b.occupant(slot) != Some(pokemon)
        || b.item(slot) != items::CUSTAP_BERRY
        || mycelium_status(b, slot, id)
    {
        return None;
    }
    let mon = b.mon(pokemon);
    let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
    // `abilityState.gluttony` is set on switch-in: always set here (see `update.rs`).
    let pinch = 4 * hp <= max_hp || (2 * hp <= max_hp && b.ability(slot) == abilities::GLUTTONY);
    (current <= 0 && pinch && super::update::eat_item(b, slot)).then_some(1)
}

// ---- Choice items ---------------------------------------------------------------------------

/// `ModifyAtk` (physical moves) or `ModifySpA` (special moves) handlers of the user's item, all
/// at priority 1:
/// - Choice Band / Choice Specs `chainModify(1.5)` (not while Dynamaxed);
/// - Light Ball (both events) for any Pikachu, Thick Club (Attack) for Cubone and Marowak
///   (`pokemon.baseSpecies.baseSpecies`), Deep Sea Tooth (Special Attack) for Clamperl:
///   `chainModify(2)`.
pub(crate) fn attack_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let item = b.item(user);
    let physical = data.category == MoveCategory::Physical;
    let (boosted, event) = if physical {
        (items::CHOICE_BAND, "onModifyAtkPriority")
    } else {
        (items::CHOICE_SPECS, "onModifySpAPriority")
    };
    // `pokemon.baseSpecies`: a transformed holder's own species.
    let base = b
        .slot_mon(user)
        .map(|m| base_species(m.untransformed_species()));
    let doubled = match item {
        i if i == items::LIGHT_BALL => base == Some(species::PIKACHU),
        i if i == items::THICK_CLUB => {
            physical && (base == Some(species::CUBONE) || base == Some(species::MAROWAK))
        }
        i if i == items::DEEP_SEA_TOOTH => !physical && base == Some(species::CLAMPERL),
        _ => false,
    };
    let modifier = if item == boosted {
        Some(MOD_ONE_POINT_FIVE)
    } else {
        doubled.then_some(MOD_DOUBLE)
    };
    if let Some(modifier) = modifier {
        let p = super::abilities::priority(item.data().event_orders, event);
        out.push(Handler::of(b, user, p, SUB_ITEM, modifier));
    }
    out
}

/// The base species' species (Showdown `species.baseSpecies`, the species itself for a base
/// forme).
fn base_species(species: SpeciesId) -> SpeciesId {
    let base = species.data().base_species;
    if base.is_none() {
        species
    } else {
        base
    }
}

/// `BasePower` handlers of the user's item other than the type-boosting ones: Muscle Band
/// (physical) and Wise Glasses (special) `[4505, 4096]` at priority 16; Punching Glove
/// `[4506, 4096]` for punching moves at priority 23.
pub(crate) fn base_power_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
) -> Vec<Handler> {
    let item = b.item(user);
    let modifier = match item {
        i if i == items::MUSCLE_BAND => (data.category == MoveCategory::Physical).then_some(4505),
        i if i == items::WISE_GLASSES => (data.category == MoveCategory::Special).then_some(4505),
        i if i == items::PUNCHING_GLOVE => data.flags.contains(MoveFlags::PUNCH).then_some(4506),
        _ => None,
    };
    modifier
        .map(|modifier| {
            let p = super::abilities::priority(item.data().event_orders, "onBasePowerPriority");
            Handler::of(b, user, p, SUB_ITEM, modifier)
        })
        .into_iter()
        .collect()
}

/// Whether the move being used has the `contact` flag after ModifyMove: Punching Glove's
/// `onModifyMove` (priority 1) deletes it from a punching move, the user's Long Reach from every
/// move. Every reader of
/// `move.flags['contact']` (and `checkMoveMakesContact`, which adds the Protective Pads check)
/// goes through this. The glove's holder is the user, whose item cannot change during its own
/// move (a supported effect that takes or gives items needs an empty-handed user).
pub(crate) fn makes_contact<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
) -> bool {
    data.flags.contains(MoveFlags::CONTACT)
        && !(data.flags.contains(MoveFlags::PUNCH) && b.item(user) == items::PUNCHING_GLOVE)
        // Long Reach's `onModifyMove`: `delete move.flags['contact']`.
        && b.ability(user) != abilities::LONG_REACH
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
    let item = b.item(target);
    let event = match defense_stat {
        Stat::Def => "onModifyDefPriority",
        _ => "onModifySpDPriority",
    };
    let modifier = match item {
        i if i == items::ASSAULT_VEST => (defense_stat == Stat::Spd).then_some(MOD_ONE_POINT_FIVE),
        // Eviolite: `pokemon.baseSpecies.nfe` (a transformed holder's own species).
        i if i == items::EVIOLITE => mon
            .untransformed_species()
            .data()
            .nfe
            .then_some(MOD_ONE_POINT_FIVE),
        // Deep Sea Scale: `onModifySpD` for Clamperl (`pokemon.baseSpecies.name`); Metal
        // Powder: `onModifyDef` for Ditto (`pokemon.species.name === 'Ditto' &&
        // !pokemon.transformed`). Both 2x at priority 2.
        i if i == items::DEEP_SEA_SCALE => (defense_stat == Stat::Spd
            && base_species(mon.untransformed_species()) == species::CLAMPERL)
            .then_some(MOD_DOUBLE),
        i if i == items::METAL_POWDER => (defense_stat == Stat::Def
            && mon.species == species::DITTO
            && mon.transformed.is_none())
        .then_some(MOD_DOUBLE),
        _ => None,
    };
    if let Some(modifier) = modifier {
        let p = super::abilities::priority(item.data().event_orders, event);
        out.push(Handler::of(b, target, p, SUB_ITEM, modifier));
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
                ..VolatileState::NONE
            },
        );
    }
}

/// `choicelock`'s `onBeforeMove` (priority 0, after paralysis): the lock ends once the item is
/// no longer a Choice item; otherwise another move fails (no PP, no `lastMove`), except Struggle
/// (`move.id !== 'struggle'`: a locked Pokémon whose locked move is disabled too — Gigaton
/// Hammer, Taunt, Disable — Struggles). `false` = the move is not used. The engine only lets a
/// locked Pokémon choose its move ([`disabled_move`]), so the failure needs a lock set later in
/// the turn.
pub(crate) fn before_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> bool {
    let lock = b.volatile(user, Volatile::ChoiceLock);
    if !lock.active {
        return true;
    }
    // `pokemon.getItem().isChoice`: the raw item (a suppressed Choice item keeps the lock).
    if !b.raw_item(user).data().is_choice {
        b.remove_volatile(user, Volatile::ChoiceLock);
        return true;
    }
    // `!pokemon.ignoringItem() && ... && move.id !== this.effectState.move && move.id !==
    // 'struggle'`: a holder ignoring its item (Klutz, Magic Room) keeps the lock but is not held
    // to it.
    ignoring_item(b.state, user) || id.0 == lock.counter || id == moves::STRUGGLE
}

/// The item `DisableMove` handlers `endTurn` runs for every active Pokémon: `choicelock`'s
/// `onDisableMove` removes the lock once the item is no longer a Choice item or the holder no
/// longer has the locked move (`!pokemon.hasMove(this.effectState.move)`: a lock a Dancer copy
/// started on a move the dancer does not know); the disabling itself is [`disabled_move`], read
/// from the state when choices are checked.
pub(crate) fn end_turn_disable_move<const N: usize>(b: &mut Battle<'_, N>) {
    for slot in State::<N>::slot_refs() {
        let Some(pokemon) = b.alive(slot) else {
            continue;
        };
        let lock = b.volatile(slot, Volatile::ChoiceLock);
        if !lock.active {
            continue;
        }
        let has_move = b.mon(pokemon).moves.iter().any(|m| m.id.0 == lock.counter);
        if !b.raw_item(slot).data().is_choice || !has_move {
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
    let ignoring = ignoring_item(state, slot);
    if !ignoring
        && mon.item == items::ASSAULT_VEST
        && id.data().category == MoveCategory::Status
        && id != moves::ME_FIRST
    {
        return Some(format!(
            "{} cannot use status moves with Assault Vest",
            mon.species.data().name
        ));
    }
    // `choicelock.onDisableMove`: `getItem().isChoice` (raw) keeps the lock; the disabling
    // itself needs `!pokemon.ignoringItem()`.
    let lock = state.slot(slot).volatiles.get(Volatile::ChoiceLock);
    if lock.active && mon.item.data().is_choice && !ignoring && id.0 != lock.counter {
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
/// (`!this.queue.willMove(target)`); and of the target's item (`onModifyAccuracy`).
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
            let moves_later = b.will_move(target).is_some();
            (!moves_later).then_some(4915)
        }
        _ => None,
    };
    if let Some(modifier) = modifier {
        let p =
            super::abilities::priority(item.data().event_orders, "onSourceModifyAccuracyPriority");
        out.push(Handler::of(b, user, p, SUB_ITEM, modifier));
    }
    // The target's item (`onModifyAccuracy`, priority -2): Bright Powder and Lax Incense
    // 3686/4096.
    let held = b.item(target);
    if held == items::BRIGHT_POWDER || held == items::LAX_INCENSE {
        let p = super::abilities::priority(held.data().event_orders, "onModifyAccuracyPriority");
        out.push(Handler::of(b, target, p, SUB_ITEM, 3686));
    }
    out
}

/// Blunder Policy (no handler: `hitStepAccuracy` itself): after the move missed a target, unless
/// it is an OHKO move, `pokemon.hasItem('blunderpolicy') && pokemon.useItem()` and then
/// `this.battle.boost({spe: 2}, pokemon)`, outside any event (no source).
pub(crate) fn blunder_policy<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) {
    if b.item(user) == items::BLUNDER_POLICY && b.use_item(user) {
        let mut up = NO_BOOSTS;
        up[4] = 2;
        b.boost_by(user, &up, None, BoostEffect::Item(items::BLUNDER_POLICY));
    }
}

/// Mental Herb's `onUpdate`: a holder with any of `attract`, `taunt`, `encore`, `torment`,
/// `disable`, `healblock` uses the item (`useItem`) and loses all of them.
pub(crate) fn mental_herb<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    const CURED: [Volatile; 6] = [
        Volatile::Attract,
        Volatile::Taunt,
        Volatile::Encore,
        Volatile::Torment,
        Volatile::Disable,
        Volatile::HealBlock,
    ];
    if b.item(slot) != items::MENTAL_HERB || !CURED.iter().any(|&v| b.volatile(slot, v).active) {
        return;
    }
    if b.use_item(slot) {
        for volatile in CURED {
            b.remove_volatile(slot, volatile);
        }
    }
}

/// Berry Juice's `onUpdate` (not a berry: used, not eaten, so Unnerve and Gluttony do not
/// apply): at half HP or less (`pokemon.hp <= pokemon.maxhp / 2`), `runEvent('TryHeal', ...,
/// 20)` (Heal Block refuses; nothing implemented changes the amount), then `useItem()` and
/// `this.heal(20)`.
pub(crate) fn berry_juice<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let Some(mon) = b.slot_mon(slot) else {
        return;
    };
    let half = 2 * i32::from(mon.hp) <= i32::from(mon.max_hp);
    if b.item(slot) != items::BERRY_JUICE || !half || b.volatile(slot, Volatile::HealBlock).active {
        return;
    }
    if b.use_item(slot) {
        b.heal(slot, 20.0);
    }
}

/// `ModifyCritRatio` of the user's item: Scope Lens and Razor Claw `return critRatio + 1`;
/// Leek `critRatio + 2` for a holder whose base species (`user.baseSpecies.baseSpecies`) is
/// Farfetch'd (either forme) or Sirfetch'd.
pub(crate) fn crit_ratio_bonus(item: ItemId, holder: &Pokemon) -> i32 {
    if item == items::SCOPE_LENS || item == items::RAZOR_CLAW {
        return 1;
    }
    let own = holder.untransformed_species();
    let base = own.data().base_species;
    let base = if base.is_none() { own } else { base };
    if item == items::LEEK && [species::FARFETCHD, species::SIRFETCHD].contains(&base) {
        return 2;
    }
    0
}

/// A Gem's `onSourceTryPrimaryHit` (the user's item, priority 0; after Gulp Missile, an ability,
/// and before the target's substitute, -1) for each target of each hit: `if (target === source ||
/// move.category === 'Status' || move.flags['pledgecombo']) return; if (move.type === <the gem's
/// type> && source.useItem()) source.addVolatile('gem');` — the gem is used up on the first
/// target (`move.type`: after ModifyType) and its condition boosts the move's power
/// (`handlers::volatile_base_power`). Only Normal Gem is supported: the other Gems are `Past` in
/// Champions.
pub(crate) fn gem_try_primary_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    category: MoveCategory,
    move_type: Type,
) {
    if target == user
        || category == MoveCategory::Status
        || move_type != Type::Normal
        || b.item(user) != items::NORMAL_GEM
    {
        return;
    }
    if b.use_item(user) {
        b.add_volatile(user, Volatile::Gem);
    }
}

/// King's Rock / Razor Fang `onModifyMove` (priority -1), and the user's Stench (`stench`; the
/// ability's `onModifyMove`, also priority -1, is the same code): a non-status move without a
/// flinch among `secondaries` (the move's secondaries at that point of ModifyMove: none once
/// Sheer Force, priority 0, deleted them) gets `{chance: 10, volatileStatus: 'flinch'}`
/// appended. Both at once append one: the second finds the first's flinch. Serene Grace
/// (priority -2) doubles its chance afterwards like the move's own.
pub(crate) fn added_secondary(
    item: ItemId,
    stench: bool,
    data: &MoveData,
    secondaries: &[Secondary],
) -> Option<Secondary> {
    let flinch_item = stench || item == items::KINGS_ROCK || item == items::RAZOR_FANG;
    let has_flinch = secondaries
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
    let survives = match b.item(target) {
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

// ---- immunities and secondaries ----------------------------------------------------------------

/// The holder's item `onImmunity` (`runStatusImmunity`): Safety Goggles make it immune to
/// sandstorm damage and powder (and hail, which is not a supported weather).
pub(crate) fn grants_immunity(item: ItemId, immunity: TypeImmunities) -> bool {
    item == items::SAFETY_GOGGLES
        && (immunity == TypeImmunities::SANDSTORM || immunity == TypeImmunities::POWDER)
}

/// The target's item `onTryHit` (priority 0): Safety Goggles stop another Pokémon's powder
/// move against a holder that is not immune by type (`this.dex.getImmunity('powder')`),
/// `return null`. `true` = the move fails on the target.
pub(crate) fn try_hit_blocks<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    data: &MoveData,
    target: SlotRef,
) -> bool {
    b.item(target) == items::SAFETY_GOGGLES
        && data.flags.contains(MoveFlags::POWDER)
        && target != user
        && b.slot_mon(target).is_some()
        && !b
            .types(target)
            .iter()
            .any(|t| t.immunities().contains(TypeImmunities::POWDER))
}

/// The target's item `onModifySecondaries` (`secondaries`, before each roll): Covert Cloak keeps
/// only the secondaries with a `self` effect. `false` = the secondary is not rolled.
pub(crate) fn keeps_secondary<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    secondary: &Secondary,
) -> bool {
    b.item(target) != items::COVERT_CLOAK || secondary.self_boosts != NO_BOOSTS
}

// ---- after the move, on hit, residual ----------------------------------------------------------

/// The user's item `onAfterMoveSecondarySelf` (`useMoveInner`, only after a move that did not
/// fail): `target` is the move's last target, `total_damage` the HP its hits took
/// (`move.totalDamage`, 0 for field moves).
/// - Life Orb: `source !== target` and a non-status move: `damage(baseMaxhp / 10)`.
/// - Shell Bell (priority -1): `heal(totalDamage / 8)`.
/// - Throat Spray: a sound move uses the item (`useItem`: its `boosts`, SpA +1, then consumed).
///
/// Forced switches (`forceSwitchFlag`) and Sheer Force are refused by `support`.
pub(crate) fn after_move_secondary_self<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
    total_damage: i32,
) {
    let Some(mon) = b.alive(user).map(|p| b.mon(p)) else {
        return;
    };
    let max_hp = f64::from(mon.max_hp);
    match b.item(user) {
        // `if (source && source !== target && move && move.category !== 'Status' &&
        // !source.forceSwitchFlag)`: a user Red Card is dragging out pays no recoil.
        i if i == items::LIFE_ORB
            && data.category != MoveCategory::Status
            && target != user
            && !b.force_switch.contains(&user) =>
        {
            b.damage(user, max_hp / 10.0, DamageSource::Indirect);
        }
        i if i == items::SHELL_BELL && total_damage > 0 => {
            b.heal(user, f64::from(total_damage) / 8.0);
        }
        i if i == items::THROAT_SPRAY && data.flags.contains(MoveFlags::SOUND) => {
            b.boost_by(user, &i.data().boosts, Some(user), BoostEffect::Item(i));
            b.use_item(user);
        }
        _ => {}
    }
}

/// Showdown `useItem` of a held item with `boosts` (Weakness Policy, the absorbing items,
/// seeds, Room Service, Adrenaline Orb): nothing unless the holder has HP; the item's `boosts`
/// are applied with the holder as the source (`this.battle.event.target` in every calling
/// handler) and the item as the effect, while it is still held; then it is consumed and
/// becomes `lastItem`. The `UseItem` / `Use` / `AfterUseItem` events have no implemented
/// handler (Unburden is refused by `support`).
pub(crate) fn use_boost_item<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) -> bool {
    let item = b.item(holder);
    if b.alive(holder).is_none() || item.is_none() {
        return false;
    }
    b.boost_by(
        holder,
        &item.data().boosts,
        Some(holder),
        BoostEffect::Item(item),
    );
    b.use_item(holder)
}

/// Items with an implemented `onDamagingHit` run by [`on_damaging_hit`]. None has an
/// `onDamagingHitOrder`, so they run after the ordered handlers (Rough Skin, Rocky Helmet).
pub(crate) fn has_damaging_hit(item: ItemId) -> bool {
    [
        items::WEAKNESS_POLICY,
        items::ABSORB_BULB,
        items::CELL_BATTERY,
        items::LUMINOUS_MOSS,
        items::SNOWBALL,
        items::JABOCA_BERRY,
        items::ROWAP_BERRY,
    ]
    .contains(&item)
}

/// The damaged `target`'s item `onDamagingHit` for a hit of `user`'s move of `move_type` and
/// `category` (Showdown `runEvent('DamagingHit')`; the holder may be at 0 HP, not yet
/// processed as fainted):
/// - Weakness Policy: `!move.damage && !move.damageCallback &&
///   target.getMoveHitData(move).typeMod > 0` uses the item (Atk and SpA +2). Fixed-damage
///   moves never compute a `typeMod` ([`Battle::type_mod_of`] is `None`).
/// - Absorb Bulb (Water, SpA +1), Cell Battery (Electric, Atk +1), Luminous Moss (Water,
///   SpD +1), Snowball (Ice, Atk +1): `if (move.type === ...) target.useItem()`.
/// - Jaboca Berry (physical) / Rowap Berry (special): `source.hp && source.isActive &&
///   !source.hasAbility('magicguard')`, then `target.eatItem()` (which, for these two, works at
///   0 HP) and `this.damage(source.baseMaxhp / (target.hasAbility('ripen') ? 4 : 8), source,
///   target)`.
pub(crate) fn on_damaging_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    item: ItemId,
    move_type: Type,
    category: MoveCategory,
) {
    let triggers = match item {
        i if i == items::WEAKNESS_POLICY => b.type_mod_of(target).is_some_and(|t| t > 0),
        i if i == items::ABSORB_BULB || i == items::LUMINOUS_MOSS => move_type == Type::Water,
        i if i == items::CELL_BATTERY => move_type == Type::Electric,
        i if i == items::SNOWBALL => move_type == Type::Ice,
        i if i == items::JABOCA_BERRY || i == items::ROWAP_BERRY => {
            let wanted = if i == items::JABOCA_BERRY {
                MoveCategory::Physical
            } else {
                MoveCategory::Special
            };
            // `source.isActive`: not a future move's user hitting from the bench.
            if category == wanted
                && b.alive(user).is_some()
                && b.absent_user != Some(user)
                && b.ability(user) != abilities::MAGIC_GUARD
                && super::update::eat_item(b, target)
            {
                let max_hp = f64::from(b.slot_mon(user).expect("alive").max_hp);
                // `source.baseMaxhp / (target.hasAbility('ripen') ? 4 : 8)`.
                let divisor = if super::abilities::ripens(b, target) {
                    4.0
                } else {
                    8.0
                };
                b.damage(user, max_hp / divisor, DamageSource::Indirect);
            }
            return;
        }
        _ => false,
    };
    if triggers {
        use_boost_item(b, target);
    }
}

/// A handler of the hit loop's `runEvent('AfterMoveSecondary', targets, ...)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterMoveSecondaryHandler {
    /// The frozen status's handler (sub-order 2): a thawing move thaws its holder.
    Thaw,
    /// The target's ability (sub-order 7): Anger Shell, Berserk
    /// (`abilities::after_move_secondary`).
    Ability,
    /// The target's item (sub-order 8): [`after_move_secondary`].
    Item,
}

/// The order of Showdown's one `runEvent('AfterMoveSecondary', targets, source, move)` over the
/// hit loop's targets (`findEventHandlers` concatenates every target's handlers, then
/// `speedSort`): priority, high first (Eject Button's `onAfterMoveSecondaryPriority` 2, the
/// others 0), then the holder's Speed (`pokemon.speed`, [`Battle::event_speed`]), high first,
/// then sub-order (status 2, ability 7, item 8); ties are shuffled. Returns `(index into
/// targets, handler)` pairs; a thaw handler exists only on a frozen target of a thawing move.
///
/// Each handler acts on its own holder, except that the first Eject Button to act flags its
/// holder and so stops every later one (`pokemon.switchFlag === true`), and the first Red Card
/// to drag the attacker stops every later one (`source.forceSwitchFlag`): the faster holder's
/// item is the one used. A tie is drawn only among tied item handlers of two or more Eject
/// Buttons or Red Cards, where it can change the outcome; other ties keep target order.
pub(crate) fn after_move_secondary_order<const N: usize>(
    b: &mut Battle<'_, N>,
    targets: &[SlotRef],
    thaws: bool,
) -> Vec<(usize, AfterMoveSecondaryHandler)> {
    // (priority, Speed, sub-order, target index, handler)
    let mut handlers: Vec<(i32, i32, u32, usize, AfterMoveSecondaryHandler)> = Vec::new();
    for (i, &t) in targets.iter().enumerate() {
        let speed = b.event_speed(t);
        let frozen = b.slot_mon(t).is_some_and(|m| m.status == Status::Freeze);
        if thaws && frozen {
            handlers.push((0, speed, SUB_CONDITION, i, AfterMoveSecondaryHandler::Thaw));
        }
        handlers.push((0, speed, SUB_ABILITY, i, AfterMoveSecondaryHandler::Ability));
        let priority = super::abilities::priority(
            b.item(t).data().event_orders,
            "onAfterMoveSecondaryPriority",
        );
        handlers.push((
            priority,
            speed,
            SUB_ITEM,
            i,
            AfterMoveSecondaryHandler::Item,
        ));
    }
    // Stable: equal keys keep target order.
    handlers.sort_by_key(|&(priority, speed, sub, _, _)| {
        (std::cmp::Reverse(priority), std::cmp::Reverse(speed), sub)
    });
    let mut start = 0;
    while start < handlers.len() {
        let key = |h: &(i32, i32, u32, usize, AfterMoveSecondaryHandler)| (h.0, h.1, h.2);
        let end = start
            + handlers[start..]
                .iter()
                .take_while(|h| key(h) == key(&handlers[start]))
                .count();
        let exclusive = [items::EJECT_BUTTON, items::RED_CARD].iter().any(|&item| {
            handlers[start..end]
                .iter()
                .filter(|h| h.4 == AfterMoveSecondaryHandler::Item && b.item(targets[h.3]) == item)
                .count()
                >= 2
        });
        if exclusive {
            // `prng.shuffle` of the tied run: a uniformly random order.
            for i in start..end - 1 {
                let j = i + b.rng.uniform(end - i);
                handlers.swap(i, j);
            }
        }
        start = end;
    }
    handlers.into_iter().map(|h| (h.3, h.4)).collect()
}

/// The target's item `onAfterMoveSecondary` (`runEvent('AfterMoveSecondary')` at the end of the
/// hit loop, skipped for a Sheer Force-boosted move): Kee Berry eats itself after a physical
/// move (Present's heal, the only exception, is not a supported move), Maranga Berry after a
/// special one (`target.eatItem()`; their `onEat` raise Def / SpD by 1); Eject Button
/// (Champions' version: the attacker's own switch flag is not cancelled) asks its holder to
/// switch out unless its side has no bench, it is being dragged out, or any active Pokémon
/// already carries an Eject Button / Emergency Exit flag; Red Card is used up and drags the
/// attacker out (`forceSwitchFlag`) unless the attacker's side has no bench, either is already
/// being dragged, or the attacker's Suction Cups stop it. A Pokémon that fainted has left its
/// slot and does nothing.
pub(crate) fn after_move_secondary<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    category: MoveCategory,
) {
    let item = b.item(target);
    if item == items::EJECT_BUTTON {
        if user == target || b.alive(target).is_none() || category == MoveCategory::Status {
            return;
        }
        if super::residual::bench(b, target.side).next().is_none()
            || b.force_switch.contains(&target)
        {
            return;
        }
        // Champions: `if (target.volatiles['commanding'] || target.volatiles['commanded'])
        // return;`
        if b.volatile(target, Volatile::Commanding).active
            || b.volatile(target, Volatile::Commanded).active
        {
            return;
        }
        // `for (const pokemon of this.getAllActive()) if (pokemon.switchFlag === true) return;`
        // — a 0-HP Pokémon not processed yet included: the move's user its own recoil (before
        // AfterMoveSecondary in Champions) knocked out after Emergency Exit flagged it (oracle
        // `dd-emergency-exit-recoil-eject-button`).
        if b.any_active_switch_flag_true() {
            return;
        }
        b.set_switch_flag(target, SwitchFlag::Effect);
        if !b.use_item(target) {
            b.clear_switch_flag(target);
        }
        return;
    }
    if item == items::RED_CARD {
        // `!source.isActive`: a future move's user hitting from the bench.
        if user == target
            || b.absent_user == Some(user)
            || b.alive(user).is_none()
            || b.alive(target).is_none()
            || category == MoveCategory::Status
        {
            return;
        }
        if super::residual::bench(b, user.side).next().is_none()
            || b.force_switch.contains(&user)
            || b.force_switch.contains(&target)
        {
            return;
        }
        // `target.useItem(source)`, then `runEvent('DragOut', source, target, move)`: the
        // attacker's own Suction Cups or Guard Dog (never suppressed by its own move) or Ingrain.
        if b.use_item(target)
            && !super::moves::drag_out_ability(b.ability(user))
            && !super::conditions::drag_out_blocked(b, user)
        {
            b.force_switch.push(user);
        }
        return;
    }
    let wanted = match item {
        i if i == items::KEE_BERRY => MoveCategory::Physical,
        i if i == items::MARANGA_BERRY => MoveCategory::Special,
        _ => return,
    };
    if category == wanted {
        super::update::eat_item(b, target);
    }
}

/// The target's item `onAfterSubDamage` (`runEvent('AfterSubDamage', target, source, move)`
/// after its substitute took a move): Air Balloon pops (`target.item = ''`, no `lastItem`;
/// then AfterUseItem: Unburden), as it does on a damaging hit.
pub(crate) fn after_sub_damage<const N: usize>(b: &mut Battle<'_, N>, target: SlotRef) {
    if b.item(target) != items::AIR_BALLOON {
        return;
    }
    let Some(pokemon) = b.occupant(target) else {
        return;
    };
    b.apply(Instruction::SetItem {
        target: pokemon,
        old: items::AIR_BALLOON,
        new: ItemId::NONE,
    });
    super::abilities::unburden(b, target);
    super::abilities::symbiosis(b, target);
}

/// The target's item `onHit` (`runEvent('Hit')` in `runMoveEffects`, after the move's own
/// `onHit`): Sticky Barb moves to an itemless user of a contact move (`takeItem`, then
/// `setItem`; Protective Pads cannot apply, as the user holds nothing).
pub(crate) fn on_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    data: &MoveData,
) {
    // Enigma Berry: `if (move && target.getMoveHitData(move).typeMod > 0) { if
    // (target.eatItem()) this.heal(target.baseMaxhp / 4); }`. Its `onTryEatItem` asks
    // `runEvent('TryHeal')` (Heal Block: `update::eat_item` does not eat it).
    if b.item(target) == items::ENIGMA_BERRY
        && b.type_mod_of(target).is_some_and(|t| t > 0)
        && super::update::eat_item(b, target)
    {
        let max_hp = f64::from(b.slot_mon(target).expect("the eater").max_hp);
        super::update::berry_heal(b, target, max_hp / 4.0);
        return;
    }
    // `this.checkMoveMakesContact(move, source, target)`: the contact flag after ModifyMove
    // (Long Reach removes it; Punching Glove and Protective Pads need an item the user lacks).
    if user == target
        || b.item(target) != items::STICKY_BARB
        || !b.raw_item(user).is_none()
        || !makes_contact(b, user, data)
    {
        return;
    }
    let Some(receiver) = b.occupant(user) else {
        return;
    };
    if !b.take_item(target) {
        return;
    }
    // `setItem` fails on a fainted or inactive user; the barb is then gone.
    if b.alive(user).is_some() {
        b.apply(Instruction::SetItem {
            target: receiver,
            old: ItemId::NONE,
            new: items::STICKY_BARB,
        });
    }
}

/// `(onResidualOrder, onResidualSubOrder)` of an item with an implemented `onResidual`
/// (Leftovers has its own handler kind).
pub(crate) fn residual_order(item: ItemId) -> Option<(u32, u32)> {
    match item {
        i if i == items::BLACK_SLUDGE => Some((5, 4)),
        i if i == items::TOXIC_ORB || i == items::FLAME_ORB || i == items::STICKY_BARB => {
            Some((28, 3))
        }
        // No `onResidualOrder`: last, with the item sub-order.
        i if i == items::MICLE_BERRY => Some((ORDER_DEFAULT, SUB_ITEM)),
        i if i == items::WHITE_HERB || i == items::MIRROR_HERB || i == items::EJECT_PACK => {
            Some((29, SUB_ITEM))
        }
        _ => None,
    }
}

/// The item's `onResidual` for the Pokémon in `slot`:
/// - Black Sludge: a Poison type heals `baseMaxhp / 16`, anyone else takes `baseMaxhp / 8`.
/// - Toxic Orb / Flame Orb: `pokemon.trySetStatus('tox' / 'brn', pokemon)` (self-inflicted;
///   every implemented SetStatus handler blocks regardless of the source).
/// - Sticky Barb: `damage(baseMaxhp / 8)`.
/// - Micle Berry, White Herb, Mirror Herb, Eject Pack: as at their other triggers.
pub(crate) fn on_residual<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, item: ItemId) {
    let Some(mon) = b.alive(slot).map(|p| b.mon(p)) else {
        return;
    };
    let max_hp = f64::from(mon.max_hp);
    match item {
        i if i == items::BLACK_SLUDGE => {
            if b.has_type(slot, Type::Poison) {
                b.heal(slot, max_hp / 16.0);
            } else {
                b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
            }
        }
        i if i == items::TOXIC_ORB => {
            b.try_set_status(slot, Status::Toxic);
        }
        i if i == items::FLAME_ORB => {
            b.try_set_status(slot, Status::Burn);
        }
        i if i == items::STICKY_BARB => {
            b.damage(slot, max_hp / 8.0, DamageSource::Indirect);
        }
        // Micle Berry: eaten at 1/4 HP (1/2 with Gluttony); `onEat` adds `micleberry`.
        i if i == items::MICLE_BERRY => {
            let hp = i32::from(mon.hp);
            let max = i32::from(mon.max_hp);
            let pinch = 4 * hp <= max || (2 * hp <= max && b.ability(slot) == abilities::GLUTTONY);
            if pinch {
                super::update::eat_item(b, slot);
            }
        }
        i if i == items::WHITE_HERB => white_herb(b, slot),
        // The Residual handler's event target is the holder itself.
        i if i == items::MIRROR_HERB => mirror_herb_use(b, slot, slot),
        i if i == items::EJECT_PACK => eject_pack_use(b, slot),
        _ => {}
    }
}

/// Showdown `eatItem` for a held berry: `TryEatItem` (Unnerve, Anger Shell, Berserk:
/// `abilities::try_eat_item`), then it is consumed and becomes `lastItem` (`AfterUseItem`:
/// Unburden, in `Battle::use_item`, after `EatItem`: Cheek Pouch, Cud Chew, Ripen). The resist
/// berries' `onEat` is empty.
fn eat_item<const N: usize>(b: &mut Battle<'_, N>, holder: SlotRef) -> bool {
    // TryEatItem: the ability handlers (`abilities::try_eat_item`).
    super::abilities::try_eat_item(b, holder) && b.use_item(holder)
}

/// `ModifyDamage` handlers of items (`modifyDamage`, after the burn halving): the user's
/// `onModifyDamage` and the target's `onSourceModifyDamage`. `type_mod` is the hit's clamped
/// effectiveness (`getMoveHitData(move).typeMod`).
///
/// - Metronome's condition on the user: its count's factor ([`metronome_try_move`]).
/// - Life Orb: `chainModify([5324, 4096])`.
/// - Expert Belt: `chainModify([4915, 4096])` on a super-effective hit.
/// - Resist berries: a hit of the berry's type (super effective, except for Chilan Berry)
///   eats the berry (`target.eatItem()`), then `chainModify(0.5)`. Showdown eats it inside the
///   handler; no other ModifyDamage handler reads the target's item, so eating it while the
///   handlers are collected gives the same result. Not when the damage is for the target's
///   substitute (`hit_substitute`: `hitSub`, the substitute's `getDamage`).
pub(crate) fn modify_damage_handlers<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    move_type: Type,
    type_mod: i32,
    hit_substitute: bool,
) -> Vec<Handler> {
    let mut out = Vec::new();
    // The user's `metronome` condition (a condition's handler: sub-order 2; it acts whether or
    // not the item is suppressed, as its TryMove already removed it then):
    // `chainModify([dmgMod[min(numConsecutive, 5)], 4096])`.
    let metronome = b.volatile(user, Volatile::Metronome);
    if metronome.active {
        let modifier = METRONOME_MODIFIERS[usize::from(metronome.counter.min(5))];
        out.push(Handler::of(b, user, 0, SUB_CONDITION, modifier));
    }
    match b.item(user) {
        i if i == items::LIFE_ORB => out.push(Handler::of(b, user, 0, SUB_ITEM, 5324)),
        i if i == items::EXPERT_BELT && type_mod > 0 => {
            out.push(Handler::of(b, user, 0, SUB_ITEM, 4915));
        }
        _ => {}
    }
    if let Some(ty) = resist_berry(b.item(target)).filter(|_| !hit_substitute) {
        // `move.type`: the type after ModifyType (a Pixilate Normal move is Fairy).
        let applies = move_type == ty && (ty == Type::Normal || type_mod > 0);
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

    /// The residual orders and handler priorities hard-coded here are the dex's.
    #[test]
    fn item_orders_match_the_dex() {
        for item in [
            items::BLACK_SLUDGE,
            items::TOXIC_ORB,
            items::FLAME_ORB,
            items::STICKY_BARB,
        ] {
            let (order, sub_order) = residual_order(item).expect("a residual item");
            let orders = item.data().event_orders;
            assert!(
                orders.contains(&("onResidualOrder", order as i16)),
                "{item:?}"
            );
            assert!(
                orders.contains(&("onResidualSubOrder", sub_order as i16)),
                "{item:?}"
            );
        }
        let p = |item: ItemId, name: &str| {
            super::super::abilities::priority(item.data().event_orders, name)
        };
        assert_eq!(p(items::FOCUS_BAND, "onDamagePriority"), -40);
        assert_eq!(p(items::FOCUS_SASH, "onDamagePriority"), -40);
        assert_eq!(p(items::KINGS_ROCK, "onModifyMovePriority"), -1);
        assert_eq!(p(items::RAZOR_FANG, "onModifyMovePriority"), -1);
        assert_eq!(p(items::SHELL_BELL, "onAfterMoveSecondarySelfPriority"), -1);
        // Custap Berry and Quick Claw share the FractionalPriority priority; Micle Berry's
        // `onResidual` has no order.
        assert_eq!(p(items::CUSTAP_BERRY, "onFractionalPriorityPriority"), -2);
        assert_eq!(p(items::QUICK_CLAW, "onFractionalPriorityPriority"), -2);
        assert!(!items::MICLE_BERRY
            .data()
            .event_orders
            .iter()
            .any(|(n, _)| n.starts_with("onResidual")));
        assert_eq!(items::THROAT_SPRAY.data().boosts, [0, 0, 1, 0, 0, 0, 0]);
        // `Battle::weight`: Heavy Metal (priority 1) before Light Metal and Float Stone (0).
        let ability_priority = |a: crate::dex::AbilityId| {
            super::super::abilities::priority(a.data().event_orders, "onModifyWeightPriority")
        };
        assert_eq!(ability_priority(abilities::HEAVY_METAL), 1);
        assert_eq!(ability_priority(abilities::LIGHT_METAL), 0);
        assert_eq!(p(items::FLOAT_STONE, "onModifyWeightPriority"), 0);
        // `speed_modifier`: Macho Brace and the Power items ignore Klutz, Iron Ball does not.
        assert!(items::MACHO_BRACE.data().ignore_klutz);
        assert!(items::POWER_WEIGHT.data().ignore_klutz);
        assert!(!items::IRON_BALL.data().ignore_klutz);
    }
}
