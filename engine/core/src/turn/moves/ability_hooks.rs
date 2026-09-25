//! Ability callbacks that run inside a move (Showdown `data/abilities.ts`; the Champions mod
//! overrides none of these): the user's `onModifyMove`, the user's foes' `onFoeTryMove`, the
//! target's `onTryHit` and `onModifySecondaries`.
//!
//! Each function is one event; `moves.rs` calls it where Showdown runs that event. Breakable
//! abilities are read through [`Battle::ability_unless_broken`], so a move that ignores
//! abilities skips them like Showdown's `runEvent` does.

use crate::dex::{
    abilities, moves, AbilityFlags, MoveCategory, MoveFlags, MoveTarget, Secondary, Type, NO_BOOSTS,
};
use crate::state::{SlotRef, Status};
use crate::volatile::Volatile;

use super::super::abilities::sheer_force_deletes_secondaries;
use super::super::battle::{cured_on_update, Battle};
use super::super::TurnError;
use super::{handlers, type_immune, ActiveMove};

/// The user's ability `onModifyMove` (`runEvent('ModifyMove')`, after the move's own):
/// - Mold Breaker, Teravolt, Turboblaze: `move.ignoreAbility = true` (the Battle's active move,
///   read by `suppressingAbility`);
/// - Sheer Force: a move with secondaries (and no `hasSheerForceBoost`) loses them and its
///   `self` effect and is marked `hasSheerForce`;
/// - Serene Grace (priority -2): every secondary chance and `self.chance` doubles.
///
/// A Pokémon has one ability, so their priorities never compete; none of the other
/// implemented ModifyMove handlers reads what these change. A move that ignores abilities is
/// then checked by [`status_cure_bypassed`].
pub(super) fn on_modify_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
) -> Result<(), TurnError> {
    let ability = b.ability(user);
    let mold_breaker = [
        abilities::MOLD_BREAKER,
        abilities::TERAVOLT,
        abilities::TURBOBLAZE,
    ];
    if mold_breaker.contains(&ability) {
        if let Some(active) = b.active_move.as_mut() {
            active.ignore_ability = true;
        }
    }
    if ability == abilities::SHEER_FORCE && sheer_force_deletes_secondaries(mv.data) {
        mv.has_sheer_force = true;
    }
    if ability == abilities::SERENE_GRACE {
        mv.secondary_chance_factor = 2;
    }
    if b.active_move.is_some_and(|m| m.ignore_ability) {
        if let Some(why) = status_cure_bypassed(b, user, mv) {
            return Err(b.unsupported(why));
        }
    }
    Ok(())
}

/// A move that ignores abilities sets statuses through the breakable abilities that block them
/// (`set_status_blocked`, `blocks_status`, `status_immune`). When such an ability also cures
/// the status in `onUpdate` (Water Veil, Water Bubble, Immunity, Insomnia, Vital Spirit,
/// Limber, Magma Armor: `cured_on_update`), Showdown cures it at the `Update` after the action,
/// once the ability is no longer suppressed; the engine has no Update event, so the move is
/// refused while any other active Pokémon has such an ability for a status the move can set
/// (conservative: targets are not resolved yet in ModifyMove). The statuses a move can set: its
/// primary status, its secondaries' (unless Sheer Force deleted them), and Dire Claw's and Tri
/// Attack's `secondary.onHit` draws; a new status-setting handler must be added here.
fn status_cure_bypassed<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Option<String> {
    let data = mv.data;
    let mut statuses: Vec<Status> = Vec::new();
    if data.status != Status::None {
        statuses.push(data.status);
    }
    if !mv.has_sheer_force {
        statuses.extend(data.secondaries.iter().map(|s| s.status));
        match mv.id {
            moves::DIRE_CLAW => statuses.extend([Status::Poison, Status::Paralyze, Status::Sleep]),
            moves::TRI_ATTACK => statuses.extend([Status::Burn, Status::Paralyze, Status::Freeze]),
            _ => {}
        }
    }
    statuses.retain(|&s| s != Status::None);
    for slot in b.all_alive() {
        if slot == user || !b.suppressing_ability(slot) {
            continue;
        }
        let ability = b.ability(slot);
        if !ability.data().flags.contains(AbilityFlags::BREAKABLE) {
            continue;
        }
        if let Some(status) = statuses.iter().find(|&&s| cured_on_update(ability, s)) {
            return Some(format!(
                "{} ignoring {}'s {}: {status:?} would be cured on the next Update (not implemented)",
                data.name,
                b.slot_mon(slot).map_or("?", |m| m.species.data().name),
                ability.data().name
            ));
        }
    }
    None
}

/// The secondaries of the move on `target` (`secondaries()`): none once Sheer Force deleted
/// them, else the target's `ModifySecondaries` result. Shield Dust (breakable) keeps only
/// secondaries with a `self` effect (`!!effect.self`); every supported move's `self` secondary
/// has boosts, the only one without (Genesis Supernova) is a Z-Move.
pub(super) fn secondaries<const N: usize>(
    b: &Battle<'_, N>,
    mv: &ActiveMove,
    target: SlotRef,
) -> Vec<&'static Secondary> {
    if mv.has_sheer_force {
        return Vec::new();
    }
    let all = mv.data.secondaries;
    if b.ability_unless_broken(target) == abilities::SHIELD_DUST {
        return all.iter().filter(|s| s.self_boosts != NO_BOOSTS).collect();
    }
    all.iter().collect()
}

/// `move.hasSheerForce && pokemon.hasAbility('sheerforce')`: the `AfterMoveSecondarySelf` and
/// `AfterMoveSecondary` events (Life Orb recoil, a thawing move's thaw) are skipped.
pub(super) fn sheer_force_skips<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    mv.has_sheer_force && b.ability(user) == abilities::SHEER_FORCE
}

/// `runEvent('TryMove', user, target, move)` for the implemented handlers: Dazzling, Queenly
/// Majesty and Armor Tail (`onFoeTryMove`, breakable) on the user's active foes. `target` is
/// `useMoveInner`'s target after `getMoveTargets` (the last resolved target, or the chosen one
/// when none is left). `false` = the move fails ("cant"), with nothing else happening.
///
/// A holder blocks a move with priority above 0 aimed at its own side (the target is the
/// holder or its ally), except `foeSide` moves and `all` moves other than Perish Song, Flower
/// Shield and Rototiller, which it blocks whatever they target. Every handler only returns
/// `false` or nothing, so their order is irrelevant.
pub(super) fn on_try_move<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    let kind = mv.data.target;
    let all_exception =
        [moves::PERISH_SONG, moves::FLOWER_SHIELD, moves::ROTOTILLER].contains(&mv.id);
    if kind == MoveTarget::FoeSide || (kind == MoveTarget::All && !all_exception) {
        return true;
    }
    // `move.priority > 0.1`: the priority after ModifyPriority, without fractional priority.
    if mv.priority <= 0 {
        return true;
    }
    !b.alive_slots(user.side.other()).into_iter().any(|holder| {
        let ability = b.ability_unless_broken(holder);
        let blocks = ability == abilities::DAZZLING
            || ability == abilities::QUEENLY_MAJESTY
            || ability == abilities::ARMOR_TAIL;
        blocks && (target.side == holder.side || kind == MoveTarget::All)
    })
}

/// The target's ability `onTryHit` (`runEvent('TryHit')`, after Psychic Terrain and Protect;
/// Sap Sipper and Overcoat have priority 1, the rest 0, but a target has one ability and the
/// other priority-0/1 handlers of the implemented effects are its own). `true` = the ability
/// blocks the move on this target (`return null`).
///
/// - Volt Absorb, Water Absorb, Earth Eater: another Pokémon's Electric / Water / Ground move
///   heals 1/4 of max HP (nothing at full HP).
/// - Motor Drive (Electric, Spe +1), Sap Sipper (Grass, Atk +1), Well-Baked Body (Fire,
///   Def +2): the boost (none at +6).
/// - Flash Fire: another Pokémon's Fire move sets `move.accuracy = true` (the move then never
///   misses its other targets) and adds the `flashfire` volatile (nothing if it is up).
/// - Bulletproof: bullet moves, the holder's own included (no `target !== source` check).
/// - Soundproof: another Pokémon's sound moves. Overcoat: another Pokémon's powder moves,
///   unless the holder's types already make it powder-immune (then `hitStepTryImmunity` does).
/// - Telepathy: an ally's damaging moves. Good as Gold: another Pokémon's status moves.
/// - Wonder Guard: another Pokémon's damaging move (not Struggle) that is not super effective
///   (`runEffectiveness <= 0`) or to which the holder is immune (`runImmunity`).
pub(super) fn on_try_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    target: SlotRef,
) -> bool {
    let ability = b.ability_unless_broken(target);
    if ability.is_none() {
        return false;
    }
    let data = mv.data;
    let ty = data.move_type;
    let other = target != user;
    let heal_type = match ability {
        a if a == abilities::VOLT_ABSORB => Some(Type::Electric),
        a if a == abilities::WATER_ABSORB => Some(Type::Water),
        a if a == abilities::EARTH_EATER => Some(Type::Ground),
        _ => None,
    };
    if other && heal_type == Some(ty) {
        // `this.heal(target.baseMaxhp / 4)`.
        let max_hp = f64::from(b.slot_mon(target).expect("a target").max_hp);
        b.heal(target, max_hp / 4.0);
        return true;
    }
    let boost = match ability {
        a if a == abilities::MOTOR_DRIVE => Some((Type::Electric, 4, 1)),
        a if a == abilities::SAP_SIPPER => Some((Type::Grass, 0, 1)),
        a if a == abilities::WELL_BAKED_BODY => Some((Type::Fire, 1, 2)),
        _ => None,
    };
    if let Some((boost_type, stat, amount)) = boost {
        if other && ty == boost_type {
            // `this.boost({...})` on the holder (`this.event.target`).
            let mut boosts = NO_BOOSTS;
            boosts[stat] = amount;
            b.boost(target, &boosts);
            return true;
        }
        return false;
    }
    let category = data.category;
    match ability {
        a if a == abilities::FLASH_FIRE => {
            if other && ty == Type::Fire {
                mv.accuracy = None;
                b.add_volatile(target, Volatile::FlashFire);
                return true;
            }
            false
        }
        a if a == abilities::BULLETPROOF => data.flags.contains(MoveFlags::BULLET),
        a if a == abilities::SOUNDPROOF => other && data.flags.contains(MoveFlags::SOUND),
        a if a == abilities::OVERCOAT => {
            other
                && data.flags.contains(MoveFlags::POWDER)
                && !b.natural_immune(target, crate::dex::TypeImmunities::POWDER)
        }
        a if a == abilities::TELEPATHY => {
            other && target.side == user.side && category != MoveCategory::Status
        }
        a if a == abilities::GOOD_AS_GOLD => other && category == MoveCategory::Status,
        a if a == abilities::WONDER_GUARD => {
            if !other || category == MoveCategory::Status || mv.id == moves::STRUGGLE {
                return false;
            }
            effectiveness(b, mv, target) <= 0 || type_immune(b, mv, target)
        }
        _ => false,
    }
}

/// Showdown `runEffectiveness(move)` for the implemented effectiveness handlers: per defending
/// type, the chart then the move's own `onEffectiveness`, summed (not clamped).
fn effectiveness<const N: usize>(b: &Battle<'_, N>, mv: &ActiveMove, target: SlotRef) -> i32 {
    let Some(mon) = b.slot_mon(target) else {
        return 0;
    };
    mon.types
        .iter()
        .filter(|&&t| t != Type::None)
        .map(|&t| {
            let chart = handlers::type_effectiveness(mv.data.move_type, t);
            handlers::on_effectiveness(mv.id, t, chart)
        })
        .sum()
}
