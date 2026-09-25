//! Move-specific Showdown callbacks (`data/moves.ts`, with the Champions overrides of
//! `data/mods/champions/moves.ts`) for moves listed in `support::MOVES_WITH_HANDLERS`.
//!
//! Each function is one event; `moves.rs` calls it where Showdown runs that event. A move not
//! handled here gets the event's neutral result.

use crate::damage::MOD_ONE_POINT_FIVE;
use crate::dex::{
    abilities, items, moves, ItemId, MoveFlags, MoveId, MoveTarget, Type, TypeRelation, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{Pokemon, PokemonRef, SideId, SlotRef, Status, BOOST_COUNT};
use crate::volatile::Volatile;

use super::super::abilities::{Handler, SUB_CONDITION};
use super::super::battle::{Battle, BoostEffect};
use super::super::conditions::HAZARDS;
use super::super::order::modify;
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
    let weather = b.effective_weather();
    let hidden = matches!(
        weather,
        Weather::Sun | Weather::Rain | Weather::HarshSun | Weather::HeavyRain
    ) && b.item(holder) == items::UTILITY_UMBRELLA;
    Ok(if hidden { Weather::None } else { weather })
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
    b: &Battle<'_, N>,
    user: SlotRef,
    target: Option<SlotRef>,
    mv: &mut ActiveMove,
) -> Result<(), TurnError> {
    match mv.id {
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
        _ => true,
    }
}

/// The move's `onTryImmunity` (`hitStepTryImmunity`, per target). `false` = the target is
/// immune.
pub(super) fn on_try_immunity<const N: usize>(
    b: &Battle<'_, N>,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    match mv.id {
        // Trick, Switcheroo: `return !target.hasAbility('stickyhold');` (`hasAbility` is not
        // skipped by Mold Breaker).
        moves::TRICK | moves::SWITCHEROO => b.ability(target) != abilities::STICKY_HOLD,
        _ => true,
    }
}

/// The move's own `onTryHit` (Champions `spreadMoveHit`: `singleEvent('TryHit', ...)` on the
/// first target, after accuracy and before the damage). `false` = the move fails. Low Kick's
/// and Grass Knot's only act on a Dynamaxed target, Poltergeist's only logs.
pub(super) fn on_try_hit<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &mut ActiveMove,
) -> bool {
    match mv.id {
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
        _ => true,
    }
}

/// The move's `onAfterHit`, once per damaged target (`spreadMoveHit`, after `DamagingHit`;
/// Champions runs it even if the user fainted). Knock Off's is in `moves.rs`.
/// `onAfterSubDamage` (the same effect against a substitute) is unreachable: substitutes are
/// refused.
pub(super) fn on_after_hit<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) {
    // Ice Spinner: `this.field.clearTerrain();`
    if mv.id == moves::ICE_SPINNER {
        super::clear_terrain(b);
    }
    // Rapid Spin: `if (!move.hasSheerForce)` the user's side loses its hazards (Leech Seed and
    // partial trapping, which it also ends, are not implemented).
    if mv.id == moves::RAPID_SPIN && !mv.has_sheer_force {
        remove_side_effects(b, user.side, &HAZARDS);
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

/// The move's `basePowerCallback` (`getDamage`, before the critical hit roll).
pub(super) fn base_power_callback<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
    base_power: i32,
) -> i32 {
    match mv.id {
        // Rising Voltage: `if (this.field.isTerrain('electricterrain') && target.isGrounded())
        // return move.basePower * 2;`
        moves::RISING_VOLTAGE if b.terrain() == Terrain::Electric && b.is_grounded(target) => {
            base_power * 2
        }
        // Acrobatics: `if (!pokemon.item) return move.basePower * 2;` (the held item).
        moves::ACROBATICS if b.item(user).is_none() => base_power * 2,
        _ => base_power,
    }
}

/// The move's own `onBasePower` modifier (BasePower handler priority 0, after type items and
/// terrain). Knock Off's is in `get_damage`.
pub(super) fn on_base_power<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Option<u32> {
    match mv.id {
        // Grav Apple: `if (this.field.getPseudoWeather('gravity')) return this.chainModify(1.5);`
        moves::GRAV_APPLE if b.field_active(FieldEffect::Gravity) => Some(MOD_ONE_POINT_FIVE),
        // Psyblade: `if (this.field.isTerrain('electricterrain')) return this.chainModify(1.5);`
        moves::PSYBLADE if b.terrain() == Terrain::Electric => Some(MOD_ONE_POINT_FIVE),
        // Expanding Force: `if (this.field.isTerrain('psychicterrain') && source.isGrounded())
        // return this.chainModify(1.5);`
        moves::EXPANDING_FORCE if b.terrain() == Terrain::Psychic && b.is_grounded(user) => {
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
) -> Vec<Handler> {
    let mut out = Vec::new();
    if b.volatile(target, Volatile::GlaiveRush).active {
        out.push(Handler::of(b, target, 0, SUB_CONDITION, 2 * 4096));
    }
    out
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
        // Steel Roller: `this.field.clearTerrain();` (returns nothing: no effect on success).
        moves::STEEL_ROLLER => {
            super::clear_terrain(b);
            return Ok(None);
        }
        moves::TRICK | moves::SWITCHEROO => trick(b, user, target)?,
        moves::INSTRUCT => instruct(b, target)?,
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
        // Defog: `this.boost({evasion: -1})` on the target (success if a stage changed); the
        // target's side loses its screens, Safeguard and Mist (no success) and its hazards, the
        // user's side its hazards (success); then `this.field.clearTerrain()`.
        moves::DEFOG => {
            let mut drop = NO_BOOSTS;
            drop[6] = -1;
            let mut success = b.boost_by(target, &drop, Some(user), BoostEffect::Move(mv.id));
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
    let fractional_tenths = super::super::items::fractional_priority_tenths(b.mon(pokemon));
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
/// species); then both `setItem`s. Abilities with TakeItem handlers (Sticky Hold, Unburden)
/// are refused on the field. An item whose `Start`, `End` or other TakeItem handler would run
/// here is not implemented.
fn trick<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
) -> Result<HitResult, TurnError> {
    let (yours, mine) = (b.item(target), b.item(user));
    for item in [yours, mine] {
        let data = item.data();
        let other_take_item = data.mega_stone.is_empty() && data.handlers.contains(&"onTakeItem");
        if other_take_item
            || data
                .handlers
                .iter()
                .any(|h| ["onStart", "onEnd"].contains(h))
        {
            return Err(b.unsupported(format!("Trick moving {} ({:?})", data.name, data.handlers)));
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
    for (slot, old, new) in [(target, yours, mine), (user, mine, yours)] {
        let pokemon = b.occupant(slot).expect("an active Pokémon");
        b.apply(Instruction::SetItem {
            target: pokemon,
            old,
            new,
        });
    }
    Ok(HitResult::Success)
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
/// so the draw happens even when the status then fails.
pub(super) fn secondary_on_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    target: SlotRef,
    mv: &ActiveMove,
) {
    let statuses = match mv.id {
        moves::DIRE_CLAW => [Status::Poison, Status::Paralyze, Status::Sleep],
        moves::TRI_ATTACK => [Status::Burn, Status::Paralyze, Status::Freeze],
        _ => return,
    };
    let status = statuses[b.rng.uniform(statuses.len())];
    b.try_set_status(target, status);
}
