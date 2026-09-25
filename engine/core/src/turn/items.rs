//! Item handlers (Showdown `data/items.ts`; the Champions mod overrides no callback of these
//! items) for the items listed in `support`.
//!
//! Each function is one Showdown event; `moves.rs`, `battle.rs`, `order.rs`, `residual.rs` and
//! `mod.rs` call it where Showdown runs that event. An item not handled here gets the event's
//! neutral result. Items are read straight from the holder: Klutz holding an item is refused
//! by `support` (Showdown's `ignoringItem`, work plan F17), so no handler here checks it.
//!
//! Refused on purpose (not in `support`'s tables):
//! - Metronome: its condition keeps `lastMove` and `numConsecutive` and reads
//!   `moveLastTurnResult` (work plan F13); the item's `onStart` adds it at switch-in.
//! - Clear Amulet: `onTryBoost` needs the boost events (F16).
//! - Utility Umbrella: its effect is `pokemon.effectiveWeather()`, read at many sites (weather
//!   damage modifier, Chlorophyll / Swift Swim, Solar Power, Rain Dish, Dry Skin, Hydration,
//!   Leaf Guard, ...); only the freeze immunity and the move handlers read it so far.
//! - Air Balloon's pop is refused at the hit ([`on_damaging_hit`], F15); grounding is done.

use crate::damage::{MOD_HALF, MOD_ONE_POINT_FIVE};
use crate::dex::{
    abilities, conditions, items, moves, ItemId, MoveCategory, MoveData, MoveFlags, MoveId,
    Secondary, Stat, Type, TypeImmunities, NO_BOOSTS,
};
use crate::field::FieldEffect;
use crate::instruction::Instruction;
use crate::state::{Pokemon, PokemonRef, SlotRef, State, Status};
use crate::volatile::{Volatile, VolatileState};

use super::abilities::{Handler, SUB_ITEM};
use super::battle::{Battle, BoostEffect, DamageSource};
use super::order::ORDER_DEFAULT;

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
/// the newcomer cannot have yet; Air Balloon only announces itself.
pub(crate) fn inert_start(item: ItemId) -> bool {
    item.data().is_choice || item == items::AIR_BALLOON
}

/// Whether `handler`, one of the item's handlers that can fire around a switch-in, is
/// implemented: an inert `onStart` ([`inert_start`]), a Seed's `onStart` (the switch-in
/// handler [`switch_in_priority`] schedules) and `onTerrainChange` (`field_events`).
pub(crate) fn start_handler_implemented(item: ItemId, handler: &str) -> bool {
    match handler {
        "onStart" => inert_start(item) || switch_in_priority(item).is_some(),
        "onTerrainChange" => super::field_events::seed_terrain(item).is_some(),
        _ => false,
    }
}

/// `onSwitchInPriority` of an item whose `onStart` acts when its holder switches in (it runs
/// in the batched `fieldEvent('SwitchIn')`, after the abilities' priority-0 handlers): the
/// Seeds (-1).
pub(crate) fn switch_in_priority(item: ItemId) -> Option<i32> {
    super::field_events::seed_terrain(item)
        .map(|_| super::abilities::priority(item.data().event_orders, "onSwitchInPriority"))
}

/// The switch-in handler [`switch_in_priority`] scheduled for the holder in `slot`, run with
/// the item it held when the handlers were collected (Showdown calls the collected callback:
/// a consumed item makes its `useItem` fail).
pub(crate) fn switch_in_item<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, item: ItemId) {
    if b.item(slot) != item {
        return;
    }
    if super::field_events::seed_terrain(item).is_some() {
        super::field_events::seed_check(b, slot);
    }
}

// ---- Speed, grounding, effectiveness, action order --------------------------------------------

/// `ModifySpe` factor of the holder's item: Choice Scarf `chainModify(1.5)` (skipped while
/// Dynamaxed, which `support` refuses); Iron Ball `chainModify(0.5)`.
pub(crate) fn speed_modifier(item: ItemId) -> Option<u32> {
    match item {
        i if i == items::CHOICE_SCARF => Some(MOD_ONE_POINT_FIVE),
        i if i == items::IRON_BALL => Some(MOD_HALF),
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

/// The constant handlers of `runEvent('FractionalPriority')` for a move action, in tenths:
/// the ability's (Stall, sub-order 7) and then the item's (sub-order 8) replace the value, so
/// the item's wins. Quick Claw's random handler runs after them ([`quick_claw`]).
pub(crate) fn fractional_priority_tenths(mon: &Pokemon) -> i8 {
    match constant_fractional_tenths(mon.item) {
        0 => super::order::fractional_priority_tenths(mon.ability),
        item => item,
    }
}

/// Quick Claw's `onFractionalPriority` (priority -2, after the constants) for a move action of
/// `pokemon` whose fractional priority is `current` tenths: `priority <= 0 &&
/// this.randomChance(1, 5)` makes it +0.1. Showdown draws it when the turn's actions are
/// queued (`resolveAction`); Mycelium Might's status-move exception is moot (the ability is
/// refused by `support`).
pub(crate) fn quick_claw<const N: usize>(
    b: &mut Battle<'_, N>,
    pokemon: PokemonRef,
    current: i8,
) -> Option<i8> {
    (b.mon(pokemon).item == items::QUICK_CLAW && current <= 0 && b.rng.chance(1, 5)).then_some(1)
}

/// Custap Berry's `onFractionalPriority` (priority -2, like Quick Claw, which a holder of it
/// cannot also hold) for a move action of the Pokémon in `slot` whose fractional priority is
/// `current` tenths: `priority <= 0` and the holder at 1/4 of its max HP or less (1/2 with
/// Gluttony) eats the berry (`eatItem`, empty `onEat`) and makes it +0.1. Showdown runs it
/// when the turn's actions are queued (`resolveAction`), so the berry is gone before the
/// first action; Mycelium Might's status-move exception is moot (refused by `support`).
pub(crate) fn custap<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    pokemon: PokemonRef,
    current: i8,
) -> Option<i8> {
    if b.occupant(slot) != Some(pokemon) || b.mon(pokemon).item != items::CUSTAP_BERRY {
        return None;
    }
    let mon = b.mon(pokemon);
    let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
    // `abilityState.gluttony` is set on switch-in: always set here (see `update.rs`).
    let pinch = 4 * hp <= max_hp || (2 * hp <= max_hp && mon.ability == abilities::GLUTTONY);
    (current <= 0 && pinch && super::update::eat_item(b, slot)).then_some(1)
}

// ---- Choice items ---------------------------------------------------------------------------

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
                ..VolatileState::NONE
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
        && b.slot_mon(target).is_some_and(|m| {
            !m.types
                .iter()
                .any(|t| t.immunities().contains(TypeImmunities::POWDER))
        })
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
    match mon.item {
        i if i == items::LIFE_ORB && data.category != MoveCategory::Status && target != user => {
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
///   0 HP) and `this.damage(source.baseMaxhp / 8, source, target)` (Ripen, 1/4, is refused).
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
            if category == wanted
                && b.alive(user).is_some()
                && b.ability(user) != abilities::MAGIC_GUARD
                && super::update::eat_item(b, target)
            {
                let max_hp = f64::from(b.slot_mon(user).expect("alive").max_hp);
                b.damage(user, max_hp / 8.0, DamageSource::Indirect);
            }
            return;
        }
        _ => false,
    };
    if triggers {
        use_boost_item(b, target);
    }
}

/// The target's item `onAfterMoveSecondary` (`runEvent('AfterMoveSecondary')` at the end of the
/// hit loop, skipped for a Sheer Force-boosted move): Kee Berry eats itself after a physical
/// move (Present's heal, the only exception, is not a supported move), Maranga Berry after a
/// special one (`target.eatItem()`; their `onEat` raise Def / SpD by 1).
pub(crate) fn after_move_secondary<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    category: MoveCategory,
) {
    let wanted = match b.item(target) {
        i if i == items::KEE_BERRY => MoveCategory::Physical,
        i if i == items::MARANGA_BERRY => MoveCategory::Special,
        _ => return,
    };
    if category == wanted {
        super::update::eat_item(b, target);
    }
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
    // `runEvent('TryHeal')`, which no supported effect answers (Heal Block, Ripen are not).
    if b.item(target) == items::ENIGMA_BERRY
        && b.type_mod_of(target).is_some_and(|t| t > 0)
        && super::update::eat_item(b, target)
    {
        let max_hp = f64::from(b.slot_mon(target).expect("the eater").max_hp);
        b.heal(target, max_hp / 4.0);
        return;
    }
    if user == target
        || b.item(target) != items::STICKY_BARB
        || !b.item(user).is_none()
        || !data.flags.contains(MoveFlags::CONTACT)
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
        _ => None,
    }
}

/// The item's `onResidual` for the Pokémon in `slot`:
/// - Black Sludge: a Poison type heals `baseMaxhp / 16`, anyone else takes `baseMaxhp / 8`.
/// - Toxic Orb / Flame Orb: `pokemon.trySetStatus('tox' / 'brn', pokemon)` (self-inflicted;
///   every implemented SetStatus handler blocks regardless of the source).
/// - Sticky Barb: `damage(baseMaxhp / 8)`.
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
            let pinch = 4 * hp <= max || (2 * hp <= max && mon.ability == abilities::GLUTTONY);
            if pinch {
                super::update::eat_item(b, slot);
            }
        }
        _ => {}
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
    }
}
