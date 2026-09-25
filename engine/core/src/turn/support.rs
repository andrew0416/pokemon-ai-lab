//! What the turn engine implements, checked before a turn runs.
//!
//! A turn is only simulated if every effect that could act in it is implemented: the moves
//! chosen, the abilities, items, statuses and volatiles on the field, and the field and side
//! effects. Anything else is a [`super::TurnError::Unsupported`] naming it. Showdown lists
//! each entry's behaviour as callback names (`handlers` in the dex); the tables here pin the
//! handler lists that are implemented, and a test fails if the dex lists change.

use crate::dex::{
    abilities, items, moves, AbilityId, ItemId, MoveCategory, MoveId, MoveTarget, Ohko,
    SelfDestruct, SelfSwitch, Type, NO_BOOSTS,
};
use crate::field::{FieldEffect, SideEffect, Weather, FIELD_EFFECT_COUNT, SIDE_EFFECT_COUNT};
use crate::state::{SideId, SlotRef, State, Status};
use crate::volatile::Volatile;

use super::battle::{cured_on_update, weather_from};
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
    (moves::GRASSY_GLIDE, &["onModifyPriority"]),
    (moves::LOW_KICK, &["basePowerCallback", "onTryHit"]),
    (moves::GRASS_KNOT, &["basePowerCallback", "onTryHit"]),
    (moves::FAKE_OUT, &["onDisableMove", "onTry"]),
    (moves::KNOCK_OFF, &["onAfterHit", "onBasePower"]),
    (moves::GRAV_APPLE, &["onBasePower"]),
    (moves::EXPANDING_FORCE, &["onBasePower", "onModifyMove"]),
    (moves::WEATHER_BALL, &["onModifyMove", "onModifyType"]),
    (moves::TERRAIN_PULSE, &["onModifyMove", "onModifyType"]),
    // `onAfterSubDamage` needs a substitute, which is refused.
    (moves::ICE_SPINNER, &["onAfterHit", "onAfterSubDamage"]),
    (moves::STEEL_ROLLER, &["onAfterSubDamage", "onHit", "onTry"]),
    // `onTryMove` only fails an ally-targeted use under Heal Block, which no supported effect
    // adds.
    (moves::POLLEN_PUFF, &["onHit", "onTryHit", "onTryMove"]),
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
    (moves::PSYBLADE, &["onBasePower"]),
    (moves::BLIZZARD, &["onModifyMove"]),
    // Still rejected for its confusion secondary; shares Thunder's handler.
    (moves::HURRICANE, &["onModifyMove"]),
    (moves::THUNDER, &["onModifyMove"]),
    (moves::FREEZE_DRY, &["onEffectiveness"]),
    (moves::FLYING_PRESS, &["onEffectiveness"]),
    (moves::POLTERGEIST, &["onTry", "onTryHit"]),
    (moves::ACROBATICS, &["basePowerCallback"]),
    (moves::FIRST_IMPRESSION, &["onDisableMove", "onTry"]),
    (moves::DIRE_CLAW, &["secondaries.onHit", "secondary.onHit"]),
    (moves::TRI_ATTACK, &["secondaries.onHit", "secondary.onHit"]),
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
    // WeatherChange (when the item starts being ignored, stops being ignored, or ends), which
    // has no implemented handler (`field_events`); `onStart` returns at once for a holder that
    // does not ignore its item.
    (items::UTILITY_UMBRELLA, &["onEnd", "onStart", "onUpdate"]),
    // `onStart` at switch-in (priority -1) and PseudoWeatherChange (`moves::add_pseudo_weather`).
    (
        items::ROOM_SERVICE,
        &["onAnyPseudoWeatherChange", "onStart"],
    ),
    // Grounding (`Battle::is_grounded`), Speed, effectiveness; Air Balloon's `onStart` only
    // announces it and its pop (`onDamagingHit`) is refused until F15 (`items::on_damaging_hit`;
    // `onAfterSubDamage` needs a substitute, which is refused).
    (
        items::AIR_BALLOON,
        &["onAfterSubDamage", "onDamagingHit", "onStart"],
    ),
    (items::IRON_BALL, &["onEffectiveness", "onModifySpe"]),
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
];

/// Abilities with callbacks that are implemented while the holder is on the field.
/// Abilities whose only callback is `onStart` act only on switch-in (see `switching`).
pub(crate) const ABILITIES_WITH_HANDLERS: &[(AbilityId, &[&str])] = &[
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
    (
        abilities::THICK_FAT,
        &["onSourceModifyAtk", "onSourceModifySpA"],
    ),
    // `onDamage`: burn damage halved in `residual.rs`.
    (
        abilities::HEATPROOF,
        &["onDamage", "onSourceModifyAtk", "onSourceModifySpA"],
    ),
    // `onSetStatus` in `Battle::try_set_status`; `onUpdate` (cure a burn) can only act on a
    // burned holder, which `check_state` rejects and `onSetStatus` prevents.
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
    // `add_volatile_blocked`). `onUpdate` cures are unreachable: see `cured_on_update`.
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
    // `onAllyTryHitSide` only acts on an ally's Grass move aimed at its own side, which no
    // supported move is (pinned by a test below).
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
/// Weather rocks, Light Clay and Terrain Extender (durations) are implemented; Heavy-Duty
/// Boots and Protective Pads only affect unsupported hazards and contact abilities.
const CORE_CHECKED_ITEMS: &[ItemId] = &[
    items::BLUNDER_POLICY,
    items::GRIP_CLAW,
    items::BINDING_BAND,
    items::ULTRANECROZIUM_Z,
];

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
        | MoveTarget::FoeSide
        | MoveTarget::RandomNormal => {}
        other => return why(&format!("target {other:?}")),
    }
    if let Some((low, high)) = m.multihit {
        // Fixed counts and the 2–5 draw are implemented (`moves::decide_hits`); other ranges
        // and hit-count dependent callbacks are not.
        if low != high && (low, high) != (2, 5) {
            return why("multi-hit range");
        }
    }
    if m.ohko != Ohko::No {
        return why("OHKO");
    }
    if m.self_switch != SelfSwitch::No || m.force_switch {
        return why("switching");
    }
    if m.selfdestruct != SelfDestruct::No {
        return why("self-destruct");
    }
    if m.breaks_protect
        || m.smart_target
        || m.calls_move
        || m.sleep_usable
        || m.steals_boosts
        || m.has_crash_damage
        || m.mind_blown_recoil
        || m.struggle_recoil
        || m.chloroblast_recoil
        || m.is_z
        || m.is_max
    {
        return why("a special mechanic");
    }
    if m.stalling_move && ![moves::PROTECT, moves::DETECT, moves::ENDURE].contains(&id) {
        return why("stalling move");
    }
    let flags = m.flags;
    use crate::dex::MoveFlags as F;
    for (flag, what) in [
        (F::CHARGE, "two-turn"),
        (F::FUTUREMOVE, "future move"),
        (F::CANTUSETWICE, "can't use twice"),
    ] {
        if flags.contains(flag) {
            return why(what);
        }
    }
    if !m.slot_condition.is_none() {
        return why("slot condition");
    }
    if !m.volatile_status.is_none() && Volatile::from_condition(m.volatile_status).is_none() {
        return why(&format!("volatile {}", m.volatile_status.id()));
    }
    if !m.side_condition.is_none() && side_effect_of(m.side_condition.id()).is_none() {
        return why(&format!("side condition {}", m.side_condition.id()));
    }
    if !m.pseudo_weather.is_none()
        && !["gravity", "trickroom", "wonderroom"].contains(&m.pseudo_weather.id())
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
        _ => return None,
    })
}

/// The implemented side effects.
const SUPPORTED_SIDE_EFFECTS: [SideEffect; 9] = [
    SideEffect::Reflect,
    SideEffect::LightScreen,
    SideEffect::AuroraVeil,
    SideEffect::Tailwind,
    SideEffect::Safeguard,
    SideEffect::Mist,
    SideEffect::LuckyChant,
    SideEffect::WideGuard,
    SideEffect::QuickGuard,
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
                || x == FieldEffect::WonderRoom as usize =>
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
            if cured_on_update(mon.ability, mon.status) {
                return Err(format!(
                    "{name}: {} would cure its status on Update (not implemented)",
                    mon.ability.data().name
                ));
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
            if let Some(why) = super::items::held_item_problem(mon) {
                return Err(why);
            }
            if !mon.species.data().handlers.is_empty() {
                return Err(format!("{name}: species callbacks"));
            }
            let slot_state = state.slot(r);
            if slot_state.substitute_hp != 0 || slot_state.dynamax.is_active() {
                return Err(format!("{name}: substitute or Dynamax"));
            }
        }
        // Water Bubble's `onUpdate` cures its holder's burn at the next Update (for a bench
        // member: when it switches in). The Update event is not implemented; its SetStatus
        // block keeps a holder from being burned during a turn, so only a burned holder at the
        // start can trigger it.
        for mon in s.party.iter().filter(|m| m.hp > 0) {
            let bubble = [mon.ability, mon.base_ability].contains(&abilities::WATER_BUBBLE);
            if bubble && mon.status == Status::Burn {
                return Err(format!(
                    "{}: burned with Water Bubble (onUpdate)",
                    mon.species.data().name
                ));
            }
        }
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

    /// No supported move ignores abilities by its data and inflicts a status. Such a move (or a
    /// Mold Breaker user's) against an ability whose `onUpdate` would cure the status is refused
    /// in ModifyMove (`moves::ability_hooks`); this keeps the data-flag case from arising
    /// unnoticed.
    #[test]
    fn no_supported_move_ignores_abilities_and_sets_a_status() {
        for id in MoveId::all() {
            let m = id.data();
            if !m.ignore_ability || move_unsupported(id).is_some() {
                continue;
            }
            assert_eq!(m.status, crate::state::Status::None, "{id:?}");
            for s in m.secondaries {
                assert_eq!(s.status, crate::state::Status::None, "{id:?}");
            }
        }
    }

    /// Sap Sipper's `onAllyTryHitSide` raises the holder's Attack when an ally uses a Grass move
    /// aimed at their own side (`allySide`/`allyTeam`; TryHitSide is not modelled). No supported
    /// move is one; this fails when one becomes supported.
    #[test]
    fn sap_sipper_ally_side_handler_is_unreachable() {
        for id in MoveId::all() {
            let m = id.data();
            if move_unsupported(id).is_some() || m.move_type != Type::Grass {
                continue;
            }
            assert!(
                !matches!(m.target, MoveTarget::AllySide | MoveTarget::AllyTeam),
                "{id:?}"
            );
        }
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
        assert!(move_unsupported(moves::U_TURN).is_some());
        assert_eq!(move_unsupported(moves::FOLLOW_ME), None);
        assert_eq!(move_unsupported(moves::RAGE_POWDER), None);
        assert_eq!(move_unsupported(moves::BULLET_SEED), None);
        assert_eq!(move_unsupported(moves::POPULATION_BOMB), None);
        assert!(move_unsupported(moves::TRIPLE_AXEL).is_some());
    }
}
