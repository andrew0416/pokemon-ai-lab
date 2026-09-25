//! Callbacks of the conditions moves create (`condition` in `data/moves.ts`): what happens
//! when a volatile starts and when its duration runs out in the residual.

use crate::dex::{
    abilities, items, moves, MoveCategory, MoveFlags, MoveId, Type, TypeImmunities, TypeRelation,
};
use crate::field::{Effect, SideEffect};
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SideId, SlotRef, State, Status, BOOST_COUNT};
use crate::volatile::{decode_types, encode_types, Volatile, VolatileState};

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
    b: &Battle<'_, N>,
    target: SlotRef,
    volatile: Volatile,
    new: &mut VolatileState,
) -> bool {
    match volatile {
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
/// Pokémon immune to `trapped`: a Ghost type): No Retreat.
pub(crate) fn trapped<const N: usize>(state: &State<N>, slot: SlotRef) -> Option<String> {
    let mon = state.active(slot)?;
    let immune = mon
        .types
        .iter()
        .any(|t| t.immunities().contains(TypeImmunities::TRAPPED));
    if immune {
        return None;
    }
    let volatiles = &state.slot(slot).volatiles;
    if volatiles.has(Volatile::NoRetreat) {
        return Some(format!(
            "{} is trapped by No Retreat",
            mon.species.data().name
        ));
    }
    None
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
