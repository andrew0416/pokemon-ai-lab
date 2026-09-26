//! What the turn engine implements, checked before a turn runs.
//!
//! A turn is only simulated if every effect that could act in it is implemented: the moves
//! chosen, the abilities, items, statuses and volatiles on the field, and the field and side
//! effects. Anything else is a [`super::TurnError::Unsupported`] naming it. Showdown lists
//! each entry's behaviour as callback names (`handlers` in the dex); the tables here pin the
//! handler lists that are implemented, and a test fails if the dex lists change.

use crate::dex::{
    abilities, items, moves, AbilityId, ItemId, MoveCategory, MoveId, MoveTarget, SelfSwitch, Type,
    NO_BOOSTS,
};
use crate::field::{FieldEffect, SideEffect, Weather, FIELD_EFFECT_COUNT, SIDE_EFFECT_COUNT};
use crate::state::{SideId, SlotRef, State};
use crate::volatile::Volatile;

use super::battle::weather_from;
use super::order::fractional_priority_tenths;

/// Moves with Showdown callbacks that are implemented, with the exact callback list.
pub(crate) const MOVES_WITH_HANDLERS: &[(MoveId, &[&str])] = &[
    (
        moves::PROTECT,
        &[
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (moves::DETECT, &["onHit", "onPrepareHit"]),
    // The other shields: the stall `onPrepareHit` / `onHit` like Protect's, `condition.onTryHit`
    // in `handlers::protect_try_hit`; `condition.onHit` only acts on Z- and Max Moves (off),
    // `condition.onStart` only logs. Broken by Feint (`handlers::break_protect`).
    (
        moves::SPIKY_SHIELD,
        &[
            "condition.onHit",
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (
        moves::BANEFUL_BUNKER,
        &[
            "condition.onHit",
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (
        moves::KINGS_SHIELD,
        &[
            "condition.onHit",
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (
        moves::OBSTRUCT,
        &[
            "condition.onHit",
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (
        moves::SILK_TRAP,
        &[
            "condition.onHit",
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (
        moves::BURNING_BULWARK,
        &[
            "condition.onHit",
            "condition.onStart",
            "condition.onTryHit",
            "onHit",
            "onPrepareHit",
        ],
    ),
    // Queue readers (`queue.rs`).
    (moves::SUCKER_PUNCH, &["onTry"]),
    (moves::THUNDERCLAP, &["onTry"]),
    (moves::UPPER_HAND, &["onTry"]),
    (moves::QUASH, &["onHit"]),
    (moves::AFTER_YOU, &["onHit"]),
    // Encore: the volatile's start/override/disable/residual/end (`battle.rs`, `lock.rs`,
    // `residual.rs`, `mod.rs::disabled`).
    (
        moves::ENCORE,
        &[
            "condition.onDisableMove",
            "condition.onEnd",
            "condition.onOverrideAction",
            "condition.onResidual",
            "condition.onStart",
        ],
    ),
    // Protect's stall `onPrepareHit` / `onHit`; `condition.onDamage` in `Battle::damage`,
    // `condition.onStart` only logs.
    (
        moves::ENDURE,
        &[
            "condition.onDamage",
            "condition.onStart",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (
        moves::FOLLOW_ME,
        &[
            "condition.onFoeRedirectTarget",
            "condition.onStart",
            "onTry",
        ],
    ),
    (
        moves::RAGE_POWDER,
        &[
            "condition.onFoeRedirectTarget",
            "condition.onStart",
            "onTry",
        ],
    ),
    (
        moves::SPOTLIGHT,
        &[
            "condition.onFoeRedirectTarget",
            "condition.onStart",
            "onTryHit",
        ],
    ),
    // Helping Hand: `onTryHit` in `handlers::on_try_hit`, the volatile's start/restart in
    // `Battle::add_volatile_from` (`counter` = applications), its BasePower handler in
    // `handlers::volatile_base_power`.
    (
        moves::HELPING_HAND,
        &[
            "condition.onBasePower",
            "condition.onRestart",
            "condition.onStart",
            "onTryHit",
        ],
    ),
    // Taunt: `condition.onStart` in `conditions::volatile_start`, `onBeforeMove` and
    // `onDisableMove` in `conditions`, `onEnd` only logs.
    (
        moves::TAUNT,
        &[
            "condition.onBeforeMove",
            "condition.onDisableMove",
            "condition.onEnd",
            "condition.onStart",
        ],
    ),
    // Disable: `onTryHit` in `handlers::on_try_hit`, `condition.onStart` in
    // `conditions::volatile_start`, `onBeforeMove` (Champions) and `onDisableMove` in
    // `conditions`, `onEnd` only logs.
    (
        moves::DISABLE,
        &[
            "condition.onBeforeMove",
            "condition.onDisableMove",
            "condition.onEnd",
            "condition.onStart",
            "onTryHit",
        ],
    ),
    // Torment: `onDisableMove` in `conditions::disabled_move`; `onStart` only fails for a
    // Dynamaxed target (refused), `onEnd` only logs.
    (
        moves::TORMENT,
        &[
            "condition.onDisableMove",
            "condition.onEnd",
            "condition.onStart",
        ],
    ),
    // Imprison: `onFoeDisableMove` and `onFoeBeforeMove` in `conditions`; `onStart` only logs.
    (
        moves::IMPRISON,
        &[
            "condition.onFoeBeforeMove",
            "condition.onFoeDisableMove",
            "condition.onStart",
        ],
    ),
    // Entry hazards: `onSideStart`/`onSideRestart` in `conditions::add_hazard`, `onSwitchIn`
    // in `conditions::entry_hazards` (run by `switching::run_switch_in`).
    (
        moves::STEALTH_ROCK,
        &["condition.onSideStart", "condition.onSwitchIn"],
    ),
    (
        moves::SPIKES,
        &[
            "condition.onSideRestart",
            "condition.onSideStart",
            "condition.onSwitchIn",
        ],
    ),
    (
        moves::TOXIC_SPIKES,
        &[
            "condition.onSideRestart",
            "condition.onSideStart",
            "condition.onSwitchIn",
        ],
    ),
    (
        moves::STICKY_WEB,
        &["condition.onSideStart", "condition.onSwitchIn"],
    ),
    // Hazard removal: Defog `onHit` (no evasion drop behind a substitute), Rapid Spin
    // `onAfterHit` and `onAfterSubDamage` (`handlers::on_after_sub_damage`), Court Change
    // `onHitField`.
    (moves::DEFOG, &["onHit"]),
    (moves::RAPID_SPIN, &["onAfterHit", "onAfterSubDamage"]),
    (moves::COURT_CHANGE, &["onHitField"]),
    // Struggle: `onModifyMove` (type `???`) in `handlers::on_modify_move`; `struggleRecoil` in
    // `moves::hit_loop`; chosen only without a usable move (`STRUGGLE_INDEX`).
    (moves::STRUGGLE, &["onModifyMove"]),
    // Glaive Rush: the `self` volatile's `onBeforeMove` (priority 100) in `moves::before_move`,
    // `onAccuracy` and `onSourceModifyDamage` in `handlers`; `onStart` only logs.
    (
        moves::GLAIVE_RUSH,
        &[
            "condition.onAccuracy",
            "condition.onBeforeMove",
            "condition.onSourceModifyDamage",
            "condition.onStart",
        ],
    ),
    // Sparkling Aria: its secondary adds the `sparklingaria` volatile, its `onAfterMove`
    // (`handlers::on_after_move`) removes it and cures burns.
    (moves::SPARKLING_ARIA, &["onAfterMove"]),
    // Sleep Talk: `onTry` and `onHit` (a random callable move through `moves::call_move`);
    // Snore: `onTry`. Both are `sleepUsable` (`moves::before_move`).
    (moves::SLEEP_TALK, &["onHit", "onTry"]),
    (moves::SNORE, &["onTry"]),
    // Instruct: `onHit` in `handlers` (a new move action with order 3).
    (moves::INSTRUCT, &["onHit"]),
    // Ally Switch: `onPrepareHit` in `handlers::on_prepare_hit` (the `allyswitch` volatile, its
    // `onStart` / `onRestart` in `Battle::add_volatile_from`), `onHit` in `handlers::on_hit`
    // (`handlers::swap_positions`).
    (
        moves::ALLY_SWITCH,
        &[
            "condition.onRestart",
            "condition.onStart",
            "onHit",
            "onPrepareHit",
        ],
    ),
    (moves::GRASSY_GLIDE, &["onModifyPriority"]),
    (moves::LOW_KICK, &["basePowerCallback", "onTryHit"]),
    (moves::GRASS_KNOT, &["basePowerCallback", "onTryHit"]),
    // Heavy Slam, Heat Crash: `handlers::base_power_callback` (weight ratio); `onTryHit` only
    // fails against a Dynamaxed target (off).
    (moves::HEAVY_SLAM, &["basePowerCallback", "onTryHit"]),
    (moves::HEAT_CRASH, &["basePowerCallback", "onTryHit"]),
    (moves::FAKE_OUT, &["onDisableMove", "onTry"]),
    (moves::KNOCK_OFF, &["onAfterHit", "onBasePower"]),
    (moves::GRAV_APPLE, &["onBasePower"]),
    // `handlers::on_base_power` (the user's status, the target's HP or poison); Facade also
    // skips the burn halving (`moves::get_damage`).
    (moves::FACADE, &["onBasePower"]),
    (moves::BRINE, &["onBasePower"]),
    (moves::VENOSHOCK, &["onBasePower"]),
    (moves::EXPANDING_FORCE, &["onBasePower", "onModifyMove"]),
    (moves::WEATHER_BALL, &["onModifyMove", "onModifyType"]),
    (moves::TERRAIN_PULSE, &["onModifyMove", "onModifyType"]),
    // `onAfterSubDamage` in `handlers::on_after_sub_damage`.
    (moves::ICE_SPINNER, &["onAfterHit", "onAfterSubDamage"]),
    (moves::STEEL_ROLLER, &["onAfterSubDamage", "onHit", "onTry"]),
    // `onTryMove` fails an ally-targeted use under Heal Block (`handlers::fail_try_move`).
    (moves::POLLEN_PUFF, &["onHit", "onTryHit", "onTryMove"]),
    // Heal Block (also Psychic Noise's secondary): the volatile's `durationCallback` and
    // `onStart` in `conditions::volatile_start`, `onRestart` in `Battle::add_volatile_from`,
    // `onBeforeMove` / `onModifyMove` / `onDisableMove` in `conditions::heal_blocked`,
    // `onTryHeal` in `Battle::heal` (and the healing berries' `onTryEatItem`,
    // `update::eat_item`); `onEnd` only logs.
    (
        moves::HEAL_BLOCK,
        &[
            "condition.durationCallback",
            "condition.onBeforeMove",
            "condition.onDisableMove",
            "condition.onEnd",
            "condition.onModifyMove",
            "condition.onRestart",
            "condition.onStart",
            "condition.onTryHeal",
        ],
    ),
    (moves::TRICK, &["onHit", "onTryImmunity"]),
    (moves::SWITCHEROO, &["onHit", "onTryImmunity"]),
    // `condition.onStart` only fails for a Terastallized user; `onType` is applied as a type
    // change (`conditions::roost_start`, undone when the volatile ends).
    (moves::ROOST, &["condition.onStart", "condition.onType"]),
    // `condition.onStart` only logs; `onEnd` in `conditions::volatile_end`.
    (
        moves::YAWN,
        &["condition.onEnd", "condition.onStart", "onTryHit"],
    ),
    // `condition.onResidual` only announces the count; `onEnd` faints the holder.
    (
        moves::PERISH_SONG,
        &["condition.onEnd", "condition.onResidual", "onHitField"],
    ),
    (moves::RISING_VOLTAGE, &["basePowerCallback"]),
    // `handlers::base_power_callback` (Triple Axel / Triple Kick read `move.hit`; Return and
    // Frustration assume Showdown's default happiness, 255, which the state does not hold).
    (moves::HEX, &["basePowerCallback"]),
    (moves::INFERNAL_PARADE, &["basePowerCallback"]),
    (moves::TRIPLE_AXEL, &["basePowerCallback"]),
    (moves::TRIPLE_KICK, &["basePowerCallback"]),
    (moves::WATER_SHURIKEN, &["basePowerCallback"]),
    (moves::ELECTRO_BALL, &["basePowerCallback"]),
    (moves::GYRO_BALL, &["basePowerCallback"]),
    (moves::ERUPTION, &["basePowerCallback"]),
    (moves::WATER_SPOUT, &["basePowerCallback"]),
    (moves::DRAGON_ENERGY, &["basePowerCallback"]),
    (moves::FLAIL, &["basePowerCallback"]),
    (moves::REVERSAL, &["basePowerCallback"]),
    (moves::CRUSH_GRIP, &["basePowerCallback"]),
    (moves::WRING_OUT, &["basePowerCallback"]),
    (moves::HARD_PRESS, &["basePowerCallback"]),
    (moves::STORED_POWER, &["basePowerCallback"]),
    (moves::POWER_TRIP, &["basePowerCallback"]),
    (moves::PUNISHMENT, &["basePowerCallback"]),
    (moves::TRUMP_CARD, &["basePowerCallback"]),
    (moves::RETURN, &["basePowerCallback"]),
    (moves::FRUSTRATION, &["basePowerCallback"]),
    (moves::BOLT_BEAK, &["basePowerCallback"]),
    (moves::FISHIOUS_REND, &["basePowerCallback"]),
    (moves::PSYBLADE, &["onBasePower"]),
    // Photon Geyser, Shell Side Arm: `onModifyMove` may make them physical
    // (`handlers::on_modify_move` → `ActiveMove::set_category`, whose data copy every chain
    // reads; Shell Side Arm then makes contact). Shell Side Arm's `onPrepareHit`, `onHit` and
    // `onAfterSubDamage` only reveal the category.
    (moves::PHOTON_GEYSER, &["onModifyMove"]),
    (
        moves::SHELL_SIDE_ARM,
        &["onAfterSubDamage", "onHit", "onModifyMove", "onPrepareHit"],
    ),
    (moves::BLIZZARD, &["onModifyMove"]),
    // Still rejected for its confusion secondary; shares Thunder's handler.
    (moves::HURRICANE, &["onModifyMove"]),
    (moves::THUNDER, &["onModifyMove"]),
    (moves::FREEZE_DRY, &["onEffectiveness"]),
    (moves::FLYING_PRESS, &["onEffectiveness"]),
    // Smack Down / Thousand Arrows: the `smackdown` volatile (`onStart` in
    // `conditions::volatile_start`, `onRestart` in `Battle::add_volatile_from`, grounding in
    // `Battle::is_grounded`); Thousand Arrows' `onEffectiveness` in
    // `handlers::thousand_arrows_neutral` (its Ground immunity is ignored by data).
    (
        moves::SMACK_DOWN,
        &["condition.onRestart", "condition.onStart"],
    ),
    (moves::THOUSAND_ARROWS, &["onEffectiveness"]),
    (moves::POLTERGEIST, &["onTry", "onTryHit"]),
    (moves::ACROBATICS, &["basePowerCallback"]),
    // Damage history (F13): `basePowerCallback`s reading `Slot.history` / `Side.history`
    // (`handlers::base_power_callback`).
    (moves::ASSURANCE, &["basePowerCallback"]),
    (moves::PAYBACK, &["basePowerCallback"]),
    (moves::AVALANCHE, &["basePowerCallback"]),
    // Revenge (Champions `Past`, not refused: the engine keeps no legality list).
    (moves::REVENGE, &["basePowerCallback"]),
    (moves::STOMPING_TANTRUM, &["basePowerCallback"]),
    (moves::TEMPER_FLARE, &["basePowerCallback"]),
    (moves::RAGE_FIST, &["basePowerCallback"]),
    (moves::LAST_RESPECTS, &["basePowerCallback"]),
    // Metal Burst, Comeuppance (`target: scripted`): `onTry` (a foe damaged the user this
    // turn), `onModifyTarget` (that foe's slot), `damageCallback` (1.5x its damage).
    (
        moves::METAL_BURST,
        &["damageCallback", "onModifyTarget", "onTry"],
    ),
    (
        moves::COMEUPPANCE,
        &["damageCallback", "onModifyTarget", "onTry"],
    ),
    // Counter, Mirror Coat (`target: scripted`): `beforeTurnCallback` is a `beforeTurnMove` queue
    // action (order 5, `moves::before_turn_move`) adding the condition (`onStart`: nothing
    // recorded yet); its `onDamagingHit` in `moves::damaging_hit`
    // (`handlers::counter_damaging_hit`), its `onRedirectTarget` last in
    // `moves::redirect_target`; `onTry` and `damageCallback` (2x the recorded damage, or 1).
    (
        moves::COUNTER,
        &[
            "beforeTurnCallback",
            "condition.onDamagingHit",
            "condition.onRedirectTarget",
            "condition.onStart",
            "damageCallback",
            "onTry",
        ],
    ),
    (
        moves::MIRROR_COAT,
        &[
            "beforeTurnCallback",
            "condition.onDamagingHit",
            "condition.onRedirectTarget",
            "condition.onStart",
            "damageCallback",
            "onTry",
        ],
    ),
    // Focus Punch, Beak Blast, Shell Trap: `priorityChargeCallback` is a `priorityChargeMove`
    // queue action (order 107, `moves::priority_charge_move`) adding the condition (`onStart`
    // only logs); its `onHit` in `handlers::volatile_on_hit` (`runEvent('Hit')` in
    // `moves::spread_move_hit`). Focus Punch: `beforeMoveCallback` in `moves::run_move_inner`,
    // `onTryAddVolatile` (flinch) in `Battle::add_volatile_blocked`. Beak Blast: `onAfterMove`
    // in `handlers::on_after_move`. Shell Trap: `onTryMove` in `handlers::null_try_move`.
    (
        moves::FOCUS_PUNCH,
        &[
            "beforeMoveCallback",
            "condition.onHit",
            "condition.onStart",
            "condition.onTryAddVolatile",
            "priorityChargeCallback",
        ],
    ),
    (
        moves::BEAK_BLAST,
        &[
            "condition.onHit",
            "condition.onStart",
            "onAfterMove",
            "priorityChargeCallback",
        ],
    ),
    (
        moves::SHELL_TRAP,
        &[
            "condition.onHit",
            "condition.onStart",
            "onTryMove",
            "priorityChargeCallback",
        ],
    ),
    // Parting Shot: `onHit` drops Atk and SpA and withdraws the switch if that failed (F6).
    (moves::PARTING_SHOT, &["onHit"]),
    // `onTry` in `handlers::on_try`: Belch (`ateBerry`, `SideHistory::ate_berry`; Champions has
    // no `onDisableMove`), Last Resort (`moveSlot.used`, `SlotHistory::moves_used`), Dark Void
    // (Darkrai or a bounced copy).
    (moves::BELCH, &["onTry"]),
    (moves::LAST_RESORT, &["onTry"]),
    (moves::DARK_VOID, &["onTry"]),
    // Slot conditions (F12): `conditions::{add_slot_condition, slot_condition_residual,
    // slot_condition_switch_in, remove_slot_condition}`; Revival Blessing's revival is a
    // mid-turn decision (`resume_turn`).
    (
        moves::WISH,
        &[
            "condition.onEnd",
            "condition.onResidual",
            "condition.onStart",
        ],
    ),
    (
        moves::HEALING_WISH,
        &["condition.onSwap", "condition.onSwitchIn", "onTryHit"],
    ),
    (moves::REVIVAL_BLESSING, &["onTryHit"]),
    // Future Sight, Doom Desire: `onTry` adds the `futuremove` slot condition
    // (`conditions::start_future_move` from `moves::try_spread_move_hit`); the condition's
    // `onResidual` (order 3) and `onEnd` are `conditions::slot_condition_residual` →
    // `moves::future_move_hit`.
    (moves::FUTURE_SIGHT, &["onTry"]),
    (moves::DOOM_DESIRE, &["onTry"]),
    // Two-turn moves (F9): `onTryMove` in `handlers::charge_try_move`; the semi-invulnerable
    // ones' condition handlers in `handlers::invulnerable`, `volatile_modify_damage`,
    // `target_volatile_base_power` and the sandstorm residual (`onImmunity`).
    (moves::SOLAR_BEAM, &["onBasePower", "onTryMove"]),
    (moves::SOLAR_BLADE, &["onBasePower", "onTryMove"]),
    (moves::METEOR_BEAM, &["onTryMove"]),
    (moves::ELECTRO_SHOT, &["onTryMove"]),
    (moves::SKY_ATTACK, &["onTryMove"]),
    (moves::PHANTOM_FORCE, &["onTryMove"]),
    (moves::SHADOW_FORCE, &["onTryMove"]),
    (
        moves::FLY,
        &[
            "condition.onInvulnerability",
            "condition.onSourceModifyDamage",
            "onTryMove",
        ],
    ),
    (
        moves::BOUNCE,
        &[
            "condition.onInvulnerability",
            "condition.onSourceBasePower",
            "onTryMove",
        ],
    ),
    (
        moves::DIG,
        &[
            "condition.onImmunity",
            "condition.onInvulnerability",
            "condition.onSourceModifyDamage",
            "onTryMove",
        ],
    ),
    (
        moves::DIVE,
        &[
            "condition.onImmunity",
            "condition.onInvulnerability",
            "condition.onSourceModifyDamage",
            "onTryMove",
        ],
    ),
    (moves::FIRST_IMPRESSION, &["onDisableMove", "onTry"]),
    (moves::DIRE_CLAW, &["secondaries.onHit", "secondary.onHit"]),
    // Substitute (F11): `onTryHit` and `onHit` in `handlers`, the volatile's `onStart` in
    // `conditions::volatile_start` (HP in `Slot::substitute_hp`), `onTryPrimaryHit` in
    // `moves::hit_substitute` (routing in `moves::spread_move_hit`), `onEnd` only logs.
    (
        moves::SUBSTITUTE,
        &[
            "condition.onEnd",
            "condition.onStart",
            "condition.onTryPrimaryHit",
            "onHit",
            "onTryHit",
        ],
    ),
    // Double Shock: `onTryMove` (`handlers::null_try_move`: no Electric type, `null`) and
    // `self.onHit` (`handlers::self_on_hit`: Electric becomes `???`, `Type::Unknown`).
    (moves::DOUBLE_SHOCK, &["onTryMove", "self.onHit"]),
    // Burn Up: the same with Fire (`handlers::null_try_move`, `handlers::self_on_hit`); its
    // `defrost` flag does not thaw a frozen user without the Fire type (`handlers::thaws_user`,
    // the `frz` status's `onBeforeMove`).
    (moves::BURN_UP, &["onTryMove", "self.onHit"]),
    // Belly Drum `onHit`; Clangorous Soul and Fillet Away: `onTry` (HP), `onTryHit` (the boosts,
    // then deleted: `handlers::boosts_applied_in_try_hit`), `onHit` (the HP cost); No Retreat:
    // `onTry`, the volatile's `onTrapPokemon` in `conditions::trapped` (`onStart` only logs).
    (moves::BELLY_DRUM, &["onHit"]),
    (moves::CLANGOROUS_SOUL, &["onHit", "onTry", "onTryHit"]),
    (moves::FILLET_AWAY, &["onHit", "onTry", "onTryHit"]),
    (
        moves::NO_RETREAT,
        &["condition.onStart", "condition.onTrapPokemon", "onTry"],
    ),
    // Status cures and heals (`handlers::on_hit`): Heal Bell / Aromatherapy (`allyTeam`, through
    // `moves::try_move_hit_field`, whose TryHitSide runs Sap Sipper's `onAllyTryHitSide`),
    // Refresh, Purify, Take Heart, Jungle Healing / Lunar Blessing (`allies`), Floral Healing;
    // Rest's `onTry` and `onHit`.
    (moves::HEAL_BELL, &["onHit"]),
    (moves::AROMATHERAPY, &["onHit"]),
    (moves::REFRESH, &["onHit"]),
    (moves::PURIFY, &["onHit"]),
    (moves::TAKE_HEART, &["onHit"]),
    (moves::JUNGLE_HEALING, &["onHit"]),
    (moves::LUNAR_BLESSING, &["onHit"]),
    (moves::FLORAL_HEALING, &["onHit"]),
    // Salt Cure: the secondary's `saltcure` volatile, its `onResidual` (order 13) in
    // `residual.rs`; `onStart` / `onEnd` only log.
    (
        moves::SALT_CURE,
        &[
            "condition.onEnd",
            "condition.onResidual",
            "condition.onStart",
        ],
    ),
    // Magnet Rise: `onTry` in `handlers::on_try`, `onImmunity` (Ground) in
    // `Battle::is_grounded`; 5 turns (residual order 18); `onStart` / `onEnd` only log.
    (
        moves::MAGNET_RISE,
        &[
            "condition.onEnd",
            "condition.onImmunity",
            "condition.onStart",
            "onTry",
        ],
    ),
    // Ingrain: `onResidual` (order 7) in `residual.rs`, `onTrapPokemon` in
    // `conditions::trapped`, `onDragOut` in `conditions::drag_out_blocked`, grounding in
    // `Battle::is_grounded`; `onStart` only logs.
    (
        moves::INGRAIN,
        &[
            "condition.onDragOut",
            "condition.onResidual",
            "condition.onStart",
            "condition.onTrapPokemon",
        ],
    ),
    // Mean Look, Block, Spider Web: `onHit` adds `trapped` linked to the user's `trapper`
    // (`conditions::add_trap`, `remove_linked_volatiles`; the trap in `conditions::trapped`).
    (moves::MEAN_LOOK, &["onHit"]),
    (moves::BLOCK, &["onHit"]),
    (moves::SPIDER_WEB, &["onHit"]),
    // Heal Pulse: `handlers::on_hit` (half the target's max HP, 3/4 from a Mega Launcher user).
    (moves::HEAL_PULSE, &["onHit"]),
    (moves::REST, &["onHit", "onTry"]),
    // `handlers::on_hit`: Psych Up, Speed Swap (the stored Speed, recalculated on leaving the
    // field in `Battle::clear_volatile`), Strength Sap, Pain Split, Spite, Reflect Type, Soak
    // (`handlers::set_types`); Endeavor's `damageCallback` and `onTryImmunity`.
    (moves::PSYCH_UP, &["onHit"]),
    (moves::SPEED_SWAP, &["onHit"]),
    (moves::STRENGTH_SAP, &["onHit"]),
    (moves::PAIN_SPLIT, &["onHit"]),
    (moves::SPITE, &["onHit"]),
    (moves::REFLECT_TYPE, &["onHit"]),
    // Ability changes (`handlers::skill_swap`, `handlers::set_ability`: the old ability's End,
    // the new one's Start through `switching`; Ability Shield blocks): Skill Swap `onHit`; Role
    // Play, Entrainment, Simple Beam `onTryHit` / `onHit`; Worry Seed `onTryImmunity` too.
    (moves::SKILL_SWAP, &["onHit"]),
    // Opus U. Gastro Acid: `onTryHit` in `handlers::on_try_hit` (a `cantsuppress` ability fails,
    // an Ability Shield `null`s it); its condition's `onStart` in `conditions::volatile_start`
    // (Ability Shield) and `abilities::gastro_acid_start` (the ability's `End`); the suppression
    // is `abilities::ignoring_ability`. `condition.onCopy` only acts through Baton Pass, which
    // is not supported.
    (
        moves::GASTRO_ACID,
        &["condition.onCopy", "condition.onStart", "onTryHit"],
    ),
    (moves::ROLE_PLAY, &["onHit", "onTryHit"]),
    (moves::ENTRAINMENT, &["onHit", "onTryHit"]),
    (moves::SIMPLE_BEAM, &["onHit", "onTryHit"]),
    (moves::WORRY_SEED, &["onHit", "onTryHit", "onTryImmunity"]),
    (moves::SOAK, &["onHit"]),
    (moves::ENDEAVOR, &["damageCallback", "onTryImmunity"]),
    // Leech Seed: `onTryImmunity` (Grass) in `handlers`, the volatile's `onResidual` in
    // `conditions::leech_seed_residual` (`onStart` only logs). Partial trapping (Bind, Wrap,
    // ...) is the `partiallytrapped` condition (`conditions::volatile_start`,
    // `partially_trapped_residual`, `trapped`), pinned in `volatile.rs`.
    (
        moves::LEECH_SEED,
        &["condition.onResidual", "condition.onStart", "onTryImmunity"],
    ),
    // Screen breakers (`handlers::on_try_hit`; Raging Bull's type in `on_modify_type`), hazard
    // setters and Mortal Spin (`on_after_hit`, `on_after_sub_damage`), crash moves (`on_move_fail`), Misty Explosion (`on_base_power`; self-destruct in
    // `moves::use_move`), Final Gambit (`damage_callback`; `ifHit` in `spread_move_hit`).
    (moves::PSYCHIC_FANGS, &["onTryHit"]),
    (moves::BRICK_BREAK, &["onTryHit"]),
    (moves::RAGING_BULL, &["onModifyType", "onTryHit"]),
    (moves::CEASELESS_EDGE, &["onAfterHit", "onAfterSubDamage"]),
    (moves::STONE_AXE, &["onAfterHit", "onAfterSubDamage"]),
    (moves::MORTAL_SPIN, &["onAfterHit", "onAfterSubDamage"]),
    (moves::HIGH_JUMP_KICK, &["onMoveFail"]),
    (moves::JUMP_KICK, &["onMoveFail"]),
    (moves::AXE_KICK, &["onMoveFail"]),
    (moves::SUPERCELL_SLAM, &["onMoveFail"]),
    (moves::MISTY_EXPLOSION, &["onBasePower"]),
    (moves::FINAL_GAMBIT, &["damageCallback"]),
    // Half the target's HP (`handlers::damage_callback`).
    (moves::SUPER_FANG, &["damageCallback"]),
    (moves::NATURES_MADNESS, &["damageCallback"]),
    (moves::RUINATION, &["damageCallback"]),
    // Destiny Bond: `onPrepareHit` in `moves::try_spread_move_hit`, the volatile's
    // `onBeforeMove` / `onMoveAborted` in `conditions::destiny_bond_before_move`, its `onFaint`
    // in `Battle::faint_messages` (`conditions::destiny_bond_faint`); `onStart` only logs.
    (
        moves::DESTINY_BOND,
        &[
            "condition.onBeforeMove",
            "condition.onFaint",
            "condition.onMoveAborted",
            "condition.onStart",
            "onPrepareHit",
        ],
    ),
    // Item moves (`handlers::on_hit`): Bug Bite / Pluck (the berry's `onEat` on the user through
    // `update::berry_on_eat`), Incinerate, Corrosive Gas, Recycle.
    (moves::BUG_BITE, &["onHit"]),
    (moves::PLUCK, &["onHit"]),
    (moves::INCINERATE, &["onHit"]),
    (moves::CORROSIVE_GAS, &["onHit"]),
    (moves::RECYCLE, &["onHit"]),
    // Throat Chop: the secondary's `onHit` adds the `throatchop` volatile
    // (`handlers::secondary_on_hit`); its `onBeforeMove`, `onModifyMove` (a called sound move,
    // `moves::use_move`) and `onDisableMove` in `conditions::throat_chopped`; `onStart` and
    // `onEnd` only log.
    (
        moves::THROAT_CHOP,
        &[
            "condition.onBeforeMove",
            "condition.onDisableMove",
            "condition.onEnd",
            "condition.onModifyMove",
            "condition.onStart",
            "secondaries.onHit",
            "secondary.onHit",
        ],
    ),
    (moves::TRI_ATTACK, &["secondaries.onHit", "secondary.onHit"]),
    // Stat-change history (`SlotHistory::stats_raised_this_turn` / `stats_lowered_this_turn`,
    // set in `Battle::boost_by`): Burning Jealousy's and Alluring Voice's secondary `onHit` in
    // `handlers::secondary_on_hit`, Lash Out's `onBasePower` in `handlers::on_base_power`.
    (
        moves::BURNING_JEALOUSY,
        &["secondaries.onHit", "secondary.onHit"],
    ),
    (
        moves::ALLURING_VOICE,
        &["secondaries.onHit", "secondary.onHit"],
    ),
    (moves::LASH_OUT, &["onBasePower"]),
    (moves::MORNING_SUN, &["onHit"]),
    (moves::MOONLIGHT, &["onHit"]),
    (moves::SYNTHESIS, &["onHit"]),
    (moves::SHORE_UP, &["onHit"]),
    (moves::HAZE, &["onHitField"]),
    (moves::CLEAR_SMOG, &["onHit"]),
    (moves::TOPSY_TURVY, &["onHit"]),
    (moves::POWER_SWAP, &["onHit"]),
    (moves::GUARD_SWAP, &["onHit"]),
    (moves::HEART_SWAP, &["onHit"]),
    (
        moves::GRAVITY,
        &[
            "condition.durationCallback",
            "condition.onBeforeMove",
            "condition.onDisableMove",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onModifyAccuracy",
            "condition.onModifyMove",
        ],
    ),
    (
        moves::TRICK_ROOM,
        &[
            "condition.durationCallback",
            "condition.onFieldEnd",
            "condition.onFieldRestart",
            "condition.onFieldStart",
        ],
    ),
    // Wonder Room: the stored Def/SpD swap and `onModifyMove` in `moves::get_damage` (and the
    // confusion self-hit), `onFieldRestart` ends it (`add_pseudo_weather`); `durationCallback`
    // only differs with Persistent (refused); `onFieldStart` / `onFieldEnd` only log.
    (
        moves::WONDER_ROOM,
        &[
            "condition.durationCallback",
            "condition.onFieldEnd",
            "condition.onFieldRestart",
            "condition.onFieldStart",
            "condition.onModifyMove",
        ],
    ),
    // Magic Room (F17): `items::ignoring_item`; End messages only at its start.
    (
        moves::MAGIC_ROOM,
        &[
            "condition.durationCallback",
            "condition.onFieldEnd",
            "condition.onFieldRestart",
            "condition.onFieldStart",
        ],
    ),
    (
        moves::TAILWIND,
        &[
            "condition.durationCallback",
            "condition.onModifySpe",
            "condition.onSideEnd",
            "condition.onSideStart",
        ],
    ),
    (
        moves::REFLECT,
        &[
            "condition.durationCallback",
            "condition.onAnyModifyDamage",
            "condition.onSideEnd",
            "condition.onSideStart",
        ],
    ),
    (
        moves::LIGHT_SCREEN,
        &[
            "condition.durationCallback",
            "condition.onAnyModifyDamage",
            "condition.onSideEnd",
            "condition.onSideStart",
        ],
    ),
    (
        moves::AURORA_VEIL,
        &[
            "condition.durationCallback",
            "condition.onAnyModifyDamage",
            "condition.onSideEnd",
            "condition.onSideStart",
            "onTry",
        ],
    ),
    // Wide Guard / Quick Guard: `onTry` (a later action), `onHitSide` (the stall counter) and
    // the side's `onTryHit` in `moves`; `onSideStart` only logs.
    (
        moves::WIDE_GUARD,
        &[
            "condition.onSideStart",
            "condition.onTryHit",
            "onHitSide",
            "onTry",
        ],
    ),
    (
        moves::QUICK_GUARD,
        &[
            "condition.onSideStart",
            "condition.onTryHit",
            "onHitSide",
            "onTry",
        ],
    ),
    // Crafty Shield, Mat Block: `onTry` in `moves::try_move_hit_field` (a later action; Mat Block
    // only on its user's first turn out), the side's `onTryHit` (priority 3, after the protect
    // family, before Magic Bounce) in `handlers::side_guard_try_hit`; `onSideStart` only logs.
    // Mat Block's `stallingMove` has no mechanical effect (it adds no `stall`).
    (
        moves::CRAFTY_SHIELD,
        &["condition.onSideStart", "condition.onTryHit", "onTry"],
    ),
    (
        moves::MAT_BLOCK,
        &["condition.onSideStart", "condition.onTryHit", "onTry"],
    ),
    // Safeguard: `onSetStatus` / `onTryAddVolatile` in `Battle` (Persistent, the only
    // `durationCallback` change, is refused); Mist: `onTryBoost` in `Battle::boost_by`; Lucky
    // Chant: `onCriticalHit: false` in `moves::get_damage`. The side start/end only log.
    (
        moves::SAFEGUARD,
        &[
            "condition.durationCallback",
            "condition.onSetStatus",
            "condition.onSideEnd",
            "condition.onSideStart",
            "condition.onTryAddVolatile",
        ],
    ),
    (
        moves::MIST,
        &[
            "condition.onSideEnd",
            "condition.onSideStart",
            "condition.onTryBoost",
        ],
    ),
    (
        moves::LUCKY_CHANT,
        &["condition.onSideEnd", "condition.onSideStart"],
    ),
    (
        moves::ELECTRIC_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onSetStatus",
            "condition.onTryAddVolatile",
        ],
    ),
    (
        moves::GRASSY_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onResidual",
        ],
    ),
    (
        moves::MISTY_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onSetStatus",
            "condition.onTryAddVolatile",
        ],
    ),
    (
        moves::PSYCHIC_TERRAIN,
        &[
            "condition.durationCallback",
            "condition.onBasePower",
            "condition.onFieldEnd",
            "condition.onFieldStart",
            "condition.onTryHit",
        ],
    ),
];

/// Items that raise one type's moves by 4915/4096 (`onBasePower`, priority 15) and do
/// nothing else.
pub(crate) const TYPE_BOOST_ITEMS: &[(ItemId, Type)] = &[
    (items::BLACK_BELT, Type::Fighting),
    (items::BLACK_GLASSES, Type::Dark),
    (items::CHARCOAL, Type::Fire),
    (items::DRAGON_FANG, Type::Dragon),
    (items::FAIRY_FEATHER, Type::Fairy),
    (items::HARD_STONE, Type::Rock),
    (items::MAGNET, Type::Electric),
    (items::METAL_COAT, Type::Steel),
    (items::MIRACLE_SEED, Type::Grass),
    (items::MYSTIC_WATER, Type::Water),
    (items::NEVER_MELT_ICE, Type::Ice),
    (items::ODD_INCENSE, Type::Psychic),
    (items::POISON_BARB, Type::Poison),
    (items::ROCK_INCENSE, Type::Rock),
    (items::ROSE_INCENSE, Type::Grass),
    (items::SEA_INCENSE, Type::Water),
    (items::SHARP_BEAK, Type::Flying),
    (items::SILK_SCARF, Type::Normal),
    (items::SILVER_POWDER, Type::Bug),
    (items::SOFT_SAND, Type::Ground),
    (items::SPELL_TAG, Type::Ghost),
    (items::TWISTED_SPOON, Type::Psychic),
    (items::WAVE_INCENSE, Type::Water),
];

/// Items with callbacks that are implemented while the holder is on the field.
pub(crate) const ITEMS_WITH_HANDLERS: &[(ItemId, &[&str])] = &[
    (items::LEFTOVERS, &["onResidual"]),
    (
        items::LIFE_ORB,
        &["onAfterMoveSecondarySelf", "onModifyDamage"],
    ),
    (items::FOCUS_SASH, &["onDamage"]),
    (items::ROCKY_HELMET, &["onDamagingHit"]),
    // Eject Button (Champions), Red Card: `items::after_move_secondary` (F6).
    (items::EJECT_BUTTON, &["onAfterMoveSecondary"]),
    (items::RED_CARD, &["onAfterMoveSecondary"]),
    // Power Herb: `onChargeMove` in `handlers::charge_try_move` (F9).
    (items::POWER_HERB, &["onChargeMove"]),
    // `moves::decide_hits` (and no accuracy re-rolls for multi-accuracy moves).
    (items::LOADED_DICE, &["onModifyMove"]),
    // Berries eaten on `Update` (`update.rs`).
    (items::SITRUS_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::ORAN_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::FIGY_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::WIKI_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::MAGO_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::AGUAV_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::IAPAPA_BERRY, &["onEat", "onTryEatItem", "onUpdate"]),
    (items::LIECHI_BERRY, &["onEat", "onUpdate"]),
    (items::GANLON_BERRY, &["onEat", "onUpdate"]),
    (items::SALAC_BERRY, &["onEat", "onUpdate"]),
    (items::PETAYA_BERRY, &["onEat", "onUpdate"]),
    (items::APICOT_BERRY, &["onEat", "onUpdate"]),
    (items::LUM_BERRY, &["onAfterSetStatus", "onEat", "onUpdate"]),
    // Gen 2's Lum Berry without `onAfterSetStatus` (`update.rs`).
    (items::MIRACLE_BERRY, &["onEat", "onUpdate"]),
    (items::CHERI_BERRY, &["onEat", "onUpdate"]),
    (items::CHESTO_BERRY, &["onEat", "onUpdate"]),
    (items::PECHA_BERRY, &["onEat", "onUpdate"]),
    (items::RAWST_BERRY, &["onEat", "onUpdate"]),
    (items::ASPEAR_BERRY, &["onEat", "onUpdate"]),
    (items::PERSIM_BERRY, &["onEat", "onUpdate"]),
    (items::LEPPA_BERRY, &["onEat", "onUpdate"]),
    (items::EXPERT_BELT, &["onModifyDamage"]),
    // `onDisableMove` in `mod.rs` `disabled` (via `items::disabled_move`).
    (items::ASSAULT_VEST, &["onDisableMove", "onModifySpD"]),
    (items::EVIOLITE, &["onModifyDef", "onModifySpD"]),
    // Accuracy, critical hits, flinch (`moves.rs`), Focus Band (`Battle::damage`).
    (items::WIDE_LENS, &["onSourceModifyAccuracy"]),
    (items::ZOOM_LENS, &["onSourceModifyAccuracy"]),
    (items::BRIGHT_POWDER, &["onModifyAccuracy"]),
    (items::LAX_INCENSE, &["onModifyAccuracy"]),
    // Opus Q unit 6. Clear Amulet: `onTryBoost` in `Battle::boost_by`. Ability Shield:
    // `onSetAbility` blocks Mummy / Lingering Aroma (`moves::ability_hooks`; Trace holding it
    // stays refused in `switching::trace`), its Mold Breaker protection is in
    // `abilities::ability_for_move` / `Battle::suppressing_ability`. Big Root: `onTryHeal` in
    // `Battle::heal_rooted` (drain, Leech Seed, Strength Sap). The stat items in
    // `items::attack_handlers` / `defense_handlers`, the power items in
    // `items::base_power_handlers`; Punching Glove's `onModifyMove` (no contact) is
    // `items::makes_contact`. Mental Herb: `onUpdate` in `update::update_event`
    // (`items::mental_herb`); `fling.effect` needs Fling, which is not supported.
    (items::CLEAR_AMULET, &["onTryBoost"]),
    (items::ABILITY_SHIELD, &["onSetAbility"]),
    (items::BIG_ROOT, &["onTryHeal"]),
    (items::MUSCLE_BAND, &["onBasePower"]),
    (items::WISE_GLASSES, &["onBasePower"]),
    (items::PUNCHING_GLOVE, &["onBasePower", "onModifyMove"]),
    (items::LIGHT_BALL, &["onModifyAtk", "onModifySpA"]),
    (items::THICK_CLUB, &["onModifyAtk"]),
    (items::DEEP_SEA_TOOTH, &["onModifySpA"]),
    (items::DEEP_SEA_SCALE, &["onModifySpD"]),
    (items::METAL_POWDER, &["onModifyDef"]),
    (items::MENTAL_HERB, &["fling.effect", "onUpdate"]),
    (items::SCOPE_LENS, &["onModifyCritRatio"]),
    (items::RAZOR_CLAW, &["onModifyCritRatio"]),
    (items::FOCUS_BAND, &["onDamage"]),
    (items::KINGS_ROCK, &["onModifyMove"]),
    (items::RAZOR_FANG, &["onModifyMove"]),
    // Residual (`residual.rs` → `items::on_residual`), after the move (`use_move`), on hit.
    (items::BLACK_SLUDGE, &["onResidual"]),
    (items::TOXIC_ORB, &["onResidual"]),
    (items::FLAME_ORB, &["onResidual"]),
    (items::STICKY_BARB, &["onHit", "onResidual"]),
    (items::SHELL_BELL, &["onAfterMoveSecondarySelf"]),
    (items::THROAT_SPRAY, &["onAfterMoveSecondarySelf"]),
    // DamagingHit (`moves::damaging_hit` → `items::on_damaging_hit`): boost + `useItem`.
    (items::WEAKNESS_POLICY, &["onDamagingHit"]),
    (items::ABSORB_BULB, &["onDamagingHit"]),
    (items::CELL_BATTERY, &["onDamagingHit"]),
    (items::LUMINOUS_MOSS, &["onDamagingHit"]),
    (items::SNOWBALL, &["onDamagingHit"]),
    // AfterMoveSecondary at the end of the hit loop (`items::after_move_secondary`), `onEat`
    // in `update::eat_item`.
    (items::KEE_BERRY, &["onAfterMoveSecondary", "onEat"]),
    (items::MARANGA_BERRY, &["onAfterMoveSecondary", "onEat"]),
    // Pinch berries on `Update` (`update.rs`): Lansat adds `focusenergy` (crit ratio +2 in
    // `moves::get_damage`), Starf raises a random stat by 2.
    (items::LANSAT_BERRY, &["onEat", "onUpdate"]),
    (items::STARF_BERRY, &["onEat", "onUpdate"]),
    // `onResidual` eats it (`items::on_residual`); its condition's `onSourceAccuracy` is the
    // `Accuracy` event in `moves::accuracy_check` (the `MicleBerry` volatile).
    (
        items::MICLE_BERRY,
        &["condition.onSourceAccuracy", "onEat", "onResidual"],
    ),
    // Eaten when the actions are queued (`items::custap`, first stage in `mod.rs`).
    (items::CUSTAP_BERRY, &["onEat", "onFractionalPriority"]),
    // `onHit` in `items::on_hit`; `onTryEatItem` asks TryHeal, which nothing supported blocks.
    (items::ENIGMA_BERRY, &["onEat", "onHit", "onTryEatItem"]),
    // `items::on_damaging_hit`.
    (items::JABOCA_BERRY, &["onDamagingHit", "onEat"]),
    (items::ROWAP_BERRY, &["onDamagingHit", "onEat"]),
    // Seeds (`field_events`): `onStart` at switch-in (priority -1, `switching::run_switch_in`)
    // and `onTerrainChange` (every terrain start and end).
    (items::ELECTRIC_SEED, &["onStart", "onTerrainChange"]),
    (items::GRASSY_SEED, &["onStart", "onTerrainChange"]),
    (items::MISTY_SEED, &["onStart", "onTerrainChange"]),
    (items::PSYCHIC_SEED, &["onStart", "onTerrainChange"]),
    // Stage items (`items.rs`): White Herb and Mirror Herb at every switch-in batch
    // (`onAnySwitchIn`), Mega Evolution, move end (`onAnyAfterMove`) and residual (order 29);
    // White Herb's `onStart` only runs from those; `fling.effect` needs Fling, which is not
    // supported; Terastallization (`onAnyAfterTerastallization`) is off. Mirror Herb's copied
    // raises (`onFoeAfterBoost`) are refused past a stage end (`items::stage_end_check`); its
    // `onEnd` forgets them with the item.
    (
        items::WHITE_HERB,
        &[
            "fling.effect",
            "onAnyAfterMega",
            "onAnyAfterMove",
            "onAnySwitchIn",
            "onResidual",
            "onStart",
            "onUse",
        ],
    ),
    (
        items::MIRROR_HERB,
        &[
            "onAnyAfterMega",
            "onAnyAfterMove",
            "onAnyAfterTerastallization",
            "onAnySwitchIn",
            "onEnd",
            "onFoeAfterBoost",
            "onResidual",
            "onUse",
        ],
    ),
    // AfterBoost (`Battle::boost_by` → `items::after_boost`).
    (items::ADRENALINE_ORB, &["onAfterBoost"]),
    // `Battle::weather_for` at every per-Pokémon weather read. The callbacks only run
    // WeatherChange on the holder (when the item starts being ignored, stops being ignored, or
    // ends in sun or rain), whose only implemented handler, Protosynthesis's, then changes
    // nothing: Utility Umbrella does not hide the sun from it, so it already has its condition;
    // `onStart` returns at once for a holder that does not ignore its item.
    (items::UTILITY_UMBRELLA, &["onEnd", "onStart", "onUpdate"]),
    // `onStart` at switch-in (priority -1) and PseudoWeatherChange (`moves::add_pseudo_weather`).
    (
        items::ROOM_SERVICE,
        &["onAnyPseudoWeatherChange", "onStart"],
    ),
    // Grounding (`Battle::is_grounded`), Speed, effectiveness; Air Balloon's `onStart` only
    // announces it; it pops on a damaging hit (`moves::damaging_hit`) and on a move its
    // holder's substitute takes (`items::after_sub_damage`).
    (
        items::AIR_BALLOON,
        &["onAfterSubDamage", "onDamagingHit", "onStart"],
    ),
    (items::IRON_BALL, &["onEffectiveness", "onModifySpe"]),
    // Speed halving (Macho Brace, the Power items) and Quick Powder: `items::speed_modifier`.
    (items::MACHO_BRACE, &["onModifySpe"]),
    (items::POWER_ANKLET, &["onModifySpe"]),
    (items::POWER_BAND, &["onModifySpe"]),
    (items::POWER_BELT, &["onModifySpe"]),
    (items::POWER_BRACER, &["onModifySpe"]),
    (items::POWER_LENS, &["onModifySpe"]),
    (items::POWER_WEIGHT, &["onModifySpe"]),
    (items::QUICK_POWDER, &["onModifySpe"]),
    // `onModifyWeight` in `Battle::weight`.
    (items::FLOAT_STONE, &["onModifyWeight"]),
    // Drawn when the actions are queued (first stage, `mod.rs`); Lagging Tail and Full Incense
    // have only a constant `onFractionalPriority` (`items::constant_fractional_tenths`).
    (items::QUICK_CLAW, &["onFractionalPriority"]),
    // `onImmunity` in `Battle::status_immune`, `onTryHit` in the move's TryHit step.
    (items::SAFETY_GOGGLES, &["onImmunity", "onTryHit"]),
    // `onModifySecondaries` in the secondaries loop (`items::keeps_secondary`).
    (items::COVERT_CLOAK, &["onModifySecondaries"]),
    // Choice items (`items.rs`): the stat in `order.rs`/`moves.rs`, `onModifyMove` adds the
    // `choicelock` volatile, `onStart` only removes a lock a newcomer cannot have.
    (
        items::CHOICE_BAND,
        &["onModifyAtk", "onModifyMove", "onStart"],
    ),
    (
        items::CHOICE_SCARF,
        &["onModifyMove", "onModifySpe", "onStart"],
    ),
    (
        items::CHOICE_SPECS,
        &["onModifyMove", "onModifySpA", "onStart"],
    ),
    // `onTrapPokemon` (priority -10) in `abilities::trapped`; `onMaybeTrapPokemon` only clears
    // a display flag.
    (items::SHED_SHELL, &["onMaybeTrapPokemon", "onTrapPokemon"]),
    // O98: `onStart` (switch-in, priority -2) and `onUpdate` in `abilities::booster_energy`,
    // `onTakeItem` in `Battle::item_can_be_taken` (`abilities::booster_energy_kept`).
    (
        items::BOOSTER_ENERGY,
        &["onStart", "onTakeItem", "onUpdate"],
    ),
];

/// Abilities with callbacks that are implemented while the holder is on the field.
/// Abilities whose only callback is `onStart` act only on switch-in (see `switching`).
pub(crate) const ABILITIES_WITH_HANDLERS: &[(AbilityId, &[&str])] = &[
    // `onModifyWeight` in `Battle::weight` (Opus R unit 8).
    (abilities::HEAVY_METAL, &["onModifyWeight"]),
    (abilities::LIGHT_METAL, &["onModifyWeight"]),
    (abilities::SAND_RUSH, &["onImmunity", "onModifySpe"]),
    (abilities::CHLOROPHYLL, &["onModifySpe"]),
    (abilities::SWIFT_SWIM, &["onModifySpe"]),
    (abilities::SLUSH_RUSH, &["onModifySpe"]),
    (abilities::PRANKSTER, &["onModifyPriority"]),
    (
        abilities::LIGHTNING_ROD,
        &["onAnyRedirectTarget", "onTryHit"],
    ),
    (abilities::STORM_DRAIN, &["onAnyRedirectTarget", "onTryHit"]),
    // Boost events (`Battle::boost_by`, F16) and the damage formula's `ModifyBoost`.
    (abilities::CONTRARY, &["onChangeBoost"]),
    (abilities::SIMPLE, &["onChangeBoost"]),
    (abilities::UNAWARE, &["onAnyModifyBoost"]),
    (abilities::CLEAR_BODY, &["onTryBoost"]),
    (abilities::WHITE_SMOKE, &["onTryBoost"]),
    (abilities::FULL_METAL_BODY, &["onTryBoost"]),
    (abilities::HYPER_CUTTER, &["onTryBoost"]),
    (abilities::BIG_PECKS, &["onTryBoost"]),
    (abilities::MIRROR_ARMOR, &["onTryBoost"]),
    // `onDragOut` only answers force-switch moves, which are all refused.
    (abilities::GUARD_DOG, &["onDragOut", "onTryBoost"]),
    (abilities::COMPETITIVE, &["onAfterEachBoost"]),
    (abilities::DEFIANT, &["onAfterEachBoost"]),
    // DamagingHit (`moves::damaging_hit`, F15).
    (abilities::ROUGH_SKIN, &["onDamagingHit"]),
    // Emergency Exit / Wimp Out: `switching::emergency_exit` at the hit loop's, `runSwitch`'s
    // and the residual phase's Update sites (F6), and on the move's user after its recoil,
    // after DamagingHit / AfterHit (Champions `spreadMoveHit`), after MoveFail and after
    // AfterMoveSecondarySelf (`moves::user_emergency_exit`; a user the recoil knocked out is
    // refused). Suction Cups: `DragOut`.
    (abilities::EMERGENCY_EXIT, &["onEmergencyExit"]),
    (abilities::WIMP_OUT, &["onEmergencyExit"]),
    (abilities::SUCTION_CUPS, &["onDragOut"]),
    // Both handlers only set the flag the pinch berries read (`update.rs`).
    (abilities::GLUTTONY, &["onDamage", "onStart"]),
    // `moves::decide_hits`.
    (abilities::SKILL_LINK, &["onModifyMove"]),
    (abilities::IRON_BARBS, &["onDamagingHit"]),
    (abilities::RATTLED, &["onAfterBoost", "onDamagingHit"]),
    (abilities::GALE_WINGS, &["onModifyPriority"]),
    (abilities::TRIAGE, &["onModifyPriority"]),
    // `moves::prepare_hit_ability`.
    (abilities::PROTEAN, &["onPrepareHit"]),
    (abilities::LIBERO, &["onPrepareHit"]),
    // F19 formes (`forme.rs`). Disguise: `onDamage` in `Battle::damage`, `onCriticalHit` and
    // `onEffectiveness` in `moves::get_damage`, `onUpdate` in `update::update_event`.
    (
        abilities::DISGUISE,
        &["onCriticalHit", "onDamage", "onEffectiveness", "onUpdate"],
    ),
    // Ice Face: as Disguise for physical moves; `onStart` in `switching::start_ability`,
    // `onWeatherChange` in `field_events::weather_changed` (`forme::ice_face_restore`).
    (
        abilities::ICE_FACE,
        &[
            "onCriticalHit",
            "onDamage",
            "onEffectiveness",
            "onStart",
            "onUpdate",
            "onWeatherChange",
        ],
    ),
    // Stance Change: `onModifyMove` in `moves::ability_hooks::on_modify_move`.
    (abilities::STANCE_CHANGE, &["onModifyMove"]),
    // Zero to Hero: `onSwitchOut` in `forme::on_switch_out` (from `switching::switch_in`),
    // `onSwitchIn` only announces.
    (abilities::ZERO_TO_HERO, &["onSwitchIn", "onSwitchOut"]),
    // Schooling, Shields Down, Hunger Switch: `onResidual` in `residual.rs` (`forme::residual`),
    // `onStart` in `switching::start_ability` (`forme::on_start`); Shields Down's `onSetStatus`
    // and `onTryAddVolatile` in `Battle::set_status_blocked` / `add_volatile_blocked`.
    (abilities::SCHOOLING, &["onResidual", "onStart"]),
    (
        abilities::SHIELDS_DOWN,
        &["onResidual", "onSetStatus", "onStart", "onTryAddVolatile"],
    ),
    (abilities::HUNGER_SWITCH, &["onResidual"]),
    // Zen Mode: `onResidual` and its condition's `onStart` / `onEnd` in `forme::zen_mode`
    // (`Volatile::ZenMode`); its `onEnd` on leaving the field gives what `clearVolatile` does
    // (`forme::revert_on_leave`), and no supported ability change can end it on the field.
    (
        abilities::ZEN_MODE,
        &[
            "condition.onEnd",
            "condition.onStart",
            "onEnd",
            "onResidual",
        ],
    ),
    // Mimicry: `onStart` in `switching::start_ability`, `onTerrainChange` in
    // `field_events::terrain_changed` (`forme::mimicry`).
    (abilities::MIMICRY, &["onStart", "onTerrainChange"]),
    // Battle Bond: both handlers do nothing unless the holder is Greninja-Bond or Greninja-Ash,
    // which `forme::field_problem` refuses.
    (
        abilities::BATTLE_BOND,
        &["onModifyMove", "onSourceAfterFaint"],
    ),
    // Damage handlers (`Battle::damage`).
    (abilities::ROCK_HEAD, &["onDamage"]),
    (abilities::MAGIC_GUARD, &["onDamage"]),
    // `onDamage` in `Battle::damage`; `onTryHit` (OHKO immunity) in `moves`.
    (abilities::STURDY, &["onDamage", "onTryHit"]),
    // Residual handlers (`residual`).
    (abilities::SPEED_BOOST, &["onResidual"]),
    (abilities::SHED_SKIN, &["onResidual"]),
    (abilities::HYDRATION, &["onResidual"]),
    // Weather abilities: `onWeather` in `residual::weather_event`; Solar Power's `onModifySpA`
    // and Dry Skin's `onSourceBasePower`/`onTryHit` in `moves`; Ice Body's `onImmunity` is for
    // hail, which is not supported.
    (abilities::RAIN_DISH, &["onWeather"]),
    (abilities::ICE_BODY, &["onImmunity", "onWeather"]),
    (abilities::SOLAR_POWER, &["onModifySpA", "onWeather"]),
    // Damage path (`abilities.rs`).
    (abilities::TECHNICIAN, &["onBasePower"]),
    (abilities::SHARPNESS, &["onBasePower"]),
    (abilities::IRON_FIST, &["onBasePower"]),
    (abilities::STRONG_JAW, &["onBasePower"]),
    (abilities::MEGA_LAUNCHER, &["onBasePower"]),
    (abilities::RECKLESS, &["onBasePower"]),
    (abilities::TOUGH_CLAWS, &["onBasePower"]),
    (
        abilities::PUNK_ROCK,
        &["onBasePower", "onSourceModifyDamage"],
    ),
    (abilities::STEELY_SPIRIT, &["onAllyBasePower"]),
    (abilities::ADAPTABILITY, &["onModifySTAB"]),
    (abilities::BLAZE, &["onModifyAtk", "onModifySpA"]),
    (abilities::TORRENT, &["onModifyAtk", "onModifySpA"]),
    (abilities::OVERGROW, &["onModifyAtk", "onModifySpA"]),
    (abilities::SWARM, &["onModifyAtk", "onModifySpA"]),
    (
        abilities::HUSTLE,
        &["onModifyAtk", "onSourceModifyAccuracy"],
    ),
    (abilities::GUTS, &["onModifyAtk"]),
    (abilities::MARVEL_SCALE, &["onModifyDef"]),
    // `abilities::attack_handlers` (the user's own modifiers) and `defense_handlers`.
    (abilities::HUGE_POWER, &["onModifyAtk"]),
    (abilities::PURE_POWER, &["onModifyAtk"]),
    (abilities::STEELWORKER, &["onModifyAtk", "onModifySpA"]),
    (abilities::TRANSISTOR, &["onModifyAtk", "onModifySpA"]),
    (abilities::DRAGONS_MAW, &["onModifyAtk", "onModifySpA"]),
    (abilities::ROCKY_PAYLOAD, &["onModifyAtk", "onModifySpA"]),
    (abilities::FIRE_MANE, &["onModifyAtk", "onModifySpA"]),
    (abilities::DEFEATIST, &["onModifyAtk", "onModifySpA"]),
    (abilities::STAKEOUT, &["onModifyAtk", "onModifySpA"]),
    (abilities::PLUS, &["onModifySpA"]),
    (abilities::MINUS, &["onModifySpA"]),
    (abilities::FUR_COAT, &["onModifyDef"]),
    (abilities::GRASS_PELT, &["onModifyDef"]),
    // `abilities::base_power_handlers`, `modify_damage_handlers`, `crit_ratio_bonus`; Sand
    // Force's `onImmunity` in `Battle::status_immune`.
    (abilities::ANALYTIC, &["onBasePower"]),
    (abilities::TOXIC_BOOST, &["onBasePower"]),
    (abilities::FLARE_BOOST, &["onBasePower"]),
    (abilities::SAND_FORCE, &["onBasePower", "onImmunity"]),
    (abilities::BATTERY, &["onAllyBasePower"]),
    (abilities::POWER_SPOT, &["onAllyBasePower"]),
    (abilities::SNIPER, &["onModifyDamage"]),
    (abilities::TINTED_LENS, &["onModifyDamage"]),
    (abilities::NEUROFORCE, &["onModifyDamage"]),
    (abilities::SUPER_LUCK, &["onModifyCritRatio"]),
    (abilities::MERCILESS, &["onModifyCritRatio"]),
    // Ruin abilities: `abilities::ruin_handler` in the Modify events of `get_damage`; `onStart`
    // only announces them (`switching`).
    (abilities::TABLETS_OF_RUIN, &["onAnyModifyAtk", "onStart"]),
    (abilities::SWORD_OF_RUIN, &["onAnyModifyDef", "onStart"]),
    (abilities::VESSEL_OF_RUIN, &["onAnyModifySpA", "onStart"]),
    (abilities::BEADS_OF_RUIN, &["onAnyModifySpD", "onStart"]),
    // `onSourceAfterFaint` in `abilities::after_faint` (from `Battle::faint_messages`);
    // Eelevate's grounding in `Battle::is_grounded`. As One: Unnerve's `onStart` / `onEnd` /
    // `onFoeTryEatItem` too (`abilities::try_eat_item`, `switching`).
    // ModifyAccuracy (`abilities::accuracy_handlers`, `accuracy_direct`). Sand Veil's
    // `onImmunity` in `Battle::status_immune`; Snow Cloak's is for hail, which is not a
    // supported weather.
    (abilities::SAND_VEIL, &["onImmunity", "onModifyAccuracy"]),
    (abilities::SNOW_CLOAK, &["onImmunity", "onModifyAccuracy"]),
    (abilities::TANGLED_FEET, &["onModifyAccuracy"]),
    (abilities::WONDER_SKIN, &["onModifyAccuracy"]),
    (abilities::VICTORY_STAR, &["onAnyModifyAccuracy"]),
    (abilities::COMPOUND_EYES, &["onSourceModifyAccuracy"]),
    // Damp: `onAnyTryMove` in `moves::ability_hooks::on_try_move`, `onAnyDamage` (Aftermath
    // only) in `ability_hooks::on_damaging_hit`.
    (abilities::DAMP, &["onAnyDamage", "onAnyTryMove"]),
    // Champions Mega abilities: Spicy Spray (`ability_hooks::on_damaging_hit`), Healer (Champions
    // 1/2 `onResidual` in `residual.rs`).
    (abilities::SPICY_SPRAY, &["onDamagingHit"]),
    (abilities::HEALER, &["onResidual"]),
    (abilities::MOXIE, &["onSourceAfterFaint"]),
    (abilities::CHILLING_NEIGH, &["onSourceAfterFaint"]),
    (abilities::GRIM_NEIGH, &["onSourceAfterFaint"]),
    (abilities::BEAST_BOOST, &["onSourceAfterFaint"]),
    (abilities::EELEVATE, &["onSourceAfterFaint"]),
    (
        abilities::AS_ONE_GLASTRIER,
        &["onEnd", "onFoeTryEatItem", "onSourceAfterFaint", "onStart"],
    ),
    (
        abilities::AS_ONE_SPECTRIER,
        &["onEnd", "onFoeTryEatItem", "onSourceAfterFaint", "onStart"],
    ),
    (
        abilities::THICK_FAT,
        &["onSourceModifyAtk", "onSourceModifySpA"],
    ),
    // `onDamage`: burn damage halved in `residual.rs`.
    (
        abilities::HEATPROOF,
        &["onDamage", "onSourceModifyAtk", "onSourceModifySpA"],
    ),
    // `onSetStatus` in `Battle::try_set_status`; `onUpdate` (cure a burn) in
    // `abilities::on_update`.
    (
        abilities::WATER_BUBBLE,
        &[
            "onModifyAtk",
            "onModifySpA",
            "onSetStatus",
            "onSourceModifyAtk",
            "onSourceModifySpA",
            "onUpdate",
        ],
    ),
    // `onTryAddVolatile` only reacts to Yawn (`Battle::add_volatile_blocked`).
    (
        abilities::PURIFYING_SALT,
        &[
            "onSetStatus",
            "onSourceModifyAtk",
            "onSourceModifySpA",
            "onTryAddVolatile",
        ],
    ),
    // `onTryHit` in the move's TryHit step, `onWeather` in the weather residual.
    (
        abilities::DRY_SKIN,
        &["onSourceBasePower", "onTryHit", "onWeather"],
    ),
    (abilities::SOLID_ROCK, &["onSourceModifyDamage"]),
    (abilities::FILTER, &["onSourceModifyDamage"]),
    (abilities::PRISM_ARMOR, &["onSourceModifyDamage"]),
    (abilities::MULTISCALE, &["onSourceModifyDamage"]),
    (abilities::SHADOW_SHIELD, &["onSourceModifyDamage"]),
    (abilities::FLUFFY, &["onSourceModifyDamage"]),
    (abilities::ICE_SCALES, &["onSourceModifyDamage"]),
    (abilities::AURA_GUARD, &["onSourceModifyDamage"]),
    (abilities::FRIEND_GUARD, &["onAnyModifyDamage"]),
    // `order.rs` (Speed, and paralysis's Quick Feet exception).
    (abilities::QUICK_FEET, &["onModifySpe"]),
    // Extra PP in `moves::deduct_pressure_pp`; `onStart` only announces the ability.
    (abilities::PRESSURE, &["onDeductPP", "onStart"]),
    // Status immunities (`Battle::set_status_blocked`, `status_immune`,
    // `add_volatile_blocked`); the `onUpdate` cures in `abilities::on_update`.
    (abilities::WATER_VEIL, &["onSetStatus", "onUpdate"]),
    (abilities::IMMUNITY, &["onSetStatus", "onUpdate"]),
    (
        abilities::INSOMNIA,
        &["onSetStatus", "onTryAddVolatile", "onUpdate"],
    ),
    (
        abilities::VITAL_SPIRIT,
        &["onSetStatus", "onTryAddVolatile", "onUpdate"],
    ),
    (abilities::LIMBER, &["onSetStatus", "onUpdate"]),
    (abilities::MAGMA_ARMOR, &["onImmunity", "onUpdate"]),
    // `onStart` only announces the ability.
    (abilities::COMATOSE, &["onSetStatus", "onStart"]),
    (abilities::LEAF_GUARD, &["onSetStatus", "onTryAddVolatile"]),
    (
        abilities::SWEET_VEIL,
        &["onAllySetStatus", "onAllyTryAddVolatile"],
    ),
    (abilities::AROMA_VEIL, &["onAllyTryAddVolatile"]),
    // Absorbing and immunity abilities: `onTryHit` in `moves::ability_hooks::on_try_hit`.
    (abilities::VOLT_ABSORB, &["onTryHit"]),
    (abilities::WATER_ABSORB, &["onTryHit"]),
    (abilities::EARTH_EATER, &["onTryHit"]),
    (abilities::MOTOR_DRIVE, &["onTryHit"]),
    // `onAllyTryHitSide` (an ally's Grass move aimed at its own side: Aromatherapy) in
    // `moves::try_move_hit_field`.
    (abilities::SAP_SIPPER, &["onAllyTryHitSide", "onTryHit"]),
    (abilities::WELL_BAKED_BODY, &["onTryHit"]),
    // The volatile's `onModifyAtk`/`onModifySpA` in `abilities::attack_handlers`; `onEnd`
    // (drop the volatile) in `switching::end_ability`; `onStart`/`condition.onEnd` only log.
    (
        abilities::FLASH_FIRE,
        &[
            "condition.onEnd",
            "condition.onModifyAtk",
            "condition.onModifySpA",
            "condition.onStart",
            "onEnd",
            "onTryHit",
        ],
    ),
    (abilities::BULLETPROOF, &["onTryHit"]),
    // `onAllyTryHitSide` only logs.
    (abilities::SOUNDPROOF, &["onAllyTryHitSide", "onTryHit"]),
    // `onImmunity` (sandstorm, powder) in `Battle::status_immune`.
    (abilities::OVERCOAT, &["onImmunity", "onTryHit"]),
    (abilities::TELEPATHY, &["onTryHit"]),
    (abilities::WONDER_GUARD, &["onTryHit"]),
    (abilities::GOOD_AS_GOLD, &["onTryHit"]),
    // `onFoeTryMove` in `moves::ability_hooks::on_try_move`.
    (abilities::DAZZLING, &["onFoeTryMove"]),
    (abilities::QUEENLY_MAJESTY, &["onFoeTryMove"]),
    (abilities::ARMOR_TAIL, &["onFoeTryMove"]),
    // `suppressWeather` in `Battle::effective_weather`; the handlers in `switching`.
    (abilities::AIR_LOCK, &["onEnd", "onStart", "onSwitchIn"]),
    (abilities::CLOUD_NINE, &["onEnd", "onStart", "onSwitchIn"]),
    // `onTryAddVolatile` (flinch) in `Battle::add_volatile_blocked`; `onTryBoost` (Intimidate
    // only) in the Intimidate start effect (`switching`).
    (abilities::INNER_FOCUS, &["onTryAddVolatile", "onTryBoost"]),
    // DamagingHit abilities (`moves::ability_hooks::on_damaging_hit`, O55). Wind Power's
    // `onSideConditionStart` in `abilities::side_condition_start`; Thermal Exchange's
    // `onSetStatus` in `Battle::set_status_blocked`, its `onUpdate` cure in
    // `abilities::on_update`.
    (abilities::STATIC, &["onDamagingHit"]),
    (abilities::FLAME_BODY, &["onDamagingHit"]),
    (abilities::POISON_POINT, &["onDamagingHit"]),
    (abilities::EFFECT_SPORE, &["onDamagingHit"]),
    (abilities::STAMINA, &["onDamagingHit"]),
    (abilities::WEAK_ARMOR, &["onDamagingHit"]),
    (abilities::COTTON_DOWN, &["onDamagingHit"]),
    (abilities::GOOEY, &["onDamagingHit"]),
    (abilities::TANGLING_HAIR, &["onDamagingHit"]),
    (abilities::SAND_SPIT, &["onDamagingHit"]),
    (abilities::SEED_SOWER, &["onDamagingHit"]),
    (abilities::ELECTROMORPHOSIS, &["onDamagingHit"]),
    (abilities::STEAM_ENGINE, &["onDamagingHit"]),
    (abilities::JUSTIFIED, &["onDamagingHit"]),
    (abilities::WATER_COMPACTION, &["onDamagingHit"]),
    (abilities::AFTERMATH, &["onDamagingHit"]),
    (abilities::INNARDS_OUT, &["onDamagingHit"]),
    // Cursed Body: 30% `disable` on the attacker (`conditions::volatile_start`).
    (abilities::CURSED_BODY, &["onDamagingHit"]),
    // Toxic Debris (Toxic Spikes layer), Perish Body (`perishsong` on both), Mummy and Lingering
    // Aroma (`Instruction::SetAbility` on the attacker after its old ability's End).
    (abilities::TOXIC_DEBRIS, &["onDamagingHit"]),
    (abilities::PERISH_BODY, &["onDamagingHit"]),
    (abilities::MUMMY, &["onDamagingHit"]),
    (abilities::LINGERING_AROMA, &["onDamagingHit"]),
    // Wind Rider: `onTryHit` in `moves::ability_hooks::on_try_hit`, `onSideConditionStart` in
    // `abilities::side_condition_start`, `onStart` in `switching::start_ability`.
    (
        abilities::WIND_RIDER,
        &["onSideConditionStart", "onStart", "onTryHit"],
    ),
    (
        abilities::WIND_POWER,
        &["onDamagingHit", "onSideConditionStart"],
    ),
    (
        abilities::THERMAL_EXCHANGE,
        &["onDamagingHit", "onSetStatus", "onUpdate"],
    ),
    // The attacker's `onSourceDamagingHit` (`moves::ability_hooks::on_source_damaging_hit`, O56).
    (abilities::POISON_TOUCH, &["onSourceDamagingHit"]),
    (abilities::TOXIC_CHAIN, &["onSourceDamagingHit"]),
    // `onDamage` in `Battle::damage`, `onAfterMoveSecondary` in `moves::hit_loop`,
    // `onTryEatItem` in the berries' eating (`abilities`).
    (
        abilities::ANGER_SHELL,
        &["onAfterMoveSecondary", "onDamage", "onTryEatItem"],
    ),
    (
        abilities::BERSERK,
        &["onAfterMoveSecondary", "onDamage", "onTryEatItem"],
    ),
    // O54 `onSwitchOut` (Champions) in `abilities::on_switch_out`, from `switching::switch_in`.
    (abilities::REGENERATOR, &["onSwitchOut"]),
    (abilities::NATURAL_CURE, &["onSwitchOut"]),
    // O64 Unburden: `onAfterUseItem` (`Battle::use_item`, Air Balloon's pop) and `onTakeItem`
    // (`Battle::take_item`, Trick) in `abilities::unburden`, `onEnd` in `switching::end_ability`,
    // the volatile's `onModifySpe` in `order.rs`.
    (
        abilities::UNBURDEN,
        &[
            "condition.onModifySpe",
            "onAfterUseItem",
            "onEnd",
            "onTakeItem",
        ],
    ),
    // O63 Unnerve: `onFoeTryEatItem` in `abilities::try_eat_item`; `onStart` / `onEnd` only
    // set and clear the flag it reads (`switching`).
    (abilities::UNNERVE, &["onEnd", "onFoeTryEatItem", "onStart"]),
    // O68 Pastel Veil: `onSetStatus` / `onAllySetStatus` in `Battle::set_status_blocked`,
    // `onUpdate` in `abilities::on_update`, `onStart` / `onAnySwitchIn` in `switching`.
    (
        abilities::PASTEL_VEIL,
        &[
            "onAllySetStatus",
            "onAnySwitchIn",
            "onSetStatus",
            "onStart",
            "onUpdate",
        ],
    ),
    // O58. Own Tempo: `onTryAddVolatile` (confusion) in `Battle::add_volatile_blocked`,
    // `onUpdate` (confusion cure) in `abilities::on_update`, `onTryBoost` (Intimidate) in
    // `Battle::boost_by`, `onHit` only logs. Oblivious: `onTryHit` in
    // `moves::ability_hooks::on_try_hit`, `onTryBoost` in `Battle::boost_by`; `onImmunity`
    // ('attract') and `onUpdate` (attract, taunt) only act on volatiles that do not exist, and an
    // ability-ignoring Attract / Captivate / Taunt against it is refused
    // (`ability_hooks::oblivious_bypassed`). Scrappy, Keen Eye, Illuminate, Mind's Eye:
    // `onModifyMove` in `moves::ability_hooks::on_modify_move`, `onTryBoost` in `Battle::boost_by`.
    (
        abilities::OWN_TEMPO,
        &["onHit", "onTryAddVolatile", "onTryBoost", "onUpdate"],
    ),
    (
        abilities::OBLIVIOUS,
        &["onImmunity", "onTryBoost", "onTryHit", "onUpdate"],
    ),
    (abilities::SCRAPPY, &["onModifyMove", "onTryBoost"]),
    (abilities::KEEN_EYE, &["onModifyMove", "onTryBoost"]),
    (abilities::ILLUMINATE, &["onModifyMove", "onTryBoost"]),
    (abilities::MINDS_EYE, &["onModifyMove", "onTryBoost"]),
    // `moves::ability_hooks`: ModifyMove, ModifySecondaries; Sheer Force's `onBasePower` in
    // `abilities::base_power_handlers`.
    (abilities::SHIELD_DUST, &["onModifySecondaries"]),
    (abilities::SERENE_GRACE, &["onModifyMove"]),
    (abilities::SHEER_FORCE, &["onBasePower", "onModifyMove"]),
    // `Battle::after_set_status`.
    (abilities::SYNCHRONIZE, &["onAfterSetStatus"]),
    // `onModifyMove` (`move.ignoreAbility = true`) in `moves::ability_hooks`; `onStart` only
    // announces the ability.
    (abilities::MOLD_BREAKER, &["onModifyMove", "onStart"]),
    (abilities::TERAVOLT, &["onModifyMove", "onStart"]),
    (abilities::TURBOBLAZE, &["onModifyMove", "onStart"]),
    // O70 type changers: `onModifyType` in `moves::ability_hooks::on_modify_type`, `onBasePower`
    // (`move.typeChangerBoosted`) in `ability_hooks::base_power_handlers`.
    (abilities::PIXILATE, &["onBasePower", "onModifyType"]),
    (abilities::AERILATE, &["onBasePower", "onModifyType"]),
    (abilities::REFRIGERATE, &["onBasePower", "onModifyType"]),
    (abilities::GALVANIZE, &["onBasePower", "onModifyType"]),
    (abilities::DRAGONIZE, &["onBasePower", "onModifyType"]),
    (abilities::NORMALIZE, &["onBasePower", "onModifyType"]),
    (abilities::LIQUID_VOICE, &["onModifyType"]),
    // Fairy Aura / Dark Aura `onAnyBasePower` and Aura Break's `onAnyTryPrimaryHit`
    // (`move.hasAuraBreak`) in `ability_hooks::base_power_handlers`; `onStart` only announces.
    (abilities::FAIRY_AURA, &["onAnyBasePower", "onStart"]),
    (abilities::DARK_AURA, &["onAnyBasePower", "onStart"]),
    (abilities::AURA_BREAK, &["onAnyTryPrimaryHit", "onStart"]),
    // `abilities::flower_veil_holder`: `onAllyTryBoost` in `Battle::boost_by` (ordered with
    // Mirror Armor, `abilities::flower_veil_first`), `onAllySetStatus` in
    // `Battle::try_set_status_from`, `onAllyTryAddVolatile` in `Battle::add_volatile_blocked`.
    (
        abilities::FLOWER_VEIL,
        &["onAllySetStatus", "onAllyTryAddVolatile", "onAllyTryBoost"],
    ),
    // `onAnyAccuracy` in `moves::ability_hooks::accuracy_event`; `onAnyInvulnerability` only
    // answers a semi-invulnerable target, which nothing supported creates (pinned by
    // `no_semi_invulnerable_state_is_supported`).
    (
        abilities::NO_GUARD,
        &["onAnyAccuracy", "onAnyInvulnerability"],
    ),
    // `onTryHit` in `moves::try_hit`, `onAllyTryHitSide` in `moves::try_move_hit_field`; the
    // bounce is `moves::bounce_move` (`ActiveMove.has_bounced`).
    (abilities::MAGIC_BOUNCE, &["onAllyTryHitSide", "onTryHit"]),
    // `onFoeTrapPokemon` in `abilities::trapped` (a trapped Pokémon cannot choose to switch:
    // `Ruleset::validate_slot_action`); `onFoeMaybeTrapPokemon` only sets a display flag.
    (
        abilities::SHADOW_TAG,
        &["onFoeMaybeTrapPokemon", "onFoeTrapPokemon"],
    ),
    (
        abilities::ARENA_TRAP,
        &["onFoeMaybeTrapPokemon", "onFoeTrapPokemon"],
    ),
    (
        abilities::MAGNET_PULL,
        &["onFoeMaybeTrapPokemon", "onFoeTrapPokemon"],
    ),
    // O72: `onStart` / `onWeatherChange` / `onTerrainChange` in `abilities::paradox_change`
    // (`field_events`, `switching::start_ability`), `onEnd` in `switching::end_ability`; the
    // condition's `onStart` stores `bestStat` / `fromBooster`, its Modify* handlers are in
    // `abilities::attack_handlers` / `defense_handlers` and `order.rs`, its `onEnd` only
    // announces. Next to Air Lock / Cloud Nine Protosynthesis is refused
    // (`abilities::paradox_suppressor_problem`).
    (
        abilities::PROTOSYNTHESIS,
        &[
            "condition.onEnd",
            "condition.onModifyAtk",
            "condition.onModifyDef",
            "condition.onModifySpA",
            "condition.onModifySpD",
            "condition.onModifySpe",
            "condition.onStart",
            "onEnd",
            "onStart",
            "onWeatherChange",
        ],
    ),
    (
        abilities::QUARK_DRIVE,
        &[
            "condition.onEnd",
            "condition.onModifyAtk",
            "condition.onModifyDef",
            "condition.onModifySpA",
            "condition.onModifySpD",
            "condition.onModifySpe",
            "condition.onStart",
            "onEnd",
            "onStart",
            "onTerrainChange",
        ],
    ),
    // Opus S. Supreme Overlord: `onStart` in `abilities::supreme_overlord_start` (from
    // `switching::start_ability`), `onBasePower` in `abilities::base_power_handlers`, `onEnd`
    // (a log) in `switching::end_ability`.
    (
        abilities::SUPREME_OVERLORD,
        &["onBasePower", "onEnd", "onStart"],
    ),
    // `onHitProtect` in `abilities::hit_protect` (read by `moves::try_hit`'s protections; the
    // quartered damage is `ActiveMove::bypass_protect`). Champions removes Unseen Fist's
    // `onModifyMove`.
    (abilities::UNSEEN_FIST, &["onHitProtect"]),
    (abilities::PIERCING_DRILL, &["onHitProtect"]),
    // Commander: `onUpdate` (from `abilities::on_update`), `onAnySwitchIn`
    // (`switching::run_switch_in`) and `onStart` (`switching::start_ability`) are all
    // `abilities::commander_update`. The `commanding` / `commanded` conditions: invulnerability
    // (`handlers::invulnerable`, Perish Song), forced pass (`check_side`, `legal`), trapping
    // (`conditions::trapped`), `onDragOut` (`conditions::drag_out_blocked`, the phazing step),
    // no self-switch or Eject Button for the commanded Pokémon.
    (
        abilities::COMMANDER,
        &["onAnySwitchIn", "onStart", "onUpdate"],
    ),
    // Gorilla Tactics: `abilities::gorilla_modify_move` / `gorilla_before_move` /
    // `gorilla_disabled_move` (ModifyMove, BeforeMove, DisableMove), `onModifyAtk` in
    // `abilities::attack_handlers`, `onStart` / `onEnd` in `switching`.
    (
        abilities::GORILLA_TACTICS,
        &[
            "onBeforeMove",
            "onDisableMove",
            "onEnd",
            "onModifyAtk",
            "onModifyMove",
            "onStart",
        ],
    ),
    // Infiltrator: `onModifyMove` sets `ActiveMoveRef::infiltrates` (`ability_hooks`), read by
    // the substitute (`moves::substitute_takes_hit`, Disguise / Ice Face `forme::hits_substitute`,
    // Defog, Aromatherapy), the screens (`moves::get_damage`), Safeguard and Mist (`battle`).
    (abilities::INFILTRATOR, &["onModifyMove"]),
    // Stalwart, Propeller Tail: `move.tracksTarget` (`abilities::tracks_target`, read by
    // `moves::get_move_targets`); `getTarget`'s original target after Ally Switch is refused
    // (`handlers::swap_positions`).
    (abilities::STALWART, &["onModifyMove"]),
    (abilities::PROPELLER_TAIL, &["onModifyMove"]),
    // Soul-Heart: `onAnyFaint` in `abilities::soul_heart` (from `Battle::faint_messages`).
    (abilities::SOUL_HEART, &["onAnyFaint"]),
    // Harvest: `onResidual` (order 28, sub-order 2) in `residual.rs` → `abilities::harvest`.
    (abilities::HARVEST, &["onResidual"]),
    // Pickpocket (`onAfterMoveSecondary`, `moves::hit_loop`) and Magician
    // (`onAfterMoveSecondarySelf`, `moves::use_move_tail`): `abilities::pickpocket` /
    // `magician`; an item the engine cannot move is refused when stolen.
    (abilities::PICKPOCKET, &["onAfterMoveSecondary"]),
    (abilities::MAGICIAN, &["onAfterMoveSecondarySelf"]),
    // Color Change: `onAfterMoveSecondary` in `abilities::color_change` (`moves::hit_loop`).
    (abilities::COLOR_CHANGE, &["onAfterMoveSecondary"]),
    // Wandering Spirit: `onDamagingHit` in `ability_hooks::on_damaging_hit` →
    // `abilities::skill_swap` (End / Start through `switching`).
    (abilities::WANDERING_SPIRIT, &["onDamagingHit"]),
    // Cute Charm: `onDamagingHit` in `ability_hooks::on_damaging_hit`, adding Attract
    // (`conditions::add_attract`: BeforeMove in `moves::before_move`, `onUpdate` in
    // `conditions::attract_update`; Mental Herb, Oblivious and Aroma Veil answer it).
    (abilities::CUTE_CHARM, &["onDamagingHit"]),
    // Rivalry: `onBasePower` in `abilities::base_power_handlers` (`Pokemon::gender`); an
    // undecided gender next to it is refused (`abilities::rivalry_problem`).
    (abilities::RIVALRY, &["onBasePower"]),
    // Symbiosis: `onAllyAfterUseItem` in `abilities::symbiosis` (from every AfterUseItem site:
    // `Battle::use_item`, `update::consume`, Air Balloon); an item it could not pass is refused
    // (`forme::field_problem` → `abilities::symbiosis_problem`).
    (abilities::SYMBIOSIS, &["onAllyAfterUseItem"]),
    // Flower Gift: `onStart` / `onWeatherChange` in `forme::flower_gift` (switch-in,
    // `field_events::weather_changed`), `onAllyModifyAtk` / `onAllyModifySpD` in
    // `abilities::attack_handlers` / `defense_handlers`. Next to Air Lock / Cloud Nine it is
    // refused (`abilities::paradox_suppressor_problem`).
    (
        abilities::FLOWER_GIFT,
        &[
            "onAllyModifyAtk",
            "onAllyModifySpD",
            "onStart",
            "onWeatherChange",
        ],
    ),
    // Opus U. Neutralizing Gas: `onSwitchIn` in `abilities::neutralizing_gas_switch_in` (from
    // `switching::run_switch_in`), `onEnd` in `abilities::neutralizing_gas_end` (switching out,
    // `Battle::faint_messages`, `switching::end_ability`); every other ability reads
    // `Battle::ability`, which is `NONE` while `abilities::ignoring_ability`.
    (abilities::NEUTRALIZING_GAS, &["onEnd", "onSwitchIn"]),
    // Opus U. Poison Heal: `onDamage` in `abilities::poison_heal` (the residual poison damage,
    // `residual.rs`: nothing else deals `psn` / `tox` damage).
    (abilities::POISON_HEAL, &["onDamage"]),
    // Opus U. Slow Start: `onStart` / `onEnd` in `switching`, `onModifyAtk` in
    // `abilities::attack_handlers`, `onModifySpe` in `Battle::speed_stat`, `onResidual` in
    // `abilities::on_residual` (`abilities::slow_start_halves` reads the counter).
    (
        abilities::SLOW_START,
        &[
            "onEnd",
            "onModifyAtk",
            "onModifySpe",
            "onResidual",
            "onStart",
        ],
    ),
    // Opus U. Truant: `onStart` in `abilities::truant_start`, `onBeforeMove` in
    // `abilities::truant_before_move` (the `truant` volatile).
    (abilities::TRUANT, &["onBeforeMove", "onStart"]),
];

pub(crate) fn type_boost_item(item: ItemId) -> Option<Type> {
    TYPE_BOOST_ITEMS
        .iter()
        .find(|&&(i, _)| i == item)
        .map(|&(_, t)| t)
}

fn listed<T: PartialEq + Copy>(table: &[(T, &[&str])], id: T) -> bool {
    table.iter().any(|&(i, _)| i == id)
}

/// Abilities without callbacks that Showdown's core checks by name (`hasAbility`), with
/// behaviour not implemented here. Levitate (grounding) and the `onCriticalHit: false`
/// abilities are implemented.
const CORE_CHECKED_ABILITIES: &[AbilityId] = &[
    abilities::CORROSION,
    abilities::DANCER,
    abilities::EARLY_BIRD,
    abilities::MULTITYPE,
    abilities::RKS_SYSTEM,
    abilities::PERSISTENT,
];

/// Items without callbacks that Showdown's core checks by name, not implemented here.
/// Weather rocks, Light Clay and Terrain Extender (durations), Heavy-Duty Boots (entry hazards),
/// Protective Pads (contact), Grip Claw and Binding Band (partial trapping) and Blunder Policy
/// (`items::blunder_policy`, from the accuracy step) are implemented.
const CORE_CHECKED_ITEMS: &[ItemId] = &[items::ULTRANECROZIUM_Z];

/// Whether an ability is inert or implemented while its holder is on the field.
pub(crate) fn ability_supported_on_field(ability: AbilityId) -> bool {
    let data = ability.data();
    // A constant `onFractionalPriority` is implemented for Stall only (`order`).
    if CORE_CHECKED_ABILITIES.contains(&ability)
        || data.fractional_priority_tenths != fractional_priority_tenths(ability)
    {
        return false;
    }
    data.handlers.is_empty()
        || data.handlers == ["onStart"]
        || listed(ABILITIES_WITH_HANDLERS, ability)
}

/// Whether an item is inert or implemented while its holder is on the field.
pub(crate) fn item_supported_on_field(item: ItemId) -> bool {
    let data = item.data();
    if CORE_CHECKED_ITEMS.contains(&item)
        || data.fractional_priority_tenths != super::items::constant_fractional_tenths(item)
    {
        return false;
    }
    data.handlers.is_empty()
        || listed(ITEMS_WITH_HANDLERS, item)
        || type_boost_item(item).is_some()
        || super::items::resist_berry(item).is_some()
        // Mega Stones only matter for Knock Off, handled by `item_can_be_taken`.
        || (!data.mega_stone.is_empty() && data.handlers == ["onTakeItem"])
}

/// Why a move cannot be simulated, if it cannot.
pub(crate) fn move_unsupported(id: MoveId) -> Option<String> {
    let m = id.data();
    let name = m.name;
    let why = |what: &str| Some(format!("move {name}: {what}"));
    if !m.handlers.is_empty() && !listed(MOVES_WITH_HANDLERS, id) {
        return why(&format!("callbacks {:?} are not implemented", m.handlers));
    }
    match m.target {
        MoveTarget::Normal
        | MoveTarget::Any
        | MoveTarget::AdjacentFoe
        | MoveTarget::AllAdjacentFoes
        | MoveTarget::AllAdjacent
        | MoveTarget::AdjacentAlly
        | MoveTarget::AdjacentAllyOrSelf
        | MoveTarget::Allies
        | MoveTarget::User
        | MoveTarget::All
        | MoveTarget::AllySide
        | MoveTarget::AllyTeam
        | MoveTarget::FoeSide
        | MoveTarget::RandomNormal
        | MoveTarget::Scripted => {}
    }
    if let Some((low, high)) = m.multihit {
        // Fixed counts and the 2–5 draw are implemented (`moves::decide_hits`); other ranges
        // and hit-count dependent callbacks are not.
        if low != high && (low, high) != (2, 5) {
            return why("multi-hit range");
        }
    }
    // OHKO moves (`moves::accuracy_check`, `get_damage`, Sturdy) and self-destruction
    // (`selfdestruct`: `moves::use_move`, `spread_move_hit`) are implemented. Self-switching
    // moves suspend the turn for a decision (F6); the volatile-passing ones (Baton Pass, Shed
    // Tail) are not implemented.
    if matches!(
        m.self_switch,
        SelfSwitch::CopyVolatile | SelfSwitch::ShedTail
    ) {
        return why("switching with volatiles");
    }
    let sleep_moves = id == moves::SLEEP_TALK || id == moves::SNORE;
    // `breaksProtect` (`handlers::break_protect`) and crash damage (`handlers::on_move_fail`)
    // are implemented.
    if m.smart_target
        || (m.calls_move && id != moves::SLEEP_TALK)
        || (m.sleep_usable && !sleep_moves)
        || m.steals_boosts
        || m.mind_blown_recoil
        || (m.struggle_recoil && id != moves::STRUGGLE)
        || m.chloroblast_recoil
        || m.is_z
        || m.is_max
    {
        return why("a special mechanic");
    }
    const STALLING_MOVES: [MoveId; 10] = [
        moves::PROTECT,
        moves::DETECT,
        moves::ENDURE,
        moves::SPIKY_SHIELD,
        moves::BANEFUL_BUNKER,
        moves::KINGS_SHIELD,
        moves::OBSTRUCT,
        moves::SILK_TRAP,
        moves::BURNING_BULWARK,
        // A side move: its `stallingMove` flag does nothing in Showdown (no StallMove check).
        moves::MAT_BLOCK,
    ];
    if m.stalling_move && !STALLING_MOVES.contains(&id) {
        return why("stalling move");
    }
    let flags = m.flags;
    use crate::dex::MoveFlags as F;
    // `cantusetwice` (Gigaton Hammer, Blood Moon) is implemented in `mod.rs::disabled`. Future
    // moves (`futuremove`) are Future Sight and Doom Desire, both implemented (their `onTry`).
    // The two-turn moves in `conditions::charge_volatile` are implemented (F9); the others
    // (Skull Bash, Razor Wind, Sky Drop, Geomancy, ...) are not.
    if flags.contains(F::CHARGE) && super::conditions::charge_volatile(id).is_none() {
        return why("two-turn move");
    }
    if !m.slot_condition.is_none()
        && super::conditions::slot_condition_of(m.slot_condition).is_none()
    {
        return why("slot condition");
    }
    if !m.volatile_status.is_none() && Volatile::from_condition(m.volatile_status).is_none() {
        return why(&format!("volatile {}", m.volatile_status.id()));
    }
    if !m.side_condition.is_none() && side_effect_of(m.side_condition.id()).is_none() {
        return why(&format!("side condition {}", m.side_condition.id()));
    }
    if !m.pseudo_weather.is_none()
        && !["gravity", "trickroom", "wonderroom", "magicroom"].contains(&m.pseudo_weather.id())
    {
        return why(&format!("field effect {}", m.pseudo_weather.id()));
    }
    if let Some(s) = m.self_effect {
        // A self volatile is implemented without self boosts only (`selfDrops`' two paths).
        let volatile_ok = s.volatile_status.is_none()
            || (s.boosts == NO_BOOSTS && Volatile::from_condition(s.volatile_status).is_some());
        if !volatile_ok || !s.side_condition.is_none() || !s.pseudo_weather.is_none() {
            return why("self effect");
        }
    }
    for s in m.secondaries {
        if !s.volatile_status.is_none() && Volatile::from_condition(s.volatile_status).is_none() {
            return why(&format!("secondary volatile {}", s.volatile_status.id()));
        }
    }
    if m.category == MoveCategory::Status && m.base_power != 0 {
        return why("status move with base power");
    }
    None
}

/// Why Sleep Talk cannot be simulated for a Pokémon with these moves: every move it may call
/// must be supported, hit once (a multi-hit move would suspend Sleep Talk's own hit) and have
/// no `onAfterMove` (Showdown runs the called move's AfterMove at the end of `runMove`).
pub(crate) fn sleep_talk_problem(moves: &[MoveId]) -> Option<String> {
    for &id in moves {
        if !super::moves::sleep_talk_calls(id) {
            continue;
        }
        let data = id.data();
        if let Some(why) = move_unsupported(id) {
            return Some(format!("Sleep Talk could call {why}"));
        }
        if data.multihit.is_some() || data.handlers.contains(&"onAfterMove") {
            return Some(format!("Sleep Talk calling {}", data.name));
        }
    }
    None
}

pub(crate) fn side_effect_of(condition: &str) -> Option<SideEffect> {
    Some(match condition {
        "reflect" => SideEffect::Reflect,
        "lightscreen" => SideEffect::LightScreen,
        "auroraveil" => SideEffect::AuroraVeil,
        "tailwind" => SideEffect::Tailwind,
        "safeguard" => SideEffect::Safeguard,
        "mist" => SideEffect::Mist,
        "luckychant" => SideEffect::LuckyChant,
        "wideguard" => SideEffect::WideGuard,
        "quickguard" => SideEffect::QuickGuard,
        "craftyshield" => SideEffect::CraftyShield,
        "matblock" => SideEffect::MatBlock,
        "stealthrock" => SideEffect::StealthRock,
        "spikes" => SideEffect::Spikes,
        "toxicspikes" => SideEffect::ToxicSpikes,
        "stickyweb" => SideEffect::StickyWeb,
        _ => return None,
    })
}

/// The implemented side effects.
const SUPPORTED_SIDE_EFFECTS: [SideEffect; 15] = [
    SideEffect::Reflect,
    SideEffect::LightScreen,
    SideEffect::AuroraVeil,
    SideEffect::Tailwind,
    SideEffect::Safeguard,
    SideEffect::Mist,
    SideEffect::LuckyChant,
    SideEffect::WideGuard,
    SideEffect::QuickGuard,
    SideEffect::CraftyShield,
    SideEffect::MatBlock,
    SideEffect::StealthRock,
    SideEffect::Spikes,
    SideEffect::ToxicSpikes,
    SideEffect::StickyWeb,
];

/// Checks everything on the field before a turn.
pub(crate) fn check_state<const N: usize>(state: &State<N>) -> Result<(), String> {
    for i in 0..FIELD_EFFECT_COUNT {
        let effect = state.field[i];
        if !effect.is_active() {
            continue;
        }
        let supported = match i {
            x if x == FieldEffect::Weather as usize => matches!(
                weather_from(effect.value),
                Weather::Sun | Weather::Rain | Weather::Sand | Weather::Snow
            ),
            x if x == FieldEffect::Terrain as usize => true,
            x if x == FieldEffect::Gravity as usize
                || x == FieldEffect::TrickRoom as usize
                || x == FieldEffect::WonderRoom as usize
                || x == FieldEffect::MagicRoom as usize =>
            {
                true
            }
            _ => false,
        };
        if !supported {
            return Err(format!("field effect #{i} (value {})", effect.value));
        }
        if effect.turns == crate::field::Effect::PERMANENT {
            return Err(format!("field effect #{i} without a duration"));
        }
    }
    for side in [SideId::One, SideId::Two] {
        let s = state.side(side);
        for i in 0..SIDE_EFFECT_COUNT {
            if s.effects[i].is_active() && !SUPPORTED_SIDE_EFFECTS.iter().any(|&e| e as usize == i)
            {
                return Err(format!("side effect #{i}"));
            }
        }
        // Hazards have no duration and 1..=max layers (0 for Stealth Rock and Sticky Web).
        for hazard in super::conditions::HAZARDS {
            let effect = s.effects[hazard as usize];
            if !effect.is_active() {
                continue;
            }
            let max = super::conditions::hazard_layers(hazard);
            let layers_ok = if max == 1 {
                effect.value == 0
            } else {
                (1..=max).contains(&effect.value)
            };
            if effect.turns != crate::field::Effect::PERMANENT || !layers_ok {
                return Err(format!("{hazard:?} with {effect:?}"));
            }
        }
        for slot in 0..N as u8 {
            let r = SlotRef { side, slot };
            let Some(mon) = state.active(r) else {
                continue;
            };
            let name = mon.species.data().name;
            if !ability_supported_on_field(mon.ability) {
                return Err(format!(
                    "{name}: ability {} ({:?})",
                    mon.ability.data().name,
                    mon.ability.data().handlers
                ));
            }
            if mon.ability == abilities::TRACE {
                return Err(format!("{name}: Trace still seeking a target"));
            }
            if let Some(why) = super::forme::field_problem(mon) {
                return Err(why);
            }
            if !item_supported_on_field(mon.item) {
                return Err(format!(
                    "{name}: item {} ({:?})",
                    mon.item.data().name,
                    mon.item.data().handlers
                ));
            }
            if let Some(why) = super::update::berry_problem(mon) {
                return Err(why);
            }
            if !mon.species.data().handlers.is_empty() {
                return Err(format!("{name}: species callbacks"));
            }
            let slot_state = state.slot(r);
            if slot_state.dynamax.is_active() {
                return Err(format!("{name}: Dynamax"));
            }
            // A substitute is its volatile plus its HP (`Slot::substitute_hp`), both or none.
            let substitute = slot_state.volatiles.has(Volatile::Substitute);
            if substitute != (slot_state.substitute_hp > 0) {
                return Err(format!(
                    "{name}: substitute volatile {substitute} with {} HP",
                    slot_state.substitute_hp
                ));
            }
        }
    }
    if let Some(why) = super::abilities::paradox_suppressor_problem(state) {
        return Err(why);
    }
    if let Some(why) = super::abilities::rivalry_problem(state) {
        return Err(why);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_tables_match_the_dex() {
        for &(id, expected) in MOVES_WITH_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, expected) in ITEMS_WITH_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, expected) in ABILITIES_WITH_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, expected, _) in crate::turn::switching::START_HANDLERS {
            assert_eq!(id.data().handlers, expected, "{id:?}");
        }
        for &(id, _) in TYPE_BOOST_ITEMS {
            let data = id.data();
            assert_eq!(data.handlers, ["onBasePower"], "{id:?}");
            assert!(
                data.event_orders.contains(&("onBasePowerPriority", 15)),
                "{id:?}"
            );
        }
        // Stall's only behaviour is its constant fractional priority.
        assert!(abilities::STALL.data().handlers.is_empty());
        assert_eq!(abilities::STALL.data().fractional_priority_tenths, -1);
        assert!(ability_supported_on_field(abilities::STALL));
        // Detect shares Protect's volatile.
        assert_eq!(
            moves::DETECT.data().volatile_status,
            moves::PROTECT.data().volatile_status
        );
    }

    /// Sap Sipper's `onAllyTryHitSide` raises the holder's Attack when an ally uses a Grass move
    /// aimed at their own side (`allySide`/`allyTeam`), which `moves::try_move_hit_field` runs
    /// for the supported ones; Aromatherapy is the only such move.
    #[test]
    fn sap_sipper_ally_side_handler_covers_the_supported_moves() {
        for id in MoveId::all() {
            let m = id.data();
            if move_unsupported(id).is_some() || m.move_type != Type::Grass {
                continue;
            }
            if matches!(m.target, MoveTarget::AllySide | MoveTarget::AllyTeam) {
                assert_eq!(id, moves::AROMATHERAPY);
            }
        }
    }

    /// The two-turn moves the engine runs are exactly `conditions::charge_volatile`'s (their
    /// semi-invulnerability is `handlers::invulnerable`, which No Guard's
    /// `onAnyInvulnerability` answers); every other charge move stays refused. Commander's
    /// `commanding` is in `handlers::invulnerable` too (Opus S).
    #[test]
    fn two_turn_moves_are_the_listed_ones() {
        use crate::dex::MoveFlags;
        for id in MoveId::all() {
            let data = id.data();
            let semi_invulnerable = data.flags.contains(MoveFlags::CHARGE)
                || data.handlers.iter().any(|&h| {
                    h == "condition.onInvulnerability" || h == "condition.onAnyInvulnerability"
                });
            if semi_invulnerable {
                assert_eq!(
                    move_unsupported(id).is_none(),
                    super::super::conditions::charge_volatile(id).is_some(),
                    "{id:?}"
                );
            }
        }
        assert!(ability_supported_on_field(abilities::COMMANDER));
    }

    /// Substitute (F11): every Showdown effect that reads a substitute or passes through one is
    /// implemented or refused.
    /// - `onAfterSubDamage`: implemented for these moves (`handlers::on_after_sub_damage`) and
    ///   Air Balloon (`items::after_sub_damage`); every other holder (Core Enforcer, Shell Side
    ///   Arm, ...) is refused.
    /// - `TryPrimaryHit`: only Aura Break's `onAnyTryPrimaryHit` besides the substitute; the
    ///   gems' `onSourceTryPrimaryHit` and Gulp Missile are refused.
    /// - `move.infiltrates`: Infiltrator (`ActiveMoveRef::infiltrates`) and Pollen Puff are
    ///   implemented, Present is refused.
    /// - Moves whose own code reads a substitute: Aromatherapy and Defog are implemented; Shed
    ///   Tail, Baton Pass, Sky Drop, Tidy Up, Transform, Sparkly Swirl are refused.
    /// - Disguise and Ice Face (`hitSub` in their handlers) are refused behind a substitute
    ///   (`check_state`, and `moves::hit_substitute` at run time).
    #[test]
    fn substitute_readers_are_implemented_or_refused() {
        use crate::dex::ItemId;
        // Shell Side Arm's only reveals its category.
        const AFTER_SUB_DAMAGE: [MoveId; 7] = [
            moves::RAPID_SPIN,
            moves::MORTAL_SPIN,
            moves::ICE_SPINNER,
            moves::STEEL_ROLLER,
            moves::CEASELESS_EDGE,
            moves::STONE_AXE,
            moves::SHELL_SIDE_ARM,
        ];
        for id in MoveId::all() {
            let handlers = id.data().handlers;
            if move_unsupported(id).is_none() && handlers.contains(&"onAfterSubDamage") {
                assert!(AFTER_SUB_DAMAGE.contains(&id), "{id:?}");
            }
            if move_unsupported(id).is_none() {
                assert!(
                    !handlers.iter().any(|h| h.contains("TryPrimaryHit"))
                        || id == moves::SUBSTITUTE,
                    "{id:?}"
                );
            }
        }
        for id in ItemId::all() {
            let handlers = id.data().handlers;
            if item_supported_on_field(id)
                && handlers
                    .iter()
                    .any(|h| h.contains("AfterSubDamage") || h.contains("TryPrimaryHit"))
            {
                assert_eq!(id, items::AIR_BALLOON);
            }
        }
        assert!(!item_supported_on_field(items::NORMAL_GEM));
        for id in AbilityId::all() {
            let handlers = id.data().handlers;
            if ability_supported_on_field(id)
                && handlers
                    .iter()
                    .any(|h| h.contains("AfterSubDamage") || h.contains("TryPrimaryHit"))
            {
                assert_eq!(id, abilities::AURA_BREAK);
            }
        }
        assert!(!ability_supported_on_field(abilities::GULP_MISSILE));
        assert!(ability_supported_on_field(abilities::INFILTRATOR));
        // Disguise and Ice Face read the substitute themselves (`hitSub`, `forme::hits_substitute`).
        for ability in [abilities::DISGUISE, abilities::ICE_FACE] {
            assert!(ability_supported_on_field(ability), "{ability:?}");
        }
        for id in [
            moves::SHED_TAIL,
            moves::BATON_PASS,
            moves::SKY_DROP,
            moves::TIDY_UP,
            moves::TRANSFORM,
            moves::SPARKLY_SWIRL,
            moves::PRESENT,
        ] {
            assert!(move_unsupported(id).is_some(), "{id:?}");
        }
        assert_eq!(move_unsupported(moves::SUBSTITUTE), None);
    }

    /// A substitute is its volatile and its HP together; Disguise behind one is fine (F19 checks
    /// `hitSub` itself).
    #[test]
    fn substitute_state_is_checked() {
        let mut state = State::<2>::default();
        let slot = SlotRef {
            side: SideId::One,
            slot: 0,
        };
        for side in [SideId::One, SideId::Two] {
            state.side_mut(side).party[0].species = crate::dex::species::MIMIKYU;
            state.side_mut(side).party[0].max_hp = 100;
            state.side_mut(side).party[0].hp = 100;
            state.side_mut(side).slots[0].party_index = Some(0);
        }
        assert_eq!(check_state(&state), Ok(()));
        state.slot_mut(slot).substitute_hp = 25;
        assert!(check_state(&state).is_err(), "HP without the volatile");
        state.slot_mut(slot).volatiles.set(
            Volatile::Substitute,
            crate::volatile::VolatileState {
                active: true,
                ..crate::volatile::VolatileState::NONE
            },
        );
        assert_eq!(check_state(&state), Ok(()));
        state.slot_mut(slot).substitute_hp = 0;
        assert!(check_state(&state).is_err(), "the volatile without HP");
        state.slot_mut(slot).substitute_hp = 25;
        state.side_mut(SideId::One).party[0].ability = abilities::DISGUISE;
        assert_eq!(check_state(&state), Ok(()));
    }

    /// Purifying Salt's `onTryAddVolatile` only blocks Yawn, which `Battle::add_volatile_blocked`
    /// implements now that Yawn is a volatile.
    #[test]
    fn yawn_is_a_supported_volatile() {
        assert_eq!(
            Volatile::from_condition(crate::dex::conditions::YAWN),
            Some(Volatile::Yawn)
        );
    }

    /// The `futuremove` moves are exactly the two implemented ones (their `onTry` and the
    /// `futuremove` slot condition).
    #[test]
    fn future_moves_are_future_sight_and_doom_desire() {
        use crate::dex::MoveFlags;
        let future: Vec<MoveId> = MoveId::all()
            .filter(|id| id.data().flags.contains(MoveFlags::FUTUREMOVE))
            .collect();
        assert_eq!(future, [moves::DOOM_DESIRE, moves::FUTURE_SIGHT]);
        for id in future {
            assert_eq!(move_unsupported(id), None, "{id:?}");
        }
    }

    #[test]
    fn common_moves_are_classified() {
        for id in [
            moves::HYPNOSIS,
            moves::ROCK_SLIDE,
            moves::HYPER_VOICE,
            moves::WOOD_HAMMER,
            moves::HIGH_HORSEPOWER,
            moves::IRON_HEAD,
            moves::FOCUS_BLAST,
            moves::PROTECT,
            moves::FAKE_OUT,
            moves::GRAVITY,
        ] {
            assert_eq!(move_unsupported(id), None, "{id:?}");
        }
        assert_eq!(move_unsupported(moves::U_TURN), None);
        assert!(move_unsupported(moves::BATON_PASS).is_some());
        assert_eq!(move_unsupported(moves::WHIRLWIND), None);
        assert_eq!(move_unsupported(moves::FOLLOW_ME), None);
        assert_eq!(move_unsupported(moves::RAGE_POWDER), None);
        assert_eq!(move_unsupported(moves::BULLET_SEED), None);
        assert_eq!(move_unsupported(moves::POPULATION_BOMB), None);
        assert_eq!(move_unsupported(moves::TRIPLE_AXEL), None);
    }
}
