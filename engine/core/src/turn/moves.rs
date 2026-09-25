//! Using a move: Showdown `runMove` → `useMove` → `trySpreadMoveHit` / `tryMoveHit` → the
//! hit steps → `spreadMoveHit` (damage, effects, secondaries) → recoil and after-move
//! effects, for the implemented moves (see [`super::support`]).

use crate::damage::{
    damage_rolls, DamageInput, MOD_HALF, MOD_ONE, MOD_ONE_POINT_FIVE, MOD_ONE_POINT_THREE,
    MOD_ONE_POINT_TWO,
};
use crate::dex::{
    items, moves, FixedDamage, IgnoreImmunity, MoveCategory, MoveData, MoveFlags, MoveId,
    MoveTarget, Stat, Type, TypeImmunities, TypeRelation, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::state::{SideId, SlotRef, Status};
use crate::volatile::Volatile;

use super::abilities::{
    self, Handler, SUB_FIELD_CONDITION, SUB_ITEM, SUB_MOVE, SUB_SIDE_CONDITION,
};
use super::battle::{Battle, DamageSource};
use super::order::{boosted_stat, modify};
use super::support::{side_effect_of, type_boost_item};
use super::TurnError;

/// The move being used, with what is decided when it is used.
struct ActiveMove {
    id: MoveId,
    data: &'static MoveData,
    /// Priority after ModifyPriority (Showdown sets `move.priority` to it).
    priority: i32,
    prankster_boosted: bool,
    spread: bool,
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

/// Showdown `runMove` for the move in `move_index`. `will_act` is `queue.willAct()`.
pub(crate) fn run_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    move_index: u8,
    target_loc: i8,
    will_act: bool,
) -> Result<(), TurnError> {
    let pokemon = b.occupant(user).expect("the caller checked the user");
    let id = b.mon(pokemon).moves[move_index as usize].id;
    b.increment_move_actions(user);
    let target = get_target(b, user, id, target_loc);
    let mut mv = ActiveMove {
        id,
        data: id.data(),
        priority: b.move_priority(user, id),
        prankster_boosted: b.prankster_boosted(user, id),
        spread: false,
    };

    if !before_move(b, user, &mv) {
        return Ok(());
    }

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
    b.set_last_move(user, id);

    use_move(b, user, &mut mv, target, will_act)?;
    b.faint_messages(true);
    b.check_win(None);
    Ok(())
}

/// The BeforeMove handlers, by priority: sleep and freeze (10), flinch (8), Gravity (6),
/// paralysis (1). `false` = the move is not used (no PP, no `lastMove`).
fn before_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) -> bool {
    let pokemon = b.occupant(user).expect("checked");
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
    // Champions paralysis: 1/8.
    if b.mon(pokemon).status == Status::Paralyze && b.rng.chance(1, 8) {
        return false;
    }
    true
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

/// Showdown `getTarget`. The returned slot may hold a fainted Pokémon (Showdown returns
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

/// Showdown `getMoveTargets` (no redirection is implemented).
fn get_move_targets<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> Vec<SlotRef> {
    match mv.data.target {
        MoveTarget::All | MoveTarget::FoeSide | MoveTarget::AllySide | MoveTarget::AllyTeam => {
            Vec::new()
        }
        MoveTarget::AllAdjacent => {
            let mut t = adjacent_allies(b, user);
            t.extend(b.alive_slots(user.side.other()));
            t
        }
        MoveTarget::AllAdjacentFoes => b.alive_slots(user.side.other()),
        _ => {
            let mut t = target;
            if b.alive(t).is_none() && t.side != user.side {
                match get_random_target(b, user, mv.data.target) {
                    Some(r) => t = r,
                    None => return Vec::new(),
                }
            }
            if b.alive(t).is_none() {
                return Vec::new();
            }
            vec![t]
        }
    }
}

// ---- use ---------------------------------------------------------------------------------------

/// Showdown `useMoveInner`. Returns whether the move succeeded.
fn use_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    target: Option<SlotRef>,
    will_act: bool,
) -> Result<bool, TurnError> {
    let pokemon = b.occupant(user).expect("checked");
    let target = if mv.data.target == MoveTarget::User {
        Some(user)
    } else {
        target
    };
    // Freeze `onModifyMove`: a defrosting move thaws the user.
    if b.mon(pokemon).status == Status::Freeze && mv.data.flags.contains(MoveFlags::DEFROST) {
        b.cure_status(pokemon);
    }
    let Some(target) = target else {
        return Ok(false);
    };

    let result;
    let mut main_target = target;
    if matches!(
        mv.data.target,
        MoveTarget::All | MoveTarget::FoeSide | MoveTarget::AllySide
    ) {
        result = try_move_hit_field(b, user, mv, target)?;
    } else {
        let targets = get_move_targets(b, user, mv, target);
        let Some(&last) = targets.last() else {
            return Ok(false);
        };
        main_target = last;
        result = try_spread_move_hit(b, user, mv, targets, will_act)?;
    }
    if result && mv.data.self_boost != NO_BOOSTS {
        b.boost(user, &mv.data.self_boost);
    }
    if !result {
        return Ok(false);
    }
    // AfterMoveSecondarySelf: Life Orb.
    if b.item(user) == items::LIFE_ORB
        && mv.data.category != MoveCategory::Status
        && main_target != user
        && b.alive(user).is_some()
    {
        let max_hp = f64::from(b.mon(pokemon).max_hp);
        b.damage(user, max_hp / 10.0, DamageSource::Indirect);
    }
    Ok(true)
}

/// Showdown `tryMoveHit` → `moveHit` for field and side moves.
fn try_move_hit_field<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> Result<bool, TurnError> {
    let data = mv.data;
    if mv.id == moves::AURORA_VEIL && b.weather() != Weather::Snow {
        return Ok(false);
    }
    // runMoveEffects on the target: undefined (nothing attempted) counts as success.
    let mut outcome: Option<bool> = None;
    let mut combine = |r: bool| outcome = Some(outcome.unwrap_or(false) || r);
    if !data.side_condition.is_none() {
        let side = target.side;
        let effect = side_effect_of(data.side_condition.id()).expect("checked by support");
        combine(add_side_condition(b, user, side, effect));
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
    Ok(outcome.unwrap_or(true))
}

/// Showdown `trySpreadMoveHit` (also for single-target moves).
fn try_spread_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    mut targets: Vec<SlotRef>,
    will_act: bool,
) -> Result<bool, TurnError> {
    mv.spread = targets.len() > 1;

    // Try: Fake Out only works on the first action after switching in.
    if mv.id == moves::FAKE_OUT && b.state.slot(user).move_actions > 1 {
        return Ok(false);
    }
    // PrepareHit: Protect and Detect need a later action and pass the stall check.
    if mv.data.stalling_move && !(will_act && stall_move(b, user)) {
        return Ok(false);
    }

    // 1. TryHit: Psychic Terrain (priority 4), then Protect (3).
    targets.retain(|&t| !blocked_by_try_hit(b, user, mv, t));
    if targets.is_empty() {
        return Ok(false);
    }
    // 2. Type immunity.
    targets.retain(|&t| !type_immune(b, mv, t));
    if targets.is_empty() {
        return Ok(false);
    }
    // 3. Move-specific immunities: powder, Prankster vs Dark.
    targets.retain(|&t| {
        let powder = mv.data.flags.contains(MoveFlags::POWDER)
            && t != user
            && b.status_immune(t, TypeImmunities::POWDER);
        let prankster = mv.prankster_boosted
            && t.side != user.side
            && b.status_immune(t, TypeImmunities::PRANKSTER);
        !powder && !prankster
    });
    if targets.is_empty() {
        return Ok(false);
    }
    // 4. Accuracy.
    let mut hit = Vec::with_capacity(targets.len());
    for &t in &targets {
        if accuracy_check(b, user, mv, t) {
            hit.push(t);
        }
    }
    if hit.is_empty() {
        return Ok(false);
    }
    // 7. The hit (single-hit moves only).
    let results = hit_loop(b, user, mv, &hit)?;
    Ok(results.iter().any(|r| r.ok()))
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

fn blocked_by_try_hit<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    if b.terrain() == Terrain::Psychic
        && mv.priority > 0
        && mv.data.target != MoveTarget::User
        && target.side != user.side
        && b.is_grounded(target)
    {
        return true;
    }
    b.volatile(target, Volatile::Protect).active && mv.data.flags.contains(MoveFlags::PROTECT)
}

/// Showdown `runImmunity(move)`: type chart immunity and Ground vs ungrounded.
fn type_immune<const N: usize>(b: &Battle<'_, N>, mv: &ActiveMove, target: SlotRef) -> bool {
    let ty = mv.data.move_type;
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
    let Some(base) = mv.data.accuracy else {
        return true;
    };
    if mv.data.target == MoveTarget::User && mv.data.category == MoveCategory::Status {
        return true;
    }
    let mut accuracy = i32::from(base);
    // ModifyAccuracy: Gravity chains 6840/4096.
    if b.field_active(FieldEffect::Gravity) {
        accuracy = modify(accuracy, 6840);
    }
    let mut boost = 0i32;
    if !mv.data.ignore_evasion {
        boost -= i32::from(b.state.slot(target).boosts[6]);
    }
    boost += i32::from(b.state.slot(user).boosts[5]);
    let boost = boost.clamp(-6, 6);
    if boost > 0 {
        accuracy = accuracy * (3 + boost) / 3;
    } else if boost < 0 {
        accuracy = accuracy * 3 / (3 - boost);
    }
    b.rng.chance(accuracy.max(0) as u32, 100)
}

/// Showdown `hitStepMoveHitLoop` for a single hit.
fn hit_loop<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    targets: &[SlotRef],
) -> Result<Vec<Hit>, TurnError> {
    let results = spread_move_hit(b, user, mv, targets)?;
    let user_fainted = b.alive(user).is_none();
    b.faint_messages(user_fainted);

    let total: i32 = results
        .iter()
        .map(|r| if let Hit::Damage(d) = r { *d } else { 0 })
        .sum();
    if total > 0 {
        if let Some(recoil) = mv.data.recoil {
            let amount = (f64::from(total) * f64::from(recoil.0) / f64::from(recoil.1))
                .round()
                .max(1.0);
            b.damage(user, amount, DamageSource::Indirect);
        }
    }
    if !results.iter().any(|r| r.ok()) {
        return Ok(results);
    }
    // AfterMoveSecondary: a thawing move thaws a frozen target.
    if mv.data.thaws_target {
        for (&t, r) in targets.iter().zip(&results) {
            if r.ok() {
                if let Some(p) = b.alive(t) {
                    if b.mon(p).status == Status::Freeze {
                        b.cure_status(p);
                    }
                }
            }
        }
    }
    Ok(results)
}

/// Showdown `spreadMoveHit` for the move's own hit.
fn spread_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    targets: &[SlotRef],
) -> Result<Vec<Hit>, TurnError> {
    let data = mv.data;
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
            note(b.boost(t, &data.boosts));
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
        if let (Hit::Done, Some(false)) = (results[i], did) {
            results[i] = Hit::Failed;
        }
    }
    // selfDrops: once, for the first target the move did not fail on.
    if let Some(effect) = data.self_effect {
        if effect.boosts != NO_BOOSTS
            && results.iter().any(|r| r.ok())
            && b.rng.chance(u32::from(effect.chance), 100)
        {
            b.boost(user, &effect.boosts);
        }
    }
    // secondaries.
    for (i, &t) in targets.iter().enumerate() {
        if !results[i].ok() {
            continue;
        }
        for secondary in data.secondaries {
            if !b.rng.chance(u32::from(secondary.chance), 100) {
                continue;
            }
            if secondary.boosts != NO_BOOSTS && b.alive(t).is_some() {
                b.boost(t, &secondary.boosts);
            }
            if secondary.status != Status::None {
                b.try_set_status(t, secondary.status);
            }
            if let Some(volatile) = Volatile::from_condition(secondary.volatile_status) {
                b.add_volatile(t, volatile);
            }
            if secondary.self_boosts != NO_BOOSTS {
                b.boost(user, &secondary.self_boosts);
            }
        }
    }
    // DamagingHit: a damaging Fire move thaws a frozen target.
    for (i, &t) in targets.iter().enumerate() {
        if let Hit::Damage(_) = results[i] {
            if data.move_type == Type::Fire && data.category != MoveCategory::Status {
                if let Some(p) = b.alive(t) {
                    if b.mon(p).status == Status::Freeze {
                        b.cure_status(p);
                    }
                }
            }
        }
    }
    // AfterHit: Knock Off removes the item of every damaged target.
    if mv.id == moves::KNOCK_OFF {
        for (i, &t) in targets.iter().enumerate() {
            if let Hit::Damage(_) = results[i] {
                b.take_item(t);
            }
        }
    }
    Ok(results)
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
    let mut base_power = i32::from(data.base_power);
    if mv.id == moves::LOW_KICK || mv.id == moves::GRASS_KNOT {
        base_power = weight_power(defender.species.data().weight_hg);
    }
    if base_power == 0 {
        return Ok(Planned::NoDamage);
    }

    // Critical hit: ratio 1..4 → 1/24, 1/8, 1/2, always.
    let crit_ratio = data.crit_ratio.min(4);
    let can_crit = !defender.ability.data().cannot_be_crit;
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
    let mut power_mods = abilities::base_power_handlers(b, user, data, base_power);
    if type_boost_item(attacker.item) == Some(data.move_type) {
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
            } else if data.move_type == Type::Grass && attacker_grounded {
                MOD_ONE_POINT_THREE
            } else {
                MOD_ONE
            }
        }
        Terrain::Electric if data.move_type == Type::Electric && attacker_grounded => {
            MOD_ONE_POINT_THREE
        }
        Terrain::Psychic if data.move_type == Type::Psychic && attacker_grounded => {
            MOD_ONE_POINT_THREE
        }
        Terrain::Misty if data.move_type == Type::Dragon && defender_grounded => MOD_HALF,
        _ => MOD_ONE,
    };
    power_mods.push(Handler::global(6, SUB_FIELD_CONDITION, terrain_mod));
    if mv.id == moves::KNOCK_OFF && b.item_can_be_taken(target) {
        power_mods.push(Handler::of(b, user, 0, SUB_MOVE, MOD_ONE_POINT_FIVE));
    }
    let power_modifier = abilities::chain(b, power_mods);

    // Attack and defense.
    let physical = data.category == MoveCategory::Physical;
    let attack_stat =
        data.override_offensive_stat
            .unwrap_or(if physical { Stat::Atk } else { Stat::Spa });
    let defense_stat =
        data.override_defensive_stat
            .unwrap_or(if physical { Stat::Def } else { Stat::Spd });
    let mut atk_boost = b.state.slot(user).boosts[stat_index(attack_stat)];
    let mut def_boost = b.state.slot(target).boosts[stat_index(defense_stat)];
    let ignore_negative_offensive = data.ignore_negative_offensive || critical;
    let ignore_positive_defensive = data.ignore_positive_defensive || critical;
    if data.ignore_offensive || (ignore_negative_offensive && atk_boost < 0) {
        atk_boost = 0;
    }
    if data.ignore_defensive || (ignore_positive_defensive && def_boost > 0) {
        def_boost = 0;
    }
    let attack = boosted_stat(
        i32::from(attacker.stats[stat_index(attack_stat)]),
        atk_boost,
    );
    let mut defense = boosted_stat(
        i32::from(defender.stats[stat_index(defense_stat)]),
        def_boost,
    );
    // ModifyDef / ModifySpD: sandstorm (Rock SpD) and snow (Ice Def), 1.5x applied directly.
    let weather = b.weather();
    if defense_stat == Stat::Spd && weather == Weather::Sand && defender.types.contains(&Type::Rock)
    {
        defense = modify(defense, MOD_ONE_POINT_FIVE);
    }
    if defense_stat == Stat::Def && weather == Weather::Snow && defender.types.contains(&Type::Ice)
    {
        defense = modify(defense, MOD_ONE_POINT_FIVE);
    }

    // modifyDamage inputs.
    let weather_modifier = match (weather, data.move_type) {
        (Weather::Sun, Type::Fire) | (Weather::Rain, Type::Water) => MOD_ONE_POINT_FIVE,
        (Weather::Sun, Type::Water) | (Weather::Rain, Type::Fire) => MOD_HALF,
        _ => MOD_ONE,
    };
    let stab = data.force_stab || attacker.types.contains(&data.move_type);
    let type_mod: i32 = defender
        .types
        .iter()
        .map(|&t| match data.move_type.against(t) {
            TypeRelation::Super => 1,
            TypeRelation::Resist => -1,
            _ => 0,
        })
        .sum::<i32>()
        .clamp(-6, 6);
    let type_effectiveness = if type_mod >= 0 {
        MOD_ONE << type_mod
    } else {
        MOD_ONE >> -type_mod
    };
    // ModifyDamage (all priority 0, so in Speed order): Life Orb, screens (side conditions,
    // Speed 0), the target's abilities.
    let mut final_mods = abilities::modify_damage_handlers(b, user, target, data);
    if attacker.item == items::LIFE_ORB {
        final_mods.push(Handler::of(b, user, 0, SUB_ITEM, 5324));
    }
    if !critical && target != user && screen_applies(b, target.side, data.category) {
        let modifier = if N > 1 { 2732 } else { MOD_HALF };
        final_mods.push(Handler::global(0, SUB_SIDE_CONDITION, modifier));
    }
    let final_modifier = abilities::chain(b, final_mods);
    let input = DamageInput {
        level: attacker.level,
        base_power: base_power as u16,
        attack: attack.clamp(1, i32::from(u16::MAX)) as u16,
        defense: defense.clamp(1, i32::from(u16::MAX)) as u16,
        base_power_modifier: power_modifier,
        spread: mv.spread,
        weather_modifier,
        critical,
        stab_modifier: if stab { MOD_ONE_POINT_FIVE } else { MOD_ONE },
        type_effectiveness,
        burned: attacker.status == Status::Burn && physical,
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
/// 5 turns, 8 with the matching rock.
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
    true
}

/// Showdown `field.setTerrain`: the same terrain again fails; 5 turns, 8 with Terrain
/// Extender.
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
    true
}

/// Showdown `addPseudoWeather`: Gravity fails if up; Trick Room ends itself on restart.
fn add_pseudo_weather<const N: usize>(b: &mut Battle<'_, N>, id: &str) -> bool {
    let effect = match id {
        "gravity" => FieldEffect::Gravity,
        "trickroom" => FieldEffect::TrickRoom,
        _ => unreachable!("checked by support"),
    };
    if b.field_active(effect) {
        if effect == FieldEffect::TrickRoom {
            b.set_field(effect, Effect::NONE);
            return true;
        }
        return false;
    }
    b.set_field(effect, Effect { value: 0, turns: 5 });
    true
}

/// Showdown `addSideCondition` for Tailwind (4 turns) and the screens (5, Light Clay 8).
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
        _ if b.item(source) == items::LIGHT_CLAY => 8,
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
