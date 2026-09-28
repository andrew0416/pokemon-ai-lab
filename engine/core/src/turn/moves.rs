//! Using a move: Showdown `runMove` ??`useMove` ??`trySpreadMoveHit` / `tryMoveHit` ??the
//! hit steps ??`spreadMoveHit` (damage, effects, secondaries) ??recoil and after-move
//! effects, for the implemented moves (see [`super::support`]).

mod ability_hooks;
mod handlers;

use handlers::HitResult;
pub(crate) use handlers::{
    called_after_move_checked, set_types, sleep_talk_calls, trick_item_start, trick_moves_item,
};

use crate::damage::{
    damage_rolls, DamageInput, MOD_HALF, MOD_ONE, MOD_ONE_POINT_FIVE, MOD_ONE_POINT_THREE,
    MOD_ONE_POINT_TWO,
};
use crate::dex::{
    abilities, items, moves, AbilityId, FixedDamage, IgnoreImmunity, ItemId, MoveCategory,
    MoveData, MoveFlags, MoveId, MoveTarget, Ohko, Secondary, SelfDestruct, SelfSwitch, Stat, Type,
    TypeImmunities, TypeRelation, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{MoveResult, PokemonRef, SideId, SlotRef, Status, SwitchFlag};
use crate::volatile::Volatile;

use super::abilities as ability_events;
use super::abilities::{Handler, SUB_FIELD_CONDITION, SUB_ITEM, SUB_MOVE, SUB_SIDE_CONDITION};
use super::battle::{ActiveMoveRef, Battle, BoostEffect, DamageSource};
use super::conditions;
use super::items as item_events;
use super::order::{boosted_stat, modify};
use super::support::{side_effect_of, type_boost_item};
use super::TurnError;
use super::{Slots, Small};

/// The move being used, with what is decided when it is used. It is part of a suspended
/// multi-hit move's progress (`MoveProgress`), so it is comparable: `data` is implied by `id`
/// and `category` and left out of the comparison (keep the manual impls below in step with new
/// fields).
#[derive(Clone, Debug)]
pub(crate) struct ActiveMove {
    id: MoveId,
    /// The dex data, or for a move whose ModifyMove changed its category ([`ActiveMove::category`])
    /// a copy with that category ([`with_category`]), so that everything reading `data`
    /// (the ability and item chains, contact, screens, Counter) sees Showdown's `move.category`.
    data: &'static MoveData,
    /// Showdown `move.category`: the dex's, or the one ModifyMove chose (Photon Geyser and Shell
    /// Side Arm become physical). Set through [`ActiveMove::set_category`].
    category: MoveCategory,
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
    /// Parental Bond's `onPrepareHit` made the move hit twice (`move.multihit = 2`,
    /// `move.multihitType = 'parentalbond'`): the second hit's damage is quartered.
    parental_bond: bool,
    /// HP taken by the move's hits (Showdown `move.totalDamage`), set once the hits are done.
    total_damage: i32,
    /// Target type after ModifyMove (Showdown `move.target`; Expanding Force widens it).
    target: MoveTarget,
    /// Type after ModifyType (Showdown `move.type`; Weather Ball, Terrain Pulse). Every rule
    /// that reads the type of the move being used reads this, not `data.move_type`.
    move_type: Type,
    /// Base power after ModifyMove (Showdown `move.basePower`), before `basePowerCallback`.
    base_power: i32,
    /// `move.ignoreEvasion` after ModifyMove (the data flag, or the user's Keen Eye,
    /// Illuminate, Mind's Eye).
    ignore_evasion: bool,
    /// Scrappy / Mind's Eye added Fighting and Normal to `move.ignoreImmunity` in ModifyMove: a
    /// move of either type ignores type immunity.
    scrappy: bool,
    /// Showdown `move.hitTargets`: the targets left after the hit steps, as a bit per slot
    /// ([`target_bit`]); empty when every hit failed.
    hit_targets: u8,
    /// Showdown `move.sourceEffect` for a move another move calls (Sleep Talk), whose PP pays
    /// Pressure's extra; `NONE` for a move used directly.
    source_effect: MoveId,
    /// `move.selfSwitch` (U-turn, Parting Shot, Baton Pass, Shed Tail, ...): the user switches
    /// out once the move landed (F6); which kind of switch ([`self_switch_flag`]) follows the
    /// move's data.
    self_switch: bool,
    /// The target location the user chose (`lastMoveTargetLoc`; 0 for a move without one or
    /// called by another): a two-turn move aims at it again on its second turn.
    target_loc: i8,
    /// Showdown `move.typeChangerBoosted`: the ability whose ModifyType changed the move's type
    /// (Pixilate, Aerilate, Refrigerate, Galvanize, Dragonize, Normalize), which then boosts it
    /// in BasePower; `NONE` otherwise.
    type_changer: AbilityId,
    /// Showdown `move.hasBounced`: a copy Magic Bounce used back at the original user (it cannot
    /// be bounced again, pays no Pressure PP and adds no Choice lock).
    has_bounced: bool,
    /// The hit of a future move (Future Sight, Doom Desire) at the residual: the move built from
    /// the stored `moveData` (`new Move(data.moveData)`), which has no `onTry` and does not
    /// ignore type immunity.
    future_hit: bool,
    /// The targets whose `getMoveHitData(move).bypassProtect` a `HitProtect` handler set (Unseen
    /// Fist, Piercing Drill: a protection that would have stopped the move let it through), as a
    /// bit per slot ([`target_bit`]): the move's damage to them is quartered.
    bypass_protect: u8,
    /// Beat Up's `move.allies` as their hits' base powers (`5 + floor(baseAtk / 10)` of each
    /// ally's set species, in hit order; 0 past the last), fixed by its `onModifyMove`
    /// ([`handlers::beat_up_powers`]). All 0 for any other move.
    beat_up: [u8; 6],
}

impl ActiveMove {
    /// `move.category = category` (with Shell Side Arm's `move.flags.contact = 1` for a
    /// physical one): the data the move is read from follows.
    fn set_category(&mut self, category: MoveCategory) {
        self.category = category;
        self.data = with_category(self.id, category);
    }
}

/// The dex data of `id` with `category` (itself when that is the dex's): Photon Geyser and Shell
/// Side Arm as physical moves (Shell Side Arm then makes contact). Kept for the program's
/// lifetime, one copy per move.
fn with_category(id: MoveId, category: MoveCategory) -> &'static MoveData {
    use std::sync::OnceLock;
    let data = id.data();
    if data.category == category {
        return data;
    }
    static PHOTON_GEYSER: OnceLock<MoveData> = OnceLock::new();
    static SHELL_SIDE_ARM: OnceLock<MoveData> = OnceLock::new();
    let (cell, contact) = match id {
        moves::PHOTON_GEYSER => (&PHOTON_GEYSER, false),
        moves::SHELL_SIDE_ARM => (&SHELL_SIDE_ARM, true),
        _ => unreachable!("only Photon Geyser and Shell Side Arm change their category"),
    };
    assert_eq!(
        category,
        MoveCategory::Physical,
        "{id:?} only becomes physical"
    );
    cell.get_or_init(|| {
        let mut changed = data.clone();
        changed.category = category;
        if contact {
            changed.flags = MoveFlags(changed.flags.bits() | MoveFlags::CONTACT.bits());
        }
        changed
    })
}

impl PartialEq for ActiveMove {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.category == other.category
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
            && self.ignore_evasion == other.ignore_evasion
            && self.scrappy == other.scrappy
            && self.hit_targets == other.hit_targets
            && self.source_effect == other.source_effect
            && self.self_switch == other.self_switch
            && self.target_loc == other.target_loc
            && self.type_changer == other.type_changer
            && self.has_bounced == other.has_bounced
            && self.future_hit == other.future_hit
            && self.bypass_protect == other.bypass_protect
            && self.beat_up == other.beat_up
            && self.parental_bond == other.parental_bond
    }
}

impl Eq for ActiveMove {}

impl std::hash::Hash for ActiveMove {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.category.hash(state);
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
        self.ignore_evasion.hash(state);
        self.scrappy.hash(state);
        self.hit_targets.hash(state);
        self.source_effect.hash(state);
        self.self_switch.hash(state);
        self.target_loc.hash(state);
        self.type_changer.hash(state);
        self.has_bounced.hash(state);
        self.future_hit.hash(state);
        self.bypass_protect.hash(state);
        self.beat_up.hash(state);
        self.parental_bond.hash(state);
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
    targets: Slots,
    main_target: SlotRef,
    /// Hits to make and hits made so far.
    hits: u8,
    hit: u8,
    /// `move.totalDamage`.
    total_damage: i32,
    /// Whether any hit so far did something (the move's success).
    any_ok: bool,
    /// The last hit's targets it did not fail on (Showdown's `targetsCopy` after
    /// `spreadMoveHit`, with the substitute's targets) and what the hit did to each. Read for
    /// `gotAttacked` and Emergency Exit once the hits are done.
    last_hit: Small<(SlotRef, LastHit), 6>,
    /// `ActiveMoveRef::ignore_ability` of the move in flight (Mold Breaker moves).
    ignore_ability: bool,
    /// `ActiveMoveRef::infiltrates` of the move in flight (Infiltrator).
    infiltrates: bool,
    /// [`Battle::raw_speed`] at the suspension: the action goes on in the next stage.
    raw_speed: Vec<(PokemonRef, i32)>,
    /// [`Battle::speed_snapshot`] at the suspension (the same action, so the same `pokemon.speed`).
    speed_snapshot: Vec<(PokemonRef, i32)>,
    /// Showdown `move.smartTarget` still on when the hits start (Dragon Darts with both of its
    /// smart targets left): hit `n` strikes `targets[n - 1]` alone, and `targets` keeps both.
    smart: bool,
    /// For a move another move called (Copycat, Sleep Talk: `useMove` from its `onHit`), the
    /// calling move, which waits for the called move's hits: [`finish_called`].
    caller: Option<Box<CallerFrame>>,
}

/// A calling move (Copycat, Sleep Talk) stopped right after the `spreadMoveHit` of its one hit,
/// whose `onHit` called a multi-hit move that suspended between its hits (R14). Once the called
/// move's hits and its `useMoveInner` tail are done, the caller's hit loop goes on from there
/// ([`hit_loop_rest`]), then its own `useMoveInner` tail and `runMove`'s AfterMove with the
/// called move as the active move. What the caller's `spreadMoveHit` still did after its `onHit`
/// (the target's Hit event, `self` and secondary effects of a self-targeting status move with
/// none) did nothing and ran before the suspension.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CallerFrame {
    /// The caller's hit loop after its hit (`mv` is the calling move).
    progress: MoveProgress,
    /// That hit's results.
    results: Small<Hit, 6>,
    /// The caller's main target (`use_move` sets it as the suspension passes).
    main_target: SlotRef,
}

/// How far a move got: finished, or suspended before its next hit. The suspension is kept inline
/// (its target lists are, board P4a): a move returns it once, a box would allocate per hit.
#[allow(clippy::large_enum_variant)]
pub(crate) enum MoveStep {
    Done,
    Suspended(MoveProgress),
}

/// A hit loop's result within `use_move`: finished (whether it succeeded, and the HP its
/// hits took), or suspended before its next hit (inline, as [`MoveStep`]).
#[allow(clippy::large_enum_variant)]
enum HitOutcome {
    Finished { ok: bool, total_damage: i32 },
    Suspended(MoveProgress),
}

/// Per-target result of a hit (Showdown's `damage[i]`: a number, `true`, or `false`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Hit {
    Failed,
    /// Hit without damage (status moves).
    Done,
    Damage(i32),
    /// The target's substitute took the hit (`TryPrimaryHit` returned `HIT_SUBSTITUTE`): the
    /// target is `null` for the rest of `spreadMoveHit` (only the user's own effects, `self`
    /// drops and the secondaries' `self` parts act) and its damage is 0.
    Substitute,
    /// The substitute stopped a move that deals no damage (`TryPrimaryHit` returned `null`):
    /// the target is `false` for the rest of `spreadMoveHit`, but the hit is no failure
    /// (`hitStepMoveHitLoop` only stops on `false`), so the move succeeds.
    Blocked,
}

impl Hit {
    /// `damage[i] !== false`: the move did not fail on this target.
    fn ok(self) -> bool {
        self != Hit::Failed
    }

    /// `targets[i]` is still the Pokémon: its effects apply to it.
    fn reached(self) -> bool {
        matches!(self, Hit::Done | Hit::Damage(_))
    }

    /// `targets[i] !== false`: the user's `self` drops and the secondaries run for it (a
    /// substitute's target is `null`, not `false`).
    fn in_targets(self) -> bool {
        matches!(self, Hit::Done | Hit::Damage(_) | Hit::Substitute)
    }
}

/// What a move's last hit did to one target it did not fail on, as the hit loop's tail reads it
/// (`gotAttacked`, Emergency Exit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LastHit {
    /// Numeric damage.
    Damage(i32),
    /// A status effect (`true`).
    Done,
    /// The target's substitute took it (`targetsCopy[i]` is `null`, its damage 0).
    Substitute,
    /// The substitute stopped it (`targetsCopy[i]` is `false`, its damage `null`).
    Blocked,
}

/// Where a move action aims: its `targetLoc` and its `originalTarget` (the Pokémon there when it
/// was queued; [`super::queue::ActionKind::Move`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Aim {
    pub loc: i8,
    pub original: Option<PokemonRef>,
}

/// Showdown `runMove` for the queued move `id` (`action.moveid`; `MoveId::NONE` is the `recharge`
/// pseudo-move). `will_act` is `queue.willAct()`. A multi-hit move returns
/// `MoveStep::Suspended` after its first hit; the turn engine resumes it with [`resume_move`] as
/// its own stage. `round_source`: the action's source effect is a
/// Round that moved it up (`queue::ActionKind::Move::round_source`).
pub(crate) fn run_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    aim: Aim,
    will_act: bool,
    round_source: Option<bool>,
) -> Result<MoveStep, TurnError> {
    let pokemon = b.occupant(user).expect("the caller checked the user");
    b.increment_move_actions(user);
    if id.is_none() {
        // The `recharge` pseudo-move: BeforeMove (`mustrecharge`, priority 11) ends it.
        let recharge = ActiveMove {
            id: MoveId::NONE,
            data: MoveId::NONE.data(),
            category: MoveId::NONE.data().category,
            priority: 0,
            prankster_boosted: false,
            spread: false,
            accuracy: None,
            has_sheer_force: false,
            secondary_chance_factor: 1,
            added_secondary: None,
            parental_bond: false,
            total_damage: 0,
            target: MoveId::NONE.data().target,
            move_type: MoveId::NONE.data().move_type,
            base_power: 0,
            ignore_evasion: false,
            scrappy: false,
            hit_targets: 0,
            source_effect: MoveId::NONE,
            self_switch: false,
            target_loc: 0,
            type_changer: AbilityId::NONE,
            has_bounced: false,
            future_hit: false,
            bypass_protect: 0,
            beat_up: [0; 6],
        };
        before_move(b, user, &recharge);
        // MoveAborted: Destiny Bond ends.
        conditions::destiny_bond_before_move(b, user, MoveId::NONE, false);
        return Ok(MoveStep::Done);
    }
    // `setActiveMove`: set for the whole move. A Round's source effect gives the move that
    // Round's `ignoreAbility` (`useMoveInner`: `move.ignoreAbility =
    // sourceEffect.ignoreAbility`; the user's own Mold Breaker can still set it in ModifyMove).
    b.active_move = Some(ActiveMoveRef {
        user,
        pokemon,
        id,
        ignore_ability: round_source.unwrap_or(id.data().ignore_ability),
        category: id.data().category,
        infiltrates: false,
        parental_bond: false,
    });
    // A finished move leaves its active move set (none after a failure's
    // `clearActiveMove(true)`): Showdown clears it with `runAction`'s `clearActiveMove()`, after
    // the phazing step, which the turn engine runs next (`drag_outs`, Opus DD unit B26).
    let result = run_move_inner(b, user, id, aim, will_act, round_source);
    if result.is_err() {
        b.active_move = None;
    }
    result
}

/// Whether the move has a `beforeTurnCallback` the engine runs (Counter, Mirror Coat): its
/// action queues a `beforeTurnMove` action too.
pub(crate) fn has_before_turn_callback(id: MoveId) -> bool {
    handlers::before_turn_volatile(id).is_some()
}

/// Showdown `runAction('beforeTurnMove')` for the move `id` of the Pokémon at `user` (the caller
/// checked it is active and not fainted): `getTarget` (a scripted move's random foe: never
/// `null` in a battle that goes on, and it does not matter otherwise), then the move's
/// `beforeTurnCallback`: Counter and Mirror Coat add their condition.
pub(crate) fn before_turn_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, id: MoveId) {
    if let Some(volatile) = handlers::before_turn_volatile(id) {
        b.add_volatile(user, volatile);
    }
}

/// Whether the move has a `priorityChargeCallback` the engine runs (Focus Punch, Beak Blast,
/// Shell Trap): its action queues a `priorityChargeMove` action too.
pub(crate) fn has_priority_charge_callback(id: MoveId) -> bool {
    handlers::priority_charge_volatile(id).is_some()
}

/// Showdown `runAction('priorityChargeMove')` for the move `id` of the Pokémon at `user` (the
/// caller checked it is active and not fainted): the move's `priorityChargeCallback` adds its
/// condition (`focuspunch`, `beakblast`, `shelltrap`), whatever the user's status.
pub(crate) fn priority_charge_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) {
    if let Some(volatile) = handlers::priority_charge_volatile(id) {
        b.add_volatile(user, volatile);
    }
}

/// Showdown `futuremove.onEnd` (from the residual, once due) for the future move `id` of
/// `source` stored at `slot`: nothing if the Pokémon there has fainted (or the position is
/// empty) or is the source itself; otherwise it loses Protect and Endure, and `trySpreadMoveHit`
/// runs a move built from the stored `moveData` (no `onTry`, no type-immunity exemption, priority
/// 0, no ModifyType / ModifyMove: Mold Breaker and Scrappy do not apply; Normalize's owner's hit
/// is Normal) with the source as the user: its current stats, boosts, ability and item if it is
/// on the field (Showdown ignores an inactive source's ability and item and uses its stored
/// stats; that case is unsupported). No PP, BeforeMove or AfterMoveSecondarySelf; only Life
/// Orb's recoil follows, for an active holder, whether or not the move hit. Eject Button ignores
/// future moves (the hit loop skips it); Red Card (its drag would wait for the end of the
/// residual) on the target is unsupported.
pub(crate) fn future_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    slot: SlotRef,
    source: PokemonRef,
    id: MoveId,
) -> Result<(), TurnError> {
    let Some(target) = b.alive(slot) else {
        return Ok(());
    };
    if target == source {
        return Ok(());
    }
    let data = id.data();
    // A user that left the field (switched out or fainted) hits from the bench (R5a): it stands
    // in a position of its side for the hit only (`AbsentUser`).
    let on_field = Battle::<N>::slots(source.side).find(|&s| b.occupant(s) == Some(source));
    let absent = match on_field {
        Some(_) => None,
        None => Some(AbsentUser::place(b, slot, source, id)?),
    };
    let user = on_field.unwrap_or_else(|| absent.as_ref().expect("placed").slot);
    // A Red Card on the target drags the (active) user out after the residual
    // (`residual::residual` runs the phazing step): board R5b.
    b.remove_volatile(slot, Volatile::Protect);
    b.remove_volatile(slot, Volatile::Endure);
    // `if (data.source.hasAbility('normalize')) data.moveData.type = 'Normal';`
    let move_type = if b.ability(user) == abilities::NORMALIZE {
        Type::Normal
    } else {
        data.move_type
    };
    let mut mv = ActiveMove {
        id,
        data,
        category: data.category,
        priority: 0,
        prankster_boosted: false,
        spread: false,
        accuracy: data.accuracy,
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        parental_bond: false,
        total_damage: 0,
        target: data.target,
        move_type,
        base_power: i32::from(data.base_power),
        ignore_evasion: data.ignore_evasion,
        scrappy: false,
        hit_targets: 0,
        source_effect: MoveId::NONE,
        self_switch: false,
        target_loc: 0,
        type_changer: AbilityId::NONE,
        has_bounced: false,
        future_hit: true,
        bypass_protect: 0,
        beat_up: [0; 6],
    };
    // `trySpreadMoveHit(..., notActive)`: `setActiveMove(move, source, target)`; the move ignores
    // no ability.
    b.active_move = Some(ActiveMoveRef {
        user,
        pokemon: source,
        id,
        ignore_ability: false,
        category: data.category,
        infiltrates: false,
        parental_bond: false,
    });
    b.move_self_switch = false;
    if let HitOutcome::Suspended(_) =
        try_spread_move_hit(b, user, &mut mv, smallvec::smallvec![slot], false)?
    {
        return Err(b.unsupported(format!("{}: a multi-hit future move", data.name)));
    }
    if let Some(absent) = absent {
        absent.remove(b);
    }
    // `if (data.source.isActive && data.source.hasItem('lifeorb'))` its
    // `onAfterMoveSecondarySelf`: `source !== target`, not a status move, no `forceSwitchFlag`.
    if b.alive(user) == Some(source)
        && b.item(user) == items::LIFE_ORB
        && !b.force_switch.contains(&user)
    {
        let max_hp = f64::from(b.mon(source).max_hp);
        b.damage(user, max_hp / 10.0, DamageSource::Indirect);
    }
    // `this.activeMove = null`: the hit never becomes `battle.lastMove`.
    b.active_move = None;
    b.check_win(None);
    Ok(())
}

/// A future move's user that left the field before the hit (switched out or fainted: Showdown
/// keeps `data.source`, the Pokémon itself, and `trySpreadMoveHit` runs with it inactive). It
/// attacks with its stored stats and no stages, volatiles, ability or item (`ignoringAbility()`
/// and `ignoringItem()` are true for an inactive Pokémon from Gen 5), and nothing the hit does
/// to it lands (`spreadDamage` and `boost` skip an inactive target; the handlers that act on the
/// source check `source.isActive`: Gulp Missile, Rowap Berry). The engine seats it in a position
/// of its side for the hit only (`Battle::absent_user`): a fresh slot, its ability and item
/// cleared; afterwards the position, ability, item and the rest of the Pokémon are restored as
/// they were.
///
/// The position is an empty one if the side has one, else one not holding the target whose
/// occupant nothing in the hit would ask for (board R5c). That occupant is out of the field
/// meanwhile while Showdown keeps it active (an ally of the inactive user: `findEventHandlers`
/// runs the `onAlly`/`onAny`/`onFoe` handlers of `target.alliesAndSelf()` and `target.foes()`
/// even for an inactive target when the source is active, and the reverse), so the hit is
/// refused when every such occupant [`occupant_matters`], and when the target's Cotton Down
/// (every active Pokémon but the target) would have reached a displaced occupant.
pub(crate) struct AbsentUser {
    slot: SlotRef,
    source: PokemonRef,
    /// The position's slot state before the user took it.
    saved_slot: crate::state::Slot,
    /// The user itself before (ability and item cleared, anything the hit changed).
    saved_mon: crate::state::Pokemon,
}

/// Handler events of an out-of-field occupant's ability or item that a future move's hit never
/// runs (`trySpreadMoveHit` has no `TryMove`, target redirection or side hit; nothing switches
/// in, mega evolves, traps or ends a move): an occupant whose other-Pokémon handlers are all
/// among these can leave the field for the hit.
const EVENTS_NOT_IN_A_FUTURE_HIT: &[&str] = &[
    "SwitchIn",
    "AfterMega",
    "AfterTerastallization",
    "AfterMove",
    "TrapPokemon",
    "MaybeTrapPokemon",
    "TryMove",
    "RedirectTarget",
    "TryHitSide",
];

/// Whether moving the (active) `occupant` of a position of the future move user's side out of
/// the field for the hit on `target` could change it: its ability suppresses abilities or the
/// weather, or its ability or item has an `onAny*`/`onAlly*`/`onFoe*` handler for an event the
/// hit can run. Three handlers never act here: No Guard's `onAnyAccuracy`/`onAnyInvulnerability`
/// (only for moves by or at its holder), Unaware's `onAnyModifyBoost` (only when its holder is
/// the active Pokémon or target) and Damp's `onAnyDamage` (only Aftermath's, a contact
/// reaction; future moves make no contact); Friend Guard's `onAnyModifyDamage` acts only on a
/// target that is its holder's ally. Returns the ability or item name that matters.
fn occupant_matters<const N: usize>(
    b: &Battle<'_, N>,
    occupant: PokemonRef,
    target: SlotRef,
) -> Option<&'static str> {
    let mon = b.mon(occupant);
    let ability = mon.ability.data();
    if ability.suppress_weather || mon.ability == abilities::NEUTRALIZING_GAS {
        return Some(ability.name);
    }
    let ability_exempt = |event: &str| match mon.ability {
        a if a == abilities::NO_GUARD => ["Accuracy", "Invulnerability"].contains(&event),
        a if a == abilities::UNAWARE => event == "ModifyBoost",
        a if a == abilities::DAMP => event == "Damage",
        a if a == abilities::FRIEND_GUARD => {
            event == "ModifyDamage" && target.side != occupant.side
        }
        _ => false,
    };
    let acts = |handlers: &[&str], exempt: &dyn Fn(&str) -> bool| {
        handlers.iter().any(|h| {
            let event = ["onAny", "onAlly", "onFoe"]
                .iter()
                .find_map(|prefix| h.strip_prefix(prefix));
            event.is_some_and(|e| !EVENTS_NOT_IN_A_FUTURE_HIT.contains(&e) && !exempt(e))
        })
    };
    if acts(ability.handlers, &ability_exempt) {
        return Some(ability.name);
    }
    let item = mon.item.data();
    if acts(item.handlers, &|_| false) {
        return Some(item.name);
    }
    None
}

impl AbsentUser {
    fn place<const N: usize>(
        b: &mut Battle<'_, N>,
        target: SlotRef,
        source: PokemonRef,
        id: MoveId,
    ) -> Result<AbsentUser, TurnError> {
        let name = |b: &Battle<'_, N>, p: PokemonRef| b.mon(p).species.data().name;
        let refuse = |b: &Battle<'_, N>, why: String| {
            Err(b.unsupported(format!(
                "{} of {} hitting after its user left the field, {why}",
                id.data().name,
                name(b, source)
            )))
        };
        let candidates: Slots = Battle::<N>::slots(source.side)
            .filter(|&s| s != target)
            .collect();
        // An empty position, else one whose Pokémon has fainted (not replaced yet).
        let empty = candidates
            .iter()
            .copied()
            .find(|&s| b.occupant(s).is_none())
            .or_else(|| candidates.iter().copied().find(|&s| b.alive(s).is_none()));
        let harmless = || {
            candidates.iter().copied().find(|&s| {
                b.alive(s)
                    .is_some_and(|occupant| occupant_matters(b, occupant, target).is_none())
            })
        };
        let Some(slot) = empty.or_else(harmless) else {
            let Some(&first) = candidates.first() else {
                return refuse(b, "with no position of its side free of the target".into());
            };
            let occupant = b.alive(first).expect("no empty position");
            let what = occupant_matters(b, occupant, target).expect("no harmless occupant");
            return refuse(
                b,
                format!("whose position holds {} with {what}", name(b, occupant)),
            );
        };
        // Cotton Down boosts every active Pokémon but its holder: a displaced occupant too.
        if b.ability(target) == abilities::COTTON_DOWN {
            if let Some(occupant) = b.alive(slot) {
                return refuse(
                    b,
                    format!(
                        "on a target with Cotton Down while {} is out of its position",
                        name(b, occupant)
                    ),
                );
            }
        }
        let saved_slot = b.state.slot(slot).clone();
        let saved_mon = b.mon(source).clone();
        b.apply(Instruction::Switch {
            slot,
            previous: Box::new(saved_slot.clone()),
            party_index: Some(source.party),
        });
        if !saved_mon.ability.is_none() {
            b.apply(Instruction::SetAbility {
                target: source,
                old: saved_mon.ability,
                new: AbilityId::NONE,
            });
        }
        if !saved_mon.item.is_none() {
            b.apply(Instruction::SetItem {
                target: source,
                old: saved_mon.item,
                new: ItemId::NONE,
            });
        }
        b.absent_user = Some(slot);
        Ok(AbsentUser {
            slot,
            source,
            saved_slot,
            saved_mon,
        })
    }

    /// The position and the user back as they were before [`AbsentUser::place`].
    fn remove<const N: usize>(self, b: &mut Battle<'_, N>) {
        b.absent_user = None;
        let mut undo = Vec::new();
        super::diff::slot_changes(
            &mut undo,
            self.slot,
            b.state.slot(self.slot),
            &self.saved_slot,
        );
        super::diff::pokemon_changes(&mut undo, self.source, b.mon(self.source), &self.saved_mon);
        for instruction in undo {
            b.apply(instruction);
        }
    }
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
        category: progress.mv.category,
        infiltrates: progress.infiltrates,
        parental_bond: progress.mv.parental_bond,
    });
    b.raw_speed = progress.raw_speed.clone();
    b.speed_snapshot = progress.speed_snapshot.clone();
    let mut progress = progress;
    let caller = progress.caller.take();
    let mut mv = progress.mv.clone();
    let result = match hit_loop(b, user, &mv, Some(progress))? {
        HitOutcome::Suspended(mut progress) => {
            progress.caller = caller;
            return Ok(MoveStep::Suspended(progress));
        }
        HitOutcome::Finished { ok, total_damage } => {
            mv.total_damage = total_damage;
            if !ok {
                mv.hit_targets = 0;
            }
            ok
        }
    };
    if let Some(frame) = caller {
        finish_called(b, user, pokemon, mv, result, main_target, *frame)?;
        return Ok(MoveStep::Done);
    }
    b.finish_move_result(user, result);
    use_move_tail(b, user, &mv, result, main_target)?;
    // `singleEvent('AfterMove', move)` (Sparkling Aria), then the rest of `runMove`.
    handlers::on_after_move(b, user, pokemon, &mv);
    run_move_tail(b, user, &mv)?;
    // The action's `clearActiveMove()`: `battle.lastMove` (a multi-hit move is never called).
    // The active move itself stays set through the phazing step (`drag_outs`), which clears it.
    b.record_battle_last_move(mv.id);
    Ok(MoveStep::Done)
}

/// The rest of a calling move's action (Copycat, Sleep Talk) once the multi-hit move it called is
/// done (R14): the called move's `useMove` ends (its result, its `useMoveInner` tail: Life Orb,
/// Shell Bell, `selfBoost`), the caller's `onHit` returns and its hit loop goes on after its hit
/// ([`hit_loop_rest`]: the Update, `faintMessages`, AfterMoveSecondary and Emergency Exit on its
/// target, the user), its own `useMove` ends, then `runMove`'s AfterMove with the called move as
/// the active move (`battle.lastMove`). No PP, and the user's `lastMove` is the caller's.
fn finish_called<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    pokemon: PokemonRef,
    called: ActiveMove,
    result: bool,
    called_target: SlotRef,
    frame: CallerFrame,
) -> Result<(), TurnError> {
    // The called move's `useMove`, after its hits (as `use_move`).
    let user = handlers::current_slot(b, user, pokemon);
    b.finish_move_result(user, result);
    b.active_target = Some((called_target, result));
    use_move_tail(b, user, &called, result, called_target)?;
    // The caller's hit loop, then its `useMove`.
    let CallerFrame {
        progress,
        results,
        main_target,
    } = frame;
    let mut caller = progress.mv.clone();
    // The caller's own `move.selfSwitch` again (see `call_move`).
    b.move_self_switch = caller.self_switch;
    let user = handlers::current_slot(b, user, pokemon);
    let ok = match hit_loop_rest(b, user, &caller, progress, results, false)? {
        HitOutcome::Finished { ok, total_damage } => {
            caller.total_damage = total_damage;
            if !ok {
                caller.hit_targets = 0;
            }
            ok
        }
        HitOutcome::Suspended(_) => unreachable!("a calling move hits once"),
    };
    let user = handlers::current_slot(b, user, pokemon);
    b.finish_move_result(user, ok);
    b.active_target = Some((main_target, ok));
    use_move_tail(b, user, &caller, ok, main_target)?;
    // `runMove`: `if (this.battle.activeMove) move = this.battle.activeMove;` — the AfterMove
    // events see the called move, which the action's `clearActiveMove()` makes
    // `battle.lastMove`.
    handlers::on_after_move(b, user, pokemon, &called);
    run_move_tail(b, user, &called)?;
    b.record_battle_last_move(called.id);
    Ok(())
}

/// The end of Showdown `runMove` after `useMove`: `AfterMove` (a locked move on its last
/// turn ends and, by fatigue, confuses; an Electric move ends Charge; White Herb and Mirror
/// Herb act), then faints.
fn run_move_tail<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Result<(), TurnError> {
    let locked = b.volatile(user, Volatile::LockedMove);
    if locked.active && locked.duration == 1 {
        b.remove_volatile(user, Volatile::LockedMove);
    }
    ability_events::charge_after_move(b, user, mv.id, mv.move_type);
    // The items' `onAnyAfterMove` (White Herb, Mirror Herb), collected only while the user
    // is still active; they and the lock above act on different holders.
    if b.active_move
        .is_some_and(|m| b.occupant(m.user) == Some(m.pokemon))
    {
        item_events::any_after_move(b, user);
        // Opportunist's `onAnyAfterMove` (each acts on its own holder).
        for slot in b.all_alive() {
            ability_events::opportunist_use(b, slot);
        }
    }
    // Dancer, after AfterMove (`move.flags['dance'] && moveDidSomething && !move.isExternal`).
    if !b.external_move {
        dance(b, user)?;
    }
    b.faint_messages(true)?;
    b.check_win(None);
    Ok(())
}

/// Showdown `runMove`'s Dancer step for the move the Pokémon in `user` just used (the battle's
/// active move: a move another called is the one that counts): a `dance` move that did
/// something ([`Battle::active_target`]) is copied by every Dancer
/// (`ability_events::dancers`, slowest first). Before each, `faintMessages` (the battle may end;
/// a fainted Dancer is skipped); a Dancer on the user's side copies it at the first dance's
/// target when that is a foe of the Dancer, every other Dancer at the user; the copy is an
/// external `runMove` ([`run_external_move`]).
fn dance<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef) -> Result<(), TurnError> {
    let Some(active) = b.active_move else {
        return Ok(());
    };
    let Some((first_target, true)) = b.active_target else {
        return Ok(());
    };
    if !active.id.data().flags.contains(MoveFlags::DANCE) {
        return Ok(());
    }
    let dancers = ability_events::dancers(b, active.pokemon)?;
    // Showdown's `activeMove` once the copies are done: the last copy's (none if its BeforeMove
    // stopped it: `clearActiveMove(true)`), until the action's `clearActiveMove()` after the
    // phazing step; the user's own without a copy.
    let mut last = Some(active);
    for (dancer, pokemon) in dancers {
        if b.faint_messages(true)? {
            break;
        }
        if b.alive(dancer) != Some(pokemon) {
            continue;
        }
        let aim = if first_target.side != dancer.side && user.side == dancer.side {
            first_target
        } else {
            user
        };
        run_external_move(b, dancer, active.id, loc_of(dancer, aim))?;
        last = b.active_move;
    }
    b.active_move = last;
    Ok(())
}

/// Showdown `runMove(id, pokemon, targetLoc, {externalMove: true})` for Dancer's copy by the
/// Pokémon at `user`: `activeMoveActions` counts it; no OverrideAction (Encore); the move is the
/// dex's (its own priority, no Prankster boost); BeforeMove runs (sleep, paralysis, Truant, the
/// Choice lock, ...); no PP is spent and `lastMove` stays; `useMove` with Dancer as the source
/// effect (no Pressure PP, no ability suppression from it); a lock the copy started ends at once
/// (`noLock`: Petal Dance's `lockedmove` is deleted); then AfterMove without Dancer. A multi-hit
/// copy (it would suspend the stage) is unsupported.
fn run_external_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    target_loc: i8,
) -> Result<(), TurnError> {
    let pokemon = b.occupant(user).expect("the dancer is active");
    b.increment_move_actions(user);
    let target = get_target(
        b,
        user,
        id,
        Aim {
            loc: target_loc,
            original: None,
        },
    );
    let data = id.data();
    b.active_move = Some(ActiveMoveRef {
        user,
        pokemon,
        id,
        ignore_ability: data.ignore_ability,
        category: data.category,
        infiltrates: false,
        parental_bond: false,
    });
    let mut mv = ActiveMove {
        id,
        data,
        category: data.category,
        priority: i32::from(data.priority),
        prankster_boosted: false,
        spread: false,
        accuracy: data.accuracy,
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        parental_bond: false,
        total_damage: 0,
        target: data.target,
        move_type: data.move_type,
        base_power: i32::from(data.base_power),
        ignore_evasion: data.ignore_evasion,
        scrappy: false,
        hit_targets: 0,
        source_effect: MoveId::NONE,
        self_switch: data.self_switch != SelfSwitch::No,
        type_changer: AbilityId::NONE,
        has_bounced: false,
        future_hit: false,
        bypass_protect: 0,
        beat_up: [0; 6],
        target_loc,
    };
    let recharging = b.volatile(user, Volatile::MustRecharge).active;
    let proceeds = before_move(b, user, &mv);
    conditions::destiny_bond_before_move(b, user, mv.id, proceeds);
    if !proceeds {
        let result = if recharging {
            MoveResult::Null
        } else {
            MoveResult::Failed
        };
        b.set_move_result(user, result);
        if b.volatile(user, Volatile::TwoTurnMove).active {
            b.remove_volatile(user, Volatile::TwoTurnMove);
        }
        ability_events::charge_after_move(b, user, mv.id, mv.move_type);
        // `clearActiveMove(true)`: no active move is left for the action's phazing step.
        b.active_move = None;
        return Ok(());
    }
    if handlers::before_move_callback(b, user, &mv) {
        b.set_move_result(user, MoveResult::Failed);
        b.active_move = None;
        return Ok(());
    }
    let no_lock = !b.volatile(user, Volatile::LockedMove).active;
    let outer = b.external_move;
    b.external_move = true;
    let will_act = b.will_act();
    if use_move(b, user, &mut mv, target, will_act)?.is_some() {
        return Err(b.unsupported(format!("Dancer copying {}: a multi-hit move", data.name)));
    }
    let user = handlers::current_slot(b, user, pokemon);
    // `if (this.battle.activeMove) move = this.battle.activeMove;`: a called move's AfterMove.
    let called = b.called_move.take();
    let tail = called.as_ref().unwrap_or(&mv);
    handlers::on_after_move(b, user, pokemon, tail);
    run_move_tail(b, user, tail)?;
    if no_lock {
        b.delete_volatile(user, Volatile::LockedMove);
    }
    b.external_move = outer;
    Ok(())
}

fn run_move_inner<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    chosen: MoveId,
    aim: Aim,
    will_act: bool,
    round_source: Option<bool>,
) -> Result<MoveStep, TurnError> {
    let pokemon = b.occupant(user).expect("the caller checked the user");
    // OverrideAction (Encore, not for Struggle): the encored move replaces the chosen one,
    // keeping the chosen move's priority and Prankster boost; its target is drawn afresh. Under
    // Champions an Encore that starts while the action is queued has already replaced the
    // action itself (`Battle::encore_change_action`), so this only remains for a Mental Herb
    // holder and for actions chosen while already encored.
    let encore = b.volatile(user, Volatile::Encore);
    let struggle = chosen == moves::STRUGGLE;
    let (id, target) = if !struggle && encore.active && encore.mv != chosen {
        let target = get_random_target(b, user, encore.mv.data().target);
        (encore.mv, target)
    } else {
        (chosen, get_target(b, user, chosen, aim))
    };
    let mut mv = ActiveMove {
        id,
        data: id.data(),
        category: id.data().category,
        priority: b.move_priority(user, chosen),
        prankster_boosted: b.prankster_boosted(user, chosen),
        spread: false,
        accuracy: id.data().accuracy,
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        parental_bond: false,
        total_damage: 0,
        target: id.data().target,
        move_type: id.data().move_type,
        base_power: i32::from(id.data().base_power),
        ignore_evasion: id.data().ignore_evasion,
        scrappy: false,
        hit_targets: 0,
        // `move.sourceEffect = sourceEffect.id` (Round), which Pressure's extra PP charges to
        // the same Round slot.
        source_effect: if round_source.is_some() {
            moves::ROUND
        } else {
            MoveId::NONE
        },
        self_switch: id.data().self_switch != SelfSwitch::No,
        type_changer: AbilityId::NONE,
        has_bounced: false,
        future_hit: false,
        bypass_protect: 0,
        beat_up: [0; 6],
        target_loc: aim.loc,
    };

    // `pokemon.moveThisTurnResult = willTryMove`: `false` from every BeforeMove handler
    // except `mustrecharge`, which returns `null` (no failure for Stomping Tantrum).
    let recharging = b.volatile(user, Volatile::MustRecharge).active;
    let proceeds = before_move(b, user, &mv);
    // Destiny Bond's `onBeforeMove` (priority -1, the last handler) for any other move, or its
    // `onMoveAborted`: the volatile ends at the holder's next move attempt.
    conditions::destiny_bond_before_move(b, user, mv.id, proceeds);
    if !proceeds {
        let result = if recharging {
            MoveResult::Null
        } else {
            MoveResult::Failed
        };
        b.set_move_result(user, result);
        // `twoturnmove.onMoveAborted`: the lock ends (and its end removes the move's volatile).
        if b.volatile(user, Volatile::TwoTurnMove).active {
            b.remove_volatile(user, Volatile::TwoTurnMove);
        }
        // MoveAborted (the move's type before ModifyType): Charge ends on an Electric move.
        ability_events::charge_after_move(b, user, mv.id, mv.move_type);
        // `clearActiveMove(true)`.
        b.active_move = None;
        return Ok(MoveStep::Done);
    }
    // The move's `beforeMoveCallback` (Focus Punch after losing focus): the move is not used
    // (no PP, no `lastMove`, no MoveAborted) and counts as failed (`clearActiveMove(true)`).
    if handlers::before_move_callback(b, user, &mv) {
        b.set_move_result(user, MoveResult::Failed);
        b.active_move = None;
        return Ok(MoveStep::Done);
    }

    // A locked move (Outrage's later turns, a two-turn move's second turn, also one Copycat
    // called that the user does not know) costs no PP, nor does Struggle (`deductPP` finds no
    // slot, and Struggle goes on anyway).
    if super::lock::locked_move(b.state, user).is_none() && !struggle {
        // `if (!pokemon.deductPP(baseMove, null, target) && move.id !== 'struggle')`: `deductPP`
        // finds the move's slot by id (`getMoveData`); none, or no PP left (Spite or Eerie Spell
        // took the last PP after the move was chosen), is `cant ... nopp`,
        // `clearActiveMove(true)`, `moveThisTurnResult = false`; no `lastMove`, no MoveAborted
        // (oracle `rr-spite-no-pp`).
        let slot_pp = b
            .mon(pokemon)
            .moves
            .iter()
            .position(|m| m.id == id)
            .map(|i| (i as u8, b.mon(pokemon).moves[i].pp));
        let Some((move_index, pp)) = slot_pp.filter(|&(_, pp)| pp > 0) else {
            b.set_move_result(user, MoveResult::Failed);
            b.active_move = None;
            return Ok(MoveStep::Done);
        };
        b.apply(crate::instruction::Instruction::SetPp {
            target: pokemon,
            move_index,
            old: pp,
            new: pp - 1,
        });
        // `deductPP`: `moveSlot.used = true` (Last Resort).
        b.record_move_used(user, usize::from(move_index));
    }
    // `pokemon.moveUsed(move, targetLoc)`: the action's target location (for an encored move
    // that replaced the chosen one, still the chosen one's).
    b.set_last_move(user, id);
    b.set_last_move_target_loc(user, aim.loc);

    if let Some(progress) = use_move(b, user, &mut mv, target, will_act)? {
        return Ok(MoveStep::Suspended(progress));
    }
    let user = handlers::current_slot(b, user, pokemon);
    // (Fling's user knocked out before its item was thrown gets the item's Update at 0 HP inside
    // the hit loop, `update::update_event`; the faint then clears the volatile.)
    // `if (this.battle.activeMove) move = this.battle.activeMove;`: the AfterMove events see the
    // move a calling move (Sleep Talk, Copycat) used, not the caller.
    let called = b.called_move.take();
    let tail = called.as_ref().unwrap_or(&mv);
    // `singleEvent('AfterMove', move)` (Sparkling Aria), then the rest of `runMove`.
    handlers::on_after_move(b, user, pokemon, tail);
    run_move_tail(b, user, tail)?;
    // The action's `clearActiveMove()`: `battle.lastMove` is the active move, a called move
    // (Sleep Talk's, Copycat's) rather than its caller.
    if let Some(active) = b.active_move {
        b.record_battle_last_move(active.id);
    }
    Ok(MoveStep::Done)
}

/// The BeforeMove handlers, by priority: Glaive Rush (100), recharge (11), sleep and freeze
/// (10), flinch (8), Disable (7), Gravity and Throat Chop (6), Taunt (5), a foe's Imprison (4),
/// confusion (3), Attract (2), paralysis (1), the Choice lock and Gorilla Tactics (0). `false` = the move is not used (no PP, no
/// `lastMove`).
fn before_move<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) -> bool {
    let pokemon = b.occupant(user).expect("checked");
    // Glaive Rush (priority 100): the drawback ends at the holder's next move attempt.
    if b.volatile(user, Volatile::GlaiveRush).active {
        b.remove_volatile(user, Volatile::GlaiveRush);
    }
    // mustrecharge (priority 11): the turn is spent recharging; Truant's volatile goes too
    // (`pokemon.removeVolatile('truant')`), so the holder moves after its recharge turn.
    if b.volatile(user, Volatile::MustRecharge).active {
        b.remove_volatile(user, Volatile::MustRecharge);
        b.remove_volatile(user, Volatile::Truant);
        return false;
    }
    match b.mon(pokemon).status {
        Status::Sleep => {
            // `if (pokemon.hasAbility('earlybird')) pokemon.statusState.time--;` before the
            // usual decrement: Early Bird sleeps half as long.
            let early_bird = i8::from(b.ability(user) == abilities::EARLY_BIRD);
            let time = b.mon(pokemon).status_turns - 1 - early_bird;
            b.set_status_turns(pokemon, time);
            if time <= 0 {
                b.cure_status(pokemon);
            } else if !mv.data.sleep_usable {
                // Sleep Talk and Snore go on (`if (move.sleepUsable) return;`).
                return false;
            }
        }
        // A `defrost` move goes on (and thaws in ModifyMove), except Burn Up from a Pokémon
        // without the Fire type.
        Status::Freeze if !handlers::thaws_user(b, user, mv.id) => {
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
    // Truant (priority 9).
    if !ability_events::truant_before_move(b, user) {
        return false;
    }
    if b.volatile(user, Volatile::Flinch).active {
        // `runEvent('Flinch', pokemon)`: Steadfast.
        ability_events::steadfast(b, user);
        return false;
    }
    // Disable (priority 7).
    if !conditions::before_move_after_flinch(b, user, mv.id) {
        return false;
    }
    if b.field_active(FieldEffect::Gravity) && mv.data.flags.contains(MoveFlags::GRAVITY) {
        return false;
    }
    // Taunt (priority 5), a foe's Imprison (4).
    if !conditions::before_move_after_gravity(b, user, mv.id) {
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
                // The self-hit is a single-hit `Move` effect whatever move was chosen (Anger
                // Shell / Berserk `onDamage`).
                ability_events::on_damage(b, user, true, false);
                return false;
            }
        }
    }
    // Attract (priority 2): half the time the holder cannot move.
    if b.volatile(user, Volatile::Attract).active && b.rng.chance(1, 2) {
        return false;
    }
    // Champions paralysis: 1/8.
    if b.mon(pokemon).status == Status::Paralyze && b.rng.chance(1, 8) {
        return false;
    }
    // The Choice lock and Gorilla Tactics (priority 0; both only fail the move).
    item_events::before_move(b, user, mv.id) && ability_events::gorilla_before_move(b, user, mv.id)
}

/// Showdown `getConfusionDamage(pokemon, 40)`: a 40-power typeless physical hit with the
/// user's own boosted Attack against its own boosted Defense, truncated to 16 bits, then the
/// usual 85–100% roll (`battle.randomizer`), at least 1. The roll goes through
/// `Chooser::roll` like every other damage roll, so the reduced
/// roll modes (`Extremes`, `Fixed`, `Median`, `Pessimistic`) reduce it as the oracle's modes do
/// (JJ-heavy-turn-parity: it drew all 16 before). For `Pessimistic` the self-hit counts as an
/// attack against the confused Pokémon's side (the maximum when that side is the pessimist).
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
    // `tr(tr(baseDamage * (100 - random(16))) / 100)`: ascending index i is the multiplier 85 + i.
    let rolls: crate::damage::DamageRolls =
        std::array::from_fn(|i| (base * (85 + i as i32) / 100).max(1) as u16);
    i32::from(b.rng.roll(&rolls, user.side.other()))
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

pub(crate) fn at_loc(user: SlotRef, loc: i8) -> SlotRef {
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

/// The target type a Pokémon chooses `id` with (Showdown `getMoves()`): Curse's is
/// `nonGhostTarget` (`self`) for a Pokémon without the Ghost type, so it takes no target then.
pub fn choice_target(mon: &crate::state::Pokemon, id: MoveId) -> MoveTarget {
    let data = id.data();
    match data.non_ghost_target {
        Some(target) if !mon.types.contains(&Type::Ghost) => target,
        _ => data.target,
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

/// Showdown `resolveAction`'s target location for a move action queued without one (Champions
/// Encore's `changeAction`): `getRandomTarget` draws it now for a move that takes a chosen
/// target; 0 for any other (a spread, self or random move draws its target when it runs, and its
/// location is read by nothing).
pub(crate) fn resolved_target_loc<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
) -> i8 {
    let target = id.data().target;
    if !takes_target(N, target) {
        return 0;
    }
    get_random_target(b, user, target).map_or(0, |t| loc_of(user, t))
}

/// Showdown `getTarget`. The returned slot may hold a fainted Pok챕mon (Showdown returns
/// the fainted object; the move then fails).
fn get_target<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    id: MoveId,
    aim: Aim,
) -> Option<SlotRef> {
    // `if (tracksTarget && originalTarget?.isActive) return originalTarget;`: the dex move's
    // `tracksTarget` (Snipe Shot) or `pokemon.hasAbility(['stalwart', 'propellertail'])` (their
    // ModifyMove comes later); the Pokémon it was aimed at when queued, wherever it stands now
    // (Ally Switch). One that left the field or fainted is not active.
    if let Some(original) = aim.original {
        if id.data().tracks_target || ability_events::tracks_original_target(b.ability(user)) {
            if let Some(slot) =
                crate::state::State::<N>::slot_refs().find(|&s| b.alive(s) == Some(original))
            {
                return Some(slot);
            }
        }
    }
    let loc = aim.loc;
    let target = id.data().target;
    // `if (move.smartTarget) { const curTarget = pokemon.getAtLoc(targetLoc); return curTarget &&
    // !curTarget.fainted ? curTarget : this.getRandomTarget(pokemon, move); }` (Dragon Darts:
    // no position checks, and a fainted ally is not kept).
    if id.data().smart_target {
        let chosen = (loc != 0).then(|| at_loc(user, loc));
        return match chosen.filter(|&t| b.alive(t).is_some()) {
            Some(t) => Some(t),
            None => get_random_target(b, user, target),
        };
    }
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

fn adjacent_allies<const N: usize>(b: &Battle<'_, N>, user: SlotRef) -> Slots {
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
) -> Result<Slots, TurnError> {
    Ok(match mv.target {
        MoveTarget::All | MoveTarget::FoeSide | MoveTarget::AllySide | MoveTarget::AllyTeam => {
            Small::new()
        }
        MoveTarget::AllAdjacent => {
            let mut t = adjacent_allies(b, user);
            t.extend(b.alive_slots(user.side.other()));
            t
        }
        MoveTarget::AllAdjacentFoes => b.alive_slots(user.side.other()).into_iter().collect(),
        // `alliesAndSelf()`: every active Pokémon on the user's side that has not fainted.
        MoveTarget::Allies => b.alive_slots(user.side).into_iter().collect(),
        _ => {
            let mut t = target;
            if b.alive(t).is_none() && t.side != user.side {
                match get_random_target(b, user, mv.target) {
                    Some(r) => t = r,
                    None => return Ok(Small::new()),
                }
            }
            let mut smart = mv.data.smart_target;
            if N > 1 && !ability_events::tracks_target(b, user, mv.data, mv.target) {
                let (redirected, cleared) = redirect_target(b, user, mv, t);
                t = redirected;
                smart &= !cleared;
            }
            let targets = if smart {
                smart_targets(b, user, t)
            } else {
                smallvec::smallvec![t]
            };
            // `if (target.fainted && !move.flags['futuremove'])`: a future move may still be aimed
            // at the position of a fainted ally.
            if b.alive(targets[0]).is_none() && !mv.data.flags.contains(MoveFlags::FUTUREMOVE) {
                return Ok(Small::new());
            }
            targets
        }
    })
}

/// Showdown `getSmartTargets(target, move)` (Dragon Darts): the target and its adjacent ally
/// (`target.adjacentAllies()[0]`: the other active Pokémon of its side with HP), in that order;
/// only the target when that ally is missing, fainted or the user itself, only the ally when the
/// target has no HP (`move.smartTarget = false` in both cases: one target left).
fn smart_targets<const N: usize>(b: &Battle<'_, N>, user: SlotRef, target: SlotRef) -> Slots {
    let ally = Battle::<N>::slots(target.side)
        .find(|&s| s != target && b.alive(s).is_some())
        .filter(|&s| s != user);
    match ally {
        None => smallvec::smallvec![target],
        Some(ally) if b.alive(target).is_none() => smallvec::smallvec![ally],
        Some(ally) => smallvec::smallvec![target, ally],
    }
}

/// Showdown `priorityEvent('RedirectTarget')`: the handlers are Follow Me, Rage Powder and
/// Spotlight on the user's foes (`onFoeRedirectTarget`, priority 1, 1, 2) and Lightning Rod /
/// Storm Drain on anyone else (`onAnyRedirectTarget`, priority 0). `runEvent` with `fastExit`
/// sorts them with `compareRedirectOrder` (a plain stable sort, no Speed-tie shuffle): priority,
/// then the holder's Speed, then the holder's `abilityState.effectOrder` (whose ability state
/// started first: switch-in, `setAbility`; `Slot::ability_order`), and the first whose holder is
/// a valid target of the move's target type wins. Rage Powder skips powder-immune users. Last comes
/// the user's own Counter / Mirror Coat condition (`onRedirectTarget`, priority -1): the slot
/// of the foe whose hit it recorded, whoever stands there now. The flag is whether a Follow Me,
/// Rage Powder, Lightning Rod or Storm Drain handler took the move (`if (move.smartTarget)
/// move.smartTarget = false;`: Dragon Darts then strikes that one target twice); Spotlight's
/// leaves `smartTarget` on.
fn redirect_target<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> (SlotRef, bool) {
    match foe_redirect_target(b, user, mv) {
        Some((slot, priority)) => (slot, priority < 2),
        None => (
            handlers::counter_redirect(b, user, mv).unwrap_or(target),
            false,
        ),
    }
}

/// The `RedirectTarget` handlers of priority 0 and above ([`redirect_target`]): the new target,
/// if one of them redirects, with the winning handler's priority (2 Spotlight, 1 Follow Me and
/// Rage Powder, 0 Lightning Rod and Storm Drain).
fn foe_redirect_target<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> Option<(SlotRef, i8)> {
    // (priority, speed, holder), in Showdown's handler collection order: the user's side
    // (`onAny`), then each foe's `onFoe` volatiles and `onAny` ability.
    let mut handlers: crate::turn::Small<(i8, i32, SlotRef), 8> = crate::turn::Small::new();
    // `breakable`: a Mold Breaker move ignores these handlers too.
    let absorbs = |b: &Battle<'_, N>, s: SlotRef| {
        b.alive(s).is_some() && absorbing_type(b.ability_unless_broken(s)) == Some(mv.move_type)
    };
    for s in Battle::<N>::slots(user.side) {
        if absorbs(b, s) {
            handlers.push((0, b.event_speed(s), s));
        }
    }
    for s in b.alive_slots(user.side.other()) {
        let speed = b.event_speed(s);
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
        return None;
    }
    // `compareRedirectOrder` in a stable `Array.prototype.sort` (no Speed-tie shuffle): priority,
    // Speed, then the holders' `abilityState.effectOrder` (`Slot::ability_order`). Two holders
    // never share one, so only one holder's own handlers keep their collection order.
    handlers.sort_by(|x, y| {
        let started = |s: SlotRef| Battle::<N>::ability_order_key(b.state.slot(s).ability_order, s);
        y.0.cmp(&x.0)
            .then(y.1.cmp(&x.1))
            .then(started(x.2).cmp(&started(y.2)))
    });

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
    handlers
        .iter()
        .find(|&&(priority, _, holder)| valid(b, priority, holder))
        .map(|&(priority, _, holder)| (holder, priority))
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
    // Dancer's `activeTarget` and `moveDidSomething`: set once the move ran.
    b.active_target = None;
    // `pokemon.moveThisTurnResult = undefined` (`useMove`); every early return below is a
    // failure (`false`).
    b.set_move_result(user, MoveResult::Undefined);
    b.move_self_switch = mv.self_switch;
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
    // A category the move's ModifyMove chose is the active move's (`move.category`).
    if let Some(active) = b.active_move.as_mut() {
        if active.id == mv.id {
            active.category = mv.category;
        }
    }
    ability_hooks::on_modify_type(b, user, mv);
    // The user's Electrify (`onModifyTypePriority: -2`, after the abilities' handlers).
    handlers::volatile_modify_type(b, user, mv);
    ability_hooks::on_modify_move(b, user, mv)?;
    // Throat Chop's `onModifyMove` (the user's volatile) returns `false` for a sound move: the
    // move is gone (`if (!move || pokemon.fainted) return false;`). Only a called move gets here
    // (BeforeMove stops a chosen one); nothing else in runEvent('ModifyMove') changes the state.
    if conditions::throat_chopped(b.state, user, mv.id) {
        return Ok(None);
    }
    // Heal Block's `onModifyMove` does the same for a `heal` move.
    if conditions::heal_blocked(b.state, user, mv.id) {
        return Ok(None);
    }
    if mv.target != base_target {
        target = get_random_target(b, user, mv.target);
    }
    // Gravity's `onModifyMove`: a Gravity-blocked move fails (only reachable for a called move;
    // BeforeMove stops a chosen one).
    if b.field_active(FieldEffect::Gravity) && mv.data.flags.contains(MoveFlags::GRAVITY) {
        b.finish_move_result(user, false);
        return Ok(None);
    }
    // Freeze `onModifyMove`: a defrosting move thaws the user.
    if b.mon(pokemon).status == Status::Freeze && mv.data.flags.contains(MoveFlags::DEFROST) {
        b.cure_status(pokemon);
    }
    // The item's onModifyMove: the Choice lock (priority 0; `choicelock`'s `onStart` fails for a
    // bounced move), King's Rock's flinch (-1).
    if !mv.has_bounced {
        item_events::on_modify_move(b, user, mv.id);
    }
    // Stench (the user's ability, ModifyMove priority -1, sub-order 7) and King's Rock / Razor
    // Fang (the item, -1, 8) append the same flinch: whichever comes second finds it there. They
    // run after Sheer Force (priority 0), which deleted the move's secondaries (a move that had
    // its own flinch gets the added one), and before Serene Grace (-2), which doubles it.
    let own: &[Secondary] = if mv.has_sheer_force {
        &[]
    } else {
        handlers::move_secondaries(b, mv)
    };
    mv.added_secondary = item_events::added_secondary(
        b.item(user),
        b.ability(user) == abilities::STENCH,
        mv.data,
        own,
    );
    // ModifyTarget (`useMoveInner`, before a random target would be drawn): Metal Burst and
    // Comeuppance aim at the slot of the foe that last damaged the user this turn.
    if let Some(scripted) = handlers::modify_target(b, user, mv) {
        target = Some(scripted);
    }
    let Some(target) = target else {
        b.finish_move_result(user, false);
        return Ok(None);
    };

    let mut main_target = target;
    // Showdown `tryMoveHit`: moves aimed at the field or a side, and `allyTeam` moves (Heal
    // Bell), whose `onHit` covers the whole party.
    let field_move = matches!(
        mv.target,
        MoveTarget::All | MoveTarget::FoeSide | MoveTarget::AllySide | MoveTarget::AllyTeam
    );
    let targets = if field_move {
        Slots::new()
    } else {
        get_move_targets(b, user, mv, target)?
    };
    deduct_pressure_pp(b, user, mv, &targets);
    // TryMove's target: the last of the targets (after redirection), else the one aimed at.
    let try_move_target = targets.last().copied().unwrap_or(target);
    // The move's own TryMove (`singleEvent('TryMove')`) returns `null` for a two-turn move's
    // charging turn (`attacker.addVolatile('twoturnmove', defender); return null;`) and for
    // Double Shock / Burn Up without the Electric / Fire type: the move stops, and `useMove`
    // stores that
    // `null` as the move's result (no failure for Stomping Tantrum and Temper Flare).
    if !handlers::charge_try_move(b, user, mv, try_move_target)
        || !handlers::null_try_move(b, user, mv)
    {
        if b.slot_history(user).move_this_turn_result == MoveResult::Undefined {
            b.set_move_result(user, MoveResult::Null);
        }
        return Ok(None);
    }
    // TryMove: the move's own that fail it (Pollen Puff under Heal Block), then Dazzling,
    // Queenly Majesty, Armor Tail (`onFoeTryMove`).
    if !handlers::fail_try_move(b, user, mv, try_move_target)
        || !ability_hooks::on_try_move(b, user, mv, try_move_target)
    {
        b.finish_move_result(user, false);
        return Ok(None);
    }
    // The Metronome item's condition (`onTryMovePriority: -2`, the last TryMove handler).
    item_events::metronome_try_move(b, user, mv.id);
    // `selfdestruct: 'always'` (Explosion, Self-Destruct, Misty Explosion): the user faints now,
    // before its hits (even without a target), and attacks at 0 HP.
    if mv.data.selfdestruct == SelfDestruct::Always {
        b.faint(user);
    }
    let result = if field_move {
        try_move_hit_field(b, user, mv, target, will_act)?
    } else {
        let Some(&last) = targets.last() else {
            b.finish_move_result(user, false);
            return Ok(None);
        };
        main_target = last;
        match try_spread_move_hit(b, user, mv, targets, will_act)? {
            HitOutcome::Finished { ok, total_damage } => {
                mv.total_damage = total_damage;
                if !ok {
                    mv.hit_targets = 0;
                }
                ok
            }
            HitOutcome::Suspended(mut progress) => {
                // A move this one called suspended (Copycat's): this move's own main target
                // waits with it in the caller frame.
                match progress.caller.as_deref_mut() {
                    Some(frame) => frame.main_target = main_target,
                    None => progress.main_target = main_target,
                }
                return Ok(Some(progress));
            }
        }
    };
    // Ally Switch moved the user (Showdown's steps below act on the Pokémon wherever it is).
    let user = handlers::current_slot(b, user, pokemon);
    // The self-switch flag was set in `spread_move_hit` (`runMoveEffects`), before the targets'
    // Emergency Exit could clear it. The request is made, or the flag dropped for a side
    // without a bench, after the action.
    b.finish_move_result(user, result);
    b.active_target = Some((main_target, result));
    use_move_tail(b, user, mv, result, main_target)?;
    Ok(None)
}

/// Showdown `useMove(id, pokemon, {target})` from a move's handler (Sleep Talk and Copycat
/// `onHit`, Mirror Move `onTryHit`): the called move takes the caller's priority and Prankster
/// boost and ability suppression, its source effect is the caller (whose PP pays Pressure), its
/// target is `target` or, without one, drawn afresh, and it runs `useMoveInner` without
/// BeforeMove, PP or `lastMove`. The called move stays the active move. A multi-hit called move
/// of Copycat or Sleep Talk suspends after its first hit like a chosen one: its progress waits
/// in [`Battle::called_suspension`] until the caller's hit loop takes it and suspends with it
/// (R14); Mirror Move's is unsupported (it calls from `onTryHit`).
fn call_move<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    caller: &ActiveMove,
    id: MoveId,
    target: Option<SlotRef>,
) -> Result<(), TurnError> {
    let pokemon = b.occupant(user).expect("the caller's user is active");
    let ignore_ability = b.active_move.is_some_and(|m| m.ignore_ability);
    b.active_move = Some(ActiveMoveRef {
        user,
        pokemon,
        id,
        ignore_ability,
        category: id.data().category,
        infiltrates: false,
        parental_bond: false,
    });
    let data = id.data();
    let mut mv = ActiveMove {
        id,
        data,
        category: data.category,
        priority: caller.priority,
        prankster_boosted: caller.prankster_boosted,
        spread: false,
        accuracy: data.accuracy,
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        parental_bond: false,
        total_damage: 0,
        target: data.target,
        move_type: data.move_type,
        base_power: i32::from(data.base_power),
        ignore_evasion: data.ignore_evasion,
        scrappy: false,
        hit_targets: 0,
        source_effect: caller.id,
        self_switch: data.self_switch != SelfSwitch::No,
        target_loc: 0,
        type_changer: AbilityId::NONE,
        has_bounced: false,
        future_hit: false,
        bypass_protect: 0,
        beat_up: [0; 6],
    };
    let target = match target {
        Some(t) => Some(t),
        None => get_random_target(b, user, data.target),
    };
    let will_act = b.will_act();
    // `move.selfSwitch` belongs to each move object: the called move's (U-turn's) must not be
    // read back by the caller's own `runMoveEffects` tail (board B41, `uu-copycat-uturn-protected`).
    let caller_self_switch = b.move_self_switch;
    if let Some(progress) = use_move(b, user, &mut mv, target, will_act)? {
        if ![moves::COPYCAT, moves::SLEEP_TALK].contains(&caller.id) {
            return Err(b.unsupported(format!(
                "{} called by {}: a multi-hit called move",
                data.name, caller.data.name
            )));
        }
        b.called_suspension = Some(progress);
        return Ok(());
    }
    b.move_self_switch = caller_self_switch;
    // It stays the active move: the caller's AfterMove sees it (`run_move_tail`).
    b.called_move = Some(mv);
    Ok(())
}

/// Whether Magic Bounce on `holder` (breakable) bounces `mv`: a `reflectable` move from another
/// Pokémon that has not bounced already (`onTryHit` / `onAllyTryHitSide`; no semi-invulnerable
/// state exists).
fn magic_bounce_reflects<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    holder: SlotRef,
) -> bool {
    holder != user
        && !mv.has_bounced
        && mv.data.flags.contains(MoveFlags::REFLECTABLE)
        && b.ability_unless_broken(holder) == abilities::MAGIC_BOUNCE
}

/// Magic Bounce's bounce: `newMove = getActiveMove(move.id)` with `hasBounced` and no Prankster
/// boost, `useMove(newMove, holder, {target: source})`. The copy keeps the original's priority
/// (`useMoveInner` copies the active move's) and its source effect is the ability, so it neither
/// ignores abilities nor pays Pressure PP; it runs `useMoveInner` without BeforeMove, PP or
/// `lastMove`. Showdown leaves the copy as the battle's active move; the engine restores the
/// original's, which only the status source of the original's remaining targets reads (Showdown
/// passes that source explicitly).
fn bounce_move<const N: usize>(
    b: &mut Battle<'_, N>,
    holder: SlotRef,
    source: SlotRef,
    original: &ActiveMove,
) -> Result<(), TurnError> {
    let saved = b.active_move;
    let pokemon = b.occupant(holder).expect("the bouncer is active");
    let (id, data) = (original.id, original.data);
    // `move.ignoreAbility = sourceEffect.ignoreAbility`: the ability has none.
    b.active_move = Some(ActiveMoveRef {
        user: holder,
        pokemon,
        id,
        ignore_ability: false,
        category: data.category,
        infiltrates: false,
        parental_bond: false,
    });
    let mut mv = ActiveMove {
        id,
        data,
        category: data.category,
        priority: original.priority,
        prankster_boosted: false,
        spread: false,
        accuracy: data.accuracy,
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        parental_bond: false,
        total_damage: 0,
        target: data.target,
        move_type: data.move_type,
        base_power: i32::from(data.base_power),
        ignore_evasion: data.ignore_evasion,
        scrappy: false,
        hit_targets: 0,
        source_effect: MoveId::NONE,
        type_changer: AbilityId::NONE,
        has_bounced: true,
        future_hit: false,
        bypass_protect: 0,
        beat_up: [0; 6],
        // A bounced Parting Shot switches the bouncer out (`moveHit` sets the flag for the
        // copy's user).
        self_switch: data.self_switch != SelfSwitch::No,
        target_loc: 0,
    };
    let will_act = b.will_act();
    if use_move(b, holder, &mut mv, Some(source), will_act)?.is_some() {
        return Err(b.unsupported(format!("{} bounced: a multi-hit move", data.name)));
    }
    b.active_move = saved;
    Ok(())
}

/// The end of Showdown `useMoveInner` after the hits: the `self` boost, then MoveFail (a
/// failed move) or AfterMoveSecondarySelf (Life Orb), each followed by the user's Emergency
/// Exit check.
fn use_move_tail<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    result: bool,
    main_target: SlotRef,
) -> Result<(), TurnError> {
    if result && mv.data.self_boost != NO_BOOSTS {
        b.boost_by(
            user,
            &mv.data.self_boost,
            Some(user),
            BoostEffect::Move(mv.id),
        );
    }
    // `if (pokemon && pokemon !== target && move.category !== 'Status')`, then the user's
    // Emergency Exit if the handlers took its HP (`originalHp`, taken just before them) to half.
    let checks_user = user != main_target && mv.data.category != MoveCategory::Status;
    let hp_before = b.slot_mon(user).map_or(0, |m| m.hp);
    if !result {
        // MoveFail: High Jump Kick's crash.
        handlers::on_move_fail(b, user, mv);
        if checks_user {
            user_emergency_exit(b, user, hp_before);
        }
        return Ok(());
    }
    // AfterMoveSecondarySelf (skipped for a Sheer Force-boosted move and a future move, which
    // hits later): the user's item (Life Orb, Shell Bell, Throat Spray).
    if !ability_hooks::sheer_force_skips(b, user, mv)
        && !mv.data.flags.contains(MoveFlags::FUTUREMOVE)
    {
        // The move's own handler (`singleEvent`: Fell Stinger, Order Up, Relic Song), then
        // `runEvent`.
        handlers::after_move_secondary_self(b, user, main_target, mv)?;
        item_events::after_move_secondary_self(b, user, main_target, mv.data, mv.total_damage);
        // Magician (an ability, sub-order 7, before the item's 8: it needs an empty-handed user,
        // so the item handlers above never acted when it can).
        ability_events::magician(b, user, mv.id, hit_target_slots::<N>(mv.hit_targets))?;
        if checks_user {
            user_emergency_exit(b, user, hp_before);
        }
    }
    Ok(())
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
    // `if (!sourceEffect || callerMoveForPressure)`: a bounced move's source effect is Magic
    // Bounce, a Dancer copy's Dancer, neither of which has PP; nor has the `lockedmove` condition,
    // which `runMove` passes as the source effect of a locked Pokémon's move (Outrage's later
    // turns, a two-turn move's second, Uproar: oracle `nn-pressure-locked-outrage`). The lock is
    // the one `runMove` saw: nothing between its check and this one starts or ends a lock. A move
    // another called pays through its caller (Copycat), locked or not.
    if mv.has_bounced
        || b.external_move
        || (mv.source_effect.is_none() && super::lock::locked_move(b.state, user).is_some())
    {
        return;
    }
    let foe = user.side.other();
    let pressure = |pressure_targets: &[SlotRef]| {
        pressure_targets
            .iter()
            .filter(|t| t.side != user.side && b.ability(**t) == abilities::PRESSURE)
            .count()
    };
    let extra = if mv.data.flags.contains(MoveFlags::MUSTPRESSURE) {
        pressure(&b.alive_slots(foe))
    } else {
        match mv.target {
            MoveTarget::All => pressure(&b.alive_slots(foe)),
            MoveTarget::FoeSide | MoveTarget::AllySide | MoveTarget::AllyTeam => 0,
            _ => pressure(targets),
        }
    };
    if extra == 0 {
        return;
    }
    let pokemon = b.occupant(user).expect("checked");
    // `deductPP(callerMoveForPressure || move)`: a called move's caller pays (Sleep Talk).
    let paying = if mv.source_effect.is_none() {
        mv.id
    } else {
        mv.source_effect
    };
    let Some(index) = b.mon(pokemon).moves.iter().position(|m| m.id == paying) else {
        return;
    };
    b.record_move_used(user, index);
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
    if [moves::WIDE_GUARD, moves::QUICK_GUARD, moves::CRAFTY_SHIELD].contains(&mv.id) && !will_act {
        return Ok(false);
    }
    // Mat Block: `if (source.activeMoveActions > 1) return false; return !!this.queue.willAct();`
    if mv.id == moves::MAT_BLOCK && (b.state.slot(user).move_actions > 1 || !will_act) {
        return Ok(false);
    }
    // PrepareHit: the user's ability (Protean, Libero).
    prepare_hit_ability(b, user, mv);
    // TryHitSide on a foe's side: Magic Bounce's `onAllyTryHitSide` on an active Pokémon of that
    // side bounces the move (`return null`: it fails without a message). A second holder then
    // sees `move.hasBounced`; which one bounces does not matter for a side condition.
    if mv.target == MoveTarget::FoeSide {
        let holder = b
            .alive_slots(target.side)
            .into_iter()
            .find(|&s| target.side != user.side && magic_bounce_reflects(b, user, mv, s));
        if let Some(holder) = holder {
            bounce_move(b, holder, user, mv)?;
            return Ok(false);
        }
    }
    // TryHitSide on a Pokémon of the user's side (`allySide`, `allyTeam`): Sap Sipper's
    // `onAllyTryHitSide` (breakable) raises the Attack of the user's ally for a Grass move
    // (`if (source === this.effectState.target || !target.isAlly(source)) return;`; Soundproof's
    // only logs; Magic Bounce's skips moves aimed at the own side).
    if matches!(mv.target, MoveTarget::AllySide | MoveTarget::AllyTeam)
        && mv.move_type == Type::Grass
    {
        for ally in adjacent_allies(b, user) {
            if b.ability_unless_broken(ally) == abilities::SAP_SIPPER {
                let mut up = NO_BOOSTS;
                up[0] = 1;
                b.boost_by(
                    ally,
                    &up,
                    Some(user),
                    BoostEffect::Ability(abilities::SAP_SIPPER),
                );
            }
        }
    }
    // runMoveEffects on the target: undefined (nothing attempted) counts as success.
    let mut outcome: Option<bool> = None;
    let mut combine = |r: bool| outcome = Some(outcome.unwrap_or(false) || r);
    // Hit: an `allyTeam` move's `onHit` (Heal Bell, Aromatherapy) on the first Pokémon of the
    // user's side (only its side matters).
    if mv.target == MoveTarget::AllyTeam {
        match handlers::on_hit(b, user, target, mv)? {
            Some(HitResult::Success) => combine(true),
            Some(HitResult::Failure) => combine(false),
            Some(HitResult::NotFail) | None => {}
        }
    }
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
    // The other `onHitSide` handlers (Gear Up, Magnetic Flux).
    if let Some(r) = handlers::on_hit_side(b, user, mv) {
        combine(r);
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
    if let Some(r) = handlers::on_hit_field(b, user, mv)? {
        combine(r);
    }
    // `if (moveData.selfSwitch)` (Chilly Reception, aimed at the field: its target is the user):
    // a success with a bench and no `commanded`, else a failure combined in.
    if data.self_switch != SelfSwitch::No {
        let can_switch = super::residual::bench(b, user.side).next().is_some()
            && !b.volatile(user, Volatile::Commanded).active;
        combine(can_switch);
    }
    let result = outcome.unwrap_or(true);
    // The end of `runMoveEffects`: `source.switchFlag = move.id` once anything happened.
    if result
        && b.move_self_switch
        && b.alive(user).is_some()
        && !b.volatile(user, Volatile::Commanded).active
    {
        b.set_switch_flag(user, self_switch_flag(data.self_switch));
    }
    Ok(result)
}

/// Showdown `trySpreadMoveHit` (also for single-target moves).
fn try_spread_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    mut targets: Slots,
    will_act: bool,
) -> Result<HitOutcome, TurnError> {
    // Dragon Darts' `move.smartTarget` (its two smart targets from `get_move_targets`): `if
    // (targets.length > 1 && !move.smartTarget) move.spreadHit = true;` — never a spread hit. Any
    // target a hit step drops turns it off (a miss, an immunity, a failure, and the protect
    // family's `onTryHit`, which returns NOT_FAIL but sets `move.smartTarget = false` first).
    let mut smart = mv.data.smart_target && targets.len() > 1;
    mv.spread = targets.len() > 1 && !smart;

    // Future Sight / Doom Desire at use: their `onTry` adds the `futuremove` slot condition at
    // the target's position and returns `NOT_FAIL` (the move succeeds, nothing hits now), or
    // fails when one is already there. Their hit later has no `onTry`.
    if mv.data.flags.contains(MoveFlags::FUTUREMOVE) && !mv.future_hit {
        let ok = conditions::start_future_move(b, user, targets[0], mv.id);
        return Ok(HitOutcome::Finished {
            ok,
            total_damage: 0,
        });
    }

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
    // Round's `onTry`: the first queued Round moves up with this one as its source effect.
    if mv.id == moves::ROUND {
        let ignore_ability = b.active_move.is_some_and(|m| m.ignore_ability);
        b.prioritize_round(ignore_ability);
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
    // Destiny Bond's `onPrepareHit`: `return !pokemon.removeVolatile('destinybond');` (it
    // fails when used again while it is up).
    if mv.id == moves::DESTINY_BOND && b.remove_volatile(user, Volatile::DestinyBond) {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // The move's other `onPrepareHit` handlers (Ally Switch, Fling).
    if !handlers::on_prepare_hit(b, user, mv) {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    prepare_hit_ability(b, user, mv);
    if parental_bond_applies(b, user, mv) {
        mv.parental_bond = true;
        if let Some(active) = b.active_move.as_mut() {
            active.parental_bond = true;
        }
    }

    // 0. Invulnerability (`hitStepInvulnerabilityEvent`): a semi-invulnerable target is not hit
    //    (a failure for the move) unless its state lets the move through, No Guard is in play,
    //    or the move is Toxic from a Poison type.
    let before = targets.len();
    targets.retain(|&mut t| !handlers::invulnerable(b, user, mv, t));
    smart &= targets.len() == before;
    if targets.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 1. TryHit: Psychic Terrain (priority 4), Protect (3), the target's ability (0). Each
    //    target's handlers only affect that target, so targets can be taken one at a time.
    //    `trySpreadMoveHit`: when no target is left and none of them failed (every one was a
    //    `NOT_FAIL` drop: Protect, the guards), `pokemon.moveThisTurnResult = null` — Stomping
    //    Tantrum and Temper Flare do not double after a move that Protect blocked.
    let mut kept = Small::with_capacity(targets.len());
    let mut at_least_one_failure = false;
    let before = targets.len();
    for t in targets {
        match try_hit(b, user, mv, t)? {
            TryHit::Hit => kept.push(t),
            TryHit::NotFail => {}
            TryHit::Fail => at_least_one_failure = true,
        }
    }
    // A failure, or a protection's NOT_FAIL (the only NOT_FAIL a smart-target move can meet on
    // one of its two targets), ends `smartTarget`.
    smart &= kept.len() == before;
    targets = kept;
    if targets.is_empty() {
        if !at_least_one_failure {
            b.set_move_result(user, MoveResult::Null);
        }
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 2. Type immunity.
    let before = targets.len();
    targets.retain(|&mut t| !type_immune(b, mv, t));
    smart &= targets.len() == before;
    if targets.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 3. Move-specific immunities: powder, the move's `onTryImmunity`, Prankster vs Dark.
    handlers::try_immunity_problem(b, user, mv, &targets)?;
    let before = targets.len();
    targets.retain(|&mut t| {
        let powder = mv.data.flags.contains(MoveFlags::POWDER)
            && t != user
            && b.natural_immune(t, TypeImmunities::POWDER);
        let prankster = mv.prankster_boosted
            && t.side != user.side
            && b.natural_immune(t, TypeImmunities::PRANKSTER);
        !powder && handlers::on_try_immunity(b, user, mv, t) && !prankster
    });
    smart &= targets.len() == before;
    if targets.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 4. Accuracy.
    let mut hit = Small::with_capacity(targets.len());
    for &t in &targets {
        if accuracy_check(b, user, mv, t) {
            hit.push(t);
        } else if mv.data.ohko == Ohko::No {
            item_events::blunder_policy(b, user);
        }
    }
    smart &= hit.len() == targets.len();
    if hit.is_empty() {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 5. `hitStepBreakProtect` (Feint, Hyperspace Hole): every target left loses its protection.
    if mv.data.breaks_protect {
        for &t in &hit {
            handlers::break_protect(b, t);
        }
    }
    // The hit's first step is the move's own `onTryHit` (Champions `spreadMoveHit`: on the
    // first target only; failing fails the move).
    if !handlers::on_try_hit(b, user, hit[0], mv)? {
        return Ok(HitOutcome::Finished {
            ok: false,
            total_damage: 0,
        });
    }
    // 7. The hit loop, on the targets left (`move.hitTargets` unless every hit fails). With
    // `smartTarget` on, the hit loop's `damage` array ends up without the first target's entry,
    // so `move.hitTargets` is the second target alone.
    mv.hit_targets = if smart {
        target_bit::<N>(hit[1])
    } else {
        hit.iter().fold(0, |bits, &t| bits | target_bit::<N>(t))
    };
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
        last_hit: Small::new(),
        ignore_ability: b.active_move.is_some_and(|a| a.ignore_ability),
        infiltrates: b.active_move.is_some_and(|a| a.infiltrates),
        raw_speed: Vec::new(),
        speed_snapshot: Vec::new(),
        smart,
        caller: None,
    };
    hit_loop(b, user, mv, Some(progress))
}

/// A slot's bit in [`ActiveMove::hit_targets`].
fn target_bit<const N: usize>(slot: SlotRef) -> u8 {
    1 << (slot.side.index() * N + usize::from(slot.slot))
}

/// The slots in a [`ActiveMove::hit_targets`] bit set, side one first, in slot order.
fn hit_target_slots<const N: usize>(bits: u8) -> Slots {
    [SideId::One, SideId::Two]
        .into_iter()
        .flat_map(Battle::<N>::slots)
        .filter(|&s| bits & target_bit::<N>(s) != 0)
        .collect()
}

/// How many times the move hits (`hitStepMoveHitLoop`): 1, a fixed count, or for 2–5 hit
/// moves Showdown's 35/35/15/15 draw (Skill Link: always the maximum; Loaded Dice: 4 or 5
/// evenly, and 4–10 evenly for a 10-hit move).
fn decide_hits<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) -> u8 {
    // Beat Up: `move.multihit = move.allies.length` (a number: no draw, no Skill Link).
    if mv.id == moves::BEAT_UP {
        return mv.beat_up.iter().filter(|&&p| p != 0).count().max(1) as u8;
    }
    // Parental Bond: `move.multihit = 2`.
    if mv.parental_bond {
        return 2;
    }
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

/// Parental Bond's `onPrepareHit` (the user's ability, `runEvent('PrepareHit')`): `if
/// (move.category === 'Status' || move.multihit || move.flags['noparentalbond'] ||
/// move.flags['charge'] || move.flags['futuremove'] || move.spreadHit || move.isZ || move.isMax)
/// return;` — otherwise the move hits twice ([`ActiveMove::parental_bond`]). `move.spreadHit`:
/// more than one target when the hit starts ([`ActiveMove::spread`]), so a spread move with one
/// target left hits twice.
fn parental_bond_applies<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
) -> bool {
    let flags = mv.data.flags;
    b.ability(user) == abilities::PARENTAL_BOND
        && mv.category != MoveCategory::Status
        && mv.data.multihit.is_none()
        && !flags.contains(MoveFlags::NOPARENTALBOND)
        && !flags.contains(MoveFlags::CHARGE)
        && !flags.contains(MoveFlags::FUTUREMOVE)
        && !mv.spread
        && !mv.data.is_z
        && !mv.data.is_max
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
    // `if (move.hasBounced || move.flags['futuremove'] || move.sourceEffect === 'snatch' ||
    // move.callsMove) return;` and `type !== '???'` (Struggle).
    if mv.data.calls_move
        || mv.move_type == Type::None
        || mv.data.flags.contains(MoveFlags::FUTUREMOVE)
    {
        return;
    }
    if b.volatile(user, Volatile::ProteanUsed).active {
        return;
    }
    let pokemon = b.occupant(user).expect("the user is active");
    let mon = b.mon(pokemon);
    let new = [mv.move_type, Type::None];
    // `source.getTypes().join() !== type` (an added type counts).
    if b.types(user) == [mv.move_type, Type::None, Type::None]
        || [493, 773].contains(&mon.species.data().num)
    {
        return;
    }
    let old = mon.types;
    if old != new {
        b.apply(crate::instruction::Instruction::SetTypes {
            target: pokemon,
            old,
            new,
        });
    }
    // `setType` drops the added type.
    conditions::clear_added_type(b, user);
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

/// The TryHit handlers for one target by priority (`compareLeftToRightOrder`: priority, then
/// target index; each target's handlers only act on that target, its attacker, or nothing that
/// another target's handlers read): Psychic Terrain, Wide Guard and Quick Guard (4), the
/// protect family (3, `handlers::protect_try_hit`: the target's volatiles, collected before its
/// side's conditions), Crafty Shield and Mat Block (3, `handlers::side_guard_try_hit`), Magic
/// Bounce (1), then the target's ability and item. `false` = the move fails on it.
/// A target's verdict from the TryHit step (`hitStepTryHitEvent`): hit, dropped without a
/// failure (Showdown `NOT_FAIL`: Protect and its family, Quick Guard, Wide Guard, Crafty
/// Shield, Mat Block), or dropped as a failure (every `null`/`false` handler).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TryHit {
    Hit,
    NotFail,
    Fail,
}

/// A protection's verdict on a move (its `onTryHit` through `checkMoveBypassesProtect`): it does
/// not apply, it stops the move (`NOT_FAIL`), or it would have and a `HitProtect` handler let the
/// move through (Unseen Fist, Piercing Drill).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guard {
    Open,
    Blocked,
    Bypassed,
}

fn try_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &mut ActiveMove,
    target: SlotRef,
) -> Result<TryHit, TurnError> {
    if psychic_terrain_blocks(b, user, mv, target) {
        return Ok(TryHit::Fail);
    }
    // `runEvent('HitProtect', source, target, move)` inside every `checkMoveBypassesProtect`
    // (Wide Guard, Quick Guard, the protect family, Mat Block): Unseen Fist and Piercing Drill let
    // a contact move through and set the target's `bypassProtect` (its damage is quartered).
    let hit_protect = ability_events::hit_protect(b, user, mv.data);
    let mut bypassed = false;
    match guarded_by_side(b, user, mv, target, hit_protect) {
        Guard::Blocked => return Ok(TryHit::NotFail),
        Guard::Bypassed => bypassed = true,
        Guard::Open => {}
    }
    match handlers::protect_try_hit(b, user, mv, target, hit_protect) {
        Guard::Blocked => return Ok(TryHit::NotFail),
        Guard::Bypassed => bypassed = true,
        Guard::Open => {}
    }
    match handlers::side_guard_try_hit(b, user, mv, target, hit_protect) {
        Guard::Blocked => return Ok(TryHit::NotFail),
        Guard::Bypassed => bypassed = true,
        Guard::Open => {}
    }
    if bypassed {
        mv.bypass_protect |= target_bit::<N>(target);
    }
    // Magic Bounce (`onTryHit`, priority 1: after Psychic Terrain, the guards, Protect and the
    // side guards, before every priority-0 handler) uses a copy of the move back at its user,
    // then `return null`. Showdown runs the TryHit handlers of all targets together by priority;
    // taking the targets one at a time only moves the other targets' handlers of priority 0..1
    // before or after the bounce, and none of them reads what a bounced status move changes.
    if magic_bounce_reflects(b, user, mv, target) {
        bounce_move(b, target, user, mv)?;
        return Ok(TryHit::Fail);
    }
    // Sturdy `onTryHit`: OHKO moves fail (breakable).
    if mv.data.ohko != Ohko::No && b.ability_unless_broken(target) == abilities::STURDY {
        return Ok(TryHit::Fail);
    }
    // The target's item `onTryHit` (Safety Goggles against powder).
    if item_events::try_hit_blocks(b, user, mv.data, target) {
        return Ok(TryHit::Fail);
    }
    // Dry Skin `onTryHit` (breakable): another Pok챕mon's Water move heals the holder by 1/4
    // of its max HP (nothing at full HP) and fails on it (`return null`).
    if mv.move_type == Type::Water
        && target != user
        && b.ability_unless_broken(target) == abilities::DRY_SKIN
    {
        let max_hp = f64::from(b.slot_mon(target).expect("a target").max_hp);
        b.heal(target, max_hp / 4.0);
        return Ok(TryHit::Fail);
    }
    // Lightning Rod / Storm Drain `onTryHit` (breakable): the holder absorbs the move.
    if absorbed_by_ability(b, user, mv, target) {
        return Ok(TryHit::Fail);
    }
    // The other abilities' `onTryHit` (absorbing and immunity abilities).
    if ability_hooks::on_try_hit(b, user, mv, target) {
        Ok(TryHit::Fail)
    } else {
        Ok(TryHit::Hit)
    }
}

/// The TryHit handlers of priority 4, which only fail the move: Psychic Terrain, Wide Guard,
/// Quick Guard.
/// Psychic Terrain's `onTryHit` (priority 4, `return null`: a failure): a priority move at a
/// grounded foe.
fn psychic_terrain_blocks<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    b.terrain() == Terrain::Psychic
        && mv.priority > 0
        && mv.target != MoveTarget::User
        && target.side != user.side
        && b.is_grounded(target)
}

/// Wide Guard / Quick Guard on the target's side (`onTryHit`, priority 4, `return
/// this.NOT_FAIL`): spread moves, or moves with positive priority (after Prankster and the
/// like), that Protect would block (`checkMoveBypassesProtect`: the `protect` flag; status
/// moves too). They also cover a move from the target's own ally. Like Protect, a guard that
/// stops the move resets the user's locked move on its first turn
/// ([`handlers::reset_first_turn_lock`]; no supported locking move is a spread or priority
/// move today).
fn guarded_by_side<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
    hit_protect: bool,
) -> Guard {
    if !mv.data.flags.contains(MoveFlags::PROTECT) {
        return Guard::Open;
    }
    let spread = matches!(
        mv.target,
        MoveTarget::AllAdjacent | MoveTarget::AllAdjacentFoes
    );
    let guarded = (spread && b.side_effect_active(target.side, SideEffect::WideGuard))
        || (mv.priority > 0 && b.side_effect_active(target.side, SideEffect::QuickGuard));
    if !guarded {
        return Guard::Open;
    }
    // `if (this.runEvent('HitProtect', source, target, move)) return;` comes before the
    // "Outrage counter is reset" line, so a bypassed guard leaves the lock alone.
    if hit_protect {
        return Guard::Bypassed;
    }
    handlers::reset_first_turn_lock(b, user);
    Guard::Blocked
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
    // A future move's hit: its `moveData` has `ignoreImmunity: false` (Future Sight) or none
    // (Doom Desire: `category === 'Status'`, false).
    let ignore = if mv.future_hit {
        IgnoreImmunity::No
    } else {
        mv.data.ignore_immunity
    };
    match ignore {
        IgnoreImmunity::All => return false,
        IgnoreImmunity::Type(t) if t == ty => return false,
        _ => {}
    }
    // Scrappy / Mind's Eye: `move.ignoreImmunity['Fighting'] = move.ignoreImmunity['Normal'] =
    // true` (keyed by the move's type when immunity is checked).
    if mv.scrappy && matches!(ty, Type::Fighting | Type::Normal) {
        return false;
    }
    // `runEvent('NegateImmunity', target, type)`: Foresight on a Ghost against Normal and
    // Fighting, Miracle Eye on a Dark type against Psychic (`handlers::immunity_negated`).
    if handlers::immunity_negated(b, target, ty) {
        return false;
    }
    if ty == Type::Ground {
        return !b.is_grounded(target);
    }
    if b.slot_mon(target).is_none() {
        return true;
    }
    // `getTypes()`: the added type too (Trick-or-Treat's Ghost).
    b.types(target)
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
    // OHKO moves bypass every accuracy modifier (`hitStepAccuracy`): against a target that is
    // not semi-invulnerable, 30 (Sheer Cold 20 for a non-Ice user) plus the level difference,
    // and a target of higher level, or of the type of a typed OHKO move (Sheer Cold vs Ice), is
    // immune; a semi-invulnerable target (reached through No Guard or Lock-On) skips all that
    // and keeps the move's own accuracy. Then `runEvent('Accuracy')` as for any move: No Guard
    // (the user's or the target's), Glaive Rush's drawback, Minimize and Lock-On make it hit;
    // Micle Berry skips OHKO moves (`if (!move.ohko)`).
    if mv.data.ohko != Ohko::No {
        let accuracy = if conditions::semi_invulnerable(b, target).is_some() {
            i32::from(mv.accuracy.unwrap_or(30))
        } else {
            let level = |s: SlotRef| b.slot_mon(s).map_or(0, |m| i32::from(m.level));
            let (mine, theirs) = (level(user), level(target));
            let immune_type = matches!(mv.data.ohko, Ohko::Typed(t) if b.has_type(target, t));
            if mine < theirs || immune_type {
                return false;
            }
            let base = match mv.data.ohko {
                Ohko::Typed(t) if !b.has_type(user, t) => 20,
                _ => 30,
            };
            base + mine - theirs
        };
        return match ability_hooks::accuracy_event(b, user, mv, target) {
            None => true,
            Some(modifier) => b.rng.chance(modify(accuracy, modifier).max(0) as u32, 100),
        };
    }
    // `accuracy = true` without the `Accuracy` event: a status move on the user, and (gen 8+)
    // Toxic used by a Poison type.
    let self_status = mv.target == MoveTarget::User && mv.data.category == MoveCategory::Status;
    if self_status || (mv.id == moves::TOXIC && b.has_type(user, Type::Poison)) {
        return true;
    }
    let Some(base) = mv.accuracy else {
        // `accuracy === true`: the `Accuracy` event still runs (Micle Berry's handler ends its
        // volatile), and nothing it does can make the move miss.
        ability_hooks::accuracy_event(b, user, mv, target);
        return true;
    };
    // ModifyAccuracy: Wonder Skin's replacement, then Gravity (6840/4096), the abilities
    // (Hustle, Compound Eyes, Sand Veil, Victory Star, ...) and items (Wide Lens, Zoom Lens,
    // Bright Powder).
    let mut accuracy = ability_events::accuracy_direct(b, user, target, mv.data, i32::from(base));
    let mut accuracy_mods = ability_events::accuracy_handlers(b, user, target, mv.data);
    accuracy_mods.extend(item_events::accuracy_handlers(b, user, target));
    if b.field_active(FieldEffect::Gravity) {
        accuracy_mods.push(Handler::global(0, SUB_FIELD_CONDITION, 6840));
    }
    accuracy = modify(accuracy, ability_events::chain(b, accuracy_mods));
    let mut boost = 0i32;
    if !mv.ignore_evasion {
        boost -= i32::from(b.boost_seen(target, 6, user, false));
    }
    boost += i32::from(b.boost_seen(user, 5, target, true));
    let boost = boost.clamp(-6, 6);
    if boost > 0 {
        accuracy = accuracy * (3 + boost) / 3;
    } else if boost < 0 {
        accuracy = accuracy * 3 / (3 - boost);
    }
    // `runEvent('Accuracy')` (`ability_hooks::accuracy_event`): No Guard and Glaive Rush make the
    // move hit, Micle Berry chains 4915/4096.
    match ability_hooks::accuracy_event(b, user, mv, target) {
        None => true,
        Some(modifier) => b.rng.chance(modify(accuracy, modifier).max(0) as u32, 100),
    }
}

/// The accuracy re-roll of a later hit of a `multiaccuracy` move (Champions
/// `hitStepMoveHitLoop`: Triple Kick, Triple Axel, Population Bomb), which differs from
/// [`accuracy_check`]: the stages come first, in floating point: the user's accuracy stage and
/// the target's evasion stage (unless the move ignores evasion) are each clamped and applied as
/// `boostTable` factors (`[1, 4/3, 5/3, 2, 7/3, 8/3, 3]`) without truncation. Then
/// `runEvent('ModifyAccuracy')` and `runEvent('Accuracy')`, whose chained modifiers
/// (`chainModify`: Compound Eyes, Hustle, Wide Lens, Gravity, Micle Berry, ...) Showdown
/// applies at the end of the event only to a non-negative integer relay value (`relayVar ===
/// Math.abs(Math.floor(relayVar))`), so a fractional accuracy passes both events unchanged.
/// Wonder Skin's direct 50 only answers status moves, which none of these is. Finally
/// `randomChance(accuracy, 100)` is `random(100) < accuracy`: it succeeds `ceil(accuracy)`
/// times in 100.
fn multi_accuracy_check<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    let Some(base) = mv.accuracy else {
        ability_hooks::accuracy_event(b, user, mv, target);
        return true;
    };
    const BOOST_TABLE: [f64; 7] = [1.0, 4.0 / 3.0, 5.0 / 3.0, 2.0, 7.0 / 3.0, 8.0 / 3.0, 3.0];
    let mut accuracy = f64::from(base);
    let stage = i32::from(b.boost_seen(user, 5, target, true)).clamp(-6, 6);
    if stage > 0 {
        accuracy *= BOOST_TABLE[stage as usize];
    } else {
        accuracy /= BOOST_TABLE[(-stage) as usize];
    }
    if !mv.ignore_evasion {
        let stage = i32::from(b.boost_seen(target, 6, user, false)).clamp(-6, 6);
        if stage > 0 {
            accuracy /= BOOST_TABLE[stage as usize];
        } else if stage < 0 {
            accuracy *= BOOST_TABLE[(-stage) as usize];
        }
    }
    // `runEvent`'s final `this.modify(relayVar, this.event.modifier)`, for an integer only.
    let integral = |x: f64| x >= 0.0 && x == x.floor();
    if integral(accuracy) {
        let mut accuracy_mods = ability_events::accuracy_handlers(b, user, target, mv.data);
        accuracy_mods.extend(item_events::accuracy_handlers(b, user, target));
        if b.field_active(FieldEffect::Gravity) {
            accuracy_mods.push(Handler::global(0, SUB_FIELD_CONDITION, 6840));
        }
        accuracy = f64::from(modify(
            accuracy as i32,
            ability_events::chain(b, accuracy_mods),
        ));
    }
    match ability_hooks::accuracy_event(b, user, mv, target) {
        None => true,
        Some(modifier) => {
            if integral(accuracy) {
                accuracy = f64::from(modify(accuracy as i32, modifier));
            }
            b.rng.chance(accuracy.ceil().clamp(0.0, 100.0) as u32, 100)
        }
    }
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
    let targets = progress.targets.clone();
    // Champions `hitStepMoveHitLoop` with `move.smartTarget` and two targets: `targetsCopy =
    // [targets[hit - 1]]`, each dart strikes one of them.
    let smart = progress.smart;
    let hit_targets: Slots = if smart {
        smallvec::smallvec![progress.targets[usize::from(hit - 1)]]
    } else {
        targets.clone()
    };
    // A later hit of a multi-accuracy move (Population Bomb) can miss and end the loop.
    let mut ended_by_miss = false;
    let rerolls = mv.data.multiaccuracy
        && b.ability(user) != abilities::SKILL_LINK
        && b.item(user) != items::LOADED_DICE;
    if hit > 1 && rerolls && !targets.is_empty() {
        let first = targets[0];
        if !multi_accuracy_check(b, user, mv, first) {
            ended_by_miss = true;
        }
    }
    let mut results = Small::new();
    if !ended_by_miss {
        // `move.totalDamage` so far only reaches the hit's handlers through Innards Out, which
        // adds it only without `smartTarget`.
        let total_before = if smart { 0 } else { progress.total_damage };
        results = spread_move_hit(b, user, mv, &hit_targets, total_before, hit)?;
        progress.hit = hit;
        progress.total_damage += results
            .iter()
            .map(|r| if let Hit::Damage(d) = r { *d } else { 0 })
            .sum::<i32>();
        let this_hit: Small<(SlotRef, LastHit), 6> = hit_targets
            .iter()
            .zip(&results)
            .filter_map(|(&t, r)| match r {
                Hit::Damage(d) => Some((t, LastHit::Damage(*d))),
                Hit::Done => Some((t, LastHit::Done)),
                Hit::Substitute => Some((t, LastHit::Substitute)),
                Hit::Blocked => Some((t, LastHit::Blocked)),
                Hit::Failed => None,
            })
            .collect();
        // With `smartTarget` each target keeps its own dart's result (`moveDamage` grows by one
        // entry per hit).
        if smart {
            progress.last_hit.extend(this_hit);
        } else {
            progress.last_hit = this_hit;
        }
        let hit_ok = results.iter().any(|r| r.ok());
        progress.any_ok |= hit_ok;
        // This hit's `onHit` (Copycat's, Sleep Talk's) called a multi-hit move that suspended
        // after its first hit: the calling move stops here and goes on once the called move's
        // hits are done ([`finish_called`]).
        if let Some(mut called) = b.called_suspension.take() {
            called.caller = Some(Box::new(CallerFrame {
                progress,
                results,
                main_target: user,
            }));
            return Ok(HitOutcome::Suspended(called));
        }
    }
    hit_loop_rest(b, user, mv, progress, results, ended_by_miss)
}

/// The rest of a hit of [`hit_loop`] after its `spreadMoveHit` (or its multi-accuracy miss): the
/// Update, the next hit's suspension, or the loop's end.
fn hit_loop_rest<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    mut progress: MoveProgress,
    results: Small<Hit, 6>,
    ended_by_miss: bool,
) -> Result<HitOutcome, TurnError> {
    let smart = progress.smart;
    if !ended_by_miss {
        let hit = progress.hit;
        let hit_ok = results.iter().any(|r| r.ok());
        // `eachEvent('Update')` after the hit's damage (berries eat before faints are
        // processed).
        super::update::update_event(b)?;
        let mut targets = progress.targets.clone();
        targets.retain(|&mut t| b.alive(t).is_some());
        let single = progress.targets.len() == 1;
        let user_standing = b.alive(user).is_some();
        // `if (!pokemon.hp && targets.length === 1) break;` — a fainted user stops a
        // single-target move; every target fainted stops any.
        if hit_ok && hit < progress.hits && !targets.is_empty() && (user_standing || !single) {
            // The smart targets stay in place: the next dart goes to the second one.
            if !smart {
                progress.targets = targets;
            }
            // A faint still waiting for `faintMessages` (the first smart target's, or the
            // user's) cannot cross a stage (the queue lives in `Battle`): the next hit comes at
            // once, in this stage.
            if b.faint_pending() {
                return hit_loop(b, user, mv, Some(progress));
            }
            progress.raw_speed = b.raw_speed.clone();
            progress.speed_snapshot = b.speed_snapshot.clone();
            return Ok(HitOutcome::Suspended(progress));
        }
    }
    // The loop ended: `faintMessages(false, false, !pokemon.hp)`, recoil, AfterMoveSecondary.
    let user_fainted = b.alive(user).is_none();
    b.faint_messages(user_fainted)?;
    let total = progress.total_damage;
    // `if (move.totalDamage) this.applyRecoilDamage(move.totalDamage, move, pokemon)`: Struggle's
    // `directDamage` (which no Damage handler sees: Rock Head, Magic Guard, Endure, Sturdy) or
    // a `recoil` move's, then Emergency Exit on the user.
    if total > 0 {
        apply_recoil_damage(b, user, mv, total);
    }
    // `gotAttacked` and `timesAttacked` (`hit - 1` = the hits made) for the last hit's
    // targets other than the user (after a later multi-accuracy miss, the previous hit's).
    // A target whose substitute took or stopped the hit is `null` / `false` in `targetsCopy`
    // and not attacked, unless a later hit missed: `targetsCopy` is then a fresh copy of the
    // targets with the earlier hit's damage (0 for a substitute's, `null` for a stopped one).
    // With `smartTarget` (`targetsCopy = targets.slice(0)`) each target is attacked by its own
    // dart (`moveDamage[i]`; a substitute's `true` is no numeric damage) once.
    for &(t, last) in &progress.last_hit.clone() {
        if t == user {
            continue;
        }
        if smart {
            let damage = match last {
                LastHit::Damage(d) => Some(d),
                _ => None,
            };
            b.record_attack(t, user, damage, 1);
            continue;
        }
        let damage = match (last, ended_by_miss) {
            (LastHit::Damage(d), _) => Some(d),
            (LastHit::Done, _) | (LastHit::Blocked, true) => None,
            (LastHit::Substitute, true) => Some(0),
            (LastHit::Substitute | LastHit::Blocked, false) => continue,
        };
        b.record_attack(t, user, damage, progress.hit);
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
    // target), one event over all of them in Showdown's handler order
    // (`items::after_move_secondary_order`: priority, holder Speed, then sub-order): a thawing
    // move thaws a frozen target (`frz`'s handler), Anger Shell / Berserk, the target's item
    // (Kee / Maranga Berry, Eject Button, Red Card). The damage a target took is its last
    // `attackedBy` entry, or `move.totalDamage` for a multi-hit move.
    super::update::update_event(b)?;
    if !ability_hooks::sheer_force_skips(b, user, mv) {
        // `targetsCopy.filter(val => !!val)`: not a target its substitute shielded. With
        // `smartTarget`, `targetsCopy` is every target again, shielded or not, each with its own
        // dart's damage.
        let dart = |t: SlotRef| match progress.last_hit.iter().find(|&&(s, _)| s == t) {
            Some(&(_, LastHit::Damage(d))) => d,
            _ => 0,
        };
        let last_hit: Small<(SlotRef, i32), 6> = if smart {
            progress.targets.iter().map(|&t| (t, dart(t))).collect()
        } else if ended_by_miss {
            progress.targets.iter().map(|&t| (t, 0)).collect()
        } else {
            progress
                .targets
                .iter()
                .zip(&results)
                .filter(|(_, r)| r.reached())
                .map(|(&t, r)| (t, if let Hit::Damage(d) = r { *d } else { 0 }))
                .collect()
        };
        let slots: Slots = last_hit.iter().map(|&(t, _)| t).collect();
        let order = item_events::after_move_secondary_order(b, &slots, mv.data.thaws_target);
        for (i, handler) in order {
            let (t, damage) = last_hit[i];
            match handler {
                item_events::AfterMoveSecondaryHandler::Thaw => {
                    if let Some(p) = b.alive(t) {
                        if b.mon(p).status == Status::Freeze {
                            b.cure_status(p);
                        }
                    }
                }
                item_events::AfterMoveSecondaryHandler::Ability => {
                    // Berserk / Anger Shell: `move.multihit && !move.smartTarget ?
                    // move.totalDamage : lastAttackedBy.damage` (Beat Up and Parental Bond count
                    // as multihit).
                    let damage = if (is_multihit(mv.id) || mv.parental_bond) && !smart {
                        total
                    } else {
                        damage
                    };
                    ability_events::after_move_secondary(b, user, t, damage, total);
                    ability_events::pickpocket(b, user, t, mv.data)?;
                    ability_events::color_change(b, t, mv.move_type, mv.data.category);
                }
                item_events::AfterMoveSecondaryHandler::Item => {
                    // Eject Button: `!move.flags['futuremove']` (a future move's hit never ejects;
                    // oracle `rr-future-sight-eject-button`).
                    if !(mv.future_hit && b.item(t) == items::EJECT_BUTTON) {
                        item_events::after_move_secondary(b, user, t, mv.data.category);
                    }
                }
            }
        }
        // `runEvent('EmergencyExit', target, pokemon)` for each of the hit loop's targets still
        // standing whose HP this move took to half: `(hurtThisTurn || 0) + curDamage > maxhp /
        // 2`, with `curDamage` the move's total damage for a single target, else the target's
        // entry of the loop's `damage` array: the last hit's numeric damage, and 0 for every
        // other result (a status effect, a failure on that target, a hit its substitute took or
        // stopped: `md === true || !md ? 0 : md`). A target the move did no damage to can still
        // qualify when its HP dropped this turn without a Damage event (Pain Split, Substitute,
        // Belly Drum) after it was last hurt above half. After a later hit's miss the array
        // still holds the previous hit's values.
        // With `smartTarget` the loop's `damage` array has lost the first target's entry
        // (`damage = [damage[hit - 1]]` each hit): only the second target is checked, with its own
        // dart's damage (`targets.length` is 2, so not the total).
        let damages: Small<(SlotRef, i32), 6> = if smart {
            smallvec::smallvec![(progress.targets[1], dart(progress.targets[1]))]
        } else if ended_by_miss {
            progress
                .targets
                .iter()
                .map(|&t| {
                    let last = progress.last_hit.iter().find(|&&(s, _)| s == t);
                    let damage = match last {
                        Some(&(_, LastHit::Damage(d))) => d,
                        _ => 0,
                    };
                    (t, damage)
                })
                .collect()
        } else {
            progress
                .targets
                .iter()
                .zip(&results)
                .map(|(&t, r)| (t, if let Hit::Damage(d) = r { *d } else { 0 }))
                .collect()
        };
        for (t, damage) in damages {
            let Some(pokemon) = b.alive(t) else {
                continue;
            };
            let current = if mv.spread || smart { damage } else { total };
            let mon = b.mon(pokemon);
            let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
            let hurt = b.slot_history(t).hurt_this_turn.map_or(0, i32::from);
            if 2 * hp <= max_hp && 2 * (hurt + current) > max_hp {
                super::switching::emergency_exit(b, t);
            }
        }
    }
    Ok(HitOutcome::Finished {
        ok: true,
        total_damage: total,
    })
}

/// Showdown `spreadMoveHit` for the move's own hit. `total_before` is `move.totalDamage` so far
/// (the earlier hits of a multi-hit move); `hit` is `move.hit` (1 for the first hit).
fn spread_move_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    targets: &[SlotRef],
    total_before: i32,
    hit: u8,
) -> Result<Small<Hit, 6>, TurnError> {
    let data = mv.data;
    // `getMoveHitData(move).typeMod` is (re)computed by this hit's `getDamage`.
    b.hit_type_mod = [[None; N]; 2];
    b.hit_crit = [[false; N]; 2];
    // 0. `tryPrimaryHitEvent` for every target first: a substitute takes the hit
    //    (`hit_substitute`). Aura Break's `onAnyTryPrimaryHit` (priority 0, before the
    //    substitute's -1) only sets a flag `get_damage` reads.
    let mut shielded = Small::<_, 6>::with_capacity(targets.len());
    for &t in targets {
        // Gulp Missile's `onSourceTryPrimaryHit` (the user's, priority 0: before the
        // substitute's -1): Surf fills Cramorant's throat.
        if mv.id == moves::SURF {
            super::forme::gulp_missile_catch(b, user);
        }
        // A Gem's `onSourceTryPrimaryHit` (the user's item, priority 0, after Gulp Missile).
        item_events::gem_try_primary_hit(b, user, t, mv.category, mv.move_type);
        shielded.push(if substitute_takes_hit(b, user, mv, t) {
            Some(hit_substitute(b, user, mv, t, hit)?)
        } else {
            None
        });
    }
    // getSpreadDamage: every other target's damage is decided before any is dealt.
    let mut planned = Small::<_, 6>::with_capacity(targets.len());
    for (&t, shield) in targets.iter().zip(&shielded) {
        planned.push(match shield {
            Some(_) => None,
            None => Some(get_damage(b, user, mv, t, hit, false)?),
        });
    }
    // spreadDamage.
    let mut results = Small::with_capacity(targets.len());
    for ((&t, plan), shield) in targets.iter().zip(&planned).zip(&shielded) {
        let Some(plan) = plan else {
            results.push(shield.expect("a shielded target has its result"));
            continue;
        };
        let result = match *plan {
            Planned::Fail => Hit::Failed,
            Planned::NoDamage => Hit::Done,
            Planned::Damage(d) => {
                let dealt = b.damage(t, f64::from(d), DamageSource::Move);
                if dealt > 0 {
                    if let Some(drain) = data.drain {
                        let amount =
                            (f64::from(dealt) * f64::from(drain.0) / f64::from(drain.1)).round();
                        // `this.battle.heal(..., pokemon, target, 'drain')`: Big Root, the
                        // target's Liquid Ooze.
                        b.heal_rooted_from(user, amount, Some(t));
                    }
                }
                Hit::Damage(dealt)
            }
        };
        results.push(result);
    }
    // runMoveEffects.
    for (i, &t) in targets.iter().enumerate() {
        match results[i] {
            Hit::Failed | Hit::Blocked => continue,
            // `targets[i]` is `null`: of the effects only the user's own act
            // (`selfdestruct: 'ifHit'`, as `damage[i]` is 0; the self-switch check only adds to
            // the result).
            Hit::Substitute => {
                if data.selfdestruct == SelfDestruct::IfHit {
                    b.faint(user);
                }
                continue;
            }
            Hit::Done | Hit::Damage(_) => {}
        }
        let mut did: Option<bool> = None;
        let mut note = |r: bool| did = Some(did.unwrap_or(false) || r);
        // `moveData.boosts` as the move's ModifyMove left them (Growth in sun).
        let boosts = handlers::move_boosts(b, user, mv)?;
        if boosts != NO_BOOSTS
            && b.alive(t).is_some()
            && !handlers::boosts_applied_in_try_hit(mv.id)
        {
            note(b.boost_by(t, &boosts, Some(user), BoostEffect::Move(mv.id)));
        }
        if let Some(heal) = handlers::move_heal(mv) {
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
        if let Some(volatile) = Volatile::from_condition(data.volatile_status)
            .filter(|_| handlers::keeps_volatile_status(b, user, mv))
        {
            // Attract (the move): `target.addVolatile('attract', source)` remembers its source
            // and checks the genders, Oblivious, Aroma Veil (`conditions::attract_fails`); its
            // `onStart` runs the Attract event (Destiny Knot: `conditions::add_attract`).
            let added = if volatile == Volatile::Attract {
                let fails = conditions::attract_fails(b, t, user)?;
                if !fails {
                    conditions::add_attract(b, t, user);
                }
                !fails
            } else {
                b.add_volatile(t, volatile)
            };
            // Gastro Acid's condition `onStart`: the suppressed ability's `End`.
            if added && volatile == Volatile::GastroAcid {
                ability_events::gastro_acid_start(b, t)?;
            }
            note(added);
        }
        // `if (moveData.slotCondition) hitResult = target.side.addSlotCondition(target,
        // moveData.slotCondition, source, move)` (Wish, Healing Wish, Revival Blessing).
        if let Some(condition) = conditions::slot_condition_of(data.slot_condition) {
            note(conditions::add_slot_condition(b, t, condition, user));
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
        // `if (moveData.selfSwitch) { if (canSwitch(source.side) &&
        // !source.volatiles['commanded']) didSomething = true; else didSomething =
        // combineResults(didSomething, false); }`
        if data.self_switch != SelfSwitch::No {
            note(
                super::residual::bench(b, user.side).next().is_some()
                    && !b.volatile(user, Volatile::Commanded).active,
            );
        }
        // `if (moveData.forceSwitch) { hitResult = !!this.battle.canSwitch(target.side);
        // didSomething = this.battle.combineResults(didSomething, hitResult); }`
        if data.force_switch {
            note(super::residual::bench(b, t.side).next().is_some());
        }
        // The move's own onHit; NOT_FAIL neither succeeds nor fails.
        match handlers::on_hit(b, user, t, mv)? {
            Some(HitResult::Success) => note(true),
            Some(HitResult::Failure) => note(false),
            Some(HitResult::NotFail) | None => {}
        }
        // `runEvent('Hit')`: the target's volatiles (Focus Punch, Beak Blast, Shell Trap;
        // condition sub-order 2), its ability (Anger Point, 7), then its item (Sticky Barb,
        // Enigma Berry, 8).
        handlers::volatile_on_hit(b, user, t, mv);
        ability_events::anger_point(b, t);
        item_events::on_hit(b, user, t, data);
        // `selfdestruct: 'ifHit'` (Memento, Final Gambit): the user faints once the move reached
        // this target (`damage[i] !== false`, before the effects' result is combined in).
        if data.selfdestruct == SelfDestruct::IfHit && results[i] != Hit::Failed {
            b.faint(user);
        }
        if let (Hit::Done, Some(false)) = (results[i], did) {
            results[i] = Hit::Failed;
        }
    }
    // The end of `runMoveEffects`: `else if (move.selfSwitch && source.hp &&
    // !source.volatiles['commanded']) source.switchFlag = move.id` once anything happened
    // (Parting Shot's `onHit` withdrew `selfSwitch` if its drops failed). It comes before the
    // targets' Emergency Exit, which clears every other active's flag: U-turn into a Pokémon it
    // takes below half leaves only that Pokémon switching. A move with a `self` effect (Baton
    // Pass, Shed Tail) sets it even when nothing happened (`!didAnything && ... &&
    // !moveData.self` is the failure branch); without a bench the request clears it again.
    if b.move_self_switch
        && b.alive(user).is_some()
        && !b.volatile(user, Volatile::Commanded).active
        && (results.iter().any(|r| *r != Hit::Failed) || data.self_effect.is_some())
    {
        b.set_switch_flag(user, self_switch_flag(data.self_switch));
    }
    // selfDrops: boosts once, for the first target the move did not fail on; an effect
    // without boosts (Roost's, Outrage's volatile) is applied to the user for every such
    // target. Sheer Force deleted `self`; Serene Grace doubled its chance. A `self` the move's
    // `onTryHit` wrote (Curse from a non-Ghost) has no chance and only boosts.
    if let Some(boosts) = handlers::try_hit_self_boosts(b, user, mv) {
        if results.iter().any(|r| r.in_targets()) {
            b.boost_by(user, &boosts, Some(user), BoostEffect::Move(mv.id));
        }
    }
    if let Some(effect) = data.self_effect.filter(|_| !mv.has_sheer_force) {
        let chance = u32::from(effect.chance) * mv.secondary_chance_factor;
        if effect.boosts != NO_BOOSTS {
            if results.iter().any(|r| r.in_targets()) && b.rng.chance(chance, 100) {
                b.boost_by(user, &effect.boosts, Some(user), BoostEffect::Move(mv.id));
            }
        } else if let Some(volatile) = Volatile::from_condition(effect.volatile_status) {
            for _ in results.iter().filter(|r| r.in_targets()) {
                if b.add_volatile_from(user, volatile, mv.id) && volatile == Volatile::Roost {
                    conditions::roost_start(b, user);
                }
            }
        } else {
            // A `self` effect with only an `onHit` (Double Shock).
            for _ in results.iter().filter(|r| r.in_targets()) {
                handlers::self_on_hit(b, user, mv);
            }
        }
    }
    // secondaries: each target's ModifySecondaries (Shield Dust), then one roll per secondary
    // (Sheer Force deleted them; Serene Grace doubled the chances).
    for (i, &t) in targets.iter().enumerate() {
        match results[i] {
            Hit::Failed | Hit::Blocked => continue,
            Hit::Substitute => {
                substitute_secondaries(b, user, mv);
                continue;
            }
            Hit::Done | Hit::Damage(_) => {}
        }
        // Secondaries (`ability_hooks::secondaries`): the move's own (Sheer Force deleted them)
        // and King's Rock's appended flinch (ModifyMove priority -1: after Sheer Force, so even
        // through it), their chances doubled by Serene Grace (-2), less what the target's
        // ModifySecondaries drops: Shield Dust there, Covert Cloak (`keeps_secondary`) below.
        // Parental Bond's `onSourceModifySecondaries`: on Secret Power's first hit only flinch
        // secondaries stay (`move.id === 'secretpower' && move.hit < 2`).
        let first_bond_hit = mv.parental_bond && mv.id == moves::SECRET_POWER && hit < 2;
        let own: Vec<(&Secondary, u32)> = ability_hooks::secondaries(b, mv, t)
            .into_iter()
            .filter(|(s, _)| !first_bond_hit || s.volatile_status == crate::dex::conditions::FLINCH)
            .collect();
        // Fling's appended secondary comes last (its PrepareHit runs after ModifyMove, so Serene
        // Grace does not double it).
        let flung = handlers::fling_secondary(b, user, mv, t);
        let flung = flung.iter().map(|s| (s, u32::from(s.chance)));
        for (secondary, chance) in own.into_iter().chain(flung) {
            if !item_events::keeps_secondary(b, t, secondary) {
                continue;
            }
            // F18: a flinch on a target with no move left this turn can never act (`flinch`
            // is read only by BeforeMove and ends at the residual; Steadfast fires only when
            // it stops a move), so the roll is skipped. The end-of-turn distribution is
            // unchanged. Not when the volatile could still be seen before the residual
            // (`flinch_observable_later`): a turn suspended for a mid-turn switch shows it
            // (FF-parity-harness), and a Dancer copy or an Instructed move would be stopped.
            if flinch_only(secondary)
                && !matches!(
                    mv.id,
                    moves::THROAT_CHOP | moves::DIRE_CLAW | moves::TRI_ATTACK
                )
                && b.will_move(t).is_none()
                && !flinch_observable_later(b, mv, t)
            {
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
            handlers::secondary_on_hit(b, user, t, mv);
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
    // 6. forceSwitch (Roar, Whirlwind, Dragon Tail, Circle Throw): a target the move did not
    // fail on, standing, with the user standing and a bench on its side, is dragged out right
    // after the action (`forceSwitchFlag`) unless `DragOut` stops it: Suction Cups and Guard Dog
    // (both breakable) return `null`, which neither drags nor fails the move.
    if data.force_switch {
        for (i, &t) in targets.iter().enumerate() {
            if !results[i].reached()
                || b.alive(t).is_none()
                || b.alive(user).is_none()
                || super::residual::bench(b, t.side).next().is_none()
            {
                continue;
            }
            // Commander's `commanding` / `commanded` `onDragOut` (priority 2, before Suction
            // Cups) returns `false`: a status move (Roar, Whirlwind) fails on that target.
            let commander = b.volatile(t, Volatile::Commanding).active
                || b.volatile(t, Volatile::Commanded).active;
            if commander && data.category == MoveCategory::Status {
                results[i] = Hit::Failed;
            }
            if commander
                || drag_out_ability(b.ability_unless_broken(t))
                || conditions::drag_out_blocked(b, t)
            {
                continue;
            }
            b.force_switch.push(t);
        }
    }
    // DamagingHit for every damaged target, then AfterHit (only while the user stands).
    let damaged: Small<(SlotRef, i32), 6> = targets
        .iter()
        .zip(&results)
        .filter_map(|(&t, r)| match r {
            Hit::Damage(d) => Some((t, *d)),
            _ => None,
        })
        .collect();
    // `pokemonOriginalHP`: the user's HP before DamagingHit and AfterHit.
    let user_hp_before = b.alive(user).map(|p| b.mon(p).hp);
    if !damaged.is_empty() {
        damaging_hit(b, user, mv, &damaged, total_before)?;
    }
    // AfterHit: Knock Off removes the item of every damaged target (`takeItem` in its
    // `onAfterHit`, which Champions runs even if the user fainted from Rocky Helmet).
    if mv.id == moves::KNOCK_OFF {
        for (i, &t) in targets.iter().enumerate() {
            if let Hit::Damage(_) = results[i] {
                b.take_item(t);
            }
        }
    }
    // AfterHit: the move's other `onAfterHit` handlers, per damaged target (Champions runs
    // them even if the user fainted).
    for (i, &t) in targets.iter().enumerate() {
        if let Hit::Damage(_) = results[i] {
            handlers::on_after_hit(b, user, t, mv)?;
        }
    }
    // Champions `spreadMoveHit`, when a target took numeric damage: `if (pokemon.hp &&
    // pokemon.hp <= pokemon.maxhp / 2 && pokemonOriginalHP > pokemon.maxhp / 2)
    // runEvent('EmergencyExit', pokemon)` (Rough Skin, Iron Barbs, Rocky Helmet took the user
    // to half). A self-switching move's user already has its `switchFlag` from
    // `runMoveEffects` (set above), which the handler respects.
    if !damaged.is_empty() && !b.move_self_switch {
        if let Some(hp_before) = user_hp_before {
            super::switching::emergency_exit_check(b, user, hp_before);
        }
    }
    Ok(results)
}

/// Showdown `move.multihit` is set: a multi-hit move of the dex, or Beat Up (its `onModifyMove`
/// sets it to the number of allies, even 1).
pub(crate) fn is_multihit(id: MoveId) -> bool {
    id.data().multihit.is_some() || id == moves::BEAT_UP
}

/// The `switchFlag` a self-switching move leaves on its user (`source.switchFlag = move.id`):
/// the move's `selfSwitch` kind decides what the switch copies (`copyVolatileFrom`).
fn self_switch_flag(kind: SelfSwitch) -> SwitchFlag {
    match kind {
        SelfSwitch::CopyVolatile => SwitchFlag::CopyVolatile,
        SelfSwitch::ShedTail => SwitchFlag::ShedTail,
        SelfSwitch::Yes | SelfSwitch::No => SwitchFlag::Move,
    }
}

/// The substitute's `onTryPrimaryHit` guard (`if (target === source || move.flags['bypasssub']
/// || move.infiltrates) return;`): whether `target`'s substitute takes this hit. Sound moves
/// carry `bypasssub` in the data.
fn substitute_takes_hit<const N: usize>(
    b: &Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    target != user
        && b.has_substitute(target)
        && !mv.data.flags.contains(MoveFlags::BYPASSSUB)
        && !handlers::infiltrates(b, user, mv, target)
}

/// The substitute's `onTryPrimaryHit` (F11): `getDamage` for the target (no damage: `null`,
/// the move stops there without failing); the substitute loses that much (capped at its HP)
/// and breaks at 0 (`removeVolatile`); the user takes the recoil of that damage
/// (`applyRecoilDamage`) and drains `Math.ceil(damage * drain)`; then `AfterSubDamage`: the
/// move's own handler, then the target's item (Air Balloon); `HIT_SUBSTITUTE`.
/// (`source.lastDamage` has no reader among the implemented moves.)
fn hit_substitute<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
    hit: u8,
) -> Result<Hit, TurnError> {
    // Disguise and Ice Face skip their shields against a hit on the substitute (`hitSub`):
    // `forme::shields_hit` inside `get_damage`.
    let damage = match get_damage(b, user, mv, target, hit, true)? {
        Planned::Damage(d) => d,
        Planned::Fail | Planned::NoDamage => return Ok(Hit::Blocked),
    };
    let sub_hp = i32::from(b.state.slot(target).substitute_hp);
    let damage = damage.min(sub_hp);
    if sub_hp - damage <= 0 {
        b.remove_volatile(target, Volatile::Substitute);
    } else {
        b.set_substitute_hp(target, (sub_hp - damage) as i16);
    }
    if damage != 0 {
        apply_recoil_damage(b, user, mv, damage);
    }
    if let Some(drain) = mv.data.drain {
        let amount = (f64::from(damage) * f64::from(drain.0) / f64::from(drain.1)).ceil();
        b.heal_rooted_from(user, amount, Some(target));
    }
    handlers::on_after_sub_damage(b, user, mv);
    item_events::after_sub_damage(b, target);
    Ok(Hit::Substitute)
}

/// Showdown `applyRecoilDamage(damage, move, pokemon)`, for the hit loop's `move.totalDamage`
/// and for the damage a substitute took (which the total leaves out): Struggle
/// `directDamage(round(baseMaxhp / 4))`, a `recoil` move `damage(max(1, round(damage *
/// recoil)))` (Rock Head, Magic Guard); then `EmergencyExit` on the user if the recoil took
/// its HP to half ([`user_emergency_exit`]).
fn apply_recoil_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    damage: i32,
) {
    let Some(pokemon) = b.alive(user) else {
        return;
    };
    let (hp_before, max_hp) = (b.mon(pokemon).hp, b.mon(pokemon).max_hp);
    if mv.data.struggle_recoil {
        let amount = (f64::from(max_hp) / 4.0).round().max(1.0) as i32;
        b.direct_damage(user, amount);
    } else if mv.data.mind_blown_recoil {
        // Steel Beam: `Math.round(pokemon.maxhp / 2)` with the move's condition as the effect
        // (not a move's damage, not `recoil`: Magic Guard stops it, Rock Head does not).
        let amount = (f64::from(max_hp) / 2.0).round();
        b.damage(user, amount, DamageSource::Indirect);
    } else if let Some(recoil) = mv.data.recoil {
        let amount = (f64::from(damage) * f64::from(recoil.0) / f64::from(recoil.1))
            .round()
            .max(1.0);
        b.damage(user, amount, DamageSource::Recoil);
    } else {
        return;
    }
    user_emergency_exit(b, user, hp_before);
}

/// `if (pokemon.hp <= pokemon.maxhp / 2 && hpBefore > pokemon.maxhp / 2)
/// runEvent('EmergencyExit', pokemon, pokemon)` for the move's user after its recoil
/// (`applyRecoilDamage`), a MoveFail crash or AfterMoveSecondarySelf (Life Orb) in
/// `useMoveInner`. Unlike the other Emergency Exit sites there is no `pokemon.hp` guard: a user
/// the recoil knocked out (`faint()` already cleared its flag) is flagged again (a crash or
/// Life Orb never knocks out from above half). It is then an active Pokémon at 0 HP with
/// `switchFlag === true` until its faint is processed, which the `getAllActive()` checks of
/// Eject Button and Eject Pack see ([`Battle::any_active_switch_flag_true`]); the fainted
/// Pokémon keeps the flag, and Showdown asks for its replacement mid-turn
/// ([`crate::state::Slot::must_switch_out`]; oracle `dd-emergency-exit-recoil-eject-pack`).
fn user_emergency_exit<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, hp_before: i16) {
    let Some(pokemon) = b.occupant(user) else {
        return;
    };
    let mon = b.mon(pokemon);
    let (hp, max_hp) = (i32::from(mon.hp), i32::from(mon.max_hp));
    if 2 * hp <= max_hp && 2 * i32::from(hp_before) > max_hp {
        super::switching::emergency_exit(b, user);
    }
}

/// `secondaries()` for a target whose substitute took the hit: `ModifySecondaries` runs without
/// a target (no Shield Dust, no Covert Cloak), every secondary is rolled, and `moveHit(null)`
/// only applies a secondary's `self` part (the user's boosts). Rolls without a `self` part
/// change nothing, so they are not drawn.
fn substitute_secondaries<const N: usize>(b: &mut Battle<'_, N>, user: SlotRef, mv: &ActiveMove) {
    if mv.has_sheer_force {
        return;
    }
    for secondary in mv.data.secondaries {
        if secondary.self_boosts == NO_BOOSTS {
            continue;
        }
        let chance = u32::from(secondary.chance) * mv.secondary_chance_factor;
        if b.rng.chance(chance, 100) {
            b.boost_by(
                user,
                &secondary.self_boosts,
                Some(user),
                BoostEffect::Move(mv.id),
            );
        }
    }
}

/// Showdown `runEvent('DamagingHit', damagedTargets, pokemon, move, damage)` (WORKPLAN F15):
/// the damaged targets' handlers sorted by `compareLeftToRightOrder` — `onDamagingHitOrder`
/// (Rough Skin / Iron Barbs 1, Rocky Helmet 2, the rest last), then target index, then a
/// Pokémon's own order (status, ability, item), then the attacker's `onSourceDamagingHit`
/// (collected once per damaged target, after that target's own handlers). A holder that
/// fainted from the hit still acts (`faintMessages` runs after the move); a handler is skipped
/// once its holder left the slot or the attacker it hits fainted earlier in the event.
/// Implemented: the `frz` thaw by a Fire move, Rough Skin, Iron Barbs, Rattled (the ability
/// half), Rocky Helmet, and the abilities of `ability_hooks::on_damaging_hit` /
/// `on_source_damaging_hit`. Other `onDamagingHit` holders are refused by `support`.
fn damaging_hit<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    damaged: &[(SlotRef, i32)],
    total_before: i32,
) -> Result<(), TurnError> {
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Kind {
        Thaw,
        Ability(AbilityId),
        Item(ItemId),
        /// The attacker's `onSourceDamagingHit` for this target.
        Source(AbilityId),
    }
    const LAST: u32 = u32::MAX;
    // Counter's and Mirror Coat's conditions (`onDamagingHit`, no order) only record the hit on
    // their holder, which no other handler reads: they can run first.
    for &(target, damage) in damaged {
        handlers::counter_damaging_hit(b, user, mv, target, damage);
    }
    let mut handlers: crate::turn::Small<(u32, usize, Kind), 8> = crate::turn::Small::new();
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
        } else if let Some(order) = ability_hooks::damaging_hit_order(ability) {
            handlers.push((order, index, Kind::Ability(ability)));
        }
        let item = b.item(target);
        if item == items::ROCKY_HELMET {
            handlers.push((2, index, Kind::Item(item)));
        } else if item == items::AIR_BALLOON || item_events::has_damaging_hit(item) {
            handlers.push((LAST, index, Kind::Item(item)));
        }
        // The attacker's own ability is never suppressed by its own move.
        let source_ability = b.ability(user);
        if ability_hooks::has_source_damaging_hit(source_ability) {
            handlers.push((LAST, index, Kind::Source(source_ability)));
        }
    }
    handlers.sort();
    let contact =
        item_events::makes_contact(b, user, mv.data) && b.item(user) != items::PROTECTIVE_PADS;
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
            Kind::Ability(a) => ability_hooks::on_damaging_hit(
                b,
                a,
                target,
                user,
                mv,
                damaged[index].1,
                contact,
                total_before,
            )?,
            Kind::Source(a) => ability_hooks::on_source_damaging_hit(b, a, target, user, mv),
            Kind::Item(i) if i == items::ROCKY_HELMET => {
                if contact && b.alive(user).is_some() {
                    let max_hp = f64::from(b.slot_mon(user).expect("alive").max_hp);
                    b.damage(user, max_hp / 6.0, DamageSource::Indirect);
                }
            }
            Kind::Item(i) if i == items::AIR_BALLOON => {
                // `target.item = ''` without `useItem`: no `lastItem`; then AfterUseItem
                // (Unburden).
                b.apply(crate::instruction::Instruction::SetItem {
                    target: pokemon,
                    old: items::AIR_BALLOON,
                    new: ItemId::NONE,
                });
                ability_events::unburden(b, target);
                ability_events::symbiosis(b, target);
            }
            // Weakness Policy, the absorbing items, Jaboca / Rowap Berry. The item may have
            // gone since the handlers were collected (a Jaboca Berry is eaten once).
            Kind::Item(i) => {
                if b.item(target) == i {
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
    Ok(())
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

/// Showdown `getDamage` + `modifyDamage`; the crit and the damage roll are decided here. `hit`
/// is `move.hit` (Triple Axel's power). `hit_substitute`: the damage is for the target's
/// substitute (the resist berries' `hitSub` check).
fn get_damage<const N: usize>(
    b: &mut Battle<'_, N>,
    user: SlotRef,
    mv: &ActiveMove,
    target: SlotRef,
    hit: u8,
    hit_substitute: bool,
) -> Result<Planned, TurnError> {
    let data = mv.data;
    if type_immune(b, mv, target) {
        return Ok(Planned::Fail);
    }
    let attacker = b.slot_mon(user).expect("checked").clone();
    let defender = b.slot_mon(target).expect("alive").clone();
    // OHKO moves deal the target's max HP (`if (move.ohko) return target.maxhp;`), then
    // `damageCallback` (Endeavor, Final Gambit), then fixed damage: no crit, no roll, no
    // modifiers.
    if data.ohko != Ohko::No {
        return Ok(Planned::Damage(i32::from(defender.max_hp)));
    }
    if let Some(damage) = handlers::damage_callback(b, user, target, mv) {
        // A 0 still "deals damage" in Showdown (DamagingHit with 0), which `Planned` cannot
        // express; the implemented callbacks never return one.
        if damage <= 0 {
            return Err(b.unsupported(format!("{}: damageCallback of {damage}", data.name)));
        }
        return Ok(Planned::Damage(damage));
    }
    match data.fixed_damage {
        Some(FixedDamage::Level) => return Ok(Planned::Damage(i32::from(attacker.level))),
        Some(FixedDamage::Hp(hp)) => return Ok(Planned::Damage(i32::from(hp))),
        None => {}
    }
    let mut base_power = mv.base_power;
    if mv.id == moves::LOW_KICK || mv.id == moves::GRASS_KNOT {
        base_power = weight_power(b.weight(target));
    }
    base_power = handlers::base_power_callback(b, user, target, mv, base_power, hit);
    if base_power == 0 {
        return Ok(Planned::NoDamage);
    }

    // Critical hit: ratio 1..4 ??1/24, 1/8, 1/2, always. `CriticalHit` handlers: Battle Armor
    // and Shell Armor (`onCriticalHit: false`, breakable). Showdown rolls first and then
    // cancels; not rolling gives the same distribution.
    // ModifyCritRatio: the user's item (Scope Lens, Razor Claw, Leek), ability and volatiles (Focus
    // Energy, Dragon Cheer; Laser Focus sets 5, and nothing lowers the ratio, so it ends at the
    // clamp), then clamped to 0..4. Lucky Chant on the target's side is a `CriticalHit` handler
    // too (`onCriticalHit: false`).
    let crit_ratio = match handlers::volatile_crit_ratio(b, user) {
        None => 4,
        Some(bonus) => (i32::from(data.crit_ratio)
            + item_events::crit_ratio_bonus(b.item(user), &attacker)
            + ability_events::crit_ratio_bonus(b, user, target)
            + bonus)
            .clamp(0, 4),
    };
    let can_crit = !b.ability_unless_broken(target).data().cannot_be_crit
        && !b.side_effect_active(target.side, SideEffect::LuckyChant)
        && !super::forme::shields_hit(b, user, target, mv.id);
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
    // The type changers' `onBasePower`, the auras' `onAnyBasePower`.
    power_mods.extend(ability_hooks::base_power_handlers(b, user, target, mv));
    if type_boost_item(b.item(user)) == Some(mv.move_type) {
        power_mods.push(Handler::of(b, user, 15, SUB_ITEM, MOD_ONE_POINT_TWO));
    }
    // Muscle Band, Wise Glasses (16), Punching Glove (23).
    power_mods.extend(item_events::base_power_handlers(b, user, data));
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
    if let Some(modifier) = handlers::on_base_power(b, user, target, mv) {
        power_mods.push(Handler::of(b, user, 0, SUB_MOVE, modifier));
    }
    // The user's volatiles (Helping Hand, priority 10) and the target's (Bounce).
    power_mods.extend(handlers::volatile_base_power(b, user));
    power_mods.extend(handlers::target_volatile_base_power(b, target, mv));
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
    let attack = ability_events::attack_direct(b.ability(user), data, attack);
    let mut attack_mods = ability_events::attack_handlers(b, user, target, data, mv.move_type);
    attack_mods.extend(item_events::attack_handlers(b, user, data));
    let attack = modify(attack, ability_events::chain(b, attack_mods));
    let mut defense = boosted_stat(
        i32::from(defender.stats[stored_stat_index(defense_stat, wonder_room)]),
        def_boost,
    );
    // ModifyDef / ModifySpD: sandstorm (Rock SpD) and snow (Ice Def), 1.5x applied directly.
    // `WeatherModifyDamage` reads `defender.effectiveWeather()` (Utility Umbrella hides sun and
    // rain; sand and snow are the same for everyone). The readers are the weathers' handlers, so
    // a Mega Sol user sees sun ([`Battle::move_weather`]): no sand / snow defense, and Mega Sol's
    // own `onWeatherModifyDamage` (priority 1, a fast exit) applies sun's modifier once.
    let weather = b.move_weather(target);
    // `this.hasType('Rock')` / `'Ice'` (an added type counts).
    if defense_stat == Stat::Spd && weather == Weather::Sand && b.has_type(target, Type::Rock) {
        defense = modify(defense, MOD_ONE_POINT_FIVE);
    }
    if defense_stat == Stat::Def && weather == Weather::Snow && b.has_type(target, Type::Ice) {
        defense = modify(defense, MOD_ONE_POINT_FIVE);
    }
    // Chained ModifyDef / ModifySpD handlers, applied after the direct weather boosts.
    let mut defense_mods = ability_events::defense_handlers(b, user, target, data, defense_stat);
    defense_mods.extend(item_events::defense_handlers(b, target, defense_stat));
    let defense = modify(defense, ability_events::chain(b, defense_mods));

    // modifyDamage inputs. Sun's `onWeatherModifyDamage` first checks `move.id === 'hydrosteam'
    // && attacker.effectiveWeather() === 'sunnyday'` (1.5x): the attacker's view, read by the
    // weather (or by Mega Sol's handler, which runs sun's), so sun on the field without the
    // attacker's Utility Umbrella, or a Mega Sol user (`Battle::move_weather`); otherwise the
    // defender's view decides as for any Water move (0.5x in its sun).
    let hydro_steam_sun = mv.id == moves::HYDRO_STEAM && b.move_weather(user) == Weather::Sun;
    let weather_modifier = match (weather, mv.move_type) {
        _ if hydro_steam_sun => MOD_ONE_POINT_FIVE,
        (Weather::Sun, Type::Fire) | (Weather::Rain, Type::Water) => MOD_ONE_POINT_FIVE,
        (Weather::Sun, Type::Water) | (Weather::Rain, Type::Fire) => MOD_HALF,
        _ => MOD_ONE,
    };
    // The `???` type (Struggle's, `Type::None` here) never gets STAB; `pokemon.hasType(type)`
    // counts an added type (Forest's Curse's Grass).
    let stab = data.force_stab || (mv.move_type != Type::None && b.has_type(user, mv.move_type));
    let stab_modifier = ability_events::modify_stab(b.ability(user), stab);
    // runEffectiveness: per defending type, the chart then the move's onEffectiveness, then
    // the target's ability (Disguise returns 0, which ends the event) and item.
    let neutral = super::forme::shields_hit(b, user, target, mv.id);
    let arrows_neutral = handlers::thousand_arrows_neutral(b, mv, target);
    // `for (const type of target.getTypes())`: the added type too.
    let type_mod: i32 = b
        .types(target)
        .iter()
        .filter(|&&t| t != Type::None)
        .map(|&t| {
            let chart = handlers::type_effectiveness(mv.move_type, t);
            let by_move = if arrows_neutral {
                0
            } else {
                handlers::on_effectiveness(mv.id, t, chart)
            };
            if neutral {
                return 0;
            }
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
    let mut final_mods = ability_events::modify_damage_handlers(
        b,
        user,
        target,
        data,
        mv.move_type,
        type_mod,
        critical,
    );
    final_mods.extend(item_events::modify_damage_handlers(
        b,
        user,
        target,
        mv.move_type,
        type_mod,
        hit_substitute,
    ));
    // Ripen's (priority -1): after a resist berry the item handlers just ate.
    final_mods.extend(ability_events::ripen_weaken(b, target));
    // The target's volatiles (`onSourceModifyDamage`: Glaive Rush).
    final_mods.extend(handlers::volatile_modify_damage(b, target, mv));
    // The screens' `onAnyModifyDamage`: `if (!target.getMoveHitData(move).crit &&
    // !move.infiltrates)`.
    let infiltrates = b.active_move.is_some_and(|m| m.infiltrates);
    if !critical && !infiltrates && target != user && screen_applies(b, target.side, data.category)
    {
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
        // `move.multihitType === 'parentalbond' && move.hit > 1` (not for a spread hit).
        parental_bond: mv.parental_bond && hit > 1,
        weather_modifier,
        critical,
        stab_modifier,
        type_effectiveness,
        // `if (this.battle.gen < 6 || move.id !== 'facade')`: Facade keeps its power burned.
        burned: ability_events::burn_halves(&attacker, b.ability(user), data)
            && mv.id != moves::FACADE,
        // `getMoveHitData(move).bypassProtect` (Unseen Fist, Piercing Drill).
        protected: mv.bypass_protect & target_bit::<N>(target) != 0,
        final_modifier,
    };
    let rolls = damage_rolls(input);
    b.hit_crit[target.side.index()][usize::from(target.slot)] = critical;
    Ok(Planned::Damage(i32::from(b.rng.roll(&rolls, user.side))))
}

/// Whether a `flinch` volatile on `target` (which has no move left this turn) could still be
/// seen before the residual removes it, so the F18 shortcut must roll it after all
/// (conservative: any possible source counts):
/// - the turn may still stop for a mid-turn switch, whose paused state shows the volatile: the
///   move in progress or a queued move switches its user out (`selfSwitch`: U-turn, Parting
///   Shot, Baton Pass, ...), a queued Revival Blessing, or an active Pokémon holds Eject Button
///   or Eject Pack or has Emergency Exit or Wimp Out (raw or effective);
/// - the target may still use a move this turn and be stopped by it: its Dancer copying a
///   dance, or a queued Instruct.
fn flinch_observable_later<const N: usize>(
    b: &Battle<'_, N>,
    mv: &ActiveMove,
    target: SlotRef,
) -> bool {
    if mv.self_switch || [b.ability(target), b.raw_ability(target)].contains(&abilities::DANCER) {
        return true;
    }
    let queued_switcher = b.queue.iter().any(|action| {
        let super::queue::ActionKind::Move { id, .. } = action.kind else {
            return false;
        };
        !id.is_none()
            && (id.data().self_switch != SelfSwitch::No
                || id == moves::REVIVAL_BLESSING
                || id == moves::INSTRUCT)
    });
    if queued_switcher {
        return true;
    }
    b.all_alive().into_iter().any(|slot| {
        let items = [b.item(slot), b.raw_item(slot)];
        let abilities = [b.ability(slot), b.raw_ability(slot)];
        items.contains(&items::EJECT_BUTTON)
            || items.contains(&items::EJECT_PACK)
            || abilities.contains(&abilities::EMERGENCY_EXIT)
            || abilities.contains(&abilities::WIMP_OUT)
    })
}

/// A secondary whose only effect is the `flinch` volatile.
fn flinch_only(secondary: &Secondary) -> bool {
    secondary.volatile_status == crate::dex::conditions::FLINCH
        && secondary.status == Status::None
        && secondary.boosts == NO_BOOSTS
        && secondary.self_boosts == NO_BOOSTS
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

/// Low Kick / Grass Knot base power from the target's weight (`getWeight()`, hectograms).
fn weight_power(weight_hg: i32) -> i32 {
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

/// Showdown `addPseudoWeather`: Gravity and Fairy Lock fail if up (no `onFieldRestart`); Trick
/// Room, Wonder Room and Magic Room end themselves on restart (`onFieldRestart`, no
/// PseudoWeatherChange); a new one (5 turns, Fairy Lock 2: Persistent, which makes the rooms last
/// 7, is refused) runs `PseudoWeatherChange`. Magic Room's `onFieldStart` runs every active
/// item's End, which `singleEvent` skips (every holder ignores its item once Magic Room is up);
/// its suppression is `items::ignoring_item`. Fairy Lock's only logs.
fn add_pseudo_weather<const N: usize>(b: &mut Battle<'_, N>, id: &str) -> bool {
    let effect = match id {
        "gravity" => FieldEffect::Gravity,
        "trickroom" => FieldEffect::TrickRoom,
        "wonderroom" => FieldEffect::WonderRoom,
        "magicroom" => FieldEffect::MagicRoom,
        "fairylock" => FieldEffect::FairyLock,
        _ => unreachable!("checked by support"),
    };
    if b.field_active(effect) {
        if matches!(
            effect,
            FieldEffect::TrickRoom | FieldEffect::WonderRoom | FieldEffect::MagicRoom
        ) {
            b.set_field(effect, Effect::NONE);
            return true;
        }
        return false;
    }
    let turns = if effect == FieldEffect::FairyLock {
        2
    } else {
        5
    };
    b.set_field(effect, Effect { value: 0, turns });
    // `runEvent('PseudoWeatherChange')`: Room Service.
    item_events::pseudo_weather_change(b);
    true
}

/// Showdown `addSideCondition` (fails if already up; none of these has `onSideRestart`):
/// Tailwind 4 turns, the screens 5 (Light Clay 8), Safeguard 5 (Persistent, which makes it 7,
/// is refused), Mist and Lucky Chant 5, Wide Guard and Quick Guard 1. Hazards:
/// `conditions::add_hazard`.
fn add_side_condition<const N: usize>(
    b: &mut Battle<'_, N>,
    source: SlotRef,
    side: SideId,
    effect: SideEffect,
) -> bool {
    if conditions::HAZARDS.contains(&effect) {
        return conditions::add_hazard(b, side, effect);
    }
    if b.side_effect_active(side, effect) {
        return false;
    }
    let turns = match effect {
        SideEffect::Tailwind => 4,
        SideEffect::WideGuard
        | SideEffect::QuickGuard
        | SideEffect::CraftyShield
        | SideEffect::MatBlock => 1,
        SideEffect::Reflect | SideEffect::LightScreen | SideEffect::AuroraVeil
            if b.item(source) == items::LIGHT_CLAY =>
        {
            8
        }
        _ => 5,
    };
    b.set_side_effect(side, effect, Effect { value: 0, turns });
    // `runEvent('SideConditionStart', side, source, condition)` (Wind Power).
    ability_events::side_condition_start(b, side, effect);
    true
}

/// The abilities whose `onDragOut` returns `null` (Suction Cups, priority 0; Guard Dog, priority
/// 1): the holder is neither dragged out nor is the phazing move failed. Both are breakable, so
/// callers pass `ability_unless_broken` (a Mold Breaker phazer) or the effective ability (Red
/// Card, whose event the attacker's own move does not suppress).
pub(crate) fn drag_out_ability(ability: AbilityId) -> bool {
    ability == abilities::SUCTION_CUPS || ability == abilities::GUARD_DOG
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

    /// Protect, Mat Block, Quick Guard and Wide Guard reset a locked move only on its first
    /// turn (`lockedmove` duration 2), by deletion: no fatigue confusion. No supported locking
    /// move is a spread or priority move, so the guards' reset has no oracle scenario.
    #[test]
    fn a_stopped_first_turn_lock_is_deleted_without_confusion() {
        use crate::volatile::VolatileState;
        for (duration, kept) in [(2, false), (1, true)] {
            let mut state = crate::state::State::<2>::default();
            for side in [SideId::One, SideId::Two] {
                for (i, p) in state.side_mut(side).party.iter_mut().enumerate() {
                    p.species = crate::dex::SpeciesId(i as u16 + 1);
                    p.max_hp = 100;
                    p.hp = 100;
                }
                for s in 0..2 {
                    state.side_mut(side).slots[s].party_index = Some(s as u8);
                }
            }
            let lock = VolatileState {
                active: true,
                duration,
                mv: moves::OUTRAGE,
                hidden: 3,
                ..VolatileState::NONE
            };
            state
                .slot_mut(P1A)
                .volatiles
                .set(Volatile::LockedMove, lock);
            let mut chooser = super::super::branch::Chooser::new();
            let mut b = Battle::new(&mut state, &mut chooser);
            handlers::reset_first_turn_lock(&mut b, P1A);
            assert_eq!(b.volatile(P1A, Volatile::LockedMove).active, kept);
            assert!(!b.volatile(P1A, Volatile::Confusion).active);
        }
    }

    #[test]
    fn weight_brackets() {
        assert_eq!(weight_power(2020), 120);
        assert_eq!(weight_power(1000), 100);
        assert_eq!(weight_power(999), 80);
        assert_eq!(weight_power(99), 20);
    }
}
