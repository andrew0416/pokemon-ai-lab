//! Ability callbacks that run inside a move (Showdown `data/abilities.ts`; the Champions mod
//! overrides none of these): the user's `onModifyMove`, the user's foes' `onFoeTryMove`, the
//! target's `onTryHit` and `onModifySecondaries`.
//!
//! Each function is one event; `moves.rs` calls it where Showdown runs that event. Breakable
//! abilities are read through [`Battle::ability_unless_broken`], so a move that ignores
//! abilities skips them like Showdown's `runEvent` does.

use crate::dex::{
    abilities, items, moves, AbilityId, MoveCategory, MoveFlags, MoveTarget, Ohko, Secondary, Type,
    TypeImmunities, NO_BOOSTS,
};
use crate::field::{SideEffect, Terrain, Weather};
use crate::state::{SlotRef, Status};
use crate::volatile::Volatile;

use super::super::abilities::{priority, sheer_force_deletes_secondaries, Handler, SUB_ABILITY};
use super::super::battle::{Battle, BoostEffect, DamageSource};
use super::super::conditions;
use super::super::TurnError;
use super::{handlers, type_immune, ActiveMove};

/// The moves the type-changing abilities leave alone (`noModifyType`); Normalize also leaves
/// Hidden Power and Struggle.
const ATE_UNCHANGED: [&str; 7] = [
    "judgment",
    "multiattack",
    "naturalgift",
    "revelationdance",
    "technoblast",
    "terrainpulse",
    "weatherball",
];

/// The user's ability `onModifyType` (`runEvent('ModifyType')`, after the move's own
/// ModifyType and ModifyMove, before the ability's ModifyMove; WORKPLAN O70). A Pokémon has one
/// ability, so the handlers' priorities (Normalize 1, the rest -1) never compete, and no other
/// ModifyType handler competes with them: Electrify's (priority -2) runs after them
/// (`handlers::volatile_modify_type`); Ion Deluge is not implemented.
/// - Pixilate, Aerilate, Refrigerate, Galvanize, Dragonize: a Normal move (not in
///   [`ATE_UNCHANGED`], not a damaging Z-Move) becomes Fairy / Flying / Ice / Electric / Dragon,
///   and `move.typeChangerBoosted` is set to the ability.
/// - Normalize: every move but those and Hidden Power and Struggle becomes Normal, boosted too.
/// - Liquid Voice: a sound move of a Pokémon that is not Dynamaxed becomes Water (no boost).
///
/// Status moves change type too (a Galvanize Glare is Electric for Volt Absorb). Tera Blast's
/// exception needs Terastallization, which is off.
pub(super) fn on_modify_type<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
) {
    let ability = b.ability(user);
    let data = mv.data;
    let damaging_z = data.is_z && data.category != MoveCategory::Status;
    let new_type = match ability {
        a if a == abilities::PIXILATE => Type::Fairy,
        a if a == abilities::AERILATE => Type::Flying,
        a if a == abilities::REFRIGERATE => Type::Ice,
        a if a == abilities::GALVANIZE => Type::Electric,
        a if a == abilities::DRAGONIZE => Type::Dragon,
        a if a == abilities::NORMALIZE => {
            let unchanged =
                ATE_UNCHANGED.contains(&data.id) || ["hiddenpower", "struggle"].contains(&data.id);
            if !damaging_z && !unchanged {
                mv.move_type = Type::Normal;
                mv.type_changer = ability;
            }
            return;
        }
        a if a == abilities::LIQUID_VOICE => {
            if data.flags.contains(MoveFlags::SOUND) && !b.state.slot(user).dynamax.is_active() {
                mv.move_type = Type::Water;
            }
            return;
        }
        _ => return,
    };
    if mv.move_type == Type::Normal && !ATE_UNCHANGED.contains(&data.id) && !damaging_z {
        mv.move_type = new_type;
        mv.type_changer = ability;
    }
}

/// BasePower handlers of abilities that read the active move (called from `get_damage` next to
/// `abilities::base_power_handlers`):
/// - the type changers' `onBasePower` (priority 23): `if (move.typeChangerBoosted ===
///   this.effect) return this.chainModify([4915, 4096])`, the user's current ability being the
///   one that changed the type;
/// - Fairy Aura / Dark Aura `onAnyBasePower` (priority 20) of every active Pokémon not at 0 HP
///   (`onAny` handlers come from `alliesAndSelf()` / `foes()`), for a non-status move of the
///   aura's type against another Pokémon: the first holder in handler order becomes
///   `move.auraBooster` and only it boosts, so there is one factor however many holders there
///   are: 5448/4096, or 3072/4096 with `move.hasAuraBreak`. No other BasePower handler has
///   priority 20, so the holder's Speed never decides the factor's place in the chain.
///
/// Aura Break's `onAnyTryPrimaryHit` (TryPrimaryHit runs for every target before the hit's
/// `getDamage`) sets `move.hasAuraBreak` for a non-status move on a target other than its user
/// while an active Pokémon has Aura Break; it is breakable, so an ability-ignoring move skips it
/// unless the holder is the user. Nothing else reads the flag, and the holders cannot change
/// between TryPrimaryHit and the damage, so it is decided here.
pub(super) fn base_power_handlers<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    target: SlotRef,
    mv: &ActiveMove,
) -> crate::turn::abilities::Handlers {
    let mut out = crate::turn::abilities::Handlers::new();
    let ability = b.ability(user);
    if !mv.type_changer.is_none() && ability == mv.type_changer {
        let p = priority(ability.data().event_orders, "onBasePowerPriority");
        out.push(Handler::of(b, user, p, SUB_ABILITY, 4915));
    }
    let aura = match mv.move_type {
        Type::Fairy => abilities::FAIRY_AURA,
        Type::Dark => abilities::DARK_AURA,
        _ => AbilityId::NONE,
    };
    if aura.is_none() || target == user || mv.data.category == MoveCategory::Status {
        return out;
    }
    let actives = b.all_alive();
    let Some(&booster) = actives.iter().find(|&&s| b.ability(s) == aura) else {
        return out;
    };
    let aura_break = actives
        .iter()
        .any(|&s| b.ability_unless_broken(s) == abilities::AURA_BREAK);
    let modifier = if aura_break { 3072 } else { 5448 };
    let p = priority(aura.data().event_orders, "onAnyBasePowerPriority");
    out.push(Handler::of(b, booster, p, SUB_ABILITY, modifier));
    out
}

/// `runEvent('Accuracy', target, user, move, accuracy)` (`hitStepAccuracy`) for the implemented
/// handlers, all priority 0:
/// - No Guard (`onAnyAccuracy`, not breakable) of an active Pokémon that is the move's user or
///   target returns `true`;
/// - the target's Glaive Rush drawback and Minimize (a `minimize` move; `onAccuracy`), and the
///   user's Lock-On on the target (`onSourceAccuracy`), return `true`;
/// - Micle Berry's volatile on the user (`onSourceAccuracy`): `if (!move.ohko)` the volatile
///   ends, and while the accuracy is still a number it chains 4915/4096.
///
/// A `true` stays `true` whatever handler runs after it, and Micle's handler ends its volatile
/// whether or not the accuracy is still a number (oracle `micle-accuracy-true`,
/// `micle-glaive-rush`), so the handlers' Speed order never shows. `None`: the move hits;
/// `Some(modifier)`: the chained modifier on the numeric accuracy (4096 without Micle). Callers
/// with `accuracy === true` ignore the result.
///
/// OHKO moves run this event too (`accuracy_check`), with the accuracy `hitStepAccuracy` gave
/// them. No Guard's other callback, `onAnyInvulnerability`, is in `handlers::invulnerable`.
pub(super) fn accuracy_event<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> Option<u32> {
    let mut modifier = crate::damage::MOD_ONE;
    if mv.data.ohko == Ohko::No && b.volatile(user, Volatile::MicleBerry).active {
        b.remove_volatile(user, Volatile::MicleBerry);
        modifier = 4915;
    }
    let no_guard = [user, target]
        .into_iter()
        .any(|s| b.alive(s).is_some() && b.ability(s) == abilities::NO_GUARD);
    if no_guard || handlers::always_hit(b, user, target, mv) {
        return None;
    }
    Some(modifier)
}

/// The user's ability `onModifyMove` (`runEvent('ModifyMove')`, after the move's own):
/// - Mold Breaker, Teravolt, Turboblaze, and Mycelium Might for a status move:
///   `move.ignoreAbility = true` (the Battle's active move, read by `suppressingAbility`);
/// - Sheer Force: a move with secondaries (and no `hasSheerForceBoost`) loses them and its
///   `self` effect and is marked `hasSheerForce`;
/// - Serene Grace (priority -2): every secondary chance and `self.chance` doubles;
/// - Keen Eye, Illuminate, Mind's Eye: `move.ignoreEvasion = true`;
/// - Scrappy, Mind's Eye (priority -5): Fighting and Normal join `move.ignoreImmunity`;
/// - Stance Change (priority 1): Aegislash takes its Blade or Shield forme for the move.
///
/// A Pokémon has one ability, so their priorities never compete; none of the other
/// implemented ModifyMove handlers reads what these change. A move that ignores abilities gets
/// through Oblivious's `onTryHit` (Attract, Captivate, Taunt); its `onUpdate` then removes the
/// `attract` or `taunt` volatile (`abilities::on_update`).
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
    // Mycelium Might: `if (move.category === 'Status') move.ignoreAbility = true;`.
    let mycelium = ability == abilities::MYCELIUM_MIGHT && mv.category == MoveCategory::Status;
    if mold_breaker.contains(&ability) || mycelium {
        if let Some(active) = b.active_move.as_mut() {
            active.ignore_ability = true;
        }
    }
    // Infiltrator: `move.infiltrates = true`.
    if ability == abilities::INFILTRATOR {
        if let Some(active) = b.active_move.as_mut() {
            active.infiltrates = true;
        }
    }
    if ability == abilities::SHEER_FORCE && sheer_force_deletes_secondaries(mv.data) {
        mv.has_sheer_force = true;
    }
    if ability == abilities::SERENE_GRACE {
        mv.secondary_chance_factor = 2;
    }
    // Stance Change (priority 1): Aegislash's forme for the move (`forme::stance_change`).
    if ability == abilities::STANCE_CHANGE {
        super::super::forme::stance_change(b, user, mv.id);
    }
    // Keen Eye, Illuminate, Mind's Eye: `move.ignoreEvasion = true`.
    if [
        abilities::KEEN_EYE,
        abilities::ILLUMINATE,
        abilities::MINDS_EYE,
    ]
    .contains(&ability)
    {
        mv.ignore_evasion = true;
    }
    // Scrappy, Mind's Eye (priority -5): `if (move.ignoreImmunity !== true)` add Fighting and
    // Normal (an `ignoreImmunity: true` move already ignores every immunity).
    if ability == abilities::SCRAPPY || ability == abilities::MINDS_EYE {
        mv.scrappy = true;
    }
    // Gorilla Tactics: the lock.
    super::ability_events::gorilla_modify_move(b, user, mv.id);
    Ok(())
}

/// The secondaries of the move on `target` (`secondaries()`) with their chances: the move's
/// own (none once Sheer Force deleted them), then the flinch King's Rock, Razor Fang or Stench
/// appended in ModifyMove, each chance doubled by Serene Grace (`secondary_chance_factor`);
/// then the target's `ModifySecondaries`, which sees all of them (`moveData.secondaries`): Shield
/// Dust (breakable) keeps only secondaries with a `self` effect (`!!effect.self`), so it drops
/// the appended flinch too. Every supported move's `self` secondary has boosts; the only one
/// without (Genesis Supernova) is a Z-Move.
pub(super) fn secondaries<'m, const N: usize>(
    b: &Battle<'_, N>,
    mv: &'m ActiveMove,
    target: SlotRef,
) -> Vec<(&'m Secondary, u32)> {
    let own: &'static [Secondary] = if mv.has_sheer_force {
        &[]
    } else {
        super::handlers::move_secondaries(b, mv)
    };
    let shield_dust = b.ability_unless_broken(target) == abilities::SHIELD_DUST;
    own.iter()
        .chain(mv.added_secondary.iter())
        .filter(|s| !shield_dust || s.self_boosts != NO_BOOSTS)
        .map(|s| (s, u32::from(s.chance) * mv.secondary_chance_factor))
        .collect()
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
    // Damp (`onAnyTryMove`, breakable) of any active Pokémon not at 0 HP, the user's own
    // included (a move never suppresses its user's ability): Explosion, Mind Blown, Misty
    // Explosion and Self-Destruct fail, before `selfdestruct: 'always'` would faint the user.
    let exploding = [
        moves::EXPLOSION,
        moves::MIND_BLOWN,
        moves::MISTY_EXPLOSION,
        moves::SELF_DESTRUCT,
    ]
    .contains(&mv.id);
    if exploding && damp_active(b) {
        return false;
    }
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

/// Whether an active Pokémon not at 0 HP has Damp as the move in progress sees it (its
/// `onAny*` handlers come from every such Pokémon; breakable, so an ability-ignoring move skips
/// every holder but its user).
fn damp_active<const N: usize>(b: &Battle<'_, N>) -> bool {
    b.all_alive()
        .into_iter()
        .any(|s| b.ability_unless_broken(s) == abilities::DAMP)
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
    // `move.type`: the type after ModifyType (Weather Ball, a Galvanize Normal move).
    let ty = mv.move_type;
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
            b.boost_by(target, &boosts, Some(user), BoostEffect::Ability(ability));
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
        // Oblivious: `if (move.id === 'attract' || move.id === 'captivate' || move.id ===
        // 'taunt') return null;` (the holder's own use included).
        a if a == abilities::OBLIVIOUS => {
            [moves::ATTRACT, moves::CAPTIVATE, moves::TAUNT].contains(&mv.id)
        }
        a if a == abilities::GOOD_AS_GOLD => other && category == MoveCategory::Status,
        // Wind Rider: another Pokémon's wind move: `this.boost({atk: 1}, target, target)` (the
        // immunity message if nothing changed), then `return null`.
        a if a == abilities::WIND_RIDER => {
            if other && data.flags.contains(MoveFlags::WIND) {
                super::super::abilities::wind_rider_boost(b, target);
                return true;
            }
            false
        }
        a if a == abilities::WONDER_GUARD => {
            if !other || category == MoveCategory::Status || mv.id == moves::STRUGGLE {
                return false;
            }
            effectiveness(b, mv, target) <= 0 || type_immune(b, mv, target)
        }
        _ => false,
    }
}

/// The `onDamagingHitOrder` of an ability whose `onDamagingHit` is implemented by
/// [`on_damaging_hit`] (`u32::MAX`: no order, after every ordered handler), or `None`. Rough
/// Skin, Iron Barbs and Rattled are handled by `moves::damaging_hit` itself.
pub(super) fn damaging_hit_order(ability: AbilityId) -> Option<u32> {
    const HANDLED: [AbilityId; 29] = [
        abilities::ILLUSION,
        abilities::GULP_MISSILE,
        abilities::WANDERING_SPIRIT,
        abilities::CUTE_CHARM,
        abilities::SPICY_SPRAY,
        abilities::CURSED_BODY,
        abilities::TOXIC_DEBRIS,
        abilities::PERISH_BODY,
        abilities::MUMMY,
        abilities::LINGERING_AROMA,
        abilities::STATIC,
        abilities::FLAME_BODY,
        abilities::POISON_POINT,
        abilities::EFFECT_SPORE,
        abilities::STAMINA,
        abilities::WEAK_ARMOR,
        abilities::COTTON_DOWN,
        abilities::GOOEY,
        abilities::TANGLING_HAIR,
        abilities::SAND_SPIT,
        abilities::SEED_SOWER,
        abilities::ELECTROMORPHOSIS,
        abilities::STEAM_ENGINE,
        abilities::JUSTIFIED,
        abilities::THERMAL_EXCHANGE,
        abilities::WATER_COMPACTION,
        abilities::WIND_POWER,
        abilities::AFTERMATH,
        abilities::INNARDS_OUT,
    ];
    if !HANDLED.contains(&ability) {
        return None;
    }
    let order = priority(ability.data().event_orders, "onDamagingHitOrder");
    Some(if order == 0 { u32::MAX } else { order as u32 })
}

/// The damaged target's ability `onDamagingHit` (`runEvent('DamagingHit')` in
/// `moves::damaging_hit`, WORKPLAN O55) for an ability [`damaging_hit_order`] lists. `holder`
/// is the damaged Pokémon, still in its slot but possibly at 0 HP (a holder the hit fainted
/// still acts: Showdown processes faints after the move); `attacker` is the move's user (it
/// may have fainted earlier in the event); `damage` is the HP the hit took from the holder;
/// `contact` is `checkMoveMakesContact` (the move's `contact` flag, cancelled by the attacker's
/// Protective Pads); `total_before` is `move.totalDamage` before this hit (earlier hits of a
/// multi-hit move).
///
/// Handlers that call `this.boost(...)` without a target boost the holder with the attacker as
/// the source (`this.event.target` / `.source`); the contact status abilities pass the holder
/// as the status's source (`source.trySetStatus(status, target)`). Random draws are skipped
/// when their outcome cannot change anything (a status on an attacker that has one or fainted),
/// which leaves the distribution unchanged.
#[allow(clippy::too_many_arguments)]
pub(super) fn on_damaging_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    ability: AbilityId,
    holder: SlotRef,
    attacker: SlotRef,
    mv: &ActiveMove,
    damage: i32,
    contact: bool,
    total_before: i32,
) -> Result<(), TurnError> {
    let boost_holder = |b: &mut Battle<'_, N>, stat: usize, amount: i8| {
        let mut boosts = NO_BOOSTS;
        boosts[stat] = amount;
        b.boost_by(
            holder,
            &boosts,
            Some(attacker),
            BoostEffect::Ability(ability),
        );
    };
    let attacker_statusable = b
        .alive(attacker)
        .is_some_and(|p| b.mon(p).status == Status::None);
    let holder_fainted = b.slot_mon(holder).is_none_or(|m| m.hp == 0);
    match ability {
        // `if (this.checkMoveMakesContact(...)) if (this.randomChance(3, 10))
        // source.trySetStatus(status, target);`
        a if a == abilities::STATIC
            || a == abilities::FLAME_BODY
            || a == abilities::POISON_POINT =>
        {
            if contact && attacker_statusable && b.rng.chance(3, 10) {
                let status = match a {
                    a if a == abilities::STATIC => Status::Paralyze,
                    a if a == abilities::FLAME_BODY => Status::Burn,
                    _ => Status::Poison,
                };
                b.try_set_status_from(attacker, status, Some(holder));
            }
        }
        // Illusion (EE2): `if (target.illusion) this.singleEvent('End', Illusion, ...)`.
        a if a == abilities::ILLUSION => {
            if let Some(pokemon) = b.occupant(holder) {
                super::ability_events::illusion_end(b, pokemon);
            }
        }
        // Gulp Missile (`forme::gulp_missile_spit`), contact or not.
        a if a == abilities::GULP_MISSILE => {
            super::super::forme::gulp_missile_spit(b, holder, attacker);
        }
        // Wandering Spirit: `if (this.checkMoveMakesContact(...)) this.skillSwap(source, target)`.
        a if a == abilities::WANDERING_SPIRIT => {
            if contact {
                super::ability_events::skill_swap(b, attacker, holder)?;
            }
        }
        // Cute Charm: `if (this.checkMoveMakesContact(...)) if (this.randomChance(3, 10))
        // source.addVolatile('attract', this.effectState.target)` (the draw is skipped when the
        // attraction cannot land: `conditions::attract_fails`).
        a if a == abilities::CUTE_CHARM => {
            if contact && !conditions::attract_fails(b, attacker, holder)? && b.rng.chance(3, 10) {
                conditions::add_attract(b, attacker, holder);
            }
        }
        // Spicy Spray (Mega Scovillain): `source.trySetStatus('brn', target)` on every damaging
        // hit, contact or not.
        a if a == abilities::SPICY_SPRAY => {
            if attacker_statusable {
                b.try_set_status_from(attacker, Status::Burn, Some(holder));
            }
        }
        // Effect Spore: contact and `source.runStatusImmunity('powder')`, then `this.random(100)`:
        // below 11 sleep, below 21 paralysis, below 30 poison.
        a if a == abilities::EFFECT_SPORE => {
            if contact && attacker_statusable && !b.status_immune(attacker, TypeImmunities::POWDER)
            {
                let status = match b.rng.weighted(&[0.11, 0.10, 0.09, 0.70]) {
                    0 => Status::Sleep,
                    1 => Status::Paralyze,
                    2 => Status::Poison,
                    _ => return Ok(()),
                };
                b.try_set_status_from(attacker, status, Some(holder));
            }
        }
        // Stamina: `this.boost({def: 1})`.
        a if a == abilities::STAMINA => boost_holder(b, 1, 1),
        // Weak Armor: a physical move: `this.boost({def: -1, spe: 2}, target, target)`.
        a if a == abilities::WEAK_ARMOR => {
            if mv.data.category == MoveCategory::Physical {
                let mut boosts = NO_BOOSTS;
                boosts[1] = -1;
                boosts[4] = 2;
                b.boost_by(holder, &boosts, Some(holder), BoostEffect::Ability(a));
            }
        }
        // Cotton Down: every other active Pokémon not processed as fainted (`getAllActive()`,
        // side one first, slot order): `this.boost({spe: -1}, pokemon, target, null, true)`.
        a if a == abilities::COTTON_DOWN => {
            let mut drop = NO_BOOSTS;
            drop[4] = -1;
            // A future move's user hitting from the bench is not active (`AbsentUser` refuses
            // the hit when an occupant left its position for it).
            for other in b.all_alive() {
                if other != holder && b.absent_user != Some(other) {
                    b.boost_by(other, &drop, Some(holder), BoostEffect::Ability(a));
                }
            }
        }
        // Gooey, Tangling Hair: contact: `this.boost({spe: -1}, source, target, null, true)`.
        a if a == abilities::GOOEY || a == abilities::TANGLING_HAIR => {
            if contact {
                let mut drop = NO_BOOSTS;
                drop[4] = -1;
                b.boost_by(attacker, &drop, Some(holder), BoostEffect::Ability(a));
            }
        }
        // Sand Spit: `this.field.setWeather('sandstorm')` with the holder as the source (its
        // Smooth Rock); an ability's attempt fails against the same weather. The weather's
        // `WeatherChange` handlers are all refused on the field.
        a if a == abilities::SAND_SPIT => {
            super::set_weather(b, holder, Weather::Sand);
        }
        // Seed Sower: `this.field.setTerrain('grassyterrain')` (the holder's Terrain Extender).
        a if a == abilities::SEED_SOWER => {
            super::set_terrain(b, holder, Terrain::Grassy);
        }
        // Cursed Body (not breakable): `if (source.volatiles['disable']) return;` then, for a move
        // that is not a Max Move, a future move or Struggle, 30%:
        // `source.addVolatile('disable', this.effectState.target)` (Disable's `onStart`: the
        // attacker's last move, one turn less as the attacker is using a move). The draw is
        // skipped when the attacker has fainted (`addVolatile` fails on it).
        a if a == abilities::CURSED_BODY => {
            let data = mv.data;
            let eligible = !data.is_max
                && !data.flags.contains(MoveFlags::FUTUREMOVE)
                && mv.id != moves::STRUGGLE;
            if eligible
                && b.alive(attacker).is_some()
                && !b.volatile(attacker, Volatile::Disable).active
                && b.rng.chance(3, 10)
            {
                b.add_volatile(attacker, Volatile::Disable);
            }
        }
        // Electromorphosis: `target.addVolatile('charge')` (nothing on a fainted holder or when
        // it is up: its `onRestart` only logs).
        a if a == abilities::ELECTROMORPHOSIS => {
            b.add_volatile(holder, Volatile::Charge);
        }
        // Wind Power: a wind move: `target.addVolatile('charge')`.
        a if a == abilities::WIND_POWER => {
            if mv.data.flags.contains(MoveFlags::WIND) {
                b.add_volatile(holder, Volatile::Charge);
            }
        }
        // Steam Engine: Water or Fire: `this.boost({spe: 6})`.
        a if a == abilities::STEAM_ENGINE => {
            if matches!(mv.move_type, Type::Water | Type::Fire) {
                boost_holder(b, 4, 6);
            }
        }
        // Justified: Dark: `this.boost({atk: 1})`.
        a if a == abilities::JUSTIFIED => {
            if mv.move_type == Type::Dark {
                boost_holder(b, 0, 1);
            }
        }
        // Thermal Exchange (breakable; the caller skips it for an ability-ignoring move): Fire:
        // `this.boost({atk: 1})`.
        a if a == abilities::THERMAL_EXCHANGE => {
            if mv.move_type == Type::Fire {
                boost_holder(b, 0, 1);
            }
        }
        // Water Compaction: Water: `this.boost({def: 2})`.
        a if a == abilities::WATER_COMPACTION => {
            if mv.move_type == Type::Water {
                boost_holder(b, 1, 2);
            }
        }
        // Aftermath: a holder the hit fainted, contact: `this.damage(source.baseMaxhp / 4,
        // source, target)`, which an active Damp (`onAnyDamage`: `effect.name ===
        // 'Aftermath'`, breakable) turns into no damage.
        a if a == abilities::AFTERMATH => {
            if holder_fainted && contact && !damp_active(b) {
                if let Some(max_hp) = b.alive(attacker).map(|p| b.mon(p).max_hp) {
                    b.damage(attacker, f64::from(max_hp) / 4.0, DamageSource::Indirect);
                }
            }
        }
        // Innards Out: a holder the hit fainted: `damage += move.totalDamage` (earlier hits;
        // smart-target moves are refused), then `this.damage(damage, source, target)`.
        // A future move's user hitting from the bench takes nothing (`spreadDamage`:
        // `!target.isActive`).
        a if a == abilities::INNARDS_OUT && holder_fainted && b.absent_user != Some(attacker) => {
            b.damage(
                attacker,
                f64::from(damage + total_before),
                DamageSource::Indirect,
            );
        }
        // Toxic Debris: a physical move adds a layer of Toxic Spikes (`addSideCondition`, below
        // two layers) to the attacker's side, or to the holder's foes' side when an ally hit it.
        a if a == abilities::TOXIC_DEBRIS => {
            if mv.data.category == MoveCategory::Physical {
                let side = if attacker.side == holder.side {
                    holder.side.other()
                } else {
                    attacker.side
                };
                let spikes = b.state.side(side).effects[SideEffect::ToxicSpikes as usize];
                if !spikes.is_active() || spikes.value < 2 {
                    conditions::add_hazard(b, side, SideEffect::ToxicSpikes);
                }
            }
        }
        // Perish Body: contact, and the attacker has no `perishsong` yet: both the attacker and
        // the holder get it (`addVolatile`: nothing on a Pokémon at 0 HP).
        a if a == abilities::PERISH_BODY => {
            if contact && !b.volatile(attacker, Volatile::PerishSong).active {
                b.add_volatile(attacker, Volatile::PerishSong);
                b.add_volatile(holder, Volatile::PerishSong);
            }
        }
        // Mummy, Lingering Aroma: unless the attacker's ability (`source.getAbility()`: the raw
        // one) is already this one (or `cantsuppress`, which `setAbility` refuses too), contact:
        // `source.setAbility(this ability, target)` (`abilities::set_ability`: nothing on an
        // attacker at 0 HP; its Ability Shield blocks it; the old ability's `End`, then the new
        // one, which has no start).
        a if (a == abilities::MUMMY || a == abilities::LINGERING_AROMA)
            && b.raw_ability(attacker) != a
            && contact =>
        {
            super::super::abilities::set_ability(b, attacker, a)?;
        }
        _ => {}
    }
    Ok(())
}

/// Whether the attacker's ability has an implemented `onSourceDamagingHit`
/// ([`on_source_damaging_hit`]): Poison Touch, Toxic Chain.
pub(super) fn has_source_damaging_hit(ability: AbilityId) -> bool {
    ability == abilities::POISON_TOUCH || ability == abilities::TOXIC_CHAIN
}

/// The attacker's ability `onSourceDamagingHit` for one damaged target (`runEvent('DamagingHit')`
/// collects it once per damaged target, after that target's own handlers; WORKPLAN O56). A
/// fainted attacker's handler still runs (it is collected before any handler).
/// - Nothing if the target has Shield Dust or holds Covert Cloak (`hasAbility` / `hasItem`: an
///   ability-ignoring move does not skip this check).
/// - Poison Touch: `checkMoveMakesContact(move, target, source)` — the *target* is passed as the
///   attacker, so the damaged target's Protective Pads cancel it and the holder's own do not —
///   then 30%: `target.trySetStatus('psn', source)`.
/// - Toxic Chain: 30%: `target.trySetStatus('tox', source)`, contact or not.
///
/// The draw is skipped when the status cannot land anyway (a fainted or already statused
/// target), which leaves the distribution unchanged.
pub(super) fn on_source_damaging_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    ability: AbilityId,
    target: SlotRef,
    attacker: SlotRef,
    mv: &ActiveMove,
) {
    // The handler was collected with the attacker's ability: one a DamagingHit handler replaced
    // since (Mummy, Lingering Aroma, Wandering Spirit) is skipped (its `abilityState` moved on).
    if b.ability(attacker) != ability {
        return;
    }
    if b.ability(target) == abilities::SHIELD_DUST || b.item(target) == items::COVERT_CLOAK {
        return;
    }
    let statusable = b
        .alive(target)
        .is_some_and(|p| b.mon(p).status == Status::None);
    if !statusable {
        return;
    }
    let status = if ability == abilities::POISON_TOUCH {
        let contact = super::item_events::makes_contact(b, attacker, mv.data)
            && b.item(target) != items::PROTECTIVE_PADS;
        if !contact {
            return;
        }
        Status::Poison
    } else if ability == abilities::TOXIC_CHAIN {
        Status::Toxic
    } else {
        return;
    };
    if b.rng.chance(3, 10) {
        b.try_set_status_from(target, status, Some(attacker));
    }
}

/// Showdown `runEffectiveness(move)` for the implemented effectiveness handlers: per defending
/// type, the chart then the move's own `onEffectiveness`, summed (not clamped).
fn effectiveness<const N: usize>(b: &Battle<'_, N>, mv: &ActiveMove, target: SlotRef) -> i32 {
    if b.slot_mon(target).is_none() {
        return 0;
    }
    // `getTypes()`: the added type too.
    b.types(target)
        .iter()
        .filter(|&&t| t != Type::None)
        .map(|&t| {
            let chart = handlers::type_effectiveness(mv.move_type, t);
            handlers::on_effectiveness(mv.id, t, chart)
        })
        .sum()
}
