//! Using a move: Showdown `runMove` ??`useMove` ??`trySpreadMoveHit` / `tryMoveHit` ??the
//! hit steps ??`spreadMoveHit` (damage, effects, secondaries) ??recoil and after-move
//! effects, for the implemented moves (see [`super::support`]).

mod ability_hooks;
mod handlers;

use handlers::HitResult;

use crate::damage::{
    damage_rolls, DamageInput, MOD_HALF, MOD_ONE, MOD_ONE_POINT_FIVE, MOD_ONE_POINT_THREE,
    MOD_ONE_POINT_TWO,
};
use crate::dex::{
    abilities, items, moves, AbilityId, FixedDamage, IgnoreImmunity, ItemId, MoveCategory,
    MoveData, MoveFlags, MoveId, MoveTarget, Ohko, Secondary, Stat, Type, TypeImmunities,
    TypeRelation, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::state::{PokemonRef, SideId, SlotRef, Status};
use crate::volatile::Volatile;

use super::abilities as ability_events;
use super::abilities::{Handler, SUB_FIELD_CONDITION, SUB_ITEM, SUB_MOVE, SUB_SIDE_CONDITION};
use super::battle::{ActiveMoveRef, Battle, BoostEffect, DamageSource};
use super::conditions;
use super::items as item_events;
use super::order::{boosted_stat, modify};
use super::support::{side_effect_of, type_boost_item};
use super::TurnError;

/// The move being used, with what is decided when it is used. It is part of a suspended
/// multi-hit move's progress (`MoveProgress`), so it is comparable: `data` is implied by `id`
/// and left out of the comparison (keep the manual impls below in step with new fields).
#[derive(Clone, Debug)]
struct ActiveMove {
    id: MoveId,
    data: &'static MoveData,
    /// Priority after ModifyPriority (Showdown sets `move.priority` to it).
    priority: i32,
    prankster_boosted: bool,
    spread: bool,
    /// Accuracy after ModifyMove; `None` never misses (Showdown `accuracy: true`).
    accuracy: Option<u8>,
    /// Showdown `move.hasSheerForce`: Sheer Force deleted the secondaries and `self` in
    /// ModifyMove (and AfterMoveSecondary(Self) effects are skipped).
    has_sheer_force: bool,
    /// Serene Grace's ModifyMove doubles every secondary chance and `self.chance` (1 or 2).
    secondary_chance_factor: u32,
    /// A secondary effect ModifyMove appended to the move's own (King's Rock's flinch). Not
    /// part of the comparison: it follows from the user's item.
    added_secondary: Option<Secondary>,
    /// HP taken by the move's hits (Showdown `move.totalDamage`), set once the hits are done.
    total_damage: i32,
    /// Target type after ModifyMove (Showdown `move.target`; Expanding Force widens it).
    target: MoveTarget,
    /// Type after ModifyType (Showdown `move.type`; Weather Ball, Terrain Pulse). Every rule
    /// that reads the type of the move being used reads this, not `data.move_type`.
    move_type: Type,
    /// Base power after ModifyMove (Showdown `move.basePower`), before `basePowerCallback`.
    base_power: i32,
}

impl PartialEq for ActiveMove {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.priority == other.priority
            && self.prankster_boosted == other.prankster_boosted
            && self.spread == other.spread
            && self.accuracy == other.accuracy
            && self.has_sheer_force == other.has_sheer_force
            && self.secondary_chance_factor == other.secondary_chance_factor
            && self.total_damage == other.total_damage
            && self.target == other.target
            && self.move_type == other.move_type
            && self.base_power == other.base_power
    }
}

impl Eq for ActiveMove {}

impl std::hash::Hash for ActiveMove {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.priority.hash(state);
        self.prankster_boosted.hash(state);
        self.spread.hash(state);
        self.accuracy.hash(state);
        self.has_sheer_force.hash(state);
        self.secondary_chance_factor.hash(state);
        self.total_damage.hash(state);
        self.target.hash(state);
        self.move_type.hash(state);
        self.base_power.hash(state);
    }
}

/// A multi-hit move suspended between two hits (WORKPLAN F10): the turn engine runs each hit
/// as its own stage so identical positions merge between hits instead of multiplying every
/// hit's damage rolls into one enumeration. Everything `hitStepMoveHitLoop` keeps across hits
/// plus what the move's tail (`useMoveInner`, `runMove`) needs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MoveProgress {
    user: SlotRef,
    pokemon: PokemonRef,
    mv: ActiveMove,
    /// Targets still standing (Showdown keeps hitting until every target fainted).
    targets: Vec<SlotRef>,
    main_target: SlotRef,
    /// Hits to make and hits made so far.
    hits: u8,
    hit: u8,
    /// `move.totalDamage`.
    total_damage: i32,
    /// Whether any hit so far did something (the move's success).
    any_ok: bool,
    /// `ActiveMoveRef::ignore_ability` of the move in flight (Mold Breaker moves).
    ignore_ability: bool,
}

/// How far a move got: finished, or suspended before its next hit.
pub(crate) enum MoveStep {
    Done,
    Suspended(MoveProgress),
}

/// A hit loop's result within `use_move`: finished (whether it succeeded, and the HP its
/// hits took), or suspended before its next hit.
enum HitOutcome {
    Finished { ok: bool, total_damage: i32 },
    Suspended(MoveProgress),
}

/// Per-target result of a hit (Showdown's `damage[i]`: a number, `true`, or `false`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Failed,
    /// Hit without damage (status moves).
    Done,
    Damage(i32),
}

impl Hit {
    fn ok(self) -> bool {
        self != Hit::Failed
    }
}

/// Showdown `runMove` for the move in `move_index`. `will_act` is `queue.willAct()`. A
/// multi-hit move returns `MoveStep::Suspended` after its first hit; the turn engine resumes
/// it with [`resume_move`] as its own stage.
pub(crate) fn run_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    move_index: u8,
    target_loc: i8,
    will_act: bool,
) -> Result<MoveStep, TurnError> {
    let pokemon = b.occupant(user).expect("the caller checked the user");
    b.increment_move_actions(user);
    if move_index == super::lock::RECHARGE_INDEX {
        // The `recharge` pseudo-move: BeforeMove (`mustrecharge`, priority 11) ends it.
        let recharge = ActiveMove {
            id: MoveId::NONE,
            data: MoveId::NONE.data(),
            priority: 0,
            prankster_boosted: false,
            spread: false,
            accuracy: None,
            has_sheer_force: false,
            secondary_chance_factor: 1,
            added_secondary: None,
            total_damage: 0,
            target: MoveId::NONE.data().target,
            move_type: MoveId::NONE.data().move_type,
            base_power: 0,
        };
        before_move(b, user, &recharge);
        return Ok(MoveStep::Done);
    }
    let id = b.mon(pokemon).moves[move_index as usize].id;
    // `setActiveMove`: set for the whole move, cleared when it ends.
    b.active_move = Some(ActiveMoveRef {
        user,
        pokemon,
        id,
        ignore_ability: id.data().ignore_ability,
    });
    let result = run_move_inner(b, user, move_index, target_loc, will_act);
    if !matches!(result, Ok(MoveStep::Suspended(_))) {
        b.active_move = None;
    }
    result
}

/// The next hit of a suspended multi-hit move, then the move's tail once the hits are done.
pub(crate) fn resume_move<const N: usize>(
    b: &mut Battle<'_, N>,
    progress: MoveProgress,
) -> Result<MoveStep, TurnError> {
    let (user, pokemon, main_target) = (progress.user, progress.pokemon, progress.main_target);
    b.active_move = Some(ActiveMoveRef {
        user,
        pokemon,
        id: progress.mv.id,
        ignore_ability: progress.ignore_ability,
    });
    let mut mv = progress.mv.clone();
    let result = match hit_loop(b, user, &mv, Some(progress))? {
        HitOutcome::Suspended(progress) => return Ok(MoveStep::Suspended(progress)),
        HitOutcome::Finished { ok, total_damage } => {
            mv.total_damage = total_damage;
            ok
        }
    };
    use_move_tail(b, user, &mv, result, main_target);
    run_move_tail(b, user);
    b.active_move = None;
    Ok(MoveStep::Done)
}

/// The end of Showdown `runMove` after `useMove`: `AfterMove` (a locked move on its last
/// turn ends and, by fatigue, confuses; White Herb and Mirror Herb act), then faints.
fn run_move_tail<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) {
    let locked = b.volatile(user, Volatile::LockedMove);
    if locked.active && locked.duration == 1 {
        b.remove_volatile(user, Volatile::LockedMove);
    }
    // The items' `onAnyAfterMove` (White Herb, Mirror Herb), collected only while the user
    // is still active; they and the lock above act on different holders.
    if b.active_move
        .is_some_and(|m| b.occupant(m.user) == Some(m.pokemon))
    {
        item_events::any_after_move(b, user);
    }
    b.faint_messages(true);
    b.check_win(None);
}

fn run_move_inner<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    move_index: u8,
    target_loc: i8,
    will_act: bool,
) -> Result<MoveStep, TurnError> {
    let pokemon = b.occupant(user).expect("the caller checked the user");
    let chosen = b.mon(pokemon).moves[move_index as usize].id;
    // OverrideAction (Encore): the encored move replaces the chosen one, keeping the chosen
    // move's priority and Prankster boost; its target is drawn afresh.
    let encore = b.volatile(user, Volatile::Encore);
    let (id, move_index, target) = if encore.active && encore.mv != chosen {
        let index = b
            .mon(pokemon)
            .moves
            .iter()
            .position(|m| m.id == encore.mv)
            .ok_or_else(|| b.unsupported("Encore into a move the user no longer has"))?;
        let target = get_random_target(b, user, encore.mv.data().target);
        (encore.mv, index as u8, target)
    } else {
        (chosen, move_index, get_target(b, user, chosen, target_loc))
    };
    let mut mv = ActiveMove {
        id,
        data: id.data(),
        priority: b.move_priority(user, chosen),
        prankster_boosted: b.prankster_boosted(user, chosen),
        spread: false,
        accuracy: id.data().accuracy,
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        total_damage: 0,
        target: id.data().target,
        move_type: id.data().move_type,
        base_power: i32::from(id.data().base_power),
    };

    if !before_move(b, user, &mv) {
        return Ok(MoveStep::Done);
    }

    // A locked move (Outrage's later turns) costs no PP.
    if super::lock::locked_move(b.state, user).is_none() {
        let pp = b.mon(pokemon).moves[move_index as usize].pp;
        if pp == 0 {
            return Err(b.unsupported(format!("{}: Struggle", mv.data.name)));
        }
        b.apply(crate::instruction::Instruction::SetPp {
            target: pokemon,
            move_index,
            old: pp,
            new: pp - 1,
        });
    }
    b.set_last_move(user, id);

    if let Some(progress) = use_move(b, user, &mut mv, target, will_act)? {
        return Ok(MoveStep::Suspended(progress));
    }
    run_move_tail(b, user);
    Ok(MoveStep::Done)
}

/// The BeforeMove handlers, by priority: sleep and freeze (10), flinch (8), Gravity (6),
/// paralysis (1), the Choice lock (0). `false` = the move is not used (no PP, no `lastMove`).
fn before_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) -> bool {
    let pokemon = b.occupant(user).expect("checked");
    // mustrecharge (priority 11): the turn is spent recharging.
    if b.volatile(user, Volatile::MustRecharge).active {
        b.remove_volatile(user, Volatile::MustRecharge);
        return false;
    }
    match b.mon(pokemon).status {
        Status::Sleep => {
            let time = b.mon(pokemon).status_turns - 1;
            b.set_status_turns(pokemon, time);
            if time <= 0 {
                b.cure_status(pokemon);
            } else {
                return false;
            }
        }
        Status::Freeze if !mv.data.flags.contains(MoveFlags::DEFROST) => {
            let time = b.mon(pokemon).status_turns - 1;
            b.set_status_turns(pokemon, time);
            if time <= 0 || b.rng.chance(1, 4) {
                b.cure_status(pokemon);
            } else {
                return false;
            }
        }
        _ => {}
    }
    if b.volatile(user, Volatile::Flinch).active {
        return false;
    }
    if b.field_active(FieldEffect::Gravity) && mv.data.flags.contains(MoveFlags::GRAVITY) {
        return false;
    }
    // Confusion (priority 3): one turn less; over at 0; otherwise a 33% hit on itself.
    let confusion = b.volatile(user, Volatile::Confusion);
    if confusion.active {
        let mut next = confusion;
        next.time -= 1;
        if next.time == 0 {
            b.remove_volatile(user, Volatile::Confusion);
        } else {
            b.set_volatile_state(user, Volatile::Confusion, next);
            if b.rng.chance(33, 100) {
                let damage = confusion_damage(b, user);
                b.damage(user, f64::from(damage), DamageSource::Move);
                return false;
            }
        }
    }
    // Champions paralysis: 1/8.
    if b.mon(pokemon).status == Status::Paralyze && b.rng.chance(1, 8) {
        return false;
    }
    // The Choice lock (priority 0).
    item_events::before_move(b, user, mv.id)
}

/// Showdown `getConfusionDamage(pokemon, 40)`: a 40-power typeless physical hit with the
/// user's own boosted Attack against its own boosted Defense, truncated to 16 bits, then the
/// usual 85–100% roll, at least 1.
fn confusion_damage<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) -> i32 {
    let mon = b.slot_mon(user).expect("checked");
    let boosts = b.state.slot(user).boosts;
    let attack = boosted_stat(i32::from(mon.stats[0]), boosts[0]);
    // `calculateStat('def', boosts.def)`: the stored SpD under Wonder Room.
    let stored_def = stored_stat_index(Stat::Def, b.field_active(FieldEffect::WonderRoom));
    let defense = boosted_stat(i32::from(mon.stats[stored_def]), boosts[1]);
    let level = i32::from(mon.level);
    let base = ((2 * level / 5 + 2) * 40 * attack / defense) / 50 + 2;
    let base = base & 0xffff;
    let roll = 100 - b.rng.uniform(16) as i32;
    (base * roll / 100).max(1)
}

// ---- targets --------------------------------------------------------------------------------

fn loc_of(user: SlotRef, target: SlotRef) -> i8 {
    let position = target.slot as i8 + 1;
    if target.side == user.side {
        -position
    } else {
        position
    }
}

fn at_loc(user: SlotRef, loc: i8) -> SlotRef {
    let side = if loc < 0 {
        user.side
    } else {
        user.side.other()
    };
    SlotRef {
        side,
        slot: loc.unsigned_abs() - 1,
    }
}

/// Showdown `validTargetLoc`.
pub fn valid_target_loc(n: usize, user: SlotRef, loc: i8, target: MoveTarget) -> bool {
    if loc == 0 {
        return true;
    }
    let n = n as i8;
    if loc.abs() > n {
        return false;
    }
    let source = -(user.slot as i8 + 1);
    let is_self = source == loc;
    let is_foe = loc > 0;
    let across = -(n + 1 - loc);
    let adjacent = if loc > 0 {
        (across - source).abs() <= 1
    } else {
        (loc - source).abs() == 1
    };
    match target {
        MoveTarget::RandomNormal | MoveTarget::Scripted | MoveTarget::Normal => adjacent,
        MoveTarget::AdjacentAlly => adjacent && !is_foe,
        MoveTarget::AdjacentAllyOrSelf => adjacent && !is_foe || is_self,
        MoveTarget::AdjacentFoe => adjacent && is_foe,
        MoveTarget::Any => !is_self,
        _ => false,
    }
}

/// Whether a move of this target type takes a chosen target in a format with `n` slots.
pub fn takes_target(n: usize, target: MoveTarget) -> bool {
    n > 1
        && matches!(
            target,
            MoveTarget::Normal
                | MoveTarget::Any
                | MoveTarget::AdjacentAlly
                | MoveTarget::AdjacentAllyOrSelf
                | MoveTarget::AdjacentFoe
        )
}

/// Showdown `getTarget`. The returned slot may hold a fainted Pok챕mon (Showdown returns
/// the fainted object; the move then fails).
fn get_target<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    loc: i8,
) -> Option<SlotRef> {
    let target = id.data().target;
    if matches!(
        target,
        MoveTarget::AdjacentAlly | MoveTarget::Any | MoveTarget::Normal
    ) && loc != 0
        && loc == loc_of(user, user)
    {
        return None;
    }
    if target != MoveTarget::RandomNormal && loc != 0 && valid_target_loc(N, user, loc, target) {
        let t = at_loc(user, loc);
        if b.alive(t).is_none() && t.side == user.side {
            if target == MoveTarget::AdjacentAllyOrSelf {
                return Some(user);
            }
            return Some(t);
        }
        if b.alive(t).is_some() {
            return Some(t);
        }
    }
    get_random_target(b, user, target)
}

/// Showdown `getRandomTarget`.
fn get_random_target<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    target: MoveTarget,
) -> Option<SlotRef> {
    match target {
        MoveTarget::User
        | MoveTarget::All
        | MoveTarget::AllySide
        | MoveTarget::AllyTeam
        | MoveTarget::AdjacentAllyOrSelf => return Some(user),
        MoveTarget::AdjacentAlly => {
            if N == 1 {
                return None;
            }
            let allies = adjacent_allies(b, user);
            if allies.is_empty() {
                return None;
            }
            let i = b.rng.uniform(allies.len());
            return Some(allies[i]);
        }
        _ => {}
    }
    let foe = user.side.other();
    if N == 1 {
        return Some(SlotRef { side: foe, slot: 0 });
    }
    let foes = b.alive_slots(foe);
    if foes.is_empty() {
        return Some(SlotRef { side: foe, slot: 0 });
    }
    let i = b.rng.uniform(foes.len());
    Some(foes[i])
}

fn adjacent_allies<const N: usize>(b: &Battle<'_, N>, user: SlotRef) -> Vec<SlotRef> {
    // With at most two slots per side every other ally is adjacent.
    b.alive_slots(user.side)
        .into_iter()
        .filter(|&s| s != user)
        .collect()
}

/// Showdown `getMoveTargets`: spread moves hit everyone in range; a single target that
/// fainted is re-rolled, then the `RedirectTarget` event may move it (doubles only).
fn get_move_targets<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> Result<Vec<SlotRef>, TurnError> {
    Ok(match mv.target {
        MoveTarget::All | MoveTarget::FoeSide | MoveTarget::AllySide | MoveTarget::AllyTeam => {
            Vec::new()
        }
        MoveTarget::AllAdjacent => {
            let mut t = adjacent_allies(b, user);
            t.extend(b.alive_slots(user.side.other()));
            t
        }
        MoveTarget::AllAdjacentFoes => b.alive_slots(user.side.other()),
        // `alliesAndSelf()`: every active Pokémon on the user's side that has not fainted.
        MoveTarget::Allies => b.alive_slots(user.side),
        _ => {
            let mut t = target;
            if b.alive(t).is_none() && t.side != user.side {
                match get_random_target(b, user, mv.target) {
                    Some(r) => t = r,
                    None => return Ok(Vec::new()),
                }
            }
            if N > 1 && !mv.data.tracks_target {
                t = redirect_target(b, user, mv, t)?;
            }
            if b.alive(t).is_none() {
                return Ok(Vec::new());
            }
            vec![t]
        }
    })
}

/// Showdown `priorityEvent('RedirectTarget')`: the handlers are Follow Me, Rage Powder and
/// Spotlight on the user's foes (`onFoeRedirectTarget`, priority 1, 1, 2) and Lightning Rod /
/// Storm Drain on anyone else (`onAnyRedirectTarget`, priority 0). They are sorted by
/// priority, then the holder's Speed (`compareRedirectOrder`), and the first whose holder is a
/// valid target of the move's target type wins. Rage Powder skips powder-immune users. A tie
/// between two valid holders is broken in Showdown by `effectOrder` (who entered the field or
/// changed ability first), which the state does not record, so it is unsupported.
fn redirect_target<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> Result<SlotRef, TurnError> {
    // (priority, speed, holder), in Showdown's handler collection order: the user's side
    // (`onAny`), then each foe's `onFoe` volatiles and `onAny` ability.
    let mut handlers: Vec<(i8, i32, SlotRef)> = Vec::new();
    // `breakable`: a Mold Breaker move ignores these handlers too.
    let absorbs = |b: &Battle<'_, N>, s: SlotRef| {
        b.alive(s).is_some() && absorbing_type(b.ability_unless_broken(s)) == Some(mv.move_type)
    };
    for s in Battle::<N>::slots(user.side) {
        if absorbs(b, s) {
            handlers.push((0, b.action_speed(s), s));
        }
    }
    for s in b.alive_slots(user.side.other()) {
        let speed = b.action_speed(s);
        if b.volatile(s, Volatile::FollowMe).active {
            handlers.push((1, speed, s));
        }
        if b.volatile(s, Volatile::RagePowder).active {
            handlers.push((1, speed, s));
        }
        if b.volatile(s, Volatile::Spotlight).active {
            handlers.push((2, speed, s));
        }
        if absorbs(b, s) {
            handlers.push((0, speed, s));
        }
    }
    if handlers.is_empty() {
        return Ok(target);
    }
    // Stable, so equal keys keep collection order.
    handlers.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    let valid = |b: &Battle<'_, N>, priority: i8, holder: SlotRef| -> bool {
        let loc = loc_of(user, holder);
        if priority == 0 {
            // Lightning Rod / Storm Drain treat `adjacentFoe`/`randomNormal` as `normal`.
            let kind = match mv.target {
                MoveTarget::AdjacentFoe | MoveTarget::RandomNormal => MoveTarget::Normal,
                other => other,
            };
            return valid_target_loc(N, user, loc, kind);
        }
        let is_rage_powder = b.volatile(holder, Volatile::RagePowder).active
            && !b.volatile(holder, Volatile::FollowMe).active
            && !b.volatile(holder, Volatile::Spotlight).active;
        if is_rage_powder && b.status_immune(user, TypeImmunities::POWDER) {
            return false;
        }
        valid_target_loc(N, user, loc, mv.target)
    };
    let mut i = 0;
    while i < handlers.len() {
        let key = (handlers[i].0, handlers[i].1);
        let mut j = i;
        let mut winners: Vec<SlotRef> = Vec::new();
        while j < handlers.len() && (handlers[j].0, handlers[j].1) == key {
            let holder = handlers[j].2;
            if valid(b, key.0, holder) && !winners.contains(&holder) {
                winners.push(holder);
            }
            j += 1;
        }
        match winners.len() {
            0 => {}
            1 => return Ok(winners[0]),
            _ => {
                return Err(b.unsupported(format!(
                    "redirection tie between {} and {} (Showdown breaks it by effectOrder)",
                    b.slot_mon(winners[0])
                        .map_or("?", |m| m.species.data().name),
                    b.slot_mon(winners[1])
                        .map_or("?", |m| m.species.data().name),
                )));
            }
        }
        i = j;
    }
    Ok(target)
}

// ---- use ---------------------------------------------------------------------------------------

/// Showdown `useMoveInner`. Returns whether the move succeeded.
fn use_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    target: Option<SlotRef>,
    will_act: bool,
) -> Result<Option<MoveProgress>, TurnError> {
    let pokemon = b.occupant(user).expect("checked");
    let base_target = mv.target;
    let mut target = if matches!(mv.target, MoveTarget::User | MoveTarget::Allies) {
        Some(user)
    } else {
        target
    };
    // ModifyType and ModifyMove: the move's own handlers (`singleEvent`), then `runEvent`: the
    // user's ability and status; a changed target type picks a new target (`getRandomTarget`).
    handlers::on_modify_type(b, user, mv)?;
    handlers::on_modify_move(b, user, target, mv)?;
    ability_hooks::on_modify_move(b, user, mv)?;
    if mv.target != base_target {
        target = get_random_target(b, user, mv.target);
    }
    // Freeze `onModifyMove`: a defrosting move thaws the user.
    if b.mon(pokemon).status == Status::Freeze && mv.data.flags.contains(MoveFlags::DEFROST) {
        b.cure_status(pokemon);
    }
    // The item's onModifyMove: the Choice lock (priority 0), King's Rock's flinch (-1).
    item_events::on_modify_move(b, user, mv.id);
    mv.added_secondary = item_events::added_secondary(b.item(user), mv.data);
    let Some(target) = target else {
        return Ok(None);
    };

    let mut main_target = target;
    let field_move = matches!(
        mv.target,
        MoveTarget::All | MoveTarget::FoeSide | MoveTarget::AllySide
    );
    let targets = if field_move {
        Vec::new()
    } else {
        get_move_targets(b, user, mv, target)?
    };
    deduct_pressure_pp(b, user, mv, &targets);
    // TryMove: Dazzling, Queenly Majesty, Armor Tail (`onFoeTryMove`).
    let try_move_target = targets.last().copied().unwrap_or(target);
    if !ability_hooks::on_try_move(b, user, mv, try_move_target) {
        return Ok(None);
    }
    let result = if field_move {
        try_move_hit_field(b, user, mv, target, will_act)?
    } else {
        let Some(&last) = targets.last() else {
            return Ok(None);
        };
        main_target = last;
        match try_spread_move_hit(b, user, mv, targets, will_act)? {
            HitOutcome::Finished { ok, total_damage } => {
                mv.total_damage = total_damage;
                ok
            }
            HitOutcome::Suspended(mut progress) => {
                progress.main_target = main_target;
                return Ok(Some(progress));
            }
        }
    };
    use_move_tail(b, user, mv, result, main_target);
    Ok(None)
}

/// The end of Showdown `useMoveInner` after the hits: the `self` boost, then
/// AfterMoveSecondarySelf (Life Orb).
fn use_move_tail<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    result: bool,
    main_target: SlotRef,
) {
    if result && mv.data.self_boost != NO_BOOSTS {
        b.boost_by(
            user,
            &mv.data.self_boost,
            Some(user),
            BoostEffect::Move(mv.id),
        );
    }
    if !result {
        return;
    }
    // AfterMoveSecondarySelf (skipped for a Sheer Force-boosted move): the user's item (Life
    // Orb, Shell Bell, Throat Spray).
    if !ability_hooks::sheer_force_skips(b, user, mv) {
        item_events::after_move_secondary_self(b, user, main_target, mv.data, mv.total_damage);
    }
}

/// The extra PP of Showdown `useMoveInner`: `runEvent('DeductPP')` for every Pok챕mon in
/// `getMoveTargets`'s `pressureTargets`, then one `deductPP(move, extraPP)` on the used move
/// (clamped at 0). Pressure (`onDeductPP`, not breakable) adds 1 unless the target is the user's
/// ally. `pressureTargets` are the move's targets, except: every active Pok챕mon for `all` moves
/// (only foes can count), nobody for `foeSide` moves, only allies for `allySide`, and all foes
/// for `mustpressure` moves. `targets` are the resolved targets of a non-field move.
fn deduct_pressure_pp<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    targets: &[SlotRef],
) {
    let foe = user.side.other();
    let pressure_targets: Vec<SlotRef> = if mv.data.flags.contains(MoveFlags::MUSTPRESSURE) {
        b.alive_slots(foe)
    } else {
        match mv.target {
            MoveTarget::All => b.alive_slots(foe),
            MoveTarget::FoeSide | MoveTarget::AllySide | MoveTarget::AllyTeam => Vec::new(),
            _ => targets.to_vec(),
        }
    };
    let extra = pressure_targets
        .iter()
        .filter(|t| t.side != user.side && b.ability(**t) == abilities::PRESSURE)
        .count();
    if extra == 0 {
        return;
    }
    let pokemon = b.occupant(user).expect("checked");
    let Some(index) = b.mon(pokemon).moves.iter().position(|m| m.id == mv.id) else {
        return;
    };
    let old = b.mon(pokemon).moves[index].pp;
    let new = old.saturating_sub(extra as u8);
    if new != old {
        b.apply(crate::instruction::Instruction::SetPp {
            target: pokemon,
            move_index: index as u8,
            old,
            new,
        });
    }
}

/// Showdown `tryMoveHit` ??`moveHit` for field and side moves.
fn try_move_hit_field<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
    will_act: bool,
) -> Result<bool, TurnError> {
    let data = mv.data;
    // Try: the move's onTry (Aurora Veil; Wide Guard and Quick Guard need a later action,
    // `!!this.queue.willAct()`).
    if mv.id == moves::AURORA_VEIL && b.effective_weather() != Weather::Snow {
        return Ok(false);
    }
    if [moves::WIDE_GUARD, moves::QUICK_GUARD].contains(&mv.id) && !will_act {
        return Ok(false);
    }
    // PrepareHit: the user's ability (Protean, Libero).
    prepare_hit_ability(b, user, mv);
    // runMoveEffects on the target: undefined (nothing attempted) counts as success.
    let mut outcome: Option<bool> = None;
    let mut combine = |r: bool| outcome = Some(outcome.unwrap_or(false) || r);
    if !data.side_condition.is_none() {
        let side = target.side;
        let effect = side_effect_of(data.side_condition.id()).expect("checked by support");
        combine(add_side_condition(b, user, side, effect));
    }
    // HitSide: Wide Guard and Quick Guard `onHitSide`: `source.addVolatile('stall')`, even if
    // the side condition was already up (returns nothing).
    if [moves::WIDE_GUARD, moves::QUICK_GUARD].contains(&mv.id) {
        b.add_volatile(user, Volatile::Stall);
    }
    if !data.weather.is_none() {
        let weather = weather_of(data.weather.id()).expect("checked by support");
        combine(set_weather(b, user, weather));
    }
    if !data.terrain.is_none() {
        let terrain = terrain_of(data.terrain.id()).expect("checked by support");
        combine(set_terrain(b, user, terrain));
    }
    if !data.pseudo_weather.is_none() {
        combine(add_pseudo_weather(b, data.pseudo_weather.id()));
    }
    // HitField: the move's onHitField (Haze).
    if let Some(r) = handlers::on_hit_field(b, user, mv) {
        combine(r);
    }
    Ok(outcome.unwrap_or(true))
}

/// Showdown `trySpreadMoveHit` (also for single-target moves).
fn try_spread_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    mut targets: Vec<SlotRef>,
    will_act: bool,
) -> Result<HitOutcome, TurnError> {
    mv.spread = targets.len() > 1;

    // Try: the move's onTry (Fake Out, First Impression, Poltergeist), on the first target.
    // Sucker Punch / Thunderclap / Upper Hand `onTry`: the target must still have an attack
    // queued (Upper Hand: a priority move above +0.1).
    if [moves::SUCKER_PUNCH, moves::THUNDERCLAP, moves::UPPER_HAND].contains(&mv.id) {
        let ok = match b.queued_move(targets[0]) {
            Some((_, category, priority)) => {
                category != MoveCategory::Status && (mv.id != moves::UPPER_HAND || priority > 1)
            }
            None => false,
        };
        if !ok {
            return Ok(HitOutcome::Finished {
                ok: false,
                total_damage: 0,
            });
        }
    }
    if !handlers::on_try(b, user, mv, targets[0]) {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // Follow Me / Rage Powder `onTry` and Spotlight `onTryHit`: doubles only.
    if matches!(
        mv.id,
        m if m == moves::FOLLOW_ME || m == moves::RAGE_POWDER || m == moves::SPOTLIGHT
    ) && N == 1
    {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // PrepareHit: Protect and Detect need a later action and pass the stall check; then the
    // user's ability (Protean, Libero).
    if mv.data.stalling_move && !(will_act && stall_move(b, user)) {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    prepare_hit_ability(b, user, mv);

    // 1. TryHit: Psychic Terrain (priority 4), Protect (3), the target's ability (0). Each
    //    target's handlers only affect that target, so targets can be taken one at a time.
    let mut kept = Vec::with_capacity(targets.len());
    for t in targets {
        if try_hit(b, user, mv, t) {
            kept.push(t);
        }
    }
    targets = kept;
    if targets.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 2. Type immunity.
    targets.retain(|&t| !type_immune(b, mv, t));
    if targets.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 3. Move-specific immunities: powder, the move's `onTryImmunity`, Prankster vs Dark.
    targets.retain(|&t| {
        let powder = mv.data.flags.contains(MoveFlags::POWDER)
            && t != user
            && b.natural_immune(t, TypeImmunities::POWDER);
        let prankster = mv.prankster_boosted
            && t.side != user.side
            && b.natural_immune(t, TypeImmunities::PRANKSTER);
        !powder && handlers::on_try_immunity(b, mv, t) && !prankster
    });
    if targets.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 4. Accuracy.
    let mut hit = Vec::with_capacity(targets.len());
    for &t in &targets {
        if accuracy_check(b, user, mv, t) {
            hit.push(t);
        }
    }
    if hit.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // The hit's first step is the move's own `onTryHit` (Champions `spreadMoveHit`: on the
    // first target only; failing fails the move).
    if !handlers::on_try_hit(b, user, hit[0], mv) {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 7. The hit loop.
    let progress = MoveProgress {
        user,
        pokemon: b.occupant(user).expect("checked"),
        mv: mv.clone(),
        targets: hit,
        main_target: user,
        hits: decide_hits(b, user, mv),
        hit: 0,
        total_damage: 0,
        any_ok: false,
        ignore_ability: b.active_move.is_some_and(|a| a.ignore_ability),
    };
    hit_loop(b, user, mv, Some(progress))
}

/// How many times the move hits (`hitStepMoveHitLoop`): 1, a fixed count, or for 2–5 hit
/// moves Showdown's 35/35/15/15 draw (Skill Link: always the maximum; Loaded Dice: 4 or 5
/// evenly, and 4–10 evenly for a 10-hit move).
fn decide_hits<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) -> u8 {
    let Some((low, high)) = mv.data.multihit else {
        return 1;
    };
    let skill_link = b.ability(user) == abilities::SKILL_LINK;
    let loaded_dice = b.item(user) == items::LOADED_DICE;
    let hits = if low == high || skill_link {
        high
    } else if (low, high) == (2, 5) {
        let drawn = [2, 3, 4, 5][b.rng.weighted(&[0.35, 0.35, 0.15, 0.15])];
        if drawn < 4 && loaded_dice {
            5 - b.rng.uniform(2) as u8
        } else {
            drawn
        }
    } else {
        low + b.rng.uniform(usize::from(high - low + 1)) as u8
    };
    if hits == 10 && loaded_dice {
        hits - b.rng.uniform(7) as u8
    } else {
        hits
    }
}

/// The user's ability's `onPrepareHit` (`runEvent('PrepareHit')`, after the move's own
/// PrepareHit). Protean and Libero: once per switch-in (`abilityState.protean` / `.libero`,
/// kept as [`Volatile::ProteanUsed`], which a switch resets), the user becomes the move's type
/// (`setType`), unless it already is exactly that type (`getTypes().join() !== type`, no flag
/// set then) or is Arceus or Silvally (`setType` fails). Terastallization, which also blocks
/// it, is not modelled; moves that call other moves or bounce are not supported.
fn prepare_hit_ability<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) {
    let ability = b.ability(user);
    if ability != abilities::PROTEAN && ability != abilities::LIBERO {
        return;
    }
    if b.volatile(user, Volatile::ProteanUsed).active {
        return;
    }
    let pokemon = b.occupant(user).expect("the user is active");
    let mon = b.mon(pokemon);
    let new = [mv.move_type, Type::None];
    if mon.types == new || [493, 773].contains(&mon.species.data().num) {
        return;
    }
    let old = mon.types;
    b.apply(crate::instruction::Instruction::SetTypes {
        target: pokemon,
        old,
        new,
    });
    b.add_volatile(user, Volatile::ProteanUsed);
}

/// Stall's `onStallMove`: success with probability 1/counter; a failure removes the counter.
fn stall_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) -> bool {
    let stall = b.volatile(user, Volatile::Stall);
    if !stall.active {
        return true;
    }
    let counter = u32::from(stall.counter.max(1));
    let success = b.rng.chance(1, counter);
    if !success {
        b.remove_volatile(user, Volatile::Stall);
    }
    success
}

/// The TryHit handlers for one target; `false` = the move fails on it.
fn try_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    target: SlotRef,
) -> bool {
    if blocked_by_try_hit(b, user, mv, target) {
        return false;
    }
    // The target's item `onTryHit` (Safety Goggles against powder).
    if item_events::try_hit_blocks(b, user, mv.data, target) {
        return false;
    }
    // Dry Skin `onTryHit` (breakable): another Pok챕mon's Water move heals the holder by 1/4
    // of its max HP (nothing at full HP) and fails on it (`return null`).
    if mv.move_type == Type::Water
        && target != user
        && b.ability_unless_broken(target) == abilities::DRY_SKIN
    {
        let max_hp = f64::from(b.slot_mon(target).expect("a target").max_hp);
        b.heal(target, max_hp / 4.0);
        return false;
    }
    // Lightning Rod / Storm Drain `onTryHit` (breakable): the holder absorbs the move.
    if absorbed_by_ability(b, user, mv, target) {
        return false;
    }
    // The other abilities' `onTryHit` (absorbing and immunity abilities).
    !ability_hooks::on_try_hit(b, user, mv, target)
}

fn blocked_by_try_hit<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    if b.terrain() == Terrain::Psychic
        && mv.priority > 0
        && mv.target != MoveTarget::User
        && target.side != user.side
        && b.is_grounded(target)
    {
        return true;
    }
    if b.volatile(target, Volatile::Protect).active && mv.data.flags.contains(MoveFlags::PROTECT) {
        return true;
    }
    // Wide Guard / Quick Guard on the target's side (`onTryHit`, priority 4): spread moves, or
    // moves with positive priority (after Prankster and the like), that Protect would block
    // (`checkMoveBypassesProtect`: the `protect` flag; status moves too). They also cover a
    // move from the target's own ally.
    if mv.data.flags.contains(MoveFlags::PROTECT) {
        let spread = matches!(
            mv.target,
            MoveTarget::AllAdjacent | MoveTarget::AllAdjacentFoes
        );
        if spread && b.side_effect_active(target.side, SideEffect::WideGuard) {
            return true;
        }
        if mv.priority > 0 && b.side_effect_active(target.side, SideEffect::QuickGuard) {
            return true;
        }
    }
    // Sturdy `onTryHit`: OHKO moves fail (breakable). OHKO moves are refused by `support`
    // for now; this keeps the immunity when they are added.
    mv.data.ohko != Ohko::No && b.ability_unless_broken(target) == abilities::STURDY
}

/// Lightning Rod / Storm Drain `onTryHit`: a move of the absorbed type aimed at the holder
/// (by anyone else) fails against it and raises its SpA one stage (no change at +6, only the
/// immune message differs).
fn absorbed_by_ability<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    if target == user || absorbing_type(b.ability_unless_broken(target)) != Some(mv.move_type) {
        return false;
    }
    let mut up = NO_BOOSTS;
    up[2] = 1;
    let absorbing = b.ability_unless_broken(target);
    b.boost_by(target, &up, Some(user), BoostEffect::Ability(absorbing));
    true
}

/// The move type an ability redirects and absorbs (Lightning Rod, Storm Drain).
fn absorbing_type(ability: AbilityId) -> Option<Type> {
    if ability == abilities::LIGHTNING_ROD {
        Some(Type::Electric)
    } else if ability == abilities::STORM_DRAIN {
        Some(Type::Water)
    } else {
        None
    }
}

/// Showdown `runImmunity(move)`: type chart immunity and Ground vs ungrounded.
fn type_immune<const N: usize>(b: &Battle<'_, N>, mv: &ActiveMove, target: SlotRef) -> bool {
    let ty = mv.move_type;
    match mv.data.ignore_immunity {
        IgnoreImmunity::All => return false,
        IgnoreImmunity::Type(t) if t == ty => return false,
        _ => {}
    }
    if ty == Type::Ground {
        return !b.is_grounded(target);
    }
    let Some(mon) = b.slot_mon(target) else {
        return true;
    };
    mon.types
        .iter()
        .any(|&t| ty.against(t) == TypeRelation::Immune)
}

/// Showdown `hitStepAccuracy` for one target.
fn accuracy_check<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    // `accuracy = true` without the `Accuracy` event: a status move on the user, and (gen 8+)
    // Toxic used by a Poison type.
    let self_status = mv.target == MoveTarget::User && mv.data.category == MoveCategory::Status;
    if self_status || (mv.id == moves::TOXIC && b.has_type(user, Type::Poison)) {
        return true;
    }
    // `runEvent('Accuracy')` (after ModifyAccuracy and the stages, which have no side effect
    // here): Micle Berry's `onSourceAccuracy` on the user ends the volatile, and chains
    // 4915/4096 onto a numeric accuracy (OHKO moves, which it skips, are refused).
    let micle = b.remove_volatile(user, Volatile::MicleBerry);
    let Some(base) = mv.accuracy else {
        return true;
    };
    let mut accuracy = i32::from(base);
    // ModifyAccuracy: Gravity (6840/4096), the user's Hustle and item (Wide Lens, Zoom Lens).
    let mut accuracy_mods = ability_events::accuracy_handlers(b, user, mv.data);
    accuracy_mods.extend(item_events::accuracy_handlers(b, user, target));
    if b.field_active(FieldEffect::Gravity) {
        accuracy_mods.push(Handler::global(0, SUB_FIELD_CONDITION, 6840));
    }
    accuracy = modify(accuracy, ability_events::chain(b, accuracy_mods));
    let mut boost = 0i32;
    if !mv.data.ignore_evasion {
        boost -= i32::from(b.boost_seen(target, 6, user, false));
    }
    boost += i32::from(b.boost_seen(user, 5, target, true));
    let boost = boost.clamp(-6, 6);
    if boost > 0 {
        accuracy = accuracy * (3 + boost) / 3;
    } else if boost < 0 {
        accuracy = accuracy * 3 / (3 - boost);
    }
    if micle {
        accuracy = modify(accuracy, 4915);
    }
    b.rng.chance(accuracy.max(0) as u32, 100)
}

/// Showdown `hitStepMoveHitLoop` for a single hit.
/// Showdown `hitStepMoveHitLoop`, one hit per call for a multi-hit move: the next hit
/// (`multiaccuracy` moves re-roll accuracy from the second hit), then either a suspension
/// (more hits to come, the user standing, a target standing) or the loop's tail: faints,
/// recoil on the total damage, and AfterMoveSecondary.
fn hit_loop<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    progress: Option<MoveProgress>,
) -> Result<HitOutcome, TurnError> {
    let mut progress = progress.expect("a hit loop starts with its progress");
    let hit = progress.hit + 1;
    let mut targets = progress.targets.clone();
    // A later hit of a multi-accuracy move (Population Bomb) can miss and end the loop.
    let mut ended_by_miss = false;
    let rerolls = mv.data.multiaccuracy
        && b.ability(user) != abilities::SKILL_LINK
        && b.item(user) != items::LOADED_DICE;
    if hit > 1 && rerolls && !targets.is_empty() {
        let first = targets[0];
        if !accuracy_check(b, user, mv, first) {
            ended_by_miss = true;
        }
    }
    let mut results = Vec::new();
    if !ended_by_miss {
        results = spread_move_hit(b, user, mv, &targets)?;
        progress.hit = hit;
        progress.total_damage += results
            .iter()
            .map(|r| if let Hit::Damage(d) = r { *d } else { 0 })
            .sum::<i32>();
        let hit_ok = results.iter().any(|r| r.ok());
        progress.any_ok |= hit_ok;
        // `eachEvent('Update')` after the hit's damage (berries eat before faints are
        // processed).
        super::update::update_event(b)?;
        targets.retain(|&t| b.alive(t).is_some());
        let single = progress.targets.len() == 1;
        let user_standing = b.alive(user).is_some();
        // `if (!pokemon.hp && targets.length === 1) break;` — a fainted user stops a
        // single-target move; every target fainted stops any.
        if hit_ok && hit < progress.hits && !targets.is_empty() && (user_standing || !single) {
            progress.targets = targets;
            return Ok(HitOutcome::Suspended(progress));
        }
    }
    // The loop ended: `faintMessages(false, false, !pokemon.hp)`, recoil, AfterMoveSecondary.
    let user_fainted = b.alive(user).is_none();
    b.faint_messages(user_fainted);
    let total = progress.total_damage;
    if total > 0 {
        if let Some(recoil) = mv.data.recoil {
            let amount = (f64::from(total) * f64::from(recoil.0) / f64::from(recoil.1))
                .round()
                .max(1.0);
            b.damage(user, amount, DamageSource::Recoil);
        }
    }
    if !progress.any_ok {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: total,
        });
    }
    // Champions `hitStepMoveHitLoop`: `eachEvent('Update')` after the recoil, then
    // AfterMoveSecondary (skipped for a Sheer Force-boosted move) for the targets of the last
    // hit it did not fail on (`targetsCopy`; after a later hit's miss, a fresh copy of every
    // target): a thawing move thaws a frozen target (`frz`'s handler), then the target's item
    // (Kee / Maranga Berry). Handlers of different targets act on their own holder only, so
    // their Speed order does not matter.
    super::update::update_event(b)?;
    let last_targets: Vec<SlotRef> = if ended_by_miss {
        progress.targets.clone()
    } else {
        progress
            .targets
            .iter()
            .zip(&results)
            .filter(|(_, r)| r.ok())
            .map(|(&t, _)| t)
            .collect()
    };
    if !ability_hooks::sheer_force_skips(b, user, mv) {
        for t in last_targets {
            if mv.data.thaws_target {
                if let Some(p) = b.alive(t) {
                    if b.mon(p).status == Status::Freeze {
                        b.cure_status(p);
                    }
                }
            }
            item_events::after_move_secondary(b, t, mv.data.category);
        }
    }
    Ok(HitOutcome::Finished {
        ok: true,
        total_damage: total,
    })
}

/// Showdown `spreadMoveHit` for the move's own hit.
fn spread_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    targets: &[SlotRef],
) -> Result<Vec<Hit>, TurnError> {
    let data = mv.data;
    // `getMoveHitData(move).typeMod` is (re)computed by this hit's `getDamage`.
    b.hit_type_mod = [[None; N]; 2];
    // getSpreadDamage: every target's damage is decided before any is dealt.
    let mut planned = Vec::with_capacity(targets.len());
    for &t in targets {
        planned.push(get_damage(b, user, mv, t)?);
    }
    // spreadDamage.
    let mut results = Vec::with_capacity(targets.len());
    for (&t, plan) in targets.iter().zip(&planned) {
        let result = match *plan {
            Planned::Fail => Hit::Failed,
            Planned::NoDamage => Hit::Done,
            Planned::Damage(d) => {
                let dealt = b.damage(t, f64::from(d), DamageSource::Move);
                if dealt > 0 {
                    if let Some(drain) = data.drain {
                        let amount =
                            (f64::from(dealt) * f64::from(drain.0) / f64::from(drain.1)).round();
                        b.heal(user, amount);
                    }
                }
                Hit::Damage(dealt)
            }
        };
        results.push(result);
    }
    // runMoveEffects.
    for (i, &t) in targets.iter().enumerate() {
        if !results[i].ok() {
            continue;
        }
        let mut did: Option<bool> = None;
        let mut note = |r: bool| did = Some(did.unwrap_or(false) || r);
        if data.boosts != NO_BOOSTS && b.alive(t).is_some() {
            note(b.boost_by(t, &data.boosts, Some(user), BoostEffect::Move(mv.id)));
        }
        if let Some(heal) = data.heal {
            let target_mon = b.occupant(t).map(|p| b.mon(p));
            let full = target_mon.is_none_or(|m| m.hp >= m.max_hp);
            if full {
                results[i] = Hit::Failed;
                continue;
            }
            let max_hp = f64::from(target_mon.expect("checked").max_hp);
            let healed = b.heal(t, (max_hp * f64::from(heal.0) / f64::from(heal.1)).round());
            if healed == 0 {
                results[i] = Hit::Failed;
                continue;
            }
            note(true);
        }
        if data.status != Status::None {
            let ok = b.try_set_status(t, data.status);
            if !ok {
                results[i] = Hit::Failed;
                continue;
            }
            note(true);
        }
        if let Some(volatile) = Volatile::from_condition(data.volatile_status) {
            note(b.add_volatile(t, volatile));
        }
        // Protect / Detect `onHit`: the stall counter.
        if data.stalling_move {
            b.add_volatile(t, Volatile::Stall);
        }
        // Quash / After You `onHit`: reorder the target's pending move (fail in singles or
        // without one).
        if mv.id == moves::QUASH || mv.id == moves::AFTER_YOU {
            match b.will_move(t).filter(|_| N > 1) {
                Some(index) if mv.id == moves::QUASH => {
                    b.quash_action(index);
                    note(true);
                }
                Some(index) => {
                    b.prioritize_action(index);
                    note(true);
                }
                None => {
                    results[i] = Hit::Failed;
                    continue;
                }
            }
        }
        // The move's own onHit; NOT_FAIL neither succeeds nor fails.
        match handlers::on_hit(b, user, t, mv)? {
            Some(HitResult::Success) => note(true),
            Some(HitResult::Failure) => note(false),
            Some(HitResult::NotFail) | None => {}
        }
        // `runEvent('Hit')`: the target's item (Sticky Barb).
        item_events::on_hit(b, user, t, data);
        if let (Hit::Done, Some(false)) = (results[i], did) {
            results[i] = Hit::Failed;
        }
    }
    // selfDrops: boosts once, for the first target the move did not fail on; an effect
    // without boosts (Roost's, Outrage's volatile) is applied to the user for every such
    // target. Sheer Force deleted `self`; Serene Grace doubled its chance.
    if let Some(effect) = data.self_effect.filter(|_| !mv.has_sheer_force) {
        let chance = u32::from(effect.chance) * mv.secondary_chance_factor;
        if effect.boosts != NO_BOOSTS {
            if results.iter().any(|r| r.ok()) && b.rng.chance(chance, 100) {
                b.boost_by(user, &effect.boosts, Some(user), BoostEffect::Move(mv.id));
            }
        } else if let Some(volatile) = Volatile::from_condition(effect.volatile_status) {
            for _ in results.iter().filter(|r| r.ok()) {
                if b.add_volatile_from(user, volatile, mv.id) && volatile == Volatile::Roost {
                    conditions::roost_start(b, user);
                }
            }
        }
    }
    // secondaries: each target's ModifySecondaries (Shield Dust), then one roll per secondary
    // (Sheer Force deleted them; Serene Grace doubled the chances).
    for (i, &t) in targets.iter().enumerate() {
        if !results[i].ok() {
            continue;
        }
        // Secondaries: Sheer Force / Shield Dust (`ability_hooks::secondaries`) decide the
        // move's own, Serene Grace doubles their chance, Covert Cloak (`ModifySecondaries`)
        // drops some, and King's Rock's added flinch comes last (ModifyMove priority -1: after
        // Serene Grace, and even through Sheer Force).
        let own: Vec<(&Secondary, u32)> = ability_hooks::secondaries(b, mv, t)
            .into_iter()
            .map(|s| (s, u32::from(s.chance) * mv.secondary_chance_factor))
            .collect();
        let added: Vec<(&Secondary, u32)> = mv
            .added_secondary
            .iter()
            .map(|s| (s, u32::from(s.chance)))
            .collect();
        for (secondary, chance) in own.into_iter().chain(added) {
            if !item_events::keeps_secondary(b, t, secondary) {
                continue;
            }
            if !b.rng.chance(chance, 100) {
                continue;
            }
            if secondary.boosts != NO_BOOSTS && b.alive(t).is_some() {
                b.boost_by(t, &secondary.boosts, Some(user), BoostEffect::Move(mv.id));
            }
            if secondary.status != Status::None {
                b.try_set_status(t, secondary.status);
            }
            if let Some(volatile) = Volatile::from_condition(secondary.volatile_status) {
                b.add_volatile(t, volatile);
            }
            handlers::secondary_on_hit(b, t, mv);
            if secondary.self_boosts != NO_BOOSTS {
                b.boost_by(
                    user,
                    &secondary.self_boosts,
                    Some(user),
                    BoostEffect::Move(mv.id),
                );
            }
        }
    }
    // DamagingHit for every damaged target, then AfterHit (only while the user stands).
    let damaged: Vec<(SlotRef, i32)> = targets
        .iter()
        .zip(&results)
        .filter_map(|(&t, r)| match r {
            Hit::Damage(d) => Some((t, *d)),
            _ => None,
        })
        .collect();
    if !damaged.is_empty() {
        damaging_hit(b, user, mv, &damaged);
    }
    // AfterHit: Knock Off removes the item of every damaged target.
    if mv.id == moves::KNOCK_OFF && b.alive(user).is_some() {
        for (i, &t) in targets.iter().enumerate() {
            if let Hit::Damage(_) = results[i] {
                b.take_item(t);
            }
        }
    }
    // AfterHit: the move's other `onAfterHit` handlers, per damaged target (Champions runs
    // them even if the user fainted).
    for result in &results {
        if let Hit::Damage(_) = result {
            handlers::on_after_hit(b, mv);
        }
    }
    Ok(results)
}

/// Showdown `runEvent('DamagingHit', damagedTargets, pokemon, move, damage)` (WORKPLAN F15):
/// the damaged targets' handlers sorted by `compareLeftToRightOrder` — `onDamagingHitOrder`
/// (Rough Skin / Iron Barbs 1, Rocky Helmet 2, the rest last), then target index, then a
/// Pokémon's own order (status, ability, item). A holder that fainted from the hit still acts
/// (`faintMessages` runs after the move); a handler is skipped once its holder left the slot or
/// the attacker it hits fainted earlier in the event. Implemented: the `frz` thaw by a Fire
/// move, Rough Skin, Iron Barbs, Rattled (the ability half), Rocky Helmet. Other
/// `onDamagingHit` holders are refused by `support`.
fn damaging_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    damaged: &[(SlotRef, i32)],
) {
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Kind {
        Thaw,
        Ability(AbilityId),
        Item(ItemId),
    }
    const LAST: u32 = u32::MAX;
    let mut handlers: Vec<(u32, usize, Kind)> = Vec::new();
    for (index, &(target, _)) in damaged.iter().enumerate() {
        let Some(pokemon) = b.occupant(target) else {
            continue;
        };
        let mon = b.mon(pokemon);
        if mon.status == Status::Freeze
            && mv.move_type == Type::Fire
            && mv.data.category != MoveCategory::Status
        {
            handlers.push((LAST, index, Kind::Thaw));
        }
        let ability = b.ability_unless_broken(target);
        if [abilities::ROUGH_SKIN, abilities::IRON_BARBS].contains(&ability) {
            handlers.push((1, index, Kind::Ability(ability)));
        } else if ability == abilities::RATTLED {
            handlers.push((LAST, index, Kind::Ability(ability)));
        }
        if mon.item == items::ROCKY_HELMET {
            handlers.push((2, index, Kind::Item(mon.item)));
        } else if mon.item == items::AIR_BALLOON || item_events::has_damaging_hit(mon.item) {
            handlers.push((LAST, index, Kind::Item(mon.item)));
        }
    }
    handlers.sort();
    let contact =
        mv.data.flags.contains(MoveFlags::CONTACT) && b.item(user) != items::PROTECTIVE_PADS;
    for (_, index, kind) in handlers {
        let target = damaged[index].0;
        let Some(pokemon) = b.occupant(target) else {
            continue;
        };
        match kind {
            Kind::Thaw => {
                if b.mon(pokemon).status == Status::Freeze {
                    b.cure_status(pokemon);
                }
            }
            Kind::Ability(a) if a == abilities::ROUGH_SKIN || a == abilities::IRON_BARBS => {
                if contact && b.alive(user).is_some() {
                    let max_hp = f64::from(b.slot_mon(user).expect("alive").max_hp);
                    b.damage(user, max_hp / 8.0, DamageSource::Indirect);
                }
            }
            Kind::Ability(a) if a == abilities::RATTLED => {
                if matches!(mv.move_type, Type::Dark | Type::Bug | Type::Ghost) {
                    let mut up = NO_BOOSTS;
                    up[4] = 1;
                    b.boost_by(target, &up, Some(target), BoostEffect::Ability(a));
                }
            }
            Kind::Ability(_) => {}
            Kind::Item(i) if i == items::ROCKY_HELMET => {
                if contact && b.alive(user).is_some() {
                    let max_hp = f64::from(b.slot_mon(user).expect("alive").max_hp);
                    b.damage(user, max_hp / 6.0, DamageSource::Indirect);
                }
            }
            Kind::Item(i) if i == items::AIR_BALLOON => {
                // `target.item = ''` without `useItem`: no `lastItem`; AfterUseItem
                // (Unburden) is not implemented.
                b.apply(crate::instruction::Instruction::SetItem {
                    target: pokemon,
                    old: items::AIR_BALLOON,
                    new: ItemId::NONE,
                });
            }
            // Weakness Policy, the absorbing items, Jaboca / Rowap Berry. The item may have
            // gone since the handlers were collected (a Jaboca Berry is eaten once).
            Kind::Item(i) => {
                if b.mon(pokemon).item == i {
                    item_events::on_damaging_hit(
                        b,
                        user,
                        target,
                        i,
                        mv.move_type,
                        mv.data.category,
                    );
                }
            }
        }
    }
}

/// Showdown `field.clearTerrain()`: the terrain ends at once (its `FieldEnd` only logs), then
/// `eachEvent('TerrainChange')` (`field_events`). Returns whether there was a terrain.
pub(crate) fn clear_terrain<const N: usize>(b: &mut Battle<'_, N>) -> bool {
    if b.terrain() == Terrain::None {
        return false;
    }
    b.set_field(FieldEffect::Terrain, Effect::NONE);
    super::field_events::terrain_changed(b);
    true
}

// ---- damage -------------------------------------------------------------------------------------

enum Planned {
    Fail,
    NoDamage,
    Damage(i32),
}

/// Showdown `getDamage` + `modifyDamage`; the crit and the damage roll are decided here.
fn get_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> Result<Planned, TurnError> {
    let data = mv.data;
    if type_immune(b, mv, target) {
        return Ok(Planned::Fail);
    }
    let attacker = b.slot_mon(user).expect("checked").clone();
    let defender = b.slot_mon(target).expect("alive").clone();
    match data.fixed_damage {
        Some(FixedDamage::Level) => return Ok(Planned::Damage(i32::from(attacker.level))),
        Some(FixedDamage::Hp(hp)) => return Ok(Planned::Damage(i32::from(hp))),
        None => {}
    }
    let mut base_power = mv.base_power;
    if mv.id == moves::LOW_KICK || mv.id == moves::GRASS_KNOT {
        base_power = weight_power(defender.species.data().weight_hg);
    }
    base_power = handlers::base_power_callback(b, user, target, mv, base_power);
    if base_power == 0 {
        return Ok(Planned::NoDamage);
    }

    // Critical hit: ratio 1..4 ??1/24, 1/8, 1/2, always. `CriticalHit` handlers: Battle Armor
    // and Shell Armor (`onCriticalHit: false`, breakable). Showdown rolls first and then
    // cancels; not rolling gives the same distribution.
    // ModifyCritRatio: the user's item (Scope Lens, Razor Claw) and its `focusenergy` volatile
    // (+2, from Lansat Berry), all additive, then clamped to 0..4. Lucky Chant on the target's
    // side is a `CriticalHit` handler too (`onCriticalHit: false`).
    let focus_energy = if b.volatile(user, Volatile::FocusEnergy).active {
        2
    } else {
        0
    };
    let crit_ratio =
        (i32::from(data.crit_ratio) + item_events::crit_ratio_bonus(b.item(user)) + focus_energy)
            .clamp(0, 4);
    let can_crit = !b.ability_unless_broken(target).data().cannot_be_crit
        && !b.side_effect_active(target.side, SideEffect::LuckyChant);
    let critical = can_crit
        && (data.will_crit
            || match crit_ratio {
                0 => false,
                1 => b.rng.chance(1, 24),
                2 => b.rng.chance(1, 8),
                3 => b.rng.chance(1, 2),
                _ => true,
            });

    // BasePower handlers: abilities (Technician 30 ... Punk Rock 7), type items (15),
    // terrain (6), the move (0).
    let mut power_mods =
        ability_events::base_power_handlers(b, user, target, data, mv.move_type, base_power);
    if type_boost_item(attacker.item) == Some(mv.move_type) {
        power_mods.push(Handler::of(b, user, 15, SUB_ITEM, MOD_ONE_POINT_TWO));
    }
    let attacker_grounded = b.is_grounded(user);
    let defender_grounded = b.is_grounded(target);
    let terrain_mod = match b.terrain() {
        Terrain::Grassy => {
            if [moves::EARTHQUAKE, moves::BULLDOZE, moves::MAGNITUDE].contains(&mv.id)
                && defender_grounded
            {
                MOD_HALF
            } else if mv.move_type == Type::Grass && attacker_grounded {
                MOD_ONE_POINT_THREE
            } else {
                MOD_ONE
            }
        }
        Terrain::Electric if mv.move_type == Type::Electric && attacker_grounded => {
            MOD_ONE_POINT_THREE
        }
        Terrain::Psychic if mv.move_type == Type::Psychic && attacker_grounded => {
            MOD_ONE_POINT_THREE
        }
        Terrain::Misty if mv.move_type == Type::Dragon && defender_grounded => MOD_HALF,
        _ => MOD_ONE,
    };
    power_mods.push(Handler::global(6, SUB_FIELD_CONDITION, terrain_mod));
    if mv.id == moves::KNOCK_OFF && b.item_can_be_taken(target) {
        power_mods.push(Handler::of(b, user, 0, SUB_MOVE, MOD_ONE_POINT_FIVE));
    }
    if let Some(modifier) = handlers::on_base_power(b, user, mv) {
        power_mods.push(Handler::of(b, user, 0, SUB_MOVE, modifier));
    }
    let power_modifier = ability_events::chain(b, power_mods);

    // Attack and defense.
    let physical = data.category == MoveCategory::Physical;
    // Wonder Room's `onModifyMove`: a move attacking with Def or SpD (Body Press) takes the
    // other one's stages (`overrideOffensiveStat` swapped); `calculateStat` then swaps the
    // stored Def and SpD it reads ([`stored_stat_index`]), while the stages and the Modify*
    // handlers stay those of the named stat.
    let wonder_room = b.field_active(FieldEffect::WonderRoom);
    let attack_stat = match data.override_offensive_stat {
        Some(Stat::Def) if wonder_room => Stat::Spd,
        Some(Stat::Spd) if wonder_room => Stat::Def,
        Some(stat) => stat,
        None if physical => Stat::Atk,
        None => Stat::Spa,
    };
    let defense_stat =
        data.override_defensive_stat
            .unwrap_or(if physical { Stat::Def } else { Stat::Spd });
    // `overrideOffensivePokemon: 'target'` (Foul Play): the target's stat and stages are used
    // (`attacker.calculateStat`); the ModifyAtk handlers stay the user's. Unaware
    // (`boost_seen`) sees the same stages Showdown's `ModifyBoost` would.
    let (offensive, offensive_mon) = if data.override_offensive_pokemon_target {
        (target, &defender)
    } else {
        (user, &attacker)
    };
    let mut atk_boost = b.boost_seen(offensive, stat_index(attack_stat), target, true);
    let mut def_boost = b.boost_seen(target, stat_index(defense_stat), user, false);
    let ignore_negative_offensive = data.ignore_negative_offensive || critical;
    let ignore_positive_defensive = data.ignore_positive_defensive || critical;
    if data.ignore_offensive || (ignore_negative_offensive && atk_boost < 0) {
        atk_boost = 0;
    }
    if data.ignore_defensive || (ignore_positive_defensive && def_boost > 0) {
        def_boost = 0;
    }
    let attack = boosted_stat(
        i32::from(offensive_mon.stats[stored_stat_index(attack_stat, wonder_room)]),
        atk_boost,
    );
    // ModifyAtk (physical) / ModifySpA (special), whatever stat the move attacks with.
    let attack = ability_events::attack_direct(attacker.ability, data, attack);
    let mut attack_mods = ability_events::attack_handlers(b, user, target, data, mv.move_type);
    attack_mods.extend(item_events::attack_handlers(b, user, data));
    let attack = modify(attack, ability_events::chain(b, attack_mods));
    let mut defense = boosted_stat(
        i32::from(defender.stats[stored_stat_index(defense_stat, wonder_room)]),
        def_boost,
    );
    // ModifyDef / ModifySpD: sandstorm (Rock SpD) and snow (Ice Def), 1.5x applied directly.
    let weather = b.effective_weather();
    if defense_stat == Stat::Spd && weather == Weather::Sand && defender.types.contains(&Type::Rock)
    {
        defense = modify(defense, MOD_ONE_POINT_FIVE);
    }
    if defense_stat == Stat::Def && weather == Weather::Snow && defender.types.contains(&Type::Ice)
    {
        defense = modify(defense, MOD_ONE_POINT_FIVE);
    }
    // Chained ModifyDef / ModifySpD handlers, applied after the direct weather boosts.
    let mut defense_mods = ability_events::defense_handlers(b, user, target, data, defense_stat);
    defense_mods.extend(item_events::defense_handlers(b, target, defense_stat));
    let defense = modify(defense, ability_events::chain(b, defense_mods));

    // modifyDamage inputs.
    let weather_modifier = match (weather, mv.move_type) {
        (Weather::Sun, Type::Fire) | (Weather::Rain, Type::Water) => MOD_ONE_POINT_FIVE,
        (Weather::Sun, Type::Water) | (Weather::Rain, Type::Fire) => MOD_HALF,
        _ => MOD_ONE,
    };
    let stab = data.force_stab || attacker.types.contains(&mv.move_type);
    let stab_modifier = ability_events::modify_stab(attacker.ability, stab);
    // runEffectiveness: per defending type, the chart then the move's onEffectiveness.
    let type_mod: i32 = defender
        .types
        .iter()
        .filter(|&&t| t != Type::None)
        .map(|&t| {
            let chart = handlers::type_effectiveness(mv.move_type, t);
            let by_move = handlers::on_effectiveness(mv.id, t, chart);
            item_events::on_effectiveness(b, target, mv.move_type, by_move)
        })
        .sum::<i32>()
        .clamp(-6, 6);
    b.hit_type_mod[target.side.index()][usize::from(target.slot)] = Some(type_mod as i8);
    let type_effectiveness = if type_mod >= 0 {
        MOD_ONE << type_mod
    } else {
        MOD_ONE >> -type_mod
    };
    // ModifyDamage (all priority 0, so in Speed order): the target's abilities, items (Life
    // Orb, resist berries), screens (side conditions, Speed 0).
    let mut final_mods =
        ability_events::modify_damage_handlers(b, user, target, data, mv.move_type, type_mod);
    final_mods.extend(item_events::modify_damage_handlers(
        b, user, target, data, type_mod,
    ));
    if !critical && target != user && screen_applies(b, target.side, data.category) {
        let modifier = if N > 1 { 2732 } else { MOD_HALF };
        final_mods.push(Handler::global(0, SUB_SIDE_CONDITION, modifier));
    }
    let final_modifier = ability_events::chain(b, final_mods);
    let input = DamageInput {
        level: attacker.level,
        base_power: base_power as u16,
        attack: attack.clamp(1, i32::from(u16::MAX)) as u16,
        defense: defense.clamp(1, i32::from(u16::MAX)) as u16,
        base_power_modifier: power_modifier,
        spread: mv.spread,
        weather_modifier,
        critical,
        stab_modifier,
        type_effectiveness,
        burned: ability_events::burn_halves(&attacker, data),
        protected: false,
        final_modifier,
    };
    let rolls = damage_rolls(input);
    Ok(Planned::Damage(i32::from(pick_roll(b, &rolls))))
}

/// One of the 16 equally likely rolls, branching once per distinct value.
fn pick_roll<const N: usize>(b: &mut Battle<'_, N>, rolls: &[u16; 16]) -> u16 {
    let mut values: Vec<(u16, u32)> = Vec::with_capacity(16);
    for &r in rolls {
        match values.iter_mut().find(|(v, _)| *v == r) {
            Some((_, count)) => *count += 1,
            None => values.push((r, 1)),
        }
    }
    if values.len() == 1 {
        return values[0].0;
    }
    let weights: Vec<f64> = values.iter().map(|&(_, c)| f64::from(c) / 16.0).collect();
    values[b.rng.weighted(&weights)].0
}

fn screen_applies<const N: usize>(b: &Battle<'_, N>, side: SideId, category: MoveCategory) -> bool {
    let reflect = b.side_effect_active(side, SideEffect::Reflect);
    let light_screen = b.side_effect_active(side, SideEffect::LightScreen);
    match category {
        MoveCategory::Physical if reflect => true,
        MoveCategory::Special if light_screen => true,
        MoveCategory::Status => false,
        // Aurora Veil does not stack with the matching screen.
        _ => b.side_effect_active(side, SideEffect::AuroraVeil),
    }
}

/// The stored stat `calculateStat(stat)` reads: under Wonder Room Def and SpD trade places
/// ("Wonder Room swaps defenses before calculating anything else").
fn stored_stat_index(stat: Stat, wonder_room: bool) -> usize {
    match stat_index(stat) {
        1 if wonder_room => 3,
        3 if wonder_room => 1,
        i => i,
    }
}

fn stat_index(stat: Stat) -> usize {
    match stat {
        Stat::Atk => 0,
        Stat::Def => 1,
        Stat::Spa => 2,
        Stat::Spd => 3,
        Stat::Spe => 4,
        Stat::Hp => unreachable!("HP is not a battle stat"),
    }
}

/// Low Kick / Grass Knot base power from the target's weight (hectograms).
fn weight_power(weight_hg: u16) -> i32 {
    match weight_hg.max(1) {
        w if w >= 2000 => 120,
        w if w >= 1000 => 100,
        w if w >= 500 => 80,
        w if w >= 250 => 60,
        w if w >= 100 => 40,
        _ => 20,
    }
}

// ---- field effects ------------------------------------------------------------------------------

fn weather_of(id: &str) -> Option<Weather> {
    Some(match id {
        "sunnyday" => Weather::Sun,
        "raindance" => Weather::Rain,
        "sandstorm" => Weather::Sand,
        "snowscape" => Weather::Snow,
        _ => return None,
    })
}

fn terrain_of(id: &str) -> Option<Terrain> {
    Some(match id {
        "electricterrain" => Terrain::Electric,
        "grassyterrain" => Terrain::Grassy,
        "mistyterrain" => Terrain::Misty,
        "psychicterrain" => Terrain::Psychic,
        _ => return None,
    })
}

/// Showdown `field.setWeather` from a move or an ability: the same weather again fails;
/// 5 turns, 8 with the matching rock; then `eachEvent('WeatherChange')` (`field_events`).
pub(crate) fn set_weather<const N: usize>(
    b: &mut Battle<'_, N>,
    source: SlotRef,
    weather: Weather,
) -> bool {
    if b.weather() == weather {
        return false;
    }
    let rock = match weather {
        Weather::Sun => items::HEAT_ROCK,
        Weather::Rain => items::DAMP_ROCK,
        Weather::Sand => items::SMOOTH_ROCK,
        Weather::Snow => items::ICY_ROCK,
        _ => unreachable!("only the four weathers are set"),
    };
    let turns = if b.item(source) == rock { 8 } else { 5 };
    b.set_field(
        FieldEffect::Weather,
        Effect {
            value: weather as u8,
            turns,
        },
    );
    super::field_events::weather_changed(b);
    true
}

/// Showdown `field.setTerrain`: the same terrain again fails; 5 turns, 8 with Terrain
/// Extender; then `eachEvent('TerrainChange')` (`field_events`: the Seeds).
pub(crate) fn set_terrain<const N: usize>(
    b: &mut Battle<'_, N>,
    source: SlotRef,
    terrain: Terrain,
) -> bool {
    if b.terrain() == terrain {
        return false;
    }
    let turns = if b.item(source) == items::TERRAIN_EXTENDER {
        8
    } else {
        5
    };
    b.set_field(
        FieldEffect::Terrain,
        Effect {
            value: terrain as u8,
            turns,
        },
    );
    super::field_events::terrain_changed(b);
    true
}

/// Showdown `addPseudoWeather`: Gravity fails if up; Trick Room and Wonder Room end
/// themselves on restart (`onFieldRestart`, no PseudoWeatherChange); a new one (5 turns:
/// Persistent, which makes Trick Room and Wonder Room last 7, is refused) runs
/// `PseudoWeatherChange`.
fn add_pseudo_weather<const N: usize>(b: &mut Battle<'_, N>, id: &str) -> bool {
    let effect = match id {
        "gravity" => FieldEffect::Gravity,
        "trickroom" => FieldEffect::TrickRoom,
        "wonderroom" => FieldEffect::WonderRoom,
        _ => unreachable!("checked by support"),
    };
    if b.field_active(effect) {
        if effect == FieldEffect::TrickRoom || effect == FieldEffect::WonderRoom {
            b.set_field(effect, Effect::NONE);
            return true;
        }
        return false;
    }
    b.set_field(effect, Effect { value: 0, turns: 5 });
    // `runEvent('PseudoWeatherChange')`: Room Service.
    item_events::pseudo_weather_change(b);
    true
}

/// Showdown `addSideCondition` (fails if already up; none of these has `onSideRestart`):
/// Tailwind 4 turns, the screens 5 (Light Clay 8), Safeguard 5 (Persistent, which makes it 7,
/// is refused), Mist and Lucky Chant 5, Wide Guard and Quick Guard 1.
fn add_side_condition<const N: usize>(
    b: &mut Battle<'_, N>,
    source: SlotRef,
    side: SideId,
    effect: SideEffect,
) -> bool {
    if b.side_effect_active(side, effect) {
        return false;
    }
    let turns = match effect {
        SideEffect::Tailwind => 4,
        SideEffect::WideGuard | SideEffect::QuickGuard => 1,
        SideEffect::Reflect | SideEffect::LightScreen | SideEffect::AuroraVeil
            if b.item(source) == items::LIGHT_CLAY =>
        {
            8
        }
        _ => 5,
    };
    b.set_side_effect(side, effect, Effect { value: 0, turns });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SideId;

    const P1A: SlotRef = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    const P1B: SlotRef = SlotRef {
        side: SideId::One,
        slot: 1,
    };

    #[test]
    fn target_locations_follow_showdown() {
        // Doubles: every foe is adjacent; the ally is -2 from slot a.
        assert!(valid_target_loc(2, P1A, 1, MoveTarget::Normal));
        assert!(valid_target_loc(2, P1A, 2, MoveTarget::Normal));
        assert!(valid_target_loc(2, P1A, -2, MoveTarget::Normal));
        // Its own position is adjacent-looking but getTarget rejects it separately.
        assert_eq!(loc_of(P1A, P1A), -1);
        assert!(valid_target_loc(2, P1B, -1, MoveTarget::AdjacentAlly));
        assert!(!valid_target_loc(2, P1B, 1, MoveTarget::AdjacentAlly));
        assert!(!valid_target_loc(2, P1A, 3, MoveTarget::Normal));
        assert_eq!(
            at_loc(P1A, 2),
            SlotRef {
                side: SideId::Two,
                slot: 1
            }
        );
        assert_eq!(at_loc(P1A, -2), P1B);
        assert_eq!(loc_of(P1A, P1B), -2);
    }

    #[test]
    fn weight_brackets() {
        assert_eq!(weight_power(2020), 120);
        assert_eq!(weight_power(1000), 100);
        assert_eq!(weight_power(999), 80);
        assert_eq!(weight_power(99), 20);
    }
}
