//! Move-specific Showdown callbacks (`data/moves.ts`, with the Champions overrides of
//! `data/mods/champions/moves.ts`) for moves listed in `support::MOVES_WITH_HANDLERS`.
//!
//! Each function is one event; `moves.rs` calls it where Showdown runs that event. A move not
//! handled here gets the event's neutral result.

use crate::damage::MOD_ONE_POINT_FIVE;
use crate::dex::{
    abilities, items, moves, ItemId, MoveCategory, MoveFlags, MoveId, MoveTarget, Type,
    TypeRelation, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{MoveResult, Pokemon, PokemonRef, SideId, SlotRef, Status, BOOST_COUNT};
use crate::volatile::{Volatile, VolatileState};

use super::super::abilities::{Handler, SUB_CONDITION};
use super::super::battle::{Battle, BoostEffect, DamageSource};
use super::super::conditions::HAZARDS;
use super::super::order::{boosted_stat, modify};
use super::super::queue::{Action, ActionKind};
use super::super::TurnError;
use super::ActiveMove;

/// Showdown `pokemon.effectiveWeather()` of `holder` while `user` is the Pokémon using a
/// move: Utility Umbrella hides sun and rain from its holder. Mega Sol (every move of its
/// holder sees sun) is not implemented.
fn effective_weather<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    holder: SlotRef,
) -> Result<Weather, TurnError> {
    if b.ability(user) == abilities::MEGA_SOL {
        return Err(b.unsupported("Mega Sol's weather for moves"));
    }
    Ok(b.weather_for(holder))
}

/// The move's `onModifyType` (`useMoveInner`, right before its `onModifyMove`).
pub(super) fn on_modify_type<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
) -> Result<(), TurnError> {
    match mv.id {
        // Weather Ball: `switch (pokemon.effectiveWeather())`: Fire in sun, Water in rain, Rock
        // in sandstorm, Ice in hail and snow.
        moves::WEATHER_BALL => {
            mv.move_type = match effective_weather(b, user, user)? {
                Weather::Sun | Weather::HarshSun => Type::Fire,
                Weather::Rain | Weather::HeavyRain => Type::Water,
                Weather::Sand => Type::Rock,
                Weather::Snow => Type::Ice,
                _ => return Ok(()),
            };
        }
        // Raging Bull: the Paldean Tauros forms make it Fighting, Fire or Water.
        moves::RAGING_BULL => {
            let species = b.slot_mon(user).map(|m| m.species);
            mv.move_type = match species {
                Some(s) if s == crate::dex::species::TAUROS_PALDEA_COMBAT => Type::Fighting,
                Some(s) if s == crate::dex::species::TAUROS_PALDEA_BLAZE => Type::Fire,
                Some(s) if s == crate::dex::species::TAUROS_PALDEA_AQUA => Type::Water,
                _ => return Ok(()),
            };
        }
        // Terrain Pulse: `if (!pokemon.isGrounded()) return;` then the type of `field.terrain`.
        moves::TERRAIN_PULSE if b.is_grounded(user) => {
            mv.move_type = match b.terrain() {
                Terrain::Electric => Type::Electric,
                Terrain::Grassy => Type::Grass,
                Terrain::Misty => Type::Fairy,
                Terrain::Psychic => Type::Psychic,
                Terrain::None => return Ok(()),
            };
        }
        _ => {}
    }
    Ok(())
}

/// The move's `onModifyMove` (`useMoveInner`, after the target is chosen and before
/// `getMoveTargets`). `target` is the chosen target.
pub(super) fn on_modify_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: Option<SlotRef>,
    mv: &mut ActiveMove,
) -> Result<(), TurnError> {
    match mv.id {
        // Photon Geyser: `if (pokemon.getStat('atk', false, true) > pokemon.getStat('spa', false,
        // true)) move.category = 'Physical';` (stages, no modifiers).
        moves::PHOTON_GEYSER => {
            let atk = unmodified_stat(b, user, 0);
            let spa = unmodified_stat(b, user, 2);
            if atk > spa {
                mv.set_category(MoveCategory::Physical);
            }
        }
        // Shell Side Arm: against the chosen target, `floor(floor(floor(floor(2 * level / 5 + 2)
        // * 90 * atk) / def) / 50)` against the same with SpA and SpD (`getStat(.., false,
        // true)` on both sides); physical (and contact) if higher, or on a tie with
        // `randomChance(1, 2)`.
        moves::SHELL_SIDE_ARM => {
            let Some(target) = target.filter(|&t| b.slot_mon(t).is_some()) else {
                return Ok(());
            };
            let level = i64::from(b.slot_mon(user).map_or(0, |m| m.level));
            let base = 2 * level / 5 + 2;
            let hit = |attack: i32, defense: i32| -> i64 {
                (base * 90 * i64::from(attack) / i64::from(defense.max(1))) / 50
            };
            let physical = hit(unmodified_stat(b, user, 0), unmodified_stat(b, target, 1));
            let special = hit(unmodified_stat(b, user, 2), unmodified_stat(b, target, 3));
            if physical > special || (physical == special && b.rng.chance(1, 2)) {
                mv.set_category(MoveCategory::Physical);
            }
        }
        // Weather Ball: `move.basePower *= 2` in sun, rain, sandstorm, hail and snow (the
        // user's effective weather).
        moves::WEATHER_BALL => {
            if matches!(
                effective_weather(b, user, user)?,
                Weather::Sun
                    | Weather::HarshSun
                    | Weather::Rain
                    | Weather::HeavyRain
                    | Weather::Sand
                    | Weather::Snow
            ) {
                mv.base_power *= 2;
            }
        }
        // Terrain Pulse: `if (this.field.terrain && pokemon.isGrounded()) move.basePower *= 2;`
        moves::TERRAIN_PULSE => {
            if b.terrain() != Terrain::None && b.is_grounded(user) {
                mv.base_power *= 2;
            }
        }
        // Expanding Force: `if (this.field.isTerrain('psychicterrain') && source.isGrounded())
        // move.target = 'allAdjacentFoes';` (the caller then re-picks the target).
        moves::EXPANDING_FORCE => {
            if b.terrain() == Terrain::Psychic && b.is_grounded(user) {
                mv.target = MoveTarget::AllAdjacentFoes;
            }
        }
        // Blizzard: `if (this.field.isWeather(['hail', 'snowscape'])) move.accuracy = true;`
        moves::BLIZZARD => {
            if b.effective_weather() == Weather::Snow {
                mv.accuracy = None;
            }
        }
        // Struggle: `move.type = '???'` (typeless: `Type::None` for the move, which no type chart
        // entry, STAB or type-based handler matches).
        moves::STRUGGLE => mv.move_type = Type::None,
        // Thunder, Hurricane: `switch (target?.effectiveWeather())`: never misses in rain,
        // accuracy 50 in sun.
        moves::THUNDER | moves::HURRICANE => {
            let Some(target) = target else {
                return Ok(());
            };
            match effective_weather(b, user, target)? {
                Weather::Rain | Weather::HeavyRain => mv.accuracy = None,
                Weather::Sun | Weather::HarshSun => mv.accuracy = Some(50),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Showdown `pokemon.getStat(stat, false, true)` for battle stat `index` (0 Atk .. 3 SpD): the
/// stored stat with its stage (no ModifyBoost, no Modify* handlers). Under Wonder Room the stage
/// is the other defense's (`statName` is swapped after the stored stat is read).
fn unmodified_stat<const N: usize>(b: &Battle<'_, N>, slot: SlotRef, index: usize) -> i32 {
    let Some(mon) = b.slot_mon(slot) else {
        return 0;
    };
    let stage_index = match index {
        1 if b.field_active(FieldEffect::WonderRoom) => 3,
        3 if b.field_active(FieldEffect::WonderRoom) => 1,
        i => i,
    };
    boosted_stat(
        i32::from(mon.stats[index]),
        b.state.slot(slot).boosts[stage_index],
    )
}

/// The move's `onTry` (`singleEvent('Try', move, null, pokemon, targets[0])`, before
/// PrepareHit and every hit step). `false` = the move fails.
pub(super) fn on_try<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    first_target: SlotRef,
) -> bool {
    match mv.id {
        // Fake Out, First Impression: `if (source.activeMoveActions > 1) return false;`
        moves::FAKE_OUT | moves::FIRST_IMPRESSION => b.state.slot(user).move_actions <= 1,
        // Poltergeist: `return !!target.item;` (the held item, even if suppressed). Its
        // `onTryHit` only logs the item.
        moves::POLTERGEIST => b.slot_mon(first_target).is_some_and(|m| !m.item.is_none()),
        // Steel Roller: `return !this.field.isTerrain('');` (no `TryTerrain` handler exists).
        moves::STEEL_ROLLER => b.terrain() != Terrain::None,
        // Sleep Talk, Snore: `return source.status === 'slp' || source.hasAbility('comatose');`
        moves::SLEEP_TALK | moves::SNORE => {
            b.slot_mon(user).is_some_and(|m| m.status == Status::Sleep)
                || b.ability(user) == abilities::COMATOSE
        }
        // Metal Burst, Comeuppance: `if (!lastDamagedBy?.thisTurn) return false;` (a foe's
        // damaging hit on the user this turn).
        moves::METAL_BURST | moves::COMEUPPANCE => {
            b.state.slot(user).history.last_damaged_by.is_some()
        }
        // Counter, Mirror Coat: `if (!source.volatiles['counter']) return false; if
        // (source.volatiles['counter'].slot === null) return false;`
        moves::COUNTER | moves::MIRROR_COAT => before_turn_volatile(mv.id)
            .is_some_and(|v| b.volatile(user, v).active && b.volatile(user, v).hidden != 0),
        // Clangorous Soul: `if (source.hp <= (source.maxhp * 33 / 100) || source.maxhp === 1)
        // return false;` Fillet Away: `source.hp <= source.maxhp / 2`.
        moves::CLANGOROUS_SOUL | moves::FILLET_AWAY => b.slot_mon(user).is_some_and(|m| {
            let (hp, max_hp) = (i32::from(m.hp), i32::from(m.max_hp));
            let enough = if mv.id == moves::CLANGOROUS_SOUL {
                hp * 100 > max_hp * 33
            } else {
                hp * 2 > max_hp
            };
            enough && max_hp != 1
        }),
        // No Retreat: `if (source.volatiles['noretreat']) return false;` (its other branch,
        // `delete move.volatileStatus` for a `trapped` user, is [`keeps_volatile_status`]).
        moves::NO_RETREAT => !b.volatile(user, Volatile::NoRetreat).active,
        // Magnet Rise: `if (target.volatiles['smackdown'] || target.volatiles['ingrain']) return
        // false;` (on itself; Smack Down's volatile is not implemented; its Gravity branch is for
        // the Z-Move, Gravity's BeforeMove already stops the move).
        moves::MAGNET_RISE => !b.volatile(first_target, Volatile::Ingrain).active,
        // Rest: fails asleep or with Comatose, at full HP, and with Insomnia or Vital Spirit
        // (`hasAbility`: the user's own ability, never suppressed by its own move).
        moves::REST => b.slot_mon(user).is_some_and(|m| {
            m.status != Status::Sleep
                && m.hp != m.max_hp
                && ![
                    abilities::COMATOSE,
                    abilities::INSOMNIA,
                    abilities::VITAL_SPIRIT,
                ]
                .contains(&m.ability)
        }),
        _ => true,
    }
}

/// Whether the move still has its `volatileStatus` when its effects run: No Retreat's `onTry`
/// deletes it for a user that is `trapped` (Mean Look, Block, Spider Web), which only gets the
/// boosts.
pub(super) fn keeps_volatile_status<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    mv.id != moves::NO_RETREAT || !b.volatile(user, Volatile::Trapped).active
}

/// The move's `onTryImmunity` (`hitStepTryImmunity`, per target). `false` = the target is
/// immune.
pub(super) fn on_try_immunity<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    match mv.id {
        // Trick, Switcheroo: `return !target.hasAbility('stickyhold');` (`hasAbility` is not
        // skipped by Mold Breaker).
        moves::TRICK | moves::SWITCHEROO => b.ability(target) != abilities::STICKY_HOLD,
        // Leech Seed: `return !target.hasType('Grass');`
        moves::LEECH_SEED => !b.has_type(target, Type::Grass),
        // Endeavor: `return pokemon.hp < target.hp;`
        moves::ENDEAVOR => {
            let hp = |s: SlotRef| b.slot_mon(s).map_or(0, |m| m.hp);
            hp(user) < hp(target)
        }
        _ => true,
    }
}

/// The move's `damageCallback` (`getDamage`, after type immunity and before the critical hit
/// roll: no crit, no roll, no modifiers). `None` = the move has none.
pub(super) fn damage_callback<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
) -> Option<i32> {
    let hp = |b: &Battle<'_, N>, s: SlotRef| b.slot_mon(s).map_or(0, |m| i32::from(m.hp));
    match mv.id {
        // Endeavor: `return target.getUndynamaxedHP() - pokemon.hp;`
        moves::ENDEAVOR => Some(hp(b, target) - hp(b, user)),
        // Super Fang: `clampIntRange(target.getUndynamaxedHP() / 2, 1)`; Nature's Madness,
        // Ruination: the same floored (`spreadDamage` truncates the fraction anyway).
        moves::SUPER_FANG | moves::NATURES_MADNESS | moves::RUINATION => {
            Some((hp(b, target) / 2).max(1))
        }
        // Final Gambit: `const damage = pokemon.hp; pokemon.faint(); return damage;`
        moves::FINAL_GAMBIT => {
            let damage = hp(b, user);
            b.faint(user);
            Some(damage)
        }
        // Metal Burst, Comeuppance: `(lastDamagedBy.damage * 1.5) || 1`, which `spreadDamage`
        // truncates (F13).
        moves::METAL_BURST | moves::COMEUPPANCE => {
            let damage = b
                .state
                .slot(user)
                .history
                .last_damaged_by
                .map_or(0, |d| i32::from(d.damage));
            let scaled = damage * 3 / 2;
            Some(if scaled == 0 { 1 } else { scaled })
        }
        // Counter, Mirror Coat: `pokemon.volatiles['counter'].damage || 1` (0 without the
        // condition, which `onTry` already failed on).
        moves::COUNTER | moves::MIRROR_COAT => {
            let recorded = before_turn_volatile(mv.id).map(|v| b.volatile(user, v));
            match recorded {
                Some(state) if state.active => Some(i32::from(state.counter).max(1)),
                _ => Some(0),
            }
        }
        _ => None,
    }
}

/// The condition a move's `beforeTurnCallback` adds to its user at the start of the turn
/// (`moves::before_turn_move`): Counter's `counter`, Mirror Coat's `mirrorcoat`.
pub(super) fn before_turn_volatile(id: MoveId) -> Option<Volatile> {
    match id {
        moves::COUNTER => Some(Volatile::Counter),
        moves::MIRROR_COAT => Some(Volatile::MirrorCoat),
        _ => None,
    }
}

/// The condition a move's `priorityChargeCallback` adds to its user once switches and Mega
/// Evolution are done (`moves::priority_charge_move`).
pub(super) fn priority_charge_volatile(id: MoveId) -> Option<Volatile> {
    match id {
        moves::FOCUS_PUNCH => Some(Volatile::FocusPunch),
        moves::BEAK_BLAST => Some(Volatile::BeakBlast),
        moves::SHELL_TRAP => Some(Volatile::ShellTrap),
        _ => None,
    }
}

/// The move's `beforeMoveCallback` (`runMove`, after BeforeMove let it through): `true` stops
/// the move. Focus Punch: `if (pokemon.volatiles['focuspunch']?.lostFocus) return true;`
pub(super) fn before_move_callback<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    mv.id == moves::FOCUS_PUNCH && b.volatile(user, Volatile::FocusPunch).counter != 0
}

/// `runEvent('Hit', target, source, move)` for the target's volatiles (after the move's own
/// `onHit`, before the target's item):
/// - Focus Punch: `if (move.category !== 'Status') this.effectState.lostFocus = true;` (any
///   attacker);
/// - Beak Blast: `if (this.checkMoveMakesContact(move, source, target)) source.trySetStatus('brn',
///   target);` (Protective Pads and Punching Glove on the attacker prevent it);
/// - Shell Trap: a foe's physical move sets `gotHit` and `queue.prioritizeAction` moves the
///   holder's pending move to the front (order 3).
pub(super) fn volatile_on_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
) {
    let focus = b.volatile(target, Volatile::FocusPunch);
    if focus.active && mv.category != MoveCategory::Status {
        b.set_volatile_state(
            target,
            Volatile::FocusPunch,
            VolatileState {
                counter: 1,
                ..focus
            },
        );
    }
    if b.volatile(target, Volatile::BeakBlast).active
        && super::item_events::makes_contact(b, user, mv.data)
        && b.item(user) != items::PROTECTIVE_PADS
    {
        b.try_set_status_from(user, Status::Burn, Some(target));
    }
    let trap = b.volatile(target, Volatile::ShellTrap);
    if trap.active && target.side != user.side && mv.category == MoveCategory::Physical {
        b.set_volatile_state(
            target,
            Volatile::ShellTrap,
            VolatileState { counter: 1, ..trap },
        );
        if let Some(index) = b.will_move(target) {
            b.prioritize_action(index);
        }
    }
}

/// Counter's and Mirror Coat's `condition.onDamagingHit` on the damaged `target`: a hit from a
/// foe (`!source.isAlly(target)`) whose category (`this.getCategory(move)`, which returns the
/// active move's own `category`, as ModifyMove left it) is physical (Counter) or special
/// (Mirror Coat) records the attacker's slot (`source.getSlot()`) and twice the damage,
/// replacing an earlier hit.
pub(super) fn counter_damaging_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
    damage: i32,
) {
    if user.side == target.side {
        return;
    }
    let volatile = match mv.category {
        MoveCategory::Physical => Volatile::Counter,
        MoveCategory::Special => Volatile::MirrorCoat,
        MoveCategory::Status => return,
    };
    let state = b.volatile(target, volatile);
    if !state.active || b.occupant(target).is_none() {
        return;
    }
    b.set_volatile_state(
        target,
        volatile,
        VolatileState {
            counter: (2 * damage).clamp(0, i32::from(u16::MAX)) as u16,
            hidden: user.slot + 1,
            ..state
        },
    );
}

/// Counter's and Mirror Coat's `condition.onRedirectTarget` (priority -1, after every other
/// handler) for the move `mv` of its holder `user`: `if (move.id !== 'counter') return; if
/// (source !== this.effectState.target || !this.effectState.slot) return; return
/// this.getAtSlot(this.effectState.slot);` — the slot of the last foe that hit it, whoever
/// stands there now (a fainted Pokémon there makes the move fail).
pub(super) fn counter_redirect<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Option<SlotRef> {
    let volatile = before_turn_volatile(mv.id)?;
    let state = b.volatile(user, volatile);
    if !state.active || state.hidden == 0 {
        return None;
    }
    Some(SlotRef {
        side: user.side.other(),
        slot: state.hidden - 1,
    })
}

/// The move's `onMoveFail` (`useMoveInner` when the move did not succeed on any target, after
/// its hits): High Jump Kick, Jump Kick, Axe Kick and Supercell Slam crash for half the user's
/// max HP (`this.damage(source.baseMaxhp / 2, source, source, condition)`: not a move's damage,
/// so Magic Guard stops it).
pub(super) fn on_move_fail<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) {
    let crash = [
        moves::HIGH_JUMP_KICK,
        moves::JUMP_KICK,
        moves::AXE_KICK,
        moves::SUPERCELL_SLAM,
    ];
    if crash.contains(&mv.id) {
        let max_hp = b.slot_mon(user).map_or(0, |m| m.max_hp);
        b.damage(user, f64::from(max_hp) / 2.0, DamageSource::Indirect);
    }
}

/// The move's own `onTryHit` (Champions `spreadMoveHit`: `singleEvent('TryHit', ...)` on the
/// first target, after accuracy and before the damage). `false` = the move fails. Low Kick's
/// and Grass Knot's only act on a Dynamaxed target, Poltergeist's only logs.
pub(super) fn on_try_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &mut ActiveMove,
) -> bool {
    match mv.id {
        // Healing Wish: `if (!this.canSwitch(source.side)) return this.NOT_FAIL;` — the target
        // drops out and the user does not faint (`selfdestruct: 'ifHit'`).
        moves::HEALING_WISH => super::super::residual::bench(b, user.side).next().is_some(),
        // Revival Blessing: `if (!source.side.pokemon.filter(ally => ally.fainted).length)
        // return false;`
        moves::REVIVAL_BLESSING => b.state.side(user.side).party.iter().any(|p| p.hp == 0),
        // Psychic Fangs, Brick Break, Raging Bull: the target's side loses Reflect, Light Screen
        // and Aurora Veil before the damage (returns nothing).
        moves::PSYCHIC_FANGS | moves::BRICK_BREAK | moves::RAGING_BULL => {
            remove_side_effects(
                b,
                target.side,
                &[
                    SideEffect::Reflect,
                    SideEffect::LightScreen,
                    SideEffect::AuroraVeil,
                ],
            );
            true
        }
        // Clangorous Soul, Fillet Away: `if (!this.boost(move.boosts!)) return null; delete
        // move.boosts;` (the move's own boosts are applied here, not in `runMoveEffects`:
        // `boosts_applied_in_try_hit`).
        moves::CLANGOROUS_SOUL | moves::FILLET_AWAY => {
            let boosts = mv.data.boosts;
            b.boost_by(target, &boosts, Some(user), BoostEffect::Move(mv.id))
        }
        // Pollen Puff: `if (source.isAlly(target)) { move.basePower = 0; move.infiltrates =
        // true; }` (`infiltrates` only matters against a substitute).
        moves::POLLEN_PUFF => {
            if target.side == user.side {
                mv.base_power = 0;
            }
            true
        }
        // Yawn: `if (target.status || !target.runStatusImmunity('slp')) return false;` (no type
        // or implemented `Immunity` handler covers sleep).
        moves::YAWN => b.slot_mon(target).is_some_and(|m| m.status == Status::None),
        // Helping Hand: `if (!target.newlySwitched && !this.queue.willMove(target)) return
        // false;`. `newlySwitched` (switched in this turn) is `move_actions == 0` here: a
        // Pokémon without a queued move either moved this turn (`runMove` counted it) or
        // switched in this turn (the count restarts at 0).
        moves::HELPING_HAND => {
            b.will_move(target).is_some() || b.state.slot(target).move_actions == 0
        }
        // Disable: `if (!target.lastMove || target.lastMove.isZOrMaxPowered ||
        // target.lastMove.isMax || target.lastMove.id === 'struggle') return false;`
        moves::DISABLE => {
            let last = b.state.slot(target).last_move;
            !last.is_none() && last != moves::STRUGGLE && !last.data().is_max
        }
        // Substitute (on its user): `NOT_FAIL` with a substitute already up, or at a quarter of
        // the max HP or less (`source.hp <= source.maxhp / 4 || source.maxhp === 1`); Champions
        // `spreadMoveHit` fails the move on any falsy TryHit result.
        moves::SUBSTITUTE => b.slot_mon(target).is_some_and(|m| {
            let (hp, max_hp) = (i32::from(m.hp), i32::from(m.max_hp));
            !b.has_substitute(target) && 4 * hp > max_hp && max_hp != 1
        }),
        _ => true,
    }
}

/// Showdown `move.infiltrates` for the implemented moves: Pollen Puff's `onTryHit` sets it on a
/// hit aimed at an ally (Infiltrator, which also sets it, is refused). It lets the move through
/// a substitute.
pub(super) fn infiltrates(user: SlotRef, mv: &ActiveMove, target: SlotRef) -> bool {
    mv.id == moves::POLLEN_PUFF && target.side == user.side
}

/// Whether the move's own `onTryHit` applies its `boosts` and deletes them (`delete
/// move.boosts`), so `runMoveEffects` has none left.
pub(super) fn boosts_applied_in_try_hit(id: MoveId) -> bool {
    id == moves::CLANGOROUS_SOUL || id == moves::FILLET_AWAY
}

/// The move's `onAfterHit`, once per damaged target (`spreadMoveHit`, after `DamagingHit`;
/// Champions runs it even if the user fainted). Knock Off's is in `moves.rs`; against a
/// substitute the move's `onAfterSubDamage` ([`on_after_sub_damage`]) runs instead.
pub(super) fn on_after_hit<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) {
    // Ice Spinner: `this.field.clearTerrain();`
    if mv.id == moves::ICE_SPINNER {
        super::clear_terrain(b);
    }
    // Rapid Spin, Mortal Spin: `if (!move.hasSheerForce)` the user loses Leech Seed, its side
    // its hazards, then the user partial trapping (`removeVolatile` does nothing for a user
    // knocked out in DamagingHit, `removeSideCondition` still acts).
    if (mv.id == moves::RAPID_SPIN || mv.id == moves::MORTAL_SPIN) && !mv.has_sheer_force {
        b.remove_volatile(user, Volatile::LeechSeed);
        remove_side_effects(b, user.side, &HAZARDS);
        b.remove_volatile(user, Volatile::PartiallyTrapped);
    }
    // Ceaseless Edge, Stone Axe: `if (!move.hasSheerForce)` the foe side gets a layer of Spikes /
    // Stealth Rock (`addSideCondition`, even from a fainted user).
    if !mv.has_sheer_force {
        let hazard = match mv.id {
            moves::CEASELESS_EDGE => Some(SideEffect::Spikes),
            moves::STONE_AXE => Some(SideEffect::StealthRock),
            _ => None,
        };
        if let Some(hazard) = hazard {
            super::super::conditions::add_hazard(b, user.side.other(), hazard);
        }
    }
}

/// The move's `onAfterSubDamage` (`singleEvent('AfterSubDamage', move)` in the substitute's
/// `onTryPrimaryHit`, after the substitute took the hit, the recoil and the drain): like its
/// `onAfterHit`, but only while the user has HP.
/// - Ice Spinner: `if (source.hp) this.field.clearTerrain();`
/// - Steel Roller: `this.field.clearTerrain();` (its `onHit` does the same on a hit).
/// - Rapid Spin, Mortal Spin: `if (!move.hasSheerForce)` and `pokemon.hp`: the user loses Leech
///   Seed, its side its hazards, the user partial trapping.
/// - Ceaseless Edge, Stone Axe: `if (!move.hasSheerForce && source.hp)` a layer of Spikes /
///   Stealth Rock on the foe side.
pub(super) fn on_after_sub_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) {
    let user_hp = b.alive(user).is_some();
    match mv.id {
        moves::ICE_SPINNER if user_hp => {
            super::clear_terrain(b);
        }
        moves::STEEL_ROLLER => {
            super::clear_terrain(b);
        }
        moves::RAPID_SPIN | moves::MORTAL_SPIN if !mv.has_sheer_force && user_hp => {
            b.remove_volatile(user, Volatile::LeechSeed);
            remove_side_effects(b, user.side, &HAZARDS);
            b.remove_volatile(user, Volatile::PartiallyTrapped);
        }
        moves::CEASELESS_EDGE if !mv.has_sheer_force && user_hp => {
            super::super::conditions::add_hazard(b, user.side.other(), SideEffect::Spikes);
        }
        moves::STONE_AXE if !mv.has_sheer_force && user_hp => {
            super::super::conditions::add_hazard(b, user.side.other(), SideEffect::StealthRock);
        }
        _ => {}
    }
}

/// The move's own `onAfterMove` (`runMove`, after `useMove`). Sparkling Aria: if the user
/// fainted (processed), or the move has Sheer Force's `hasSheerForce`, every active Pokémon just
/// loses the `sparklingaria` volatile; otherwise each hit target but the user that is still
/// active loses it, and is cured of a burn if it had it or the move hit several targets.
pub(super) fn on_after_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    pokemon: PokemonRef,
    mv: &ActiveMove,
) {
    // Beak Blast: `pokemon.removeVolatile('beakblast')` (nothing on a user at 0 HP).
    if mv.id == moves::BEAK_BLAST {
        if b.occupant(user) == Some(pokemon) {
            b.remove_volatile(user, Volatile::BeakBlast);
        }
        return;
    }
    if mv.id != moves::SPARKLING_ARIA {
        return;
    }
    if b.occupant(user) != Some(pokemon) || mv.has_sheer_force {
        for side in [SideId::One, SideId::Two] {
            for slot in Battle::<N>::slots(side) {
                if b.occupant(slot).is_some() {
                    b.delete_volatile(slot, Volatile::SparklingAria);
                }
            }
        }
        return;
    }
    let targets = super::hit_target_slots::<N>(mv.hit_targets);
    let several = targets.len() > 1;
    for target in targets {
        let Some(hit) = b.alive(target) else {
            continue;
        };
        if target == user {
            continue;
        }
        let had = b.remove_volatile(target, Volatile::SparklingAria);
        if (had || several) && b.mon(hit).status == Status::Burn {
            b.cure_status(hit);
        }
    }
}

/// `side.removeSideCondition` for each of `effects`; whether any was there.
fn remove_side_effects<const N: usize>(
    b: &mut Battle<'_, N>,
    side: SideId,
    effects: &[SideEffect],
) -> bool {
    let mut removed = false;
    for &effect in effects {
        if b.side_effect_active(side, effect) {
            b.set_side_effect(side, effect, Effect::NONE);
            removed = true;
        }
    }
    removed
}

/// The move's `basePowerCallback` (`getDamage`, before the critical hit roll), then
/// `clampIntRange(basePower, 1)` (a fraction is floored, and 0 means no damage). `hit` is
/// `move.hit`, the hit being made (1 for a single hit).
pub(super) fn base_power_callback<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
    base_power: i32,
    hit: u8,
) -> i32 {
    let hp = |s: SlotRef| {
        b.slot_mon(s)
            .map_or((0, 1), |m| (i32::from(m.hp), i32::from(m.max_hp)))
    };
    let positive_boosts = |s: SlotRef| -> i32 {
        b.state
            .slot(s)
            .boosts
            .iter()
            .filter(|&&v| v > 0)
            .map(|&v| i32::from(v))
            .sum()
    };
    let power = match mv.id {
        // Rising Voltage: `if (this.field.isTerrain('electricterrain') && target.isGrounded())
        // return move.basePower * 2;`
        moves::RISING_VOLTAGE if b.terrain() == Terrain::Electric && b.is_grounded(target) => {
            base_power * 2
        }
        // Acrobatics: `if (!pokemon.item) return move.basePower * 2;` (the held item).
        moves::ACROBATICS if b.raw_item(user).is_none() => base_power * 2,
        // Assurance: `if (target.hurtThisTurn) return move.basePower * 2;` (the HP left after
        // the latest damage this turn; 0 is falsy).
        moves::ASSURANCE
            if b.state
                .slot(target)
                .history
                .hurt_this_turn
                .is_some_and(|hp| hp != 0) =>
        {
            base_power * 2
        }
        // Payback: `if (target.newlySwitched || this.queue.willMove(target)) return
        // move.basePower; return move.basePower * 2;`
        moves::PAYBACK
            if !b.state.slot(target).history.newly_switched && b.will_move(target).is_none() =>
        {
            base_power * 2
        }
        // Avalanche, Revenge: `pokemon.attackedBy.some(p => p.source === target && p.damage > 0
        // && p.thisTurn)`.
        moves::AVALANCHE | moves::REVENGE
            if b.occupant(target)
                .is_some_and(|t| b.state.slot(user).history.damaged_by(t)) =>
        {
            base_power * 2
        }
        // Stomping Tantrum, Temper Flare: `if (pokemon.moveLastTurnResult === false)`.
        moves::STOMPING_TANTRUM | moves::TEMPER_FLARE
            if b.state.slot(user).history.move_last_turn_result == MoveResult::Failed =>
        {
            base_power * 2
        }
        // Rage Fist: `Math.min(350, 50 + 50 * pokemon.timesAttacked)`; Champions resets the
        // count on switch-out.
        moves::RAGE_FIST => {
            (50 + 50 * i32::from(b.state.slot(user).history.times_attacked)).min(350)
        }
        // Last Respects: `50 + 50 * pokemon.side.totalFainted`.
        moves::LAST_RESPECTS => 50 + 50 * i32::from(b.state.side(user.side).history.total_fainted),
        // Hex, Infernal Parade: `if (target.status || target.hasAbility('comatose'))` double.
        moves::HEX | moves::INFERNAL_PARADE => {
            let statused = b.slot_mon(target).is_some_and(|m| m.status != Status::None)
                || b.ability(target) == abilities::COMATOSE;
            if statused {
                base_power * 2
            } else {
                base_power
            }
        }
        // Heavy Slam, Heat Crash: by `pokemon.getWeight()` against `target.getWeight()` (the
        // species' weight, at least 1; no `ModifyWeight` handler is supported): 120 at 5x or
        // more, 100 at 4x, 80 at 3x, 60 at 2x, else 40.
        moves::HEAVY_SLAM | moves::HEAT_CRASH => {
            let weight = |s: SlotRef| {
                b.slot_mon(s)
                    .map_or(1, |m| i32::from(m.species.data().weight_hg.max(1)))
            };
            let (mine, theirs) = (weight(user), weight(target));
            match mine {
                w if w >= theirs * 5 => 120,
                w if w >= theirs * 4 => 100,
                w if w >= theirs * 3 => 80,
                w if w >= theirs * 2 => 60,
                _ => 40,
            }
        }
        // Triple Axel: `20 * move.hit`; Triple Kick: `10 * move.hit`.
        moves::TRIPLE_AXEL => 20 * i32::from(hit),
        moves::TRIPLE_KICK => 10 * i32::from(hit),
        // Water Shuriken: 5 more for an untransformed Greninja-Ash with Battle Bond.
        moves::WATER_SHURIKEN => {
            let ash = b
                .slot_mon(user)
                .is_some_and(|m| m.species == crate::dex::species::GRENINJA_ASH);
            if ash && b.ability(user) == abilities::BATTLE_BOND {
                base_power + 5
            } else {
                base_power
            }
        }
        // Electro Ball: `[40, 60, 80, 120, 150][min(floor(user Spe / target Spe), 4)]`
        // (`getStat('spe')`: stages and modifiers; a 0 divisor gives 0).
        moves::ELECTRO_BALL => {
            let (mine, theirs) = (b.speed_stat(user), b.speed_stat(target));
            let ratio = if theirs == 0 { 0 } else { mine / theirs };
            [40, 60, 80, 120, 150][ratio.clamp(0, 4) as usize]
        }
        // Gyro Ball: `Math.floor(25 * target Spe / user Spe) + 1`, at most 150 (1 against a
        // user at 0).
        moves::GYRO_BALL => {
            let (mine, theirs) = (b.speed_stat(user), b.speed_stat(target));
            if mine == 0 {
                1
            } else {
                (25 * theirs / mine + 1).min(150)
            }
        }
        // Eruption, Water Spout, Dragon Energy: `move.basePower * pokemon.hp / pokemon.maxhp`.
        moves::ERUPTION | moves::WATER_SPOUT | moves::DRAGON_ENERGY => {
            let (current, max) = hp(user);
            base_power * current / max
        }
        // Flail, Reversal: by `max(floor(hp * 48 / maxhp), 1)`.
        moves::FLAIL | moves::REVERSAL => {
            let (current, max) = hp(user);
            match (current * 48 / max).max(1) {
                r if r < 2 => 200,
                r if r < 5 => 150,
                r if r < 10 => 100,
                r if r < 17 => 80,
                r if r < 33 => 40,
                _ => 20,
            }
        }
        // Crush Grip, Wring Out (120), Hard Press (100): `Math.floor(Math.floor((max * (100 *
        // Math.floor(hp * 4096 / maxHP)) + 2048 - 1) / 4096) / 100) || 1` on the target's HP.
        moves::CRUSH_GRIP | moves::WRING_OUT | moves::HARD_PRESS => {
            let (current, max) = hp(target);
            let top = if mv.id == moves::HARD_PRESS { 100 } else { 120 };
            let fraction = i64::from(current) * 4096 / i64::from(max);
            (((top * 100 * fraction + 2047) / 4096) / 100).max(1) as i32
        }
        // Stored Power, Power Trip: `move.basePower + 20 * pokemon.positiveBoosts()`.
        moves::STORED_POWER | moves::POWER_TRIP => base_power + 20 * positive_boosts(user),
        // Punishment: `60 + 20 * target.positiveBoosts()`, at most 200.
        moves::PUNISHMENT => (60 + 20 * positive_boosts(target)).min(200),
        // Trump Card: by the PP left (after this use) in the slot of the move that called it
        // (`move.sourceEffect`, e.g. Sleep Talk) or its own: 200, 80, 60, 50 for 0–3, else 40
        // (40 without a slot).
        moves::TRUMP_CARD => {
            let caller = if mv.source_effect.is_none() {
                mv.id
            } else {
                mv.source_effect
            };
            let pp = b
                .slot_mon(user)
                .and_then(|m| m.moves.iter().find(|s| s.id == caller))
                .map(|s| s.pp);
            match pp {
                Some(0) => 200,
                Some(1) => 80,
                Some(2) => 60,
                Some(3) => 50,
                _ => 40,
            }
        }
        // Return: `Math.floor((pokemon.happiness * 10) / 25) || 1`; Frustration: `(255 -
        // happiness)`. The state has no happiness: every Pokémon has Showdown's default 255 (the
        // loader rejects a `happiness` field).
        moves::RETURN => 255 * 10 / 25,
        moves::FRUSTRATION => 1,
        // Bolt Beak, Fishious Rend: double `if (target.newlySwitched ||
        // this.queue.willMove(target))` (`newlySwitched`: no move action since switching in,
        // as Helping Hand reads it).
        moves::BOLT_BEAK | moves::FISHIOUS_REND => {
            if b.will_move(target).is_some() || b.state.slot(target).move_actions == 0 {
                base_power * 2
            } else {
                base_power
            }
        }
        _ => return base_power,
    };
    // `clampIntRange(basePower, 1)` (only a callback can produce a value below 1 here).
    power.max(1)
}

/// The move's `onModifyTarget` (`useMoveInner`): Metal Burst and Comeuppance target
/// `getAtSlot(lastDamagedBy.slot)`, the slot the foe that last damaged the user hit from.
pub(super) fn modify_target<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Option<SlotRef> {
    match mv.id {
        moves::METAL_BURST | moves::COMEUPPANCE => {
            b.state.slot(user).history.last_damaged_by.map(|d| d.slot)
        }
        _ => None,
    }
}

/// The move's own `onBasePower` modifier (BasePower handler priority 0, after type items and
/// terrain). Knock Off's is in `get_damage`.
pub(super) fn on_base_power<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
) -> Option<u32> {
    let target_mon = b.slot_mon(target);
    match mv.id {
        // Facade: `if (pokemon.status && pokemon.status !== 'slp') return this.chainModify(2);`
        moves::FACADE
            if b.slot_mon(user)
                .is_some_and(|m| !matches!(m.status, Status::None | Status::Sleep)) =>
        {
            Some(2 * 4096)
        }
        // Brine: `if (target.hp * 2 <= target.maxhp) return this.chainModify(2);`
        moves::BRINE if target_mon.is_some_and(|m| 2 * i32::from(m.hp) <= i32::from(m.max_hp)) => {
            Some(2 * 4096)
        }
        // Venoshock: `if (target.status === 'psn' || target.status === 'tox')` double.
        moves::VENOSHOCK
            if target_mon.is_some_and(|m| matches!(m.status, Status::Poison | Status::Toxic)) =>
        {
            Some(2 * 4096)
        }
        // Solar Beam, Solar Blade: half power in rain, sand and snow (`pokemon.effectiveWeather()`,
        // which Utility Umbrella changes).
        moves::SOLAR_BEAM | moves::SOLAR_BLADE
            if matches!(
                b.weather_for(user),
                Weather::Rain | Weather::HeavyRain | Weather::Sand | Weather::Snow
            ) =>
        {
            Some(crate::damage::MOD_HALF)
        }
        // Lash Out: `if (source.statsLoweredThisTurn) return this.chainModify(2);`
        moves::LASH_OUT if b.state.slot(user).history.stats_lowered_this_turn => Some(2 * 4096),
        // Grav Apple: `if (this.field.getPseudoWeather('gravity')) return this.chainModify(1.5);`
        moves::GRAV_APPLE if b.field_active(FieldEffect::Gravity) => Some(MOD_ONE_POINT_FIVE),
        // Psyblade: `if (this.field.isTerrain('electricterrain')) return this.chainModify(1.5);`
        moves::PSYBLADE if b.terrain() == Terrain::Electric => Some(MOD_ONE_POINT_FIVE),
        // Expanding Force: `if (this.field.isTerrain('psychicterrain') && source.isGrounded())
        // return this.chainModify(1.5);`
        moves::EXPANDING_FORCE if b.terrain() == Terrain::Psychic && b.is_grounded(user) => {
            Some(MOD_ONE_POINT_FIVE)
        }
        // Misty Explosion: the same in Misty Terrain for a grounded user.
        moves::MISTY_EXPLOSION if b.terrain() == Terrain::Misty && b.is_grounded(user) => {
            Some(MOD_ONE_POINT_FIVE)
        }
        _ => None,
    }
}

/// BasePower handlers of the user's volatiles (`condition.onBasePower`): Helping Hand
/// (priority 10) `chainModify(this.effectState.multiplier)`, 1.5 per application.
pub(super) fn volatile_base_power<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
) -> Vec<Handler> {
    let mut out = Vec::new();
    let helping_hand = b.volatile(user, Volatile::HelpingHand);
    if helping_hand.active {
        let multiplier = 1.5f64.powi(i32::from(helping_hand.counter));
        let modifier = (multiplier * 4096.0).trunc() as u32;
        let priority = super::super::abilities::priority(
            moves::HELPING_HAND.data().event_orders,
            "condition.onBasePowerPriority",
        );
        out.push(Handler::of(b, user, priority, SUB_CONDITION, modifier));
    }
    out
}

/// The protect family's volatiles, with whether each also blocks status moves (the `blockStatus`
/// argument of `checkMoveBypassesProtect`: Protect / Detect, Spiky Shield and Baneful Bunker do;
/// King's Shield, Obstruct, Silk Trap and Burning Bulwark pass `false`).
const PROTECTIONS: [(Volatile, bool); 7] = [
    (Volatile::Protect, true),
    (Volatile::SpikyShield, true),
    (Volatile::BanefulBunker, true),
    (Volatile::KingsShield, false),
    (Volatile::Obstruct, false),
    (Volatile::SilkTrap, false),
    (Volatile::BurningBulwark, false),
];

/// The protect family's `condition.onTryHit` (priority 3) on `target`. A move with the `protect`
/// flag (a status move only if the shield blocks those; `HitProtect` has no handler) is stopped
/// (`NOT_FAIL`); a locked move on its first turn (`lockedmove` duration 2: "Outrage counter is
/// reset") loses its volatile without `onEnd`; and a contact move (`checkMoveMakesContact`: not
/// through Protective Pads) is punished: Spiky Shield `this.damage(source.baseMaxhp / 8)`,
/// Baneful Bunker / Burning Bulwark `source.trySetStatus('psn' / 'brn', target)`, King's Shield,
/// Obstruct, Silk Trap `this.boost({atk: -1} / {def: -2} / {spe: -1}, source, target, move)`.
/// The shields' `condition.onHit` only acts on Z- and Max Moves (off in Champions). Returns
/// whether the move is blocked.
pub(super) fn protect_try_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    for (volatile, blocks_status) in PROTECTIONS {
        if !b.volatile(target, volatile).active {
            continue;
        }
        let bypassed = (mv.data.category == MoveCategory::Status && !blocks_status)
            || !mv.data.flags.contains(MoveFlags::PROTECT);
        if bypassed {
            continue;
        }
        let locked = b.volatile(user, Volatile::LockedMove);
        if locked.active && locked.duration == 2 {
            b.delete_volatile(user, Volatile::LockedMove);
        }
        let contact = super::item_events::makes_contact(b, user, mv.data)
            && b.item(user) != items::PROTECTIVE_PADS;
        if contact {
            let mut drop = NO_BOOSTS;
            let effect = match volatile {
                Volatile::SpikyShield => {
                    let max_hp = b.slot_mon(user).map_or(0, |m| m.max_hp);
                    b.damage(user, f64::from(max_hp) / 8.0, DamageSource::Indirect);
                    None
                }
                Volatile::BanefulBunker => {
                    b.try_set_status_from(user, Status::Poison, Some(target));
                    None
                }
                Volatile::BurningBulwark => {
                    b.try_set_status_from(user, Status::Burn, Some(target));
                    None
                }
                Volatile::KingsShield => {
                    drop[0] = -1;
                    Some(moves::KINGS_SHIELD)
                }
                Volatile::Obstruct => {
                    drop[1] = -2;
                    Some(moves::OBSTRUCT)
                }
                Volatile::SilkTrap => {
                    drop[4] = -1;
                    Some(moves::SILK_TRAP)
                }
                _ => None,
            };
            if let Some(shield) = effect {
                b.boost_by(user, &drop, Some(target), BoostEffect::Move(shield));
            }
        }
        return true;
    }
    false
}

/// The side conditions' `onTryHit` (priority 3, after the target's protect-family volatiles):
/// Crafty Shield stops a status move unless it targets `self` or `all` (from anyone, the side's
/// own Pokémon included); Mat Block stops a move that does not target `self` and that Protect
/// without `blockStatus` would (`checkMoveBypassesProtect(move, source, target, false)`: a
/// damaging move with the `protect` flag; `HitProtect` has no handler), resetting a locked move
/// on its first turn as Protect does. Both return `NOT_FAIL`. Returns whether the move is
/// stopped.
pub(super) fn side_guard_try_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    let side = target.side;
    let status = mv.data.category == MoveCategory::Status;
    if b.side_effect_active(side, SideEffect::CraftyShield)
        && status
        && !matches!(mv.target, MoveTarget::User | MoveTarget::All)
    {
        return true;
    }
    if b.side_effect_active(side, SideEffect::MatBlock)
        && mv.target != MoveTarget::User
        && !status
        && mv.data.flags.contains(MoveFlags::PROTECT)
    {
        let locked = b.volatile(user, Volatile::LockedMove);
        if locked.active && locked.duration == 2 {
            b.delete_volatile(user, Volatile::LockedMove);
        }
        return true;
    }
    false
}

/// `hitStepBreakProtect` for one target of a `breaksProtect` move (Feint): its protect-family
/// volatiles are removed (none has `onEnd`) and its side loses Crafty Shield, Mat Block, Quick
/// Guard and Wide Guard; if anything was broken, its `stall` counter is deleted (gen 6+).
pub(super) fn break_protect<const N: usize>(b: &mut Battle<'_, N>, target: SlotRef) {
    let mut broke = false;
    for (volatile, _) in PROTECTIONS {
        broke |= b.remove_volatile(target, volatile);
    }
    broke |= remove_side_effects(
        b,
        target.side,
        &[
            SideEffect::CraftyShield,
            SideEffect::MatBlock,
            SideEffect::QuickGuard,
            SideEffect::WideGuard,
        ],
    );
    if broke {
        b.delete_volatile(target, Volatile::Stall);
    }
}

/// The `Accuracy` event's handlers that make a move hit `target` whatever its accuracy: Glaive
/// Rush's drawback (`condition.onAccuracy() { return true; }`).
pub(super) fn always_hit<const N: usize>(b: &Battle<'_, N>, target: SlotRef) -> bool {
    b.volatile(target, Volatile::GlaiveRush).active
}

/// ModifyDamage handlers of the target's volatiles (`onSourceModifyDamage`): Glaive Rush's
/// drawback `chainModify(2)` (priority 0).
pub(super) fn volatile_modify_damage<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    mv: &ActiveMove,
) -> Vec<Handler> {
    let mut out = Vec::new();
    if b.volatile(target, Volatile::GlaiveRush).active {
        out.push(Handler::of(b, target, 0, SUB_CONDITION, 2 * 4096));
    }
    // Fly (Gust, Twister), Dig (Earthquake, Magnitude) and Dive (Surf, Whirlpool) take double
    // damage from the moves that reach them (`onSourceModifyDamage`).
    let doubled = (b.volatile(target, Volatile::Fly).active
        && [moves::GUST, moves::TWISTER].contains(&mv.id))
        || (b.volatile(target, Volatile::Dig).active
            && [moves::EARTHQUAKE, moves::MAGNITUDE].contains(&mv.id))
        || (b.volatile(target, Volatile::Dive).active
            && [moves::SURF, moves::WHIRLPOOL].contains(&mv.id));
    if doubled {
        out.push(Handler::of(b, target, 0, SUB_CONDITION, 2 * 4096));
    }
    out
}

/// BasePower handlers of the target's volatiles: Bounce's `onSourceBasePower` doubles Gust and
/// Twister.
pub(super) fn target_volatile_base_power<const N: usize>(
    b: &Battle<'_, N>,
    target: SlotRef,
    mv: &ActiveMove,
) -> Vec<Handler> {
    let mut out = Vec::new();
    if b.volatile(target, Volatile::Bounce).active && [moves::GUST, moves::TWISTER].contains(&mv.id)
    {
        out.push(Handler::of(b, target, 0, SUB_CONDITION, 2 * 4096));
    }
    out
}

/// The two-turn moves' `onTryMove` (`singleEvent('TryMove')`, F9): on the second turn the move's
/// own volatile is removed and the move goes on; otherwise this is the charging turn: Meteor
/// Beam and Electro Shot raise SpA first; Solar Beam and Solar Blade in sun and Electro Shot in
/// rain (`effectiveWeather`) skip the charge, as does Power Herb (`ChargeMove`: `useItem`);
/// else `twoturnmove` (duration 2, the move, the chosen target location) and the move's own
/// volatile start, PrepareHit runs (Protean), and the move stops (`return null`). `false` =
/// the move stops here.
pub(super) fn charge_try_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    let Some(own) = super::super::conditions::charge_volatile(mv.id) else {
        return true;
    };
    if b.remove_volatile(user, own) {
        return true;
    }
    if mv.id == moves::METEOR_BEAM || mv.id == moves::ELECTRO_SHOT {
        let mut up = NO_BOOSTS;
        up[2] = 1;
        b.boost_by(user, &up, Some(user), BoostEffect::Move(mv.id));
    }
    let weather = b.weather_for(user);
    let skip = match mv.id {
        i if i == moves::SOLAR_BEAM || i == moves::SOLAR_BLADE => {
            matches!(weather, Weather::Sun | Weather::HarshSun)
        }
        i if i == moves::ELECTRO_SHOT => matches!(weather, Weather::Rain | Weather::HeavyRain),
        _ => false,
    };
    if skip {
        return true;
    }
    if b.item(user) == items::POWER_HERB && b.use_item(user) {
        return true;
    }
    b.set_volatile_state(
        user,
        Volatile::TwoTurnMove,
        VolatileState {
            active: true,
            duration: Volatile::TwoTurnMove.initial_duration(),
            mv: mv.id,
            counter: super::super::lock::encode_target_loc(mv.target_loc),
            ..VolatileState::NONE
        },
    );
    b.add_volatile(user, own);
    super::prepare_hit_ability(b, user, mv);
    false
}

/// The type Double Shock and Burn Up need and spend (`None` for any other move).
fn spent_type(id: MoveId) -> Option<Type> {
    if id == moves::DOUBLE_SHOCK {
        Some(Type::Electric)
    } else if id == moves::BURN_UP {
        Some(Type::Fire)
    } else {
        None
    }
}

/// The move's own `onTryMove` of moves that stop with `null` (not a failure: `useMove` leaves
/// `moveThisTurnResult` `null`). Double Shock and Burn Up: `if (pokemon.hasType('Electric' /
/// 'Fire')) return;`, otherwise `-fail` and `return null`. Shell Trap: `if
/// (!pokemon.volatiles['shelltrap']?.gotHit) return null;`. `false` = the move stops here.
pub(super) fn null_try_move<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    if mv.id == moves::SHELL_TRAP {
        return b.volatile(user, Volatile::ShellTrap).counter != 0;
    }
    spent_type(mv.id).is_none_or(|t| b.has_type(user, t))
}

/// Burn Up used by a Pokémon without the Fire type does not thaw it: the `frz` status's
/// `onBeforeMove` skips a `defrost` move only `if (move.flags['defrost'] && !(move.id ===
/// 'burnup' && !pokemon.hasType('Fire')))` (Champions keeps the exception).
pub(super) fn thaws_user<const N: usize>(b: &Battle<'_, N>, user: SlotRef, id: MoveId) -> bool {
    id.data().flags.contains(MoveFlags::DEFROST)
        && !(id == moves::BURN_UP && !b.has_type(user, Type::Fire))
}

/// The move's `self.onHit` (`selfDrops` → `moveHit(source, source, move, move.self)`, once per
/// target the move did not fail on). Double Shock and Burn Up:
/// `pokemon.setType(pokemon.getTypes(true).map(type => type === "Electric" / "Fire" ? "???" :
/// type))` (Arceus and Silvally keep their types: `setType` refuses).
pub(super) fn self_on_hit<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) {
    let Some(spent) = spent_type(mv.id) else {
        return;
    };
    let Some(mon) = b.slot_mon(user) else {
        return;
    };
    if [493, 773].contains(&mon.species.data().num) {
        return;
    }
    let types = mon
        .types
        .map(|t| if t == spent { Type::Unknown } else { t });
    set_types(b, user, types);
}

/// `hitStepInvulnerabilityEvent` for one target: a semi-invulnerable target is not hit unless
/// the move is one its state lets through, No Guard (`onAnyInvulnerability`, priority 1) is the
/// user's or the target's ability, or the move is Toxic from a Poison type.
pub(super) fn invulnerable<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    let Some(state) = super::super::conditions::semi_invulnerable(b, target) else {
        return false;
    };
    if mv.id == moves::TOXIC && b.has_type(user, Type::Poison) {
        return false;
    }
    if b.ability(user) == abilities::NO_GUARD || b.ability(target) == abilities::NO_GUARD {
        return false;
    }
    let passes: &[MoveId] = match state {
        Volatile::Fly | Volatile::Bounce => &[
            moves::GUST,
            moves::TWISTER,
            moves::SKY_UPPERCUT,
            moves::THUNDER,
            moves::HURRICANE,
            moves::SMACK_DOWN,
            moves::THOUSAND_ARROWS,
        ],
        Volatile::Dig => &[moves::EARTHQUAKE, moves::MAGNITUDE],
        Volatile::Dive => &[moves::SURF, moves::WHIRLPOOL],
        _ => &[],
    };
    !passes.contains(&mv.id)
}

/// Showdown `this.dex.getEffectiveness(attacking, defending)` for one defending type:
/// 1 super effective, -1 resisted, 0 otherwise (immunity is checked separately).
pub(super) fn type_effectiveness(attacking: Type, defending: Type) -> i32 {
    match attacking.against(defending) {
        TypeRelation::Super => 1,
        TypeRelation::Resist => -1,
        _ => 0,
    }
}

/// The move's `onEffectiveness` for one defending type (`runEffectiveness`), given the
/// chart's `type_mod` for it.
pub(super) fn on_effectiveness(id: MoveId, defending: Type, type_mod: i32) -> i32 {
    match id {
        // Freeze-Dry: `if (type === 'Water') return 1;`
        moves::FREEZE_DRY if defending == Type::Water => 1,
        // Flying Press: `return typeMod + this.dex.getEffectiveness('Flying', type);`
        moves::FLYING_PRESS => type_mod + type_effectiveness(Type::Flying, defending),
        _ => type_mod,
    }
}

/// What an `onHit` handler returned: a truthy value, `false`, or `NOT_FAIL` (the move does
/// not count as failed, but the target takes no further effects).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HitResult {
    Success,
    Failure,
    NotFail,
}

/// The move's primary `onHit` on one target (`runMoveEffects`, after the data effects).
/// `None` = the move has no `onHit`.
pub(super) fn on_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
) -> Result<Option<HitResult>, TurnError> {
    let result = match mv.id {
        // Parting Shot: `const success = this.boost({atk: -1, spa: -1}, target, source); if
        // (!success && !target.hasAbility('mirrorarmor')) delete move.selfSwitch;` (the
        // callback returns nothing: neither success nor failure).
        moves::PARTING_SHOT => {
            let mut drop = NO_BOOSTS;
            drop[0] = -1;
            drop[2] = -1;
            let success = b.boost_by(target, &drop, Some(user), BoostEffect::Move(mv.id));
            if !success && b.ability(target) != abilities::MIRROR_ARMOR {
                b.move_self_switch = false;
            }
            HitResult::NotFail
        }
        // Morning Sun, Moonlight, Synthesis: `this.modify(pokemon.maxhp, factor)`, factor
        // 0.667 in sun, 0.25 in rain, sand, hail and snow, 0.5 otherwise (effective weather).
        moves::MORNING_SUN | moves::MOONLIGHT | moves::SYNTHESIS => {
            let modifier = match effective_weather(b, user, target)? {
                Weather::Sun | Weather::HarshSun => 2732,
                Weather::Rain | Weather::HeavyRain | Weather::Sand | Weather::Snow => 1024,
                _ => 2048,
            };
            weather_heal(b, target, modifier)
        }
        // Shore Up: 0.667 in sandstorm (`field.isWeather`, no Utility Umbrella), 0.5 otherwise.
        moves::SHORE_UP => {
            let modifier = if b.effective_weather() == Weather::Sand {
                2732
            } else {
                2048
            };
            weather_heal(b, target, modifier)
        }
        // Clear Smog: `target.clearBoosts()`.
        moves::CLEAR_SMOG => {
            clear_boosts(b, target);
            HitResult::Success
        }
        // Topsy-Turvy: every stage negated (`target.boosts[i] = -target.boosts[i]`); fails
        // when there is none.
        moves::TOPSY_TURVY => {
            let boosts = b.state.slot(target).boosts;
            if boosts.iter().all(|&v| v == 0) {
                HitResult::Failure
            } else {
                set_boosts(b, target, boosts.map(|v| -v));
                HitResult::Success
            }
        }
        // Power Swap (atk, spa), Guard Swap (def, spd), Heart Swap (every stage): the user and
        // the target exchange those stages (`setBoost`).
        moves::POWER_SWAP | moves::GUARD_SWAP | moves::HEART_SWAP => {
            let stats: &[usize] = match mv.id {
                moves::POWER_SWAP => &[0, 2],
                moves::GUARD_SWAP => &[1, 3],
                _ => &[0, 1, 2, 3, 4, 5, 6],
            };
            let (before_user, before_target) =
                (b.state.slot(user).boosts, b.state.slot(target).boosts);
            let (mut to_user, mut to_target) = (before_user, before_target);
            for &stat in stats {
                to_user[stat] = before_target[stat];
                to_target[stat] = before_user[stat];
            }
            set_boosts(b, user, to_user);
            set_boosts(b, target, to_target);
            HitResult::Success
        }
        // Belly Drum: fails at half HP or less, at +6 Attack or with 1 max HP; otherwise
        // `this.directDamage(target.maxhp / 2)` and `this.boost({atk: 12}, target)` (source: the
        // user itself, so Contrary turns it into -12 and nothing blocks it).
        moves::BELLY_DRUM => {
            let Some(mon) = b.slot_mon(target) else {
                return Ok(Some(HitResult::Failure));
            };
            let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
            if hp * 2 <= max_hp || b.state.slot(target).boosts[0] >= 6 || max_hp == 1 {
                HitResult::Failure
            } else {
                b.direct_damage(target, max_hp / 2);
                let mut up = NO_BOOSTS;
                up[0] = 12;
                b.boost_by(target, &up, Some(user), BoostEffect::Move(mv.id));
                HitResult::Success
            }
        }
        // Clangorous Soul: `this.directDamage(pokemon.maxhp * 33 / 100)`; Fillet Away:
        // `this.directDamage(pokemon.maxhp / 2)` (the boosts came in `onTryHit`).
        moves::CLANGOROUS_SOUL | moves::FILLET_AWAY => {
            let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
            // `clampIntRange(damage, 1)`: a nonzero fraction is at least 1.
            let amount = if mv.id == moves::CLANGOROUS_SOUL {
                max_hp * 33 / 100
            } else {
                max_hp / 2
            };
            b.direct_damage(target, amount.max(1));
            HitResult::Success
        }
        moves::HEAL_BELL | moves::AROMATHERAPY => party_cure(b, user, target.side, mv.id),
        // Refresh: `if (['', 'slp', 'frz'].includes(pokemon.status)) return false;` then cure.
        moves::REFRESH => match b.alive(target) {
            Some(p)
                if matches!(
                    b.mon(p).status,
                    Status::Burn | Status::Paralyze | Status::Poison | Status::Toxic
                ) =>
            {
                b.cure_status(p);
                HitResult::Success
            }
            _ => HitResult::Failure,
        },
        // Purify: `if (!target.cureStatus()) return this.NOT_FAIL;` then the user heals
        // `Math.ceil(source.maxhp * 0.5)`.
        moves::PURIFY => {
            if !cure_status(b, target) {
                HitResult::NotFail
            } else {
                let max_hp = b.slot_mon(user).map_or(0, |m| i32::from(m.max_hp));
                b.heal(user, f64::from((max_hp + 1) / 2));
                HitResult::Success
            }
        }
        // Take Heart: `const success = !!this.boost({spa: 1, spd: 1}); return
        // pokemon.cureStatus() || success;`
        moves::TAKE_HEART => {
            let mut up = NO_BOOSTS;
            up[2] = 1;
            up[3] = 1;
            let boosted = b.boost_by(target, &up, Some(user), BoostEffect::Move(mv.id));
            success(cure_status(b, target) || boosted)
        }
        // Jungle Healing, Lunar Blessing (each ally and the user): `const success =
        // !!this.heal(this.modify(pokemon.maxhp, 0.25)); return pokemon.cureStatus() || success;`
        moves::JUNGLE_HEALING | moves::LUNAR_BLESSING => {
            let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
            let healed = b.heal(target, f64::from(modify(max_hp, 1024))) > 0;
            success(cure_status(b, target) || healed)
        }
        // Floral Healing: `this.modify(target.baseMaxhp, 0.667)` in Grassy Terrain, otherwise
        // `Math.ceil(target.baseMaxhp * 0.5)`; `NOT_FAIL` when nothing is healed.
        moves::FLORAL_HEALING => {
            let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
            let amount = if b.terrain() == Terrain::Grassy {
                modify(max_hp, 2732)
            } else {
                (max_hp + 1) / 2
            };
            if b.heal(target, f64::from(amount)) > 0 {
                HitResult::Success
            } else {
                HitResult::NotFail
            }
        }
        // Rest: `target.setStatus('slp', source, move)` (an existing status is replaced; the
        // SetStatus handlers still block it: terrains, Sweet Veil, Leaf Guard, Purifying Salt;
        // Safeguard ignores the user's own effect), then `statusState.time = 3` and
        // `this.heal(target.maxhp)`. The sleep condition's own draw of 2 or 3 turns is overwritten,
        // so it is not drawn here; AfterSetStatus: Synchronize ignores sleep, Lum Berry is eaten
        // at once (the 3 turns then land on no status).
        moves::REST => {
            let Some(pokemon) = b.alive(target) else {
                return Ok(Some(HitResult::Failure));
            };
            let blocked = b.set_status_blocked(target, Status::Sleep)
                || super::super::abilities::blocks_status(
                    b.ability_unless_broken(target),
                    Status::Sleep,
                );
            if blocked {
                HitResult::Failure
            } else {
                let old = b.mon(pokemon).status;
                b.apply(Instruction::ChangeStatus {
                    target: pokemon,
                    old,
                    new: Status::Sleep,
                });
                b.set_status_turns(pokemon, 3);
                super::super::update::after_set_status(b, target);
                let max_hp = b.mon(pokemon).max_hp;
                b.heal(target, f64::from(max_hp));
                HitResult::Success
            }
        }
        // Psych Up: the user takes the target's stages (`source.boosts[i] = target.boosts[i]`, no
        // boost events), then loses its critical-hit volatiles and copies the target's (of
        // Dragon Cheer, Focus Energy, G-Max Chi Strike, Laser Focus only Focus Energy exists).
        moves::PSYCH_UP => {
            let boosts = b.state.slot(target).boosts;
            set_boosts(b, user, boosts);
            b.remove_volatile(user, Volatile::FocusEnergy);
            if b.volatile(target, Volatile::FocusEnergy).active {
                b.add_volatile(user, Volatile::FocusEnergy);
            }
            HitResult::Success
        }
        // Speed Swap: the stored Speed stats trade places (`storedStats.spe`); `setSpecies`
        // recalculates them when a Pokémon leaves the field (`Battle::clear_volatile`).
        moves::SPEED_SWAP => {
            let (Some(p), Some(q)) = (b.occupant(user), b.occupant(target)) else {
                return Ok(Some(HitResult::Failure));
            };
            let (mine, theirs) = (b.mon(p).forme(), b.mon(q).forme());
            for (pokemon, old, speed) in [(p, mine, theirs.stats[4]), (q, theirs, mine.stats[4])] {
                let mut new = old;
                new.stats[4] = speed;
                if new != old {
                    b.apply(Instruction::SetForme {
                        target: pokemon,
                        old,
                        new,
                    });
                }
            }
            HitResult::Success
        }
        // Strength Sap: fails at -6 Attack; otherwise the target's Attack with its stages but no
        // modifiers (`getStat('atk', false, true)`), then `this.boost({atk: -1}, target, source)`
        // and the user heals that much; `!!(healed || boosted)`.
        moves::STRENGTH_SAP => {
            let Some(mon) = b.slot_mon(target) else {
                return Ok(Some(HitResult::Failure));
            };
            let stage = b.state.slot(target).boosts[0];
            if stage == -6 {
                HitResult::Failure
            } else {
                let attack = boosted_stat(i32::from(mon.stats[0]), stage);
                let mut drop = NO_BOOSTS;
                drop[0] = -1;
                let boosted = b.boost_by(target, &drop, Some(user), BoostEffect::Move(mv.id));
                let healed = b.heal_rooted(user, f64::from(attack)) > 0;
                success(healed || boosted)
            }
        }
        // Pain Split: both take the average of their HP (`Math.floor((targetHP + pokemon.hp) / 2)
        // || 1`) through `sethp` (capped at the max HP, no Damage/Heal events).
        moves::PAIN_SPLIT => {
            let hp = |b: &Battle<'_, N>, s: SlotRef| b.slot_mon(s).map_or(0, |m| i32::from(m.hp));
            let average = ((hp(b, target) + hp(b, user)) / 2).max(1);
            set_hp(b, target, average);
            set_hp(b, user, average);
            HitResult::Success
        }
        // Spite: the target's last move (none, or Struggle, which has no slot: fails) loses up to
        // 4 PP (`deductPP(move.id, 4)`; fails when it has none left).
        moves::SPITE => {
            let last = b.state.slot(target).last_move;
            let Some(pokemon) = b.alive(target) else {
                return Ok(Some(HitResult::Failure));
            };
            let index = b.mon(pokemon).moves.iter().position(|m| m.id == last);
            match index {
                Some(i) if !last.is_none() && b.mon(pokemon).moves[i].pp > 0 => {
                    let old = b.mon(pokemon).moves[i].pp;
                    b.apply(Instruction::SetPp {
                        target: pokemon,
                        move_index: i as u8,
                        old,
                        new: old.saturating_sub(4),
                    });
                    HitResult::Success
                }
                _ => HitResult::Failure,
            }
        }
        // Reflect Type: fails for Arceus and Silvally users; the user takes the target's types
        // (`getTypes(true)`: without an added type, which the engine never has; Roost's filter
        // is already in the stored types) without `???` (`filter(type => type !== '???')`),
        // failing if none is left; `setType` then clears the user's added type.
        moves::REFLECT_TYPE => {
            let Some(mon) = b.slot_mon(user) else {
                return Ok(Some(HitResult::Failure));
            };
            if [493, 773].contains(&mon.species.data().num) {
                HitResult::Failure
            } else {
                let types = b.slot_mon(target).map_or([Type::None; 2], |m| m.types);
                let mut kept = types
                    .into_iter()
                    .filter(|&t| t != Type::Unknown && t != Type::None);
                let types = [
                    kept.next().unwrap_or(Type::None),
                    kept.next().unwrap_or(Type::None),
                ];
                if types[0] == Type::None {
                    HitResult::Failure
                } else {
                    set_types(b, user, types);
                    HitResult::Success
                }
            }
        }
        // Soak: fails (`null`) on a pure Water type (`getTypes().join() === 'Water'`) or an
        // Arceus / Silvally (`setType` refuses); otherwise the target becomes pure Water.
        moves::SOAK => {
            let Some(mon) = b.slot_mon(target) else {
                return Ok(Some(HitResult::Failure));
            };
            let water = [Type::Water, Type::None];
            if mon.types == water || [493, 773].contains(&mon.species.data().num) {
                HitResult::Failure
            } else {
                set_types(b, target, water);
                HitResult::Success
            }
        }
        // Bug Bite, Pluck: a user with HP takes the target's berry (`takeItem`, even from a
        // target the hit knocked out) and eats it itself (`singleEvent('Eat', item, ..., source,
        // source, move)`: the berry's `onEat` on the user; no `TryEatItem`, no `lastItem`).
        // Resist berries and berries without handlers have an empty `onEat`. Returns nothing.
        moves::BUG_BITE | moves::PLUCK => {
            let item = b.item(target);
            if let Some(eater) = b.alive(user).filter(|_| item.data().is_berry) {
                let empty = item.data().handlers.is_empty()
                    || super::super::items::resist_berry(item).is_some();
                if b.take_item(target)
                    && !empty
                    && !super::super::update::berry_on_eat(b, user, eater, item)
                {
                    return Err(b.unsupported(format!(
                        "{} eating {}",
                        mv.data.name,
                        item.data().name
                    )));
                }
            }
            HitResult::Success
        }
        // Incinerate: `if ((item.isBerry || item.isGem) && pokemon.takeItem(source))` (a target
        // the hit knocked out too). Corrosive Gas: `target.takeItem(source)` (a failure only
        // logs). Both return nothing.
        moves::INCINERATE | moves::CORROSIVE_GAS => {
            let data = b.item(target).data();
            if mv.id == moves::CORROSIVE_GAS || data.is_berry || data.is_gem {
                b.take_item(target);
            }
            HitResult::Success
        }
        // Recycle: fails with an item or without a `lastItem`; otherwise `lastItem` goes back to
        // being held (`setItem`: its `Start` event runs; an item whose `onStart` acts is refused).
        moves::RECYCLE => {
            let Some(pokemon) = b.alive(target) else {
                return Ok(Some(HitResult::Failure));
            };
            let (item, last) = (b.mon(pokemon).item, b.mon(pokemon).last_item);
            if !item.is_none() || last.is_none() {
                HitResult::Failure
            } else {
                if last.data().handlers.contains(&"onStart")
                    && !super::super::items::inert_start(last)
                {
                    return Err(b.unsupported(format!(
                        "Recycle restoring {} (its onStart)",
                        last.data().name
                    )));
                }
                b.apply(Instruction::SetLastItem {
                    target: pokemon,
                    old: last,
                    new: ItemId::NONE,
                });
                b.apply(Instruction::SetItem {
                    target: pokemon,
                    old: ItemId::NONE,
                    new: last,
                });
                HitResult::Success
            }
        }
        // Substitute: `this.directDamage(target.maxhp / 4)` after the volatile started (returns
        // nothing).
        moves::SUBSTITUTE => {
            let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
            b.direct_damage(target, (max_hp / 4).max(1));
            return Ok(None);
        }
        // Steel Roller: `this.field.clearTerrain();` (returns nothing: no effect on success).
        moves::STEEL_ROLLER => {
            super::clear_terrain(b);
            return Ok(None);
        }
        moves::TRICK | moves::SWITCHEROO => trick(b, user, target)?,
        moves::INSTRUCT => instruct(b, target)?,
        // Ally Switch (on its user): `NOT_FAIL` outside doubles and triples, or when the other
        // position's Pokémon has fainted; otherwise `swapPosition` (returns nothing). Triples'
        // positions are not supported.
        moves::ALLY_SWITCH => {
            if N > 2 {
                return Err(b.unsupported("Ally Switch in triples"));
            }
            let other = SlotRef {
                side: target.side,
                slot: 1 - target.slot.min(1),
            };
            if N != 2 || b.alive(other).is_none() {
                HitResult::NotFail
            } else {
                swap_positions(b, target, other)?;
                return Ok(None);
            }
        }
        // Sleep Talk: one of the user's moves Sleep Talk may call, uniformly at random, used
        // through `useMove` (`moves::call_move`); fails without one. It returns nothing.
        moves::SLEEP_TALK => {
            let known = b
                .slot_mon(user)
                .map_or([MoveId::NONE; 4], |m| m.moves.map(|s| s.id));
            let callable: Vec<MoveId> = known
                .into_iter()
                .filter(|&id| sleep_talk_calls(id))
                .collect();
            if callable.is_empty() {
                HitResult::Failure
            } else {
                let called = callable[b.rng.uniform(callable.len())];
                super::call_move(b, user, mv, called)?;
                HitResult::NotFail
            }
        }
        // Defog: `if (!target.volatiles['substitute'] || move.infiltrates)` `this.boost({evasion:
        // -1})` on the target (success if a stage changed); the target's side loses its screens,
        // Safeguard and Mist (no success) and its hazards, the user's side its hazards
        // (success); then `this.field.clearTerrain()`.
        moves::DEFOG => {
            let mut drop = NO_BOOSTS;
            drop[6] = -1;
            let mut success = !b.has_substitute(target)
                && b.boost_by(target, &drop, Some(user), BoostEffect::Move(mv.id));
            remove_side_effects(
                b,
                target.side,
                &[
                    SideEffect::Reflect,
                    SideEffect::LightScreen,
                    SideEffect::AuroraVeil,
                    SideEffect::Safeguard,
                    SideEffect::Mist,
                ],
            );
            success |= remove_side_effects(b, target.side, &HAZARDS);
            success |= remove_side_effects(b, user.side, &HAZARDS);
            super::clear_terrain(b);
            if success {
                HitResult::Success
            } else {
                HitResult::Failure
            }
        }
        // Mean Look, Block, Spider Web: `return target.addVolatile('trapped', source, move,
        // 'trapper');`
        moves::MEAN_LOOK | moves::BLOCK | moves::SPIDER_WEB => {
            success(super::super::conditions::add_trap(b, target, user))
        }
        // Heal Pulse: `this.heal(this.modify(target.baseMaxhp, 0.75))` from a Mega Launcher user
        // (`source.hasAbility`: its own ability), otherwise `this.heal(Math.ceil(target.baseMaxhp
        // * 0.5))`; `NOT_FAIL` when nothing is healed (full HP). Heal Block is not supported.
        moves::HEAL_PULSE => {
            let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
            let amount = if b.ability(user) == abilities::MEGA_LAUNCHER {
                modify(max_hp, 3072)
            } else {
                (max_hp + 1) / 2
            };
            if b.heal(target, f64::from(amount)) > 0 {
                HitResult::Success
            } else {
                HitResult::NotFail
            }
        }
        // Pollen Puff: an ally is healed `Math.floor(target.baseMaxhp * 0.5)`; `NOT_FAIL` if
        // nothing is healed. A foe gets nothing more (`undefined`).
        moves::POLLEN_PUFF => {
            if target.side != user.side {
                return Ok(None);
            }
            let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
            if b.heal(target, f64::from(max_hp / 2)) > 0 {
                HitResult::Success
            } else {
                HitResult::NotFail
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

/// Instruct `onHit`: the target repeats its last move right away. It fails without a last move,
/// or when that move has `failinstruct`, `charge` or `recharge`, is a Z- or Max move, or its
/// slot has no PP; otherwise a move action for it goes to the front of the queue
/// (`queue.prioritizeAction(queue.resolveAction(...))`: order 3) and runs as a full `runMove`
/// (PP, BeforeMove, `lastMove`). Showdown aims it at `target.lastMoveTargetLoc`, which the
/// state does not keep, so a last move with a chosen target (`normal`, `any`, ...) is
/// unsupported, as are a last move the target does not know (Struggle) and a Quick Claw
/// holder (`resolveAction` draws its fractional priority again).
fn instruct<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
) -> Result<HitResult, TurnError> {
    let Some(pokemon) = b.alive(target) else {
        return Ok(HitResult::Failure);
    };
    let last = b.state.slot(target).last_move;
    if last.is_none() {
        return Ok(HitResult::Failure);
    }
    let data = last.data();
    let Some(index) = b.mon(pokemon).moves.iter().position(|m| m.id == last) else {
        return Err(b.unsupported(format!(
            "Instruct repeating {}, which the target does not know",
            data.name
        )));
    };
    let blocked = data.flags.contains(MoveFlags::FAILINSTRUCT)
        || data.flags.contains(MoveFlags::CHARGE)
        || data.flags.contains(MoveFlags::RECHARGE)
        || data.is_z
        || data.is_max
        || b.mon(pokemon).moves[index].pp == 0;
    if blocked {
        return Ok(HitResult::Failure);
    }
    if super::takes_target(N, data.target) {
        return Err(b.unsupported(format!(
            "Instruct repeating {} (its lastMoveTargetLoc is not kept)",
            data.name
        )));
    }
    if b.item(target) == items::QUICK_CLAW {
        return Err(b.unsupported("Instruct on a Quick Claw holder"));
    }
    let fractional_tenths = super::super::items::fractional_priority_tenths(b.state, target);
    b.queue.push(Action {
        slot: target,
        pokemon,
        kind: ActionKind::Move {
            index: index as u8,
            target: 0,
            fractional_tenths,
        },
        order: Some(3),
    });
    Ok(HitResult::Success)
}

/// The move's own `onPrepareHit` handlers other than the stall moves' (`trySpreadMoveHit`, before
/// the user's ability's PrepareHit). Ally Switch: `return pokemon.addVolatile('allyswitch');`
/// (its `onRestart` draws the 1/`counter` chance). `false` = the move fails.
pub(super) fn on_prepare_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    match mv.id {
        moves::ALLY_SWITCH => b.add_volatile(user, Volatile::AllySwitch),
        _ => true,
    }
}

/// Where `pokemon`, which started its move in `user`, stands now: Ally Switch moves it during the
/// move, and Showdown's later steps follow the Pokémon. A user that fainted keeps its old slot.
pub(super) fn current_slot<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    pokemon: PokemonRef,
) -> SlotRef {
    Battle::<N>::slots(user.side)
        .find(|&s| b.occupant(s) == Some(pokemon))
        .unwrap_or(user)
}

/// Showdown `swapPosition(pokemon, newPosition)` for the Pokémon in `from` and its ally in `to`
/// (both standing): everything Showdown keeps on the Pokémon (boosts, volatiles, `lastMove`,
/// damage history, switch flag, substitute) goes with it (the two slots trade places through
/// [`super::super::diff::slot_changes`]); slot conditions stay with the position. Their queued
/// actions follow them (Showdown's actions hold the Pokémon; a target location is read from the
/// user's position when the move runs), as does the move in progress. Then `runEvent('Swap')`
/// for the ally at its new position and the user at its own: Healing Wish's `onSwap` heals one
/// that needs it. Snipe Shot (`tracksTarget`) keeps aiming at the Pokémon it was aimed at
/// (`action.originalTarget`), which the queue does not hold: aimed at this side, it is refused.
fn swap_positions<const N: usize>(
    b: &mut Battle<'_, N>,
    from: SlotRef,
    to: SlotRef,
) -> Result<(), TurnError> {
    for action in &b.queue {
        let ActionKind::Move { index, target, .. } = action.kind else {
            continue;
        };
        let id = super::super::lock::action_move_id(b.mon(action.pokemon), index);
        if target != 0
            && id.data().tracks_target
            && super::at_loc(action.slot, target).side == from.side
        {
            return Err(b.unsupported(format!(
                "{} aimed at a side whose Pokémon Ally Switch swapped (it tracks its original target)",
                id.data().name
            )));
        }
    }
    let (user, ally) = (b.occupant(from), b.occupant(to));
    let (a, c) = (b.state.slot(from).clone(), b.state.slot(to).clone());
    let mut swap = Vec::new();
    super::super::diff::slot_changes(&mut swap, from, &a, &c);
    super::super::diff::slot_changes(&mut swap, to, &c, &a);
    for instruction in swap {
        b.apply(instruction);
    }
    for action in &mut b.queue {
        if Some(action.pokemon) == user {
            action.slot = to;
        } else if Some(action.pokemon) == ally {
            action.slot = from;
        }
    }
    if let Some(active) = b.active_move.as_mut() {
        if Some(active.pokemon) == user {
            active.user = to;
        } else if Some(active.pokemon) == ally {
            active.user = from;
        }
    }
    super::super::conditions::slot_condition_switch_in(b, from);
    super::super::conditions::slot_condition_switch_in(b, to);
    Ok(())
}

/// Whether Sleep Talk's `onHit` may pick `id`: not `nosleeptalk` (Sleep Talk itself, Assist,
/// Metronome, ...), not a charge move, not a Z- or Max move.
pub(crate) fn sleep_talk_calls(id: MoveId) -> bool {
    let data = id.data();
    !id.is_none()
        && !data.flags.contains(MoveFlags::NOSLEEPTALK)
        && !data.flags.contains(MoveFlags::CHARGE)
        && !(data.is_z && data.base_power != 1)
        && !data.is_max
}

/// Trick and Switcheroo `onHit`: `target.takeItem(source)` and `source.takeItem()` (`undefined`
/// without an item, `false` when the item's own TakeItem handler refuses: `onTakeItem: false`,
/// or a Mega Stone of its holder's species); fail if either refuses or both are empty; then
/// each item's TakeItem handler again with its new holder (a Mega Stone cannot go to its own
/// species); then `target.setItem(myItem)` and `source.setItem(yourItem)`, each running the
/// item's `Start` on its new holder ([`trick_item_start`]). Each `takeItem` of a held item first
/// runs the holder's ability TakeItem handler (Unburden adds its volatile even if the trade then
/// fails; Sticky Hold is refused on the field) and then the item's `End` on its old holder.
/// Items with `Start` / `End` handlers move only if [`trick_moves_item`]; an item with another
/// TakeItem handler is not implemented.
fn trick<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
) -> Result<HitResult, TurnError> {
    // `target.item` / `source.item`: the raw items, suppressed or not.
    let (yours, mine) = (b.raw_item(target), b.raw_item(user));
    for item in [yours, mine] {
        let data = item.data();
        let other_take_item = data.mega_stone.is_empty() && data.handlers.contains(&"onTakeItem");
        let start_or_end = data
            .handlers
            .iter()
            .any(|h| ["onStart", "onEnd"].contains(h));
        if other_take_item || (start_or_end && !trick_moves_item(item)) {
            return Err(b.unsupported(format!("Trick moving {} ({:?})", data.name, data.handlers)));
        }
    }
    // `target.takeItem(source)`, then `source.takeItem()`: the TakeItem event (Unburden).
    for (slot, item) in [(target, yours), (user, mine)] {
        if !item.is_none() {
            super::super::abilities::unburden(b, slot);
        }
    }
    let taken = |slot: SlotRef, item: ItemId| item.is_none() || b.item_can_be_taken(slot);
    if !taken(target, yours) || !taken(user, mine) || (yours.is_none() && mine.is_none()) {
        return Ok(HitResult::Failure);
    }
    let received = |item: ItemId, receiver: SlotRef| {
        item.is_none() || b.slot_mon(receiver).is_some_and(|m| holds_freely(item, m))
    };
    if !received(mine, target) || !received(yours, user) {
        return Ok(HitResult::Failure);
    }
    // The taken items' `End` on their old holders: Mirror Herb forgets its copied raises (none
    // can be pending inside Trick's own action); Utility Umbrella's changes nothing.
    for (slot, item) in [(target, yours), (user, mine)] {
        if item == items::MIRROR_HERB {
            if let Some(holder) = b.occupant(slot) {
                b.mirror_herb.retain(|&(p, _)| p != holder);
            }
        }
    }
    for (slot, old, new) in [(target, yours, mine), (user, mine, yours)] {
        let pokemon = b.occupant(slot).expect("an active Pokémon");
        b.apply(Instruction::SetItem {
            target: pokemon,
            old,
            new,
        });
        if !new.is_none() {
            trick_item_start(b, slot, new);
        }
    }
    Ok(HitResult::Success)
}

/// Whether Trick can move `item` although it has `Start` / `End` handlers, because those are
/// implemented for a new holder ([`trick_item_start`]) and an old one: the Choice items, the
/// Seeds, Room Service, White Herb, Air Balloon (its `onStart` only announces it), Utility
/// Umbrella (its `onStart` / `onEnd` only run WeatherChange for a holder ignoring its item, and
/// no implemented WeatherChange handler acts on sun or rain from it), Mirror Herb (`onEnd`).
fn trick_moves_item(item: ItemId) -> bool {
    item.data().is_choice
        || super::super::field_events::seed_terrain(item).is_some()
        || [
            items::ROOM_SERVICE,
            items::WHITE_HERB,
            items::AIR_BALLOON,
            items::UTILITY_UMBRELLA,
            items::MIRROR_HERB,
        ]
        .contains(&item)
}

/// `setItem`'s `singleEvent('Start', item)` on the new holder in `slot` (skipped while it ignores
/// its item): a Choice item removes the holder's `choicelock` (a lock from its old Choice item,
/// or from this very move's ModifyMove); a Seed, Room Service and White Herb act as when their
/// holder switches in (`items::switch_in_item`: used in its terrain, in Trick Room, with a
/// lowered stat).
fn trick_item_start<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, item: ItemId) {
    if b.item(slot) != item {
        return;
    }
    if item.data().is_choice {
        b.remove_volatile(slot, Volatile::ChoiceLock);
    } else if item != items::MIRROR_HERB {
        super::super::items::switch_in_item(b, slot, item);
    }
}

/// Whether `item`'s own TakeItem handler lets `holder` part with it (or, called with the new
/// holder, receive it): not `onTakeItem: false`, and not a Mega Stone of the holder's species
/// (`item.megaStone?.[holder.baseSpecies.baseSpecies]`).
fn holds_freely(item: ItemId, holder: &Pokemon) -> bool {
    let data = item.data();
    let base = holder.species.data().base_species;
    let base = if base.is_none() { holder.species } else { base };
    !data.cannot_be_taken && !data.mega_stone.iter().any(|&(from, _)| from == base)
}

/// The move's `onHitField` (moves targeting the whole field). `None` = none.
pub(super) fn on_hit_field<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Option<bool> {
    match mv.id {
        // Perish Song: every active Pokémon (side one first, slot order) gets the `perishsong`
        // volatile unless `runEvent('TryHit')` returns `null` for it (it still counts as a
        // success) or it already has one; fails when nobody was affected. No semi-invulnerable
        // state exists (`Invulnerability`).
        moves::PERISH_SONG => {
            let mut result = false;
            for side in [SideId::One, SideId::Two] {
                for slot in Battle::<N>::slots(side) {
                    if b.alive(slot).is_none() {
                        continue;
                    }
                    if perish_song_try_hit_null(b, user, mv, slot) {
                        result = true;
                    } else if !b.volatile(slot, Volatile::PerishSong).active {
                        b.add_volatile(slot, Volatile::PerishSong);
                        result = true;
                    }
                }
            }
            Some(result)
        }
        // Court Change: the listed side conditions of both sides trade places, with their
        // durations and layers; fails when neither side has one. (The Pledge and G-Max
        // conditions on the list are not implemented.)
        moves::COURT_CHANGE => {
            const SWAPPED: [SideEffect; 11] = [
                SideEffect::Mist,
                SideEffect::LightScreen,
                SideEffect::Reflect,
                SideEffect::Spikes,
                SideEffect::Safeguard,
                SideEffect::Tailwind,
                SideEffect::ToxicSpikes,
                SideEffect::StealthRock,
                SideEffect::StickyWeb,
                SideEffect::AuroraVeil,
                SideEffect::LuckyChant,
            ];
            let (mine, theirs) = (user.side, user.side.other());
            let mut success = false;
            for effect in SWAPPED {
                let a = b.state.side(mine).effects[effect as usize];
                let c = b.state.side(theirs).effects[effect as usize];
                success |= a.is_active() || c.is_active();
                b.set_side_effect(mine, effect, c);
                b.set_side_effect(theirs, effect, a);
            }
            Some(success)
        }
        // Haze: `for (const pokemon of this.getAllActive()) pokemon.clearBoosts();`
        moves::HAZE => {
            for side in [SideId::One, SideId::Two] {
                for slot in Battle::<N>::slots(side) {
                    if b.occupant(slot).is_some() {
                        clear_boosts(b, slot);
                    }
                }
            }
            Some(true)
        }
        _ => None,
    }
}

/// Whether a `TryHit` handler returns `null` for Perish Song on `target` (only `null` spares
/// it; `false`, e.g. Good as Gold, does not): Psychic Terrain against a Prankster-boosted
/// Perish Song on a grounded foe, and Soundproof (breakable) on anyone but the user. Any other
/// `null`-returning TryHit handler added later must be listed here.
fn perish_song_try_hit_null<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    let psychic_terrain = b.terrain() == Terrain::Psychic
        && mv.priority > 0
        && target.side != user.side
        && b.is_grounded(target);
    let soundproof = target != user && b.ability_unless_broken(target) == abilities::SOUNDPROOF;
    psychic_terrain || soundproof
}

/// Showdown `clearBoosts`.
fn clear_boosts<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) {
    set_boosts(b, slot, [0; BOOST_COUNT]);
}

/// Showdown `setBoost` for every stage: set directly (no clamping, no boost events).
fn set_boosts<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, boosts: [i8; BOOST_COUNT]) {
    for (stat, &new) in boosts.iter().enumerate() {
        let old = b.state.slot(slot).boosts[stat];
        if old != new {
            b.apply(Instruction::Boost {
                target: slot,
                stat: stat as u8,
                amount: new - old,
            });
        }
    }
}

/// Showdown `pokemon.sethp(hp)` on an active Pokémon with HP: at least 1, at most its max HP,
/// set outright (no Damage or Heal event).
fn set_hp<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, hp: i32) {
    let Some(pokemon) = b.alive(slot) else {
        return;
    };
    let mon = b.mon(pokemon);
    let (old, new) = (mon.hp, hp.clamp(1, i32::from(mon.max_hp)) as i16);
    if new < old {
        b.apply(Instruction::Damage {
            target: pokemon,
            amount: old - new,
        });
    } else if new > old {
        b.apply(Instruction::Heal {
            target: pokemon,
            amount: new - old,
        });
    }
}

/// Showdown `pokemon.setType(types)` (the caller has checked that it may): the types are
/// replaced. On a Pokémon under Roost the stored types keep Roost's filter (Flying left out,
/// Normal when nothing is left) and the volatile remembers the new types to restore when it
/// ends (nothing to restore without Flying).
fn set_types<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef, types: [Type; 2]) {
    let Some(pokemon) = b.occupant(slot) else {
        return;
    };
    let roost = b.volatile(slot, Volatile::Roost);
    let shown = if roost.active && types.contains(&Type::Flying) {
        let mut kept = types
            .into_iter()
            .filter(|&t| t != Type::Flying && t != Type::None);
        [
            kept.next().unwrap_or(Type::Normal),
            kept.next().unwrap_or(Type::None),
        ]
    } else {
        types
    };
    if roost.active {
        let counter = if shown == types {
            0
        } else {
            crate::volatile::encode_types(types)
        };
        b.set_volatile_state(slot, Volatile::Roost, VolatileState { counter, ..roost });
    }
    let old = b.mon(pokemon).types;
    if old != shown {
        b.apply(Instruction::SetTypes {
            target: pokemon,
            old,
            new: shown,
        });
    }
}

/// A handler's boolean result.
fn success(ok: bool) -> HitResult {
    if ok {
        HitResult::Success
    } else {
        HitResult::Failure
    }
}

/// Showdown `pokemon.cureStatus()` on the Pokémon in `slot`: whether it had a status to lose
/// (not at 0 HP).
fn cure_status<const N: usize>(b: &mut Battle<'_, N>, slot: SlotRef) -> bool {
    match b.alive(slot) {
        Some(p) if b.mon(p).status != Status::None => {
            b.cure_status(p);
            true
        }
        _ => false,
    }
}

/// Heal Bell and Aromatherapy `onHit`: every Pokémon of `side`'s party (`side.pokemon`, benched
/// ones included) loses its status (`cureStatus`: not at 0 HP), except an active one other than
/// the user whose ability (`hasAbility`: active only; skipped while the move suppresses it) is
/// Soundproof (Heal Bell), Sap Sipper (Aromatherapy) or Good as Gold, or that is behind a
/// substitute (Aromatherapy: `if (ally.volatiles['substitute'] && !move.infiltrates) continue;`,
/// inside the same not-suppressed check). Succeeds if anyone was cured.
fn party_cure<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    side: SideId,
    id: MoveId,
) -> HitResult {
    let immune_ability = if id == moves::HEAL_BELL {
        abilities::SOUNDPROOF
    } else {
        abilities::SAP_SIPPER
    };
    let user_pokemon = b.occupant(user);
    let mut cured = false;
    for party in 0..b.state.side(side).party.len() as u8 {
        let pokemon = PokemonRef { side, party };
        let active = Battle::<N>::slots(side).find(|&s| b.occupant(s) == Some(pokemon));
        if let Some(slot) = active {
            if Some(pokemon) != user_pokemon && !b.suppressing_ability(slot) {
                let ability = b.ability(slot);
                if ability == immune_ability || ability == abilities::GOOD_AS_GOLD {
                    continue;
                }
                if id == moves::AROMATHERAPY && b.has_substitute(slot) {
                    continue;
                }
            }
        }
        let mon = b.mon(pokemon);
        if mon.hp > 0 && mon.status != Status::None {
            b.cure_status(pokemon);
            cured = true;
        }
    }
    success(cured)
}

/// `this.heal(this.modify(pokemon.maxhp, factor))` with `factor` as a 4096-based modifier
/// (0.667 → 2732); `NOT_FAIL` when nothing is healed.
fn weather_heal<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    modifier: u32,
) -> HitResult {
    let max_hp = b.slot_mon(target).map_or(0, |m| i32::from(m.max_hp));
    if b.heal(target, f64::from(modify(max_hp, modifier))) > 0 {
        HitResult::Success
    } else {
        HitResult::NotFail
    }
}

/// The secondary effect's `onHit` (`secondaries` → `moveHit`, after the chance roll):
/// Dire Claw and Tri Attack draw one of three statuses (`this.sample`) and `trySetStatus` it,
/// so the draw happens even when the status then fails. Throat Chop: `target.addVolatile(
/// 'throatchop')` (no `onRestart`: an existing one keeps its duration; a fainted target gets
/// none).
pub(super) fn secondary_on_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    mv: &ActiveMove,
) {
    if mv.id == moves::THROAT_CHOP {
        b.add_volatile(target, Volatile::ThroatChop);
        return;
    }
    // Burning Jealousy: `if (target?.statsRaisedThisTurn) target.trySetStatus('brn', source,
    // move);` Alluring Voice: `if (target?.statsRaisedThisTurn) target.addVolatile('confusion',
    // source, move);`
    if mv.id == moves::BURNING_JEALOUSY || mv.id == moves::ALLURING_VOICE {
        let raised =
            b.occupant(target).is_some() && b.state.slot(target).history.stats_raised_this_turn;
        if raised && mv.id == moves::BURNING_JEALOUSY {
            b.try_set_status(target, Status::Burn);
        } else if raised {
            b.add_volatile(target, Volatile::Confusion);
        }
        return;
    }
    let statuses = match mv.id {
        moves::DIRE_CLAW => [Status::Poison, Status::Paralyze, Status::Sleep],
        moves::TRI_ATTACK => [Status::Burn, Status::Paralyze, Status::Freeze],
        _ => return,
    };
    let status = statuses[b.rng.uniform(statuses.len())];
    b.try_set_status(target, status);
}
