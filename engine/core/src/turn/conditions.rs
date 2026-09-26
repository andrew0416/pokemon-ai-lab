//! Callbacks of the conditions moves create (`condition` in `data/moves.ts`): what happens
//! when a volatile starts and when its duration runs out in the residual.

use crate::dex::{
    abilities, items, moves, ConditionId, MoveCategory, MoveFlags, MoveId, Type, TypeImmunities,
    TypeRelation,
};
use crate::field::{Effect, SideEffect, SlotCondition, SlotEffect};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SideId, SlotHistory, SlotRef, State, Status, BOOST_COUNT};
use crate::volatile::{
    decode_pokemon, decode_slot, decode_types, encode_pokemon, encode_slot, encode_types, Volatile,
    VolatileState,
};

use super::battle::{Battle, BoostEffect, DamageSource};
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
/// The slot condition a move's `slotCondition` names, if the engine runs it.
pub(crate) fn slot_condition_of(condition: ConditionId) -> Option<SlotCondition> {
    Some(match condition {
        c if c == crate::dex::conditions::WISH => SlotCondition::Wish,
        c if c == crate::dex::conditions::HEALINGWISH => SlotCondition::HealingWish,
        c if c == crate::dex::conditions::REVIVALBLESSING => SlotCondition::RevivalBlessing,
        _ => return None,
    })
}

pub(crate) fn slot_condition<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
    condition: SlotCondition,
) -> SlotEffect {
    b.state.side(slot.side).slot_conditions[usize::from(slot.slot)][condition as usize]
}

fn set_slot_condition<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    condition: SlotCondition,
    new: SlotEffect,
) {
    let old = slot_condition(b, slot, condition);
    if old != new {
        b.apply(Instruction::SetSlotCondition {
            side: slot.side,
            slot: slot.slot,
            condition,
            old,
            new,
        });
    }
}

/// Showdown `side.addSlotCondition(target, condition, source, move)`: fails if the condition is
/// up (none of these has `onRestart`); Wish's `onStart` keeps the wisher's max HP (it heals
/// half) and the starting turn; Revival Blessing starts with duration 1.
pub(crate) fn add_slot_condition<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    condition: SlotCondition,
    source: SlotRef,
) -> bool {
    if slot_condition(b, slot, condition).is_active() {
        return false;
    }
    let new = match condition {
        SlotCondition::Wish => SlotEffect {
            value: b.slot_mon(source).map_or(0, |m| m.max_hp as u16),
            turn: b.state.turn,
        },
        SlotCondition::HealingWish => SlotEffect { value: 1, turn: 0 },
        SlotCondition::RevivalBlessing => SlotEffect { value: 1, turn: 1 },
    };
    set_slot_condition(b, slot, condition, new);
    true
}

/// Showdown `side.removeSlotCondition`: the condition's `End` on the Pokémon in the slot, then
/// it is gone. Wish's `onEnd` heals a standing occupant half the wisher's max HP.
pub(crate) fn remove_slot_condition<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    condition: SlotCondition,
) -> bool {
    let state = slot_condition(b, slot, condition);
    if !state.is_active() {
        return false;
    }
    if condition == SlotCondition::Wish && b.alive(slot).is_some() {
        b.heal(slot, f64::from(state.value) / 2.0);
    }
    set_slot_condition(b, slot, condition, SlotEffect::NONE);
    true
}

/// Healing Wish's `onSwitchIn` → `onSwap` for the newcomer in `slot` (a SwitchIn handler,
/// slot-condition sub-order 3, before the entry hazards): a newcomer below full HP or with a
/// status is healed fully and cured, and the condition ends; otherwise it waits.
pub(crate) fn slot_condition_switch_in<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    if !slot_condition(b, slot, SlotCondition::HealingWish).is_active() {
        return;
    }
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let mon = b.mon(pokemon);
    if mon.hp < mon.max_hp || mon.status != Status::None {
        let max_hp = f64::from(mon.max_hp);
        b.heal(slot, max_hp);
        b.cure_status(pokemon);
        set_slot_condition(b, slot, SlotCondition::HealingWish, SlotEffect::NONE);
    }
}

/// Wish's `onResidual` (order 4): once the turn count passed its starting turn the condition
/// ends (and heals); Revival Blessing's duration counts down.
pub(crate) fn slot_condition_residual<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    condition: SlotCondition,
) {
    let state = slot_condition(b, slot, condition);
    if !state.is_active() {
        return;
    }
    match condition {
        SlotCondition::Wish => {
            if b.state.turn > state.turn {
                remove_slot_condition(b, slot, condition);
            }
        }
        SlotCondition::RevivalBlessing => {
            if state.turn <= 1 {
                remove_slot_condition(b, slot, condition);
            } else {
                set_slot_condition(
                    b,
                    slot,
                    condition,
                    SlotEffect {
                        turn: state.turn - 1,
                        ..state
                    },
                );
            }
        }
        SlotCondition::HealingWish => {}
    }
}

/// The two-turn moves the engine runs (`charge` flag) and their own volatile
/// (`attacker.addVolatile(move.id)` in `twoturnmove`'s start). Other charge moves (Skull Bash,
/// Razor Wind, Sky Drop, ... ) are refused.
pub(crate) fn charge_volatile(id: MoveId) -> Option<Volatile> {
    Some(match id {
        i if i == moves::SOLAR_BEAM => Volatile::SolarBeam,
        i if i == moves::SOLAR_BLADE => Volatile::SolarBlade,
        i if i == moves::METEOR_BEAM => Volatile::MeteorBeam,
        i if i == moves::ELECTRO_SHOT => Volatile::ElectroShot,
        i if i == moves::SKY_ATTACK => Volatile::SkyAttack,
        i if i == moves::FLY => Volatile::Fly,
        i if i == moves::BOUNCE => Volatile::Bounce,
        i if i == moves::DIG => Volatile::Dig,
        i if i == moves::DIVE => Volatile::Dive,
        i if i == moves::PHANTOM_FORCE => Volatile::PhantomForce,
        i if i == moves::SHADOW_FORCE => Volatile::ShadowForce,
        _ => return None,
    })
}

/// The semi-invulnerable state of the Pokémon in `slot`, if any (`isSemiInvulnerable`).
pub(crate) fn semi_invulnerable<const N: usize>(
    b: &Battle<'_, N>,
    slot: SlotRef,
) -> Option<Volatile> {
    [
        Volatile::Fly,
        Volatile::Bounce,
        Volatile::Dig,
        Volatile::Dive,
        Volatile::PhantomForce,
        Volatile::ShadowForce,
    ]
    .into_iter()
    .find(|&v| b.volatile(slot, v).active)
}

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
        // `twoturnmove.onEnd`: `target.removeVolatile(this.effectState.move)`.
        Volatile::TwoTurnMove => {
            if let Some(own) = charge_volatile(state.mv) {
                b.remove_volatile(slot, own);
            }
        }
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
    b: &mut Battle<'_, N>,
    target: SlotRef,
    volatile: Volatile,
    new: &mut VolatileState,
) -> bool {
    // The source (`addVolatile(status, source)`): the user of the move adding it.
    let source = b
        .active_move
        .filter(|m| b.occupant(m.user) == Some(m.pokemon));
    match volatile {
        // Leech Seed: `sourceSlot = source.getSlot()` (whoever stands there at the residual is
        // healed).
        Volatile::LeechSeed => {
            let Some(source) = source else {
                return false;
            };
            new.counter = encode_slot(source.user);
            true
        }
        // Partial trapping: `durationCallback`: 8 if the source holds Grip Claw, else
        // `this.random(5, 7)`; `onStart`: `boundDivisor` 6 with Binding Band, else 8; the
        // source is kept for the residual and the trap.
        Volatile::PartiallyTrapped => {
            let Some(source) = source else {
                return false;
            };
            let item = b.item(source.user);
            new.duration = if item == items::GRIP_CLAW {
                8
            } else {
                5 + b.rng.uniform(2) as u8
            };
            new.hidden = if item == items::BINDING_BAND { 6 } else { 8 };
            new.counter = encode_pokemon(source.pokemon);
            true
        }
        // Taunt: `if (target.activeTurns && !this.queue.willMove(target))
        // this.effectState.duration++;` (`activeTurns` is `active_since_turn_start`).
        Volatile::Taunt => {
            if b.active_since_turn_start(target) && b.will_move(target).is_none() {
                new.duration += 1;
            }
            true
        }
        // Disable: one turn less `if (this.queue.willMove(pokemon) || (pokemon ===
        // this.activePokemon && this.activeMove && !this.activeMove.isExternal))` (the second
        // case: Cursed Body disabling the attacker's move while it is used); it fails without a
        // last move or when the last move's slot has no PP; `this.effectState.move =
        // pokemon.lastMove.id`.
        Volatile::Disable => {
            let using_a_move = b
                .active_move
                .is_some_and(|m| m.user == target && b.occupant(target) == Some(m.pokemon));
            if b.will_move(target).is_some() || using_a_move {
                new.duration -= 1;
            }
            let last = b.state.slot(target).last_move;
            if last.is_none() {
                return false;
            }
            let Some(pokemon) = b.occupant(target) else {
                return false;
            };
            if b.mon(pokemon)
                .moves
                .iter()
                .any(|m| m.id == last && m.pp == 0)
            {
                return false;
            }
            new.mv = last;
            true
        }
        // Substitute (F11): `this.effectState.hp = Math.floor(target.maxhp / 4)`; partial
        // trapping ends silently (`delete target.volatiles['partiallytrapped']`, no `onEnd`).
        Volatile::Substitute => {
            let Some(pokemon) = b.occupant(target) else {
                return false;
            };
            let hp = b.mon(pokemon).max_hp / 4;
            b.set_substitute_hp(target, hp);
            b.delete_volatile(target, Volatile::PartiallyTrapped);
            true
        }
        _ => true,
    }
}

/// The user's condition `onBeforeMove` handlers between flinch (priority 8) and Gravity (6):
/// Disable (7; the Champions override) fails the disabled move unless it has the
/// `cantusetwice` flag. `false` = the move is not used.
pub(crate) fn before_move_after_flinch<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> bool {
    let disable = b.volatile(user, Volatile::Disable);
    !(disable.active && disable.mv == id && !id.data().flags.contains(MoveFlags::CANTUSETWICE))
}

/// The condition `onBeforeMove` handlers from Gravity's priority (6) down to confusion (3):
/// the user's Throat Chop (6, next to Gravity: both only fail the move) fails a sound move; its
/// Taunt (5) fails a status move other than Me First; a foe's Imprison (`onFoeBeforeMove`, 4)
/// fails a move its holder knows (not Struggle). `false` = the move is not used.
pub(crate) fn before_move_after_gravity<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> bool {
    let data = id.data();
    let taunted = b.volatile(user, Volatile::Taunt).active
        && data.category == MoveCategory::Status
        && id != moves::ME_FIRST;
    !throat_chopped(b.state, user, id) && !taunted && !imprisoned(b.state, user, id)
}

/// Throat Chop's `onBeforeMove` (priority 6), `onModifyMove` and `onDisableMove`: the holder's
/// sound moves are neither chosen nor used (`if (!move.isZOrMaxPowered &&
/// move.flags['sound'])`; Z- and Max Moves are off in Champions).
pub(crate) fn throat_chopped<const N: usize>(state: &State<N>, slot: SlotRef, id: MoveId) -> bool {
    state.slot(slot).volatiles.has(Volatile::ThroatChop)
        && id.data().flags.contains(MoveFlags::SOUND)
}

/// Whether an active foe of the Pokémon in `slot` has Imprison and knows `id` (Imprison's
/// `onFoeDisableMove` / `onFoeBeforeMove`: `this.effectState.source` is the holder itself).
fn imprisoned<const N: usize>(state: &State<N>, slot: SlotRef, id: MoveId) -> bool {
    if id == moves::STRUGGLE {
        return false;
    }
    (0..N as u8).any(|i| {
        let foe = SlotRef {
            side: slot.side.other(),
            slot: i,
        };
        state.slot(foe).volatiles.has(Volatile::Imprison)
            && state
                .active(foe)
                .is_some_and(|m| m.hp > 0 && m.moves.iter().any(|s| s.id == id))
    })
}

/// Why the Pokémon in `slot` cannot choose `id` because of a condition on it (the conditions'
/// `DisableMove` handlers that `endTurn` runs): Taunt disables every status move but Me First,
/// Disable its move, Torment the last move (not Struggle), a foe's Imprison every move its
/// holder knows, Throat Chop every sound move.
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
    let disable = volatiles.get(Volatile::Disable);
    if disable.active && disable.mv == id {
        return Some(format!("{} is disabled by Disable", data.name));
    }
    let last = state.slot(slot).last_move;
    if volatiles.has(Volatile::Torment) && last == id && id != moves::STRUGGLE {
        return Some(format!("{} is disabled by Torment", data.name));
    }
    if imprisoned(state, slot, id) {
        return Some(format!("{} is disabled by Imprison", data.name));
    }
    if throat_chopped(state, slot, id) {
        return Some(format!("{} is disabled by Throat Chop", data.name));
    }
    None
}

/// Why the Pokémon in `slot` cannot switch out because of a condition on it (the
/// `TrapPokemon` handlers `endTurn` runs, each calling `pokemon.tryTrap()`, which fails for a
/// Pokémon immune to `trapped`: a Ghost type): No Retreat; Mean Look, Block and Spider Web
/// (`trapped`); partial trapping while its source is active (`if
/// (this.effectState.source?.isActive) pokemon.tryTrap();`). Shed Shell's `onTrapPokemon`
/// (priority -10, after every other handler: `pokemon.trapped = false`) frees its holder from
/// all of them unless the item is suppressed. Commander's `commanding` and `commanded`
/// (priority -11, after Shed Shell) set `pokemon.trapped = true` without `tryTrap`: no type
/// immunity or item frees them.
pub(crate) fn trapped<const N: usize>(state: &State<N>, slot: SlotRef) -> Option<String> {
    let mon = state.active(slot)?;
    let commander = &state.slot(slot).volatiles;
    if commander.has(Volatile::Commanding) || commander.has(Volatile::Commanded) {
        return Some(format!("{} is in Commander", mon.species.data().name));
    }
    let immune = mon
        .types
        .iter()
        .any(|t| t.immunities().contains(TypeImmunities::TRAPPED));
    if immune || (mon.item == items::SHED_SHELL && !super::items::ignoring_item(state, slot)) {
        return None;
    }
    let name = mon.species.data().name;
    let volatiles = &state.slot(slot).volatiles;
    if volatiles.has(Volatile::NoRetreat) {
        return Some(format!("{name} is trapped by No Retreat"));
    }
    if volatiles.has(Volatile::Trapped) {
        return Some(format!("{name} is trapped (Mean Look, Block, Spider Web)"));
    }
    if volatiles.has(Volatile::Ingrain) {
        return Some(format!("{name} is rooted by Ingrain"));
    }
    let trap = volatiles.get(Volatile::PartiallyTrapped);
    if trap.active {
        let source = decode_pokemon(trap.counter);
        let source_active = state
            .side(source.side)
            .slots
            .iter()
            .any(|s| s.party_index == Some(source.party));
        if source_active {
            return Some(format!("{name} is partially trapped"));
        }
    }
    None
}

/// The conditions' `onDragOut` on the Pokémon in `slot`: Ingrain returns `null` (no drag, and
/// the move does not fail), Commander's `commanding` and `commanded` return `false` (every
/// caller only drags on a truthy result). Suction Cups is checked by the callers.
pub(crate) fn drag_out_blocked<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> bool {
    b.volatile(slot, Volatile::Ingrain).active
        || b.volatile(slot, Volatile::Commanding).active
        || b.volatile(slot, Volatile::Commanded).active
}

/// Mean Look, Block, Spider Web `onHit`: `target.addVolatile('trapped', source, move,
/// 'trapper')`. Fails on a fainted target, one already trapped (no `onRestart`) or one immune to
/// `trapped` (`runStatusImmunity`: a Ghost type); otherwise the target gets `trapped` (its
/// `onStart` only logs) linked to the source's `trapper`, which gains the target.
pub(crate) fn add_trap<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    source: SlotRef,
) -> bool {
    let (Some(trapped), Some(trapper)) = (b.alive(target), b.alive(source)) else {
        return false;
    };
    if b.volatile(target, Volatile::Trapped).active
        || b.status_immune(target, TypeImmunities::TRAPPED)
        || b.add_volatile_blocked(target, Volatile::Trapped)
    {
        return false;
    }
    b.set_volatile_state(
        target,
        Volatile::Trapped,
        VolatileState {
            active: true,
            counter: encode_pokemon(trapper),
            ..VolatileState::NONE
        },
    );
    let links = b.volatile(source, Volatile::Trapper);
    b.set_volatile_state(
        source,
        Volatile::Trapper,
        VolatileState {
            active: true,
            counter: links.counter | SlotHistory::attacker_bit(trapped),
            ..VolatileState::NONE
        },
    );
    true
}

/// Showdown `removeLinkedVolatiles` for the Pokémon `pokemon` leaving the field from `slot`
/// (`clearVolatile`): each Pokémon it trapped loses `trapped` (`removeVolatile`: not at 0 HP);
/// if it was trapped, its trapper forgets it and loses `trapper` once it has trapped nobody
/// left.
pub(crate) fn remove_linked_volatiles<const N: usize>(
    b: &mut Battle<'_, N>,
    pokemon: PokemonRef,
    slot: SlotRef,
) {
    let links = b.volatile(slot, Volatile::Trapper);
    if links.active {
        for other in State::<N>::slot_refs() {
            let Some(held) = b.occupant(other) else {
                continue;
            };
            let trap = b.volatile(other, Volatile::Trapped);
            if links.counter & SlotHistory::attacker_bit(held) != 0
                && trap.active
                && decode_pokemon(trap.counter) == pokemon
            {
                b.remove_volatile(other, Volatile::Trapped);
            }
        }
    }
    let trap = b.volatile(slot, Volatile::Trapped);
    if trap.active {
        let trapper = decode_pokemon(trap.counter);
        let held = Battle::<N>::slots(trapper.side).find(|&s| b.occupant(s) == Some(trapper));
        if let Some(held) = held {
            let links = b.volatile(held, Volatile::Trapper);
            if links.active {
                let rest = links.counter & !SlotHistory::attacker_bit(pokemon);
                if rest == 0 {
                    b.remove_volatile(held, Volatile::Trapper);
                } else {
                    b.set_volatile_state(
                        held,
                        Volatile::Trapper,
                        VolatileState {
                            counter: rest,
                            ..links
                        },
                    );
                }
            }
        }
    }
}

/// Destiny Bond at the holder's move attempt: its `onBeforeMove` (priority -1, after every other
/// handler) removes it unless the move is Destiny Bond itself, and its `onMoveAborted` removes
/// it when BeforeMove stopped the move (`proceeds` false). `removeVolatile` needs HP.
pub(crate) fn destiny_bond_before_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    proceeds: bool,
) {
    if !proceeds || id != moves::DESTINY_BOND {
        b.remove_volatile(user, Volatile::DestinyBond);
    }
}

/// Destiny Bond's `onFaint` (`runEvent('Faint', pokemon, source, effect)` in `faintMessages`,
/// before the fainted Pokémon's volatiles are cleared): when the holder in `slot` fainted from
/// the damage of a move used by `attacker` on the other side (`effect.effectType === 'Move'`,
/// no future move; the engine records the attacker only for a move's own damage), the attacker
/// faints too (`source.faint()`: no source, nothing if it already has no HP).
pub(crate) fn destiny_bond_faint<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    attacker: Option<crate::state::PokemonRef>,
) {
    let Some(attacker) = attacker else {
        return;
    };
    if attacker.side == slot.side || !b.volatile(slot, Volatile::DestinyBond).active {
        return;
    }
    if let Some(source_slot) =
        Battle::<N>::slots(attacker.side).find(|&s| b.occupant(s) == Some(attacker))
    {
        b.faint(source_slot);
    }
}

/// Leech Seed's `onResidual` (order 8) on the seeded Pokémon in `slot`: nothing if the
/// Pokémon now in the seeder's slot (`getAtSlot(sourceSlot)`, whoever it is) is missing or
/// fainted; otherwise `this.damage(pokemon.baseMaxhp / 8, pokemon, target)` (not a move's
/// damage: Magic Guard stops it) and that Pokémon heals what was taken.
pub(crate) fn leech_seed_residual<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let seed = b.volatile(slot, Volatile::LeechSeed);
    if !seed.active {
        return;
    }
    let healer = decode_slot(seed.counter);
    if b.alive(healer).is_none() {
        return;
    }
    let max_hp = b.slot_mon(slot).map_or(0, |m| m.max_hp);
    let taken = b.damage(slot, f64::from(max_hp) / 8.0, DamageSource::Indirect);
    if taken > 0 {
        // The heal's effect is the `leechseed` condition (Big Root).
        b.heal_rooted(healer, f64::from(taken));
    }
}

/// Partial trapping's `onResidual` (order 13, after its duration went down) on the trapped
/// Pokémon in `slot`: the volatile is deleted (no `onEnd`) once its source left the field, has
/// no HP or switched in this turn (`!source.activeTurns`); otherwise
/// `this.damage(pokemon.baseMaxhp / boundDivisor)`.
pub(crate) fn partially_trapped_residual<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    let trap = b.volatile(slot, Volatile::PartiallyTrapped);
    if !trap.active {
        return;
    }
    let source = decode_pokemon(trap.counter);
    let source_slot = Battle::<N>::slots(source.side).find(|&s| b.occupant(s) == Some(source));
    let holds = source_slot.is_some_and(|s| b.mon(source).hp > 0 && b.active_since_turn_start(s));
    if !holds {
        b.delete_volatile(slot, Volatile::PartiallyTrapped);
        return;
    }
    let max_hp = b.slot_mon(slot).map_or(0, |m| m.max_hp);
    b.damage(
        slot,
        f64::from(max_hp) / f64::from(trap.hidden),
        DamageSource::Indirect,
    );
}

// ---- entry hazards ------------------------------------------------------------------------------

/// The entry hazards, in the order the engine runs their `onSwitchIn`. Showdown runs them in the
/// order they were set (`effectOrder`), which the state does not keep; [`entry_hazards`] refuses
/// the switch-ins where the order would show.
pub(crate) const HAZARDS: [SideEffect; 4] = [
    SideEffect::StealthRock,
    SideEffect::Spikes,
    SideEffect::ToxicSpikes,
    SideEffect::StickyWeb,
];

/// Most layers a hazard stacks to (its `onSideRestart`); Stealth Rock and Sticky Web have no
/// restart handler, so a second use fails.
pub(crate) fn hazard_layers(effect: SideEffect) -> u8 {
    match effect {
        SideEffect::Spikes => 3,
        SideEffect::ToxicSpikes => 2,
        _ => 1,
    }
}

/// Showdown `addSideCondition` for a hazard: a new one has no duration (`Effect::PERMANENT`)
/// and, for Spikes and Toxic Spikes, `layers = 1` (`Effect.value`; 0 for the others); an
/// existing one runs `onSideRestart`: one more layer below the maximum, otherwise it fails.
pub(crate) fn add_hazard<const N: usize>(
    b: &mut Battle<'_, N>,
    side: SideId,
    effect: SideEffect,
) -> bool {
    let current = b.state.side(side).effects[effect as usize];
    let layered = matches!(effect, SideEffect::Spikes | SideEffect::ToxicSpikes);
    if current.is_active() {
        if !layered || current.value >= hazard_layers(effect) {
            return false;
        }
        let more = Effect {
            value: current.value + 1,
            ..current
        };
        b.set_side_effect(side, effect, more);
        return true;
    }
    let new = Effect {
        value: u8::from(layered),
        turns: Effect::PERMANENT,
    };
    b.set_side_effect(side, effect, new);
    true
}

/// `dex.getEffectiveness('Rock', type)` summed over the holder's types, each passed through its
/// item's `onEffectiveness` (`runEffectiveness` of Stealth Rock's active move), clamped to
/// -6..6.
fn stealth_rock_type_mod<const N: usize>(b: &Battle<'_, N>, slot: SlotRef) -> i32 {
    let Some(mon) = b.slot_mon(slot) else {
        return 0;
    };
    mon.types
        .iter()
        .filter(|&&t| t != Type::None)
        .map(|&t| {
            let chart = match Type::Rock.against(t) {
                TypeRelation::Super => 1,
                TypeRelation::Resist => -1,
                _ => 0,
            };
            super::items::on_effectiveness(b, slot, Type::Rock, chart)
        })
        .sum::<i32>()
        .clamp(-6, 6)
}

/// The HP Stealth Rock or Spikes would take from the newcomer in `slot`, before the Damage
/// handlers (0 when the hazard does not apply).
fn hazard_damage<const N: usize>(b: &Battle<'_, N>, slot: SlotRef, effect: SideEffect) -> f64 {
    let Some(mon) = b.slot_mon(slot) else {
        return 0.0;
    };
    let max_hp = f64::from(mon.max_hp);
    let boots = mon.item == items::HEAVY_DUTY_BOOTS;
    let layers = b.state.side(slot.side).effects[effect as usize].value;
    match effect {
        // `if (pokemon.hasItem('heavydutyboots')) return; ... this.damage(pokemon.maxhp *
        // 2 ** typeMod / 8);`
        SideEffect::StealthRock if !boots => {
            max_hp * 2f64.powi(stealth_rock_type_mod(b, slot)) / 8.0
        }
        // `if (!pokemon.isGrounded() || pokemon.hasItem('heavydutyboots')) return;
        // const damageAmounts = [0, 3, 4, 6]; this.damage(damageAmounts[layers] * maxhp / 24);`
        SideEffect::Spikes if !boots && b.is_grounded(slot) => {
            f64::from([0u8, 3, 4, 6][usize::from(layers.min(3))]) * max_hp / 24.0
        }
        _ => 0.0,
    }
}

/// The entry hazards' `onSwitchIn` for the newcomer in `slot` (`fieldEvent('SwitchIn')`: the
/// side conditions of its side, sub-order 4, run before its ability, 7), each followed by
/// `faintMessages`. Stealth Rock: Rock-effectiveness damage (none with Heavy-Duty Boots);
/// Spikes: 1/8, 1/6, 1/4 to a grounded holder without Boots; Toxic Spikes: a grounded Poison
/// type removes them, a grounded non-Steel holder without Boots is poisoned (badly with two
/// layers) with the foe in slot 0 (`pokemon.side.foe.active[0]`) as the source, which Safeguard
/// stops; Sticky Web: -1 Speed to a grounded holder without Boots, from the foe in slot 0
/// (Defiant and Mirror Armor react).
///
/// Showdown runs several hazards in the order they were set. The order only shows when a
/// damaging hazard can knock the newcomer out and Toxic Spikes (status or absorption) or Sticky
/// Web against Mirror Armor (the reflected drop) also act on it; that case is unsupported, and
/// so is Toxic Spikes poisoning a Synchronize holder (Synchronize ignores Toxic Spikes, which
/// `Battle::try_set_status_from` cannot tell).
pub(crate) fn entry_hazards<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
) -> Result<(), TurnError> {
    let side = slot.side;
    let present: Vec<SideEffect> = HAZARDS
        .into_iter()
        .filter(|&h| b.side_effect_active(side, h))
        .collect();
    let Some(pokemon) = b.alive(slot) else {
        return Ok(());
    };
    if present.is_empty() {
        return Ok(());
    }
    let mon = b.mon(pokemon);
    let grounded = b.is_grounded(slot);
    let boots = mon.item == items::HEAVY_DUTY_BOOTS;
    let toxic_spikes_act = present.contains(&SideEffect::ToxicSpikes) && grounded;
    let poisons = toxic_spikes_act
        && !mon.types.contains(&Type::Poison)
        && !mon.types.contains(&Type::Steel)
        && !boots;
    let web_reflects = present.contains(&SideEffect::StickyWeb)
        && grounded
        && !boots
        && mon.ability == abilities::MIRROR_ARMOR;
    let damage: f64 = present
        .iter()
        .map(|&h| hazard_damage(b, slot, h))
        .filter(|&d| d > 0.0)
        .map(|d| d.floor().max(1.0))
        .sum();
    let can_faint = mon.ability != abilities::MAGIC_GUARD && damage >= f64::from(mon.hp);
    if can_faint && (toxic_spikes_act || web_reflects) {
        return Err(b.unsupported(format!(
            "{} switching into hazards whose order (Showdown effectOrder) decides the outcome",
            mon.species.data().name
        )));
    }
    if poisons && mon.ability == abilities::SYNCHRONIZE {
        return Err(b.unsupported(
            "Toxic Spikes poisoning a Synchronize holder (Synchronize ignores Toxic Spikes)",
        ));
    }
    let foe_lead = SlotRef {
        side: side.other(),
        slot: 0,
    };
    for hazard in present {
        if b.alive(slot) != Some(pokemon) {
            break;
        }
        // A hazard removed since the handlers were gathered (Toxic Spikes absorbed by an
        // earlier newcomer) no longer acts.
        if !b.side_effect_active(side, hazard) {
            continue;
        }
        match hazard {
            SideEffect::StealthRock | SideEffect::Spikes => {
                let amount = hazard_damage(b, slot, hazard);
                if amount > 0.0 {
                    b.damage(slot, amount, DamageSource::Indirect);
                }
            }
            SideEffect::ToxicSpikes if b.is_grounded(slot) => {
                if b.has_type(slot, Type::Poison) {
                    b.set_side_effect(side, hazard, Effect::NONE);
                } else if !b.has_type(slot, Type::Steel) && b.item(slot) != items::HEAVY_DUTY_BOOTS
                {
                    let layers = b.state.side(side).effects[hazard as usize].value;
                    let status = if layers >= 2 {
                        Status::Toxic
                    } else {
                        Status::Poison
                    };
                    b.try_set_status_from(slot, status, Some(foe_lead));
                }
            }
            SideEffect::StickyWeb
                if b.is_grounded(slot) && b.item(slot) != items::HEAVY_DUTY_BOOTS =>
            {
                let mut drop = [0i8; BOOST_COUNT];
                drop[4] = -1;
                b.boost_by(
                    slot,
                    &drop,
                    Some(foe_lead),
                    BoostEffect::Move(moves::STICKY_WEB),
                );
            }
            _ => {}
        }
        if b.faint_messages(true) {
            break;
        }
    }
    Ok(())
}
