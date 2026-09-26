//! Volatile conditions: per-slot effects that end on switch-out (Showdown `pokemon.volatiles`).
//!
//! Like field and side effects, volatiles are a table indexed by kind instead of one struct
//! field each. A variant exists only once the turn engine implements it; moves, abilities and
//! items that would create any other volatile are rejected before the turn runs.
//!
//! A few kinds are slot state Showdown keeps elsewhere (an ability's `abilityState`); they live
//! here because they reset exactly like volatiles, and [`Volatile::showdown_state`] hides them
//! from the canonical state.

use crate::dex::{conditions, ConditionId, MoveId, Type};
use crate::state::{PokemonRef, SideId, SlotRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Volatile {
    /// Protect's single-turn shield (Showdown `protect`, duration 1).
    Protect = 0,
    /// Consecutive-protection counter (Showdown `stall`, duration 2, `counter` 3, 9, ...).
    Stall,
    /// Flinch (duration 1).
    Flinch,
    /// Follow Me: redirects foes' single-target moves to the holder (duration 1).
    FollowMe,
    /// Rage Powder: like Follow Me, but not for powder-immune attackers (duration 1).
    RagePowder,
    /// Spotlight: like Follow Me with higher redirect priority (duration 1).
    Spotlight,
    /// Confusion: `time` turns (2–5), 33% self-hit before each move.
    Confusion,
    /// Outrage / Petal Dance / Thrash: locked into `mv` (duration 2, hidden `trueDuration`
    /// 2–3), confusion when it ends by fatigue.
    LockedMove,
    /// Hyper Beam's recharge turn (duration 2, `recharge` locks the next action).
    MustRecharge,
    /// Encore: locked into `mv` (duration 3, one more if the target already moved).
    Encore,
    /// Flash Fire's boost after absorbing a Fire move (the ability's own `condition`, no
    /// duration; `noCopy`).
    FlashFire,
    /// Choice item lock (Showdown `choicelock`, no duration): `counter` holds the locked
    /// move's `MoveId` (Showdown `effectState.move`).
    ChoiceLock,
    /// Roost: Flying is left out of the holder's types until the end of the turn (duration 1,
    /// residual order 25). Showdown filters the types in `onType`; the engine changes them with
    /// `SetTypes` and keeps the types from before in `counter` ([`encode_types`]; 0 = nothing
    /// was removed) to restore them when Roost ends.
    Roost,
    /// Yawn: the holder falls asleep when it ends (duration 2, residual order 23).
    Yawn,
    /// Perish Song's count (duration 4, residual order 24): the holder faints when it ends.
    /// Showdown adds it by name in the move's `onHitField`, so the dex has no condition id.
    PerishSong,
    /// Endure: a move's damage leaves the holder at 1 HP at least (duration 1).
    Endure,
    /// Not a Showdown volatile: Protean's / Libero's `abilityState.protean` / `.libero` flag
    /// (the type already changed since switching in). No duration; hidden in the canonical
    /// state.
    ProteanUsed,
    /// Charge (the move's condition, also added by Electromorphosis and Wind Power): the
    /// holder's next Electric move has double power (`onBasePower`, priority 9); it ends after
    /// an Electric move (`onAfterMove`, `onMoveAborted`). No duration.
    Charge,
    /// Not a Showdown volatile: Anger Shell's / Berserk's `abilityState.checkedAngerShell` /
    /// `.checkedBerserk === false` (a single-hit move's damage is waiting for the
    /// `AfterMoveSecondary` check; healing berries are not eaten meanwhile, `onTryEatItem`). No
    /// duration; hidden in the canonical state.
    AngerShellUnchecked,
    /// Unburden's own condition (`addVolatile('unburden')` once its holder uses or loses its
    /// item): Speed doubles while the holder has no item. No duration.
    Unburden,
    /// Focus Energy's condition (`focusenergy`, no duration): critical-hit ratio +2. Added by
    /// the move and by Lansat Berry; it and Dragon Cheer exclude each other (`onStart`).
    FocusEnergy,
    /// Micle Berry's own condition (`micleberry`, duration 2): the holder's next accuracy check
    /// (`onSourceAccuracy`) is 4915/4096 and ends it.
    MicleBerry,
    /// Helping Hand: the holder's moves this turn get more power (duration 1). `counter` counts
    /// the applications (Showdown keeps `multiplier` = 1.5 per application instead, which the
    /// canonical state does not print, so neither is `counter`).
    HelpingHand,
    /// Taunt: status moves can be neither chosen nor used (duration 3, one more if the holder
    /// was active since the turn started and has no move left; residual order 15).
    Taunt,
    /// Disable: `mv` (the holder's last move when it started) can be neither chosen nor used
    /// (duration 5, one less if the holder still has a move to come; residual order 17).
    Disable,
    /// Torment: the holder's last move cannot be chosen (no duration).
    Torment,
    /// Imprison, on its user: the user's foes can neither choose nor use a move the user knows
    /// (no duration).
    Imprison,
    /// Glaive Rush's drawback on its user until its next move attempt (no duration): moves
    /// against it cannot miss and deal double damage.
    GlaiveRush,
    /// Sparkling Aria's secondary effect on a target it hit (no duration): the move's
    /// `onAfterMove` removes it again, curing a burn.
    SparklingAria,
    /// Protosynthesis's own condition (no duration, `noCopy`): the holder's best stat
    /// (`effectState.bestStat`, kept in `counter`: 0 Atk, 1 Def, 2 SpA, 3 SpD, 4 Spe) is raised
    /// 5325/4096 (Speed 1.5x). `hidden` = 1 for `effectState.fromBooster` (from Booster Energy:
    /// it outlasts the sun). Neither field is in Showdown's canonical state.
    Protosynthesis,
    /// Quark Drive's own condition: as [`Volatile::Protosynthesis`], for Electric Terrain.
    QuarkDrive,
    /// Throat Chop's secondary effect (`throatchop`, duration 2, residual order 22): sound moves
    /// can be neither chosen nor used. Added by name, so the dex has no condition id.
    ThroatChop,
    /// Spiky Shield's single-turn shield (duration 1): like Protect, and a contact move costs
    /// its user 1/8 of its max HP.
    SpikyShield,
    /// Baneful Bunker (duration 1): like Protect, and a contact move poisons its user.
    BanefulBunker,
    /// King's Shield (duration 1): blocks damaging moves only; contact lowers Attack by 1.
    KingsShield,
    /// Obstruct (duration 1): blocks damaging moves only; contact lowers Defense by 2.
    Obstruct,
    /// Silk Trap (duration 1): blocks damaging moves only; contact lowers Speed by 1.
    SilkTrap,
    /// Burning Bulwark (duration 1): blocks damaging moves only; contact burns.
    BurningBulwark,
    /// No Retreat (no duration): the holder cannot switch out (`onTrapPokemon`) unless it is
    /// immune to trapping (Ghost), and cannot use No Retreat again.
    NoRetreat,
    /// Leech Seed (no duration, residual order 8): the holder loses 1/8 of its max HP to
    /// whoever stands in the seeder's slot (`sourceSlot`, kept in `counter`:
    /// [`encode_slot`]; hidden in the canonical state).
    LeechSeed,
    /// Partial trapping (Bind, Wrap, Fire Spin, ...; duration 5–6, 8 with Grip Claw, residual
    /// order 13): 1/8 (1/6 with Binding Band: `boundDivisor`, kept in `hidden`) of the max HP
    /// each turn and no switching while the trapper (`source`, kept in `counter`:
    /// [`encode_pokemon`]; hidden in the canonical state) stays in.
    PartiallyTrapped,
    /// Destiny Bond (no duration): if a foe's move knocks the holder out, the foe faints too;
    /// it ends at the holder's next move attempt.
    DestinyBond,
    /// `twoturnmove` (duration 2; F9): the user is locked into `mv` next turn (`onLockMove`),
    /// aimed at the target location it chose (`targetLoc`, kept in `counter`; hidden). Its end
    /// removes the move's own volatile; a BeforeMove abort removes it early.
    TwoTurnMove,
    /// The charging move's own volatile (`attacker.addVolatile(move.id)`), removed by its
    /// `onTryMove` on the second turn. These five have no condition data (no duration).
    SolarBeam,
    SolarBlade,
    MeteorBeam,
    ElectroShot,
    SkyAttack,
    /// The semi-invulnerable moves' own volatiles (duration 2): `onInvulnerability` (Fly and
    /// Bounce let Gust, Twister, Sky Uppercut, Thunder, Hurricane, Smack Down and Thousand
    /// Arrows through; Dig Earthquake and Magnitude; Dive Surf and Whirlpool), double damage
    /// from those (`onSourceModifyDamage`; Bounce's `onSourceBasePower`), and Dig / Dive's
    /// immunity to sandstorm damage (`onImmunity`).
    Fly,
    Bounce,
    Dig,
    Dive,
    PhantomForce,
    ShadowForce,
    /// Substitute (F11, no duration): its HP (`effectState.hp`) is the slot's
    /// [`crate::state::Slot::substitute_hp`], set by its `onStart` and lowered by the moves it
    /// takes (`onTryPrimaryHit`); it ends at 0.
    Substitute,
    /// Zen Mode's own condition (`zenmode`, no duration; WORKPLAN F19): its start changes the
    /// holder to its Zen forme and its end back (`turn/forme.rs`). It exists exactly while the
    /// holder is in a Zen forme.
    ZenMode,
    /// Ally Switch's own condition (`allyswitch`, duration 2, `counterMax` 729): `counter` is
    /// the success chance's denominator for the next use (3, then tripled per success);
    /// `onRestart` succeeds with probability 1/`counter` or deletes it. Added by name in the
    /// move's `onPrepareHit`, so the dex has no condition id.
    AllySwitch,
    /// Mean Look / Block / Spider Web on their target (`trapped`, no duration): it cannot
    /// switch out (`onTrapPokemon`) unless immune to trapping. Linked to its trapper's
    /// [`Volatile::Trapper`] (`addVolatile('trapped', source, move, 'trapper')`): the trapper
    /// is kept in `counter` ([`encode_pokemon`]; hidden in the canonical state).
    Trapped,
    /// The linked `trapper` volatile on the Pokémon that trapped others (no handlers): the
    /// Pokémon it trapped are bits of `counter` (`SlotHistory::attacker_bit`; hidden). When
    /// either side of the link leaves the field, the other end is removed
    /// (`conditions::remove_linked_volatiles`).
    Trapper,
    /// Salt Cure's secondary effect (`saltcure`, no duration, residual order 13): the holder
    /// loses baseMaxhp / 8 each turn if Water or Steel, else / 16 (Champions halves both).
    SaltCure,
    /// Ingrain (no duration, residual order 7): heals baseMaxhp / 16 each turn, grounds the
    /// holder, keeps it from switching out (`onTrapPokemon`) and from being dragged out
    /// (`onDragOut`).
    Ingrain,
    /// Magnet Rise (duration 5, residual order 18): the holder is not grounded (immune to
    /// Ground).
    MagnetRise,
    /// Counter's own condition (duration 1), added by its `beforeTurnCallback` when the turn
    /// starts: the last physical hit from a foe is recorded (`onDamagingHit`): twice its damage
    /// in `counter` (Showdown `effectState.damage`) and the attacker's slot in `hidden`
    /// (`effectState.slot`: 1 + the slot index on the holder's foe side; 0 = `null`). Both are
    /// hidden in the canonical state. Added by name, so the dex has no condition id.
    Counter,
    /// Mirror Coat's condition: as [`Volatile::Counter`], for special hits.
    MirrorCoat,
    /// Focus Punch's condition (duration 1), added by its `priorityChargeCallback` (order 107):
    /// a non-status move hitting the holder sets `lostFocus` (`counter` 1, hidden), which makes
    /// Focus Punch fail (`beforeMoveCallback`); it also blocks flinching (`onTryAddVolatile`).
    FocusPunch,
    /// Beak Blast's condition (duration 1, from `priorityChargeCallback`): a contact move hitting
    /// the holder burns its user (`onHit`); Beak Blast's `onAfterMove` removes it.
    BeakBlast,
    /// Shell Trap's condition (duration 1, from `priorityChargeCallback`): a foe's physical move
    /// hitting the holder sets `gotHit` (`counter` 1, hidden) and moves the holder's Shell Trap to
    /// the front of the queue; without it Shell Trap stops (`onTryMove`).
    ShellTrap,
    /// Heal Block (duration 5, 2 from Psychic Noise; residual order 20): the holder's `heal`
    /// moves can be neither chosen nor used, and every `battle.heal` on it fails (`onTryHeal`).
    HealBlock,
    /// Smack Down / Thousand Arrows (`smackdown`, no duration): the holder is grounded
    /// (`isGrounded`, right after Ingrain). It only starts on a Pokémon that was airborne
    /// (Flying, Levitate, Magnet Rise, or in the air with Fly / Bounce, which it brings down).
    SmackDown,
    /// Not a Showdown volatile: Supreme Overlord's `abilityState.fallen` (its `onStart` stores
    /// `min(side.totalFainted, 5)` when that is not 0), kept in `counter`. No duration; hidden
    /// in the canonical state.
    SupremeOverlord,
    /// Commander on Tatsugiri inside its Dondozo ally (`commanding`, no duration): it cannot be
    /// hit (`hitStepInvulnerabilityEvent`, `onInvulnerability`), acts never (its choice is a
    /// pass, a queued action is cancelled), and can be neither switched out (`onTrapPokemon`,
    /// after Shed Shell) nor dragged out (`onDragOut`).
    Commanding,
    /// Commander on the Dondozo it commands (`commanded`, no duration): +2 in every stat when it
    /// starts; trapped and not dragged out, as `commanding`, and it does not switch itself out
    /// (`selfSwitch`, Eject Button).
    Commanded,
    /// Not a Showdown volatile: Gorilla Tactics' `abilityState.choiceLock`, the move (`mv`) its
    /// holder is locked into since its first move after starting. No duration; hidden in the
    /// canonical state.
    GorillaTactics,
    /// Attract (the move's `condition`, no duration; only Cute Charm adds it): 50% the holder
    /// cannot move (BeforeMove, priority 2); it ends once its source (`effectState.source`, kept
    /// in `counter`: [`encode_pokemon`]; hidden in the canonical state) is no longer active
    /// (`onUpdate`).
    Attract,
    /// Not a Showdown volatile: Eject Pack's `itemState.eject` (a stat of the holder was lowered
    /// and the pack has not been used yet: `items::eject_pack_use`). It lives on the item's
    /// state, which ends with the item (used, knocked off) and on switching out or fainting
    /// (`onEnd`). No duration; hidden in the canonical state.
    EjectPack,
    /// The Metronome item's condition (`metronome`, no duration; its `onStart` adds it when the
    /// holder switches in or gets the item): `mv` is `effectState.lastMove` and `counter`
    /// `effectState.numConsecutive` (kept at 5 at most: only `min(numConsecutive, 5)` is read),
    /// neither a canonical field. It stays after the item is gone until the next TryMove
    /// (`items::metronome_try_move`). The item's `condition` is not a named dex condition.
    Metronome,
    /// Nightmare (the move's condition, no duration, residual order 11): a sleeping (or
    /// Comatose) holder loses baseMaxhp / 4 each turn. `cureStatus` / `clearStatus` of a sleeping
    /// holder and a new sleep's `onStart` remove it (`Battle::cure_status`).
    Nightmare,
    /// Octolock on its target (no duration, residual order 14): while its source
    /// (`effectState.source`, kept in `counter`: [`encode_pokemon`]; hidden in the canonical
    /// state) is active the holder cannot switch out (`onTrapPokemon`) and loses 1 Def and 1 SpD
    /// each turn; the residual deletes it once the source left, fainted or just switched in.
    Octolock,
    /// Dragon Cheer (no duration): the holder's critical-hit ratio +2 if it was a Dragon type
    /// when the condition started (`effectState.hasDragonType`, kept in `hidden`; not a canonical
    /// field), else +1. It and Focus Energy exclude each other (`onStart`).
    DragonCheer,
    /// Laser Focus (duration 2; its `onRestart` sets the duration to 2 again): the holder's
    /// critical-hit ratio becomes 5 (`onModifyCritRatio`), so its moves always crit.
    LaserFocus,
    /// Aqua Ring (no duration, residual order 6): heals baseMaxhp / 16 each turn (Big Root
    /// applies).
    AquaRing,
    /// Power Trick (no duration): the holder's stored Attack and Defense trade places when it
    /// starts and again when it ends (using the move again ends it: `onRestart`). Leaving the
    /// field recalculates the stored stats anyway.
    PowerTrick,
    /// Power Shift: as [`Volatile::PowerTrick`] (the same swap in this Showdown version).
    PowerShift,
    /// The charging move's own volatile (as [`Volatile::SolarBeam`]; no condition data, no
    /// duration) for Skull Bash, Razor Wind, Freeze Shock, Ice Burn and Geomancy.
    SkullBash,
    RazorWind,
    FreezeShock,
    IceBurn,
    Geomancy,
    /// Stockpile (no duration, `noCopy`): `effectState.layers` (1–3, kept in `counter` and written
    /// as `layers`) and how many of its +1 Def / +1 SpD raises took (`effectState.def` / `.spd`,
    /// negated; kept in `hidden`: Def in bits 0–1, SpD in bits 2–3; not canonical fields), which
    /// its `onEnd` takes back. Spit Up and Swallow end it.
    Stockpile,
    /// Foresight / Odor Sleuth on their target (`foresight`, no duration, `noCopy`): a Ghost
    /// holder loses its immunity to Normal and Fighting moves (`onNegateImmunity`) and its
    /// positive evasion stages are ignored (`onModifyBoost`).
    Foresight,
    /// Miracle Eye on its target (`miracleeye`, no duration, `noCopy`): as Foresight, for a Dark
    /// holder against Psychic moves.
    MiracleEye,
    /// Defense Curl (`defensecurl`, no duration, `noCopy`; its `onRestart` returns `null`):
    /// Rollout and Ice Ball have double power.
    DefenseCurl,
    /// Rollout's own condition (`rollout`, duration 1, 2 again after each hit below the fifth):
    /// the holder is locked into Rollout (`onLockMove`) aimed at the target location it chose
    /// (`lastMoveTargetLoc`, kept in `counter` as `lock::encode_target_loc`); `hitCount` (=
    /// `contactHitCount`) in `hidden`. Neither is a canonical field. Added by name, so the dex has
    /// no condition id.
    Rollout,
    /// Ice Ball's own condition (`iceball`): as [`Volatile::Rollout`].
    IceBall,
    /// Gastro Acid (`gastroacid`, no duration): the holder's ability is suppressed
    /// (`Pokemon.ignoringAbility`, `abilities::ignoring_ability`) unless it is `cantsuppress`.
    GastroAcid,
    /// Not a Showdown volatile: Neutralizing Gas's `abilityState.ending` (its `onEnd` ran: the
    /// holder no longer suppresses other abilities while it stays, and a second `End` does
    /// nothing). No duration; hidden in the canonical state.
    NeutralizingGasEnding,
    /// Not a Showdown volatile: Slow Start's `abilityState.counter` (5 from its `onStart`, one
    /// less at each residual of a turn the holder was active from the start; gone at 0), kept in
    /// `counter`. No duration; hidden in the canonical state.
    SlowStart,
    /// Truant's own condition (`truant`, no duration, no handlers): the holder loafs at its next
    /// move attempt (Truant's `onBeforeMove` removes it and stops the move, or adds it).
    Truant,
    /// Not a Showdown volatile: Cud Chew's `abilityState.berry` (the berry's `ItemId` in
    /// `counter`) and `.counter` (in `hidden`): the berry is eaten again when the counter runs
    /// out at a residual. No duration; hidden in the canonical state.
    CudChew,
    /// Not a Showdown volatile: Ripen's `abilityState.berryWeaken` (the last berry it ate was a
    /// resist berry: the holder's next hit taken is halved once more). No duration; hidden in
    /// the canonical state.
    RipenWeaken,
    /// Not a Showdown volatile: Opportunist's `effectState.boosts`, the foes' raises it copied
    /// and has not used yet: 4 bits per stat (0..=12, more cannot change a stage), Atk..SpD in
    /// `counter`, Spe and accuracy in `hidden`, evasion in `time`
    /// (`abilities::opportunist_boosts`). No duration; hidden in the canonical state.
    Opportunist,
}

pub const VOLATILE_COUNT: usize = 101;

impl Volatile {
    pub const ALL: [Volatile; VOLATILE_COUNT] = [
        Volatile::Protect,
        Volatile::Stall,
        Volatile::Flinch,
        Volatile::FollowMe,
        Volatile::RagePowder,
        Volatile::Spotlight,
        Volatile::Confusion,
        Volatile::LockedMove,
        Volatile::MustRecharge,
        Volatile::Encore,
        Volatile::FlashFire,
        Volatile::ChoiceLock,
        Volatile::Roost,
        Volatile::Yawn,
        Volatile::PerishSong,
        Volatile::Endure,
        Volatile::ProteanUsed,
        Volatile::Charge,
        Volatile::AngerShellUnchecked,
        Volatile::Unburden,
        Volatile::FocusEnergy,
        Volatile::MicleBerry,
        Volatile::HelpingHand,
        Volatile::Taunt,
        Volatile::Disable,
        Volatile::Torment,
        Volatile::Imprison,
        Volatile::GlaiveRush,
        Volatile::SparklingAria,
        Volatile::Protosynthesis,
        Volatile::QuarkDrive,
        Volatile::ThroatChop,
        Volatile::SpikyShield,
        Volatile::BanefulBunker,
        Volatile::KingsShield,
        Volatile::Obstruct,
        Volatile::SilkTrap,
        Volatile::BurningBulwark,
        Volatile::NoRetreat,
        Volatile::LeechSeed,
        Volatile::PartiallyTrapped,
        Volatile::DestinyBond,
        Volatile::TwoTurnMove,
        Volatile::SolarBeam,
        Volatile::SolarBlade,
        Volatile::MeteorBeam,
        Volatile::ElectroShot,
        Volatile::SkyAttack,
        Volatile::Fly,
        Volatile::Bounce,
        Volatile::Dig,
        Volatile::Dive,
        Volatile::PhantomForce,
        Volatile::ShadowForce,
        Volatile::Substitute,
        Volatile::ZenMode,
        Volatile::AllySwitch,
        Volatile::Trapped,
        Volatile::Trapper,
        Volatile::SaltCure,
        Volatile::Ingrain,
        Volatile::MagnetRise,
        Volatile::Counter,
        Volatile::MirrorCoat,
        Volatile::FocusPunch,
        Volatile::BeakBlast,
        Volatile::ShellTrap,
        Volatile::HealBlock,
        Volatile::SmackDown,
        Volatile::SupremeOverlord,
        Volatile::Commanding,
        Volatile::Commanded,
        Volatile::GorillaTactics,
        Volatile::Attract,
        Volatile::EjectPack,
        Volatile::Metronome,
        Volatile::Nightmare,
        Volatile::Octolock,
        Volatile::DragonCheer,
        Volatile::LaserFocus,
        Volatile::AquaRing,
        Volatile::PowerTrick,
        Volatile::PowerShift,
        Volatile::SkullBash,
        Volatile::RazorWind,
        Volatile::FreezeShock,
        Volatile::IceBurn,
        Volatile::Geomancy,
        Volatile::Stockpile,
        Volatile::Foresight,
        Volatile::MiracleEye,
        Volatile::DefenseCurl,
        Volatile::Rollout,
        Volatile::IceBall,
        Volatile::GastroAcid,
        Volatile::NeutralizingGasEnding,
        Volatile::SlowStart,
        Volatile::Truant,
        Volatile::CudChew,
        Volatile::RipenWeaken,
        Volatile::Opportunist,
    ];

    /// The Showdown condition this volatile is. `ConditionId::NONE` for a volatile that is an
    /// ability's own `condition` (Flash Fire), which the dex does not export as a named
    /// condition: no move data can refer to it.
    pub fn condition(self) -> ConditionId {
        match self {
            Volatile::Protect => conditions::PROTECT,
            Volatile::Stall => conditions::STALL,
            Volatile::Flinch => conditions::FLINCH,
            Volatile::FollowMe => conditions::FOLLOWME,
            Volatile::RagePowder => conditions::RAGEPOWDER,
            Volatile::Spotlight => conditions::SPOTLIGHT,
            Volatile::Confusion => conditions::CONFUSION,
            Volatile::LockedMove => conditions::LOCKEDMOVE,
            Volatile::MustRecharge => conditions::MUSTRECHARGE,
            Volatile::Encore => conditions::ENCORE,
            Volatile::FlashFire
            | Volatile::Unburden
            | Volatile::Protosynthesis
            | Volatile::QuarkDrive
            | Volatile::ZenMode => ConditionId::NONE,
            Volatile::ChoiceLock => conditions::CHOICELOCK,
            Volatile::Roost => conditions::ROOST,
            Volatile::Yawn => conditions::YAWN,
            Volatile::Endure => conditions::ENDURE,
            Volatile::Charge => conditions::CHARGE,
            Volatile::FocusEnergy => conditions::FOCUSENERGY,
            Volatile::HelpingHand => conditions::HELPINGHAND,
            Volatile::Taunt => conditions::TAUNT,
            Volatile::Disable => conditions::DISABLE,
            Volatile::Torment => conditions::TORMENT,
            Volatile::Imprison => conditions::IMPRISON,
            Volatile::GlaiveRush => conditions::GLAIVERUSH,
            Volatile::SparklingAria => conditions::SPARKLINGARIA,
            Volatile::SpikyShield => conditions::SPIKYSHIELD,
            Volatile::BanefulBunker => conditions::BANEFULBUNKER,
            Volatile::KingsShield => conditions::KINGSSHIELD,
            Volatile::Obstruct => conditions::OBSTRUCT,
            Volatile::SilkTrap => conditions::SILKTRAP,
            Volatile::BurningBulwark => conditions::BURNINGBULWARK,
            Volatile::NoRetreat => conditions::NORETREAT,
            Volatile::LeechSeed => conditions::LEECHSEED,
            Volatile::PartiallyTrapped => conditions::PARTIALLYTRAPPED,
            Volatile::DestinyBond => conditions::DESTINYBOND,
            Volatile::TwoTurnMove => conditions::TWOTURNMOVE,
            Volatile::Substitute => conditions::SUBSTITUTE,
            Volatile::SaltCure => conditions::SALTCURE,
            Volatile::Ingrain => conditions::INGRAIN,
            Volatile::MagnetRise => conditions::MAGNETRISE,
            Volatile::HealBlock => conditions::HEALBLOCK,
            Volatile::SmackDown => conditions::SMACKDOWN,
            Volatile::Attract => conditions::ATTRACT,
            Volatile::Nightmare => conditions::NIGHTMARE,
            Volatile::Octolock => conditions::OCTOLOCK,
            Volatile::DragonCheer => conditions::DRAGONCHEER,
            Volatile::LaserFocus => conditions::LASERFOCUS,
            Volatile::AquaRing => conditions::AQUARING,
            Volatile::PowerTrick => conditions::POWERTRICK,
            Volatile::PowerShift => conditions::POWERSHIFT,
            Volatile::Stockpile => conditions::STOCKPILE,
            Volatile::Foresight => conditions::FORESIGHT,
            Volatile::MiracleEye => conditions::MIRACLEEYE,
            Volatile::DefenseCurl => conditions::DEFENSECURL,
            Volatile::Rollout | Volatile::IceBall => ConditionId::NONE,
            Volatile::GastroAcid => conditions::GASTROACID,
            // Micle Berry is an item's condition: the dex exports no named condition for it.
            Volatile::PerishSong
            | Volatile::ProteanUsed
            | Volatile::AngerShellUnchecked
            | Volatile::MicleBerry
            | Volatile::ThroatChop
            | Volatile::SolarBeam
            | Volatile::SolarBlade
            | Volatile::MeteorBeam
            | Volatile::ElectroShot
            | Volatile::SkyAttack
            | Volatile::Fly
            | Volatile::Bounce
            | Volatile::Dig
            | Volatile::Dive
            | Volatile::PhantomForce
            | Volatile::ShadowForce
            | Volatile::AllySwitch
            | Volatile::Trapped
            | Volatile::Trapper
            | Volatile::Counter
            | Volatile::MirrorCoat
            | Volatile::FocusPunch
            | Volatile::BeakBlast
            | Volatile::ShellTrap
            | Volatile::SupremeOverlord
            | Volatile::Commanding
            | Volatile::Commanded
            | Volatile::GorillaTactics
            | Volatile::EjectPack
            | Volatile::Metronome
            | Volatile::SkullBash
            | Volatile::RazorWind
            | Volatile::FreezeShock
            | Volatile::IceBurn
            | Volatile::Geomancy
            | Volatile::NeutralizingGasEnding
            | Volatile::SlowStart
            | Volatile::Truant
            | Volatile::CudChew
            | Volatile::RipenWeaken
            | Volatile::Opportunist => ConditionId::NONE,
        }
    }

    /// Showdown id, as written in canonical states.
    pub fn id(self) -> &'static str {
        match self {
            Volatile::Protect => "protect",
            Volatile::Stall => "stall",
            Volatile::Flinch => "flinch",
            Volatile::FollowMe => "followme",
            Volatile::RagePowder => "ragepowder",
            Volatile::Spotlight => "spotlight",
            Volatile::Confusion => "confusion",
            Volatile::LockedMove => "lockedmove",
            Volatile::MustRecharge => "mustrecharge",
            Volatile::Encore => "encore",
            Volatile::FlashFire => "flashfire",
            Volatile::ChoiceLock => "choicelock",
            Volatile::Roost => "roost",
            Volatile::Yawn => "yawn",
            Volatile::PerishSong => "perishsong",
            Volatile::Endure => "endure",
            Volatile::ProteanUsed => "protean",
            Volatile::Charge => "charge",
            Volatile::AngerShellUnchecked => "angershellunchecked",
            Volatile::Unburden => "unburden",
            Volatile::FocusEnergy => "focusenergy",
            Volatile::MicleBerry => "micleberry",
            Volatile::HelpingHand => "helpinghand",
            Volatile::Taunt => "taunt",
            Volatile::Disable => "disable",
            Volatile::Torment => "torment",
            Volatile::Imprison => "imprison",
            Volatile::GlaiveRush => "glaiverush",
            Volatile::SparklingAria => "sparklingaria",
            Volatile::Protosynthesis => "protosynthesis",
            Volatile::QuarkDrive => "quarkdrive",
            Volatile::ThroatChop => "throatchop",
            Volatile::SpikyShield => "spikyshield",
            Volatile::BanefulBunker => "banefulbunker",
            Volatile::KingsShield => "kingsshield",
            Volatile::Obstruct => "obstruct",
            Volatile::SilkTrap => "silktrap",
            Volatile::BurningBulwark => "burningbulwark",
            Volatile::NoRetreat => "noretreat",
            Volatile::LeechSeed => "leechseed",
            Volatile::PartiallyTrapped => "partiallytrapped",
            Volatile::DestinyBond => "destinybond",
            Volatile::TwoTurnMove => "twoturnmove",
            Volatile::SolarBeam => "solarbeam",
            Volatile::SolarBlade => "solarblade",
            Volatile::MeteorBeam => "meteorbeam",
            Volatile::ElectroShot => "electroshot",
            Volatile::SkyAttack => "skyattack",
            Volatile::Fly => "fly",
            Volatile::Bounce => "bounce",
            Volatile::Dig => "dig",
            Volatile::Dive => "dive",
            Volatile::PhantomForce => "phantomforce",
            Volatile::ShadowForce => "shadowforce",
            Volatile::Substitute => "substitute",
            Volatile::ZenMode => "zenmode",
            Volatile::AllySwitch => "allyswitch",
            Volatile::Trapped => "trapped",
            Volatile::Trapper => "trapper",
            Volatile::SaltCure => "saltcure",
            Volatile::Ingrain => "ingrain",
            Volatile::MagnetRise => "magnetrise",
            Volatile::Counter => "counter",
            Volatile::MirrorCoat => "mirrorcoat",
            Volatile::FocusPunch => "focuspunch",
            Volatile::BeakBlast => "beakblast",
            Volatile::ShellTrap => "shelltrap",
            Volatile::HealBlock => "healblock",
            Volatile::SmackDown => "smackdown",
            Volatile::SupremeOverlord => "supremeoverlord",
            Volatile::Commanding => "commanding",
            Volatile::Commanded => "commanded",
            Volatile::GorillaTactics => "gorillatactics",
            Volatile::Attract => "attract",
            Volatile::EjectPack => "ejectpack",
            Volatile::Metronome => "metronome",
            Volatile::Nightmare => "nightmare",
            Volatile::Octolock => "octolock",
            Volatile::DragonCheer => "dragoncheer",
            Volatile::LaserFocus => "laserfocus",
            Volatile::AquaRing => "aquaring",
            Volatile::PowerTrick => "powertrick",
            Volatile::PowerShift => "powershift",
            Volatile::SkullBash => "skullbash",
            Volatile::RazorWind => "razorwind",
            Volatile::FreezeShock => "freezeshock",
            Volatile::IceBurn => "iceburn",
            Volatile::Geomancy => "geomancy",
            Volatile::Stockpile => "stockpile",
            Volatile::Foresight => "foresight",
            Volatile::MiracleEye => "miracleeye",
            Volatile::DefenseCurl => "defensecurl",
            Volatile::Rollout => "rollout",
            Volatile::IceBall => "iceball",
            Volatile::GastroAcid => "gastroacid",
            Volatile::NeutralizingGasEnding => "neutralizinggasending",
            Volatile::SlowStart => "slowstartcounter",
            Volatile::Truant => "truant",
            Volatile::CudChew => "cudchewberry",
            Volatile::RipenWeaken => "berryweaken",
            Volatile::Opportunist => "opportunistboosts",
        }
    }

    /// The volatile implementing `condition`, if any (never for `NONE`).
    pub fn from_condition(condition: ConditionId) -> Option<Volatile> {
        if condition.is_none() {
            return None;
        }
        Volatile::ALL
            .into_iter()
            .find(|v| v.condition() == condition)
    }

    /// Duration a fresh instance starts with (0 = none).
    pub fn initial_duration(self) -> u8 {
        match self {
            Volatile::Protect
            | Volatile::Flinch
            | Volatile::FollowMe
            | Volatile::RagePowder
            | Volatile::Spotlight
            | Volatile::Roost
            | Volatile::Endure
            | Volatile::HelpingHand
            | Volatile::SpikyShield
            | Volatile::BanefulBunker
            | Volatile::KingsShield
            | Volatile::Obstruct
            | Volatile::SilkTrap
            | Volatile::BurningBulwark
            | Volatile::Counter
            | Volatile::MirrorCoat
            | Volatile::FocusPunch
            | Volatile::BeakBlast
            | Volatile::ShellTrap => 1,
            Volatile::Stall
            | Volatile::LockedMove
            | Volatile::MustRecharge
            | Volatile::Yawn
            | Volatile::MicleBerry
            | Volatile::ThroatChop
            | Volatile::TwoTurnMove
            | Volatile::Fly
            | Volatile::Bounce
            | Volatile::Dig
            | Volatile::Dive
            | Volatile::PhantomForce
            | Volatile::ShadowForce
            | Volatile::AllySwitch
            | Volatile::LaserFocus => 2,
            Volatile::Encore | Volatile::Taunt => 3,
            Volatile::PerishSong => 4,
            // Partial trapping's and Heal Block's `durationCallback` replace it when they start
            // (`conditions::volatile_start`).
            Volatile::Disable
            | Volatile::PartiallyTrapped
            | Volatile::MagnetRise
            | Volatile::HealBlock => 5,
            Volatile::Confusion
            | Volatile::FlashFire
            | Volatile::ChoiceLock
            | Volatile::ProteanUsed
            | Volatile::Charge
            | Volatile::AngerShellUnchecked
            | Volatile::Unburden
            | Volatile::FocusEnergy
            | Volatile::Torment
            | Volatile::Imprison
            | Volatile::GlaiveRush
            | Volatile::SparklingAria
            | Volatile::Protosynthesis
            | Volatile::QuarkDrive
            | Volatile::NoRetreat
            | Volatile::LeechSeed
            | Volatile::DestinyBond
            | Volatile::SolarBeam
            | Volatile::SolarBlade
            | Volatile::MeteorBeam
            | Volatile::ElectroShot
            | Volatile::SkyAttack
            | Volatile::Substitute
            | Volatile::Trapped
            | Volatile::Trapper
            | Volatile::SaltCure
            | Volatile::Ingrain
            | Volatile::SmackDown
            | Volatile::SupremeOverlord
            | Volatile::Commanding
            | Volatile::Commanded
            | Volatile::GorillaTactics
            | Volatile::Attract
            | Volatile::EjectPack
            | Volatile::Metronome
            | Volatile::Nightmare
            | Volatile::Octolock
            | Volatile::DragonCheer
            | Volatile::AquaRing
            | Volatile::PowerTrick
            | Volatile::PowerShift
            | Volatile::SkullBash
            | Volatile::RazorWind
            | Volatile::FreezeShock
            | Volatile::IceBurn
            | Volatile::Geomancy
            | Volatile::Stockpile
            | Volatile::Foresight
            | Volatile::MiracleEye
            | Volatile::DefenseCurl
            | Volatile::GastroAcid
            | Volatile::NeutralizingGasEnding
            | Volatile::SlowStart
            | Volatile::Truant
            | Volatile::CudChew
            | Volatile::RipenWeaken
            | Volatile::Opportunist => 0,
            Volatile::Rollout | Volatile::IceBall => 1,
            Volatile::ZenMode => 0,
        }
    }

    /// The condition's `onResidualOrder` (`None`: Showdown's default, after every ordered
    /// handler). Its duration is counted down by that residual handler.
    pub fn residual_order(self) -> Option<u32> {
        match self {
            Volatile::Ingrain => Some(7),
            Volatile::LeechSeed => Some(8),
            Volatile::PartiallyTrapped | Volatile::SaltCure => Some(13),
            Volatile::MagnetRise => Some(18),
            Volatile::AquaRing => Some(6),
            Volatile::Nightmare => Some(11),
            Volatile::Octolock => Some(14),
            Volatile::Taunt => Some(15),
            Volatile::Encore => Some(16),
            Volatile::Disable => Some(17),
            Volatile::HealBlock => Some(20),
            Volatile::ThroatChop => Some(22),
            Volatile::Yawn => Some(23),
            Volatile::PerishSong => Some(24),
            Volatile::Roost => Some(25),
            _ => None,
        }
    }

    /// What Showdown's `pokemon.volatiles` holds for this kind: `None` for engine-only kinds,
    /// and the effect state without engine-only payload (Roost's saved types, Helping Hand's
    /// application count).
    pub fn showdown_state(self, state: VolatileState) -> Option<VolatileState> {
        match self {
            Volatile::ProteanUsed
            | Volatile::AngerShellUnchecked
            | Volatile::SupremeOverlord
            | Volatile::GorillaTactics
            | Volatile::EjectPack
            | Volatile::NeutralizingGasEnding
            | Volatile::SlowStart
            | Volatile::CudChew
            | Volatile::RipenWeaken
            | Volatile::Opportunist => None,
            // Two-turn move: the target location is not a canonical field.
            Volatile::Roost
            | Volatile::HelpingHand
            | Volatile::LeechSeed
            | Volatile::TwoTurnMove
            | Volatile::Trapped
            | Volatile::Trapper
            | Volatile::Attract
            | Volatile::Octolock => Some(VolatileState {
                counter: 0,
                ..state
            }),
            // Metronome: `lastMove` and `numConsecutive` are not canonical fields.
            Volatile::Metronome => Some(VolatileState {
                counter: 0,
                mv: MoveId::NONE,
                ..state
            }),
            // `bestStat` / `fromBooster` (Protosynthesis, Quark Drive) and the trapper /
            // `boundDivisor` (partial trapping) are not canonical fields.
            // Counter / Mirror Coat: neither `damage` nor `slot` is a canonical field; nor are
            // Focus Punch's `lostFocus` and Shell Trap's `gotHit`.
            Volatile::Protosynthesis
            | Volatile::QuarkDrive
            | Volatile::PartiallyTrapped
            | Volatile::Counter
            | Volatile::MirrorCoat
            | Volatile::FocusPunch
            | Volatile::ShellTrap
            | Volatile::Rollout
            | Volatile::IceBall => Some(VolatileState {
                counter: 0,
                hidden: 0,
                ..state
            }),
            _ => Some(state),
        }
    }
}

/// A slot in one `counter` (Leech Seed's `sourceSlot`): never 0.
pub fn encode_slot(slot: SlotRef) -> u16 {
    1 + (slot.side.index() as u16) * 256 + u16::from(slot.slot)
}

/// The slot [`encode_slot`] stored.
pub fn decode_slot(counter: u16) -> SlotRef {
    let value = counter - 1;
    SlotRef {
        side: if value >= 256 {
            SideId::Two
        } else {
            SideId::One
        },
        slot: (value % 256) as u8,
    }
}

/// A party member in one `counter` (partial trapping's `source`): never 0.
pub fn encode_pokemon(pokemon: PokemonRef) -> u16 {
    1 + (pokemon.side.index() as u16) * 256 + u16::from(pokemon.party)
}

/// The party member [`encode_pokemon`] stored.
pub fn decode_pokemon(counter: u16) -> PokemonRef {
    let slot = decode_slot(counter);
    PokemonRef {
        side: slot.side,
        party: slot.slot,
    }
}

/// Two types in one `counter` (first type in the high byte).
pub fn encode_types(types: [Type; 2]) -> u16 {
    (u16::from(types[0] as u8) << 8) | u16::from(types[1] as u8)
}

/// The types [`encode_types`] stored.
pub fn decode_types(counter: u16) -> [Type; 2] {
    // `Type::ALL` plus Double Shock's `???` (`Type::Unknown`, not in `ALL`).
    let decode = |v: u16| {
        Type::ALL
            .into_iter()
            .chain([Type::Unknown])
            .find(|&t| u16::from(t as u8) == v)
            .unwrap_or(Type::None)
    };
    [decode(counter >> 8), decode(counter & 0xff)]
}

/// One volatile's state: Showdown's effect-state fields the canonical output writes
/// (`duration`, `counter`, `time`, `move`, 0/none = unset) plus `hidden` for state Showdown
/// keeps but does not print (a locked move's `trueDuration`; written as `trueDuration` since it
/// decides later outcomes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VolatileState {
    pub active: bool,
    pub duration: u8,
    pub counter: u16,
    pub time: u8,
    pub mv: MoveId,
    pub hidden: u8,
}

impl VolatileState {
    pub const NONE: VolatileState = VolatileState {
        active: false,
        duration: 0,
        counter: 0,
        time: 0,
        mv: MoveId::NONE,
        hidden: 0,
    };
}

/// All volatiles of one slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Volatiles(pub [VolatileState; VOLATILE_COUNT]);

/// Manual: `Default` is only derived for arrays of up to 32 elements.
impl Default for Volatiles {
    fn default() -> Self {
        Volatiles([VolatileState::NONE; VOLATILE_COUNT])
    }
}

impl Volatiles {
    pub fn get(&self, volatile: Volatile) -> VolatileState {
        self.0[volatile as usize]
    }

    pub fn set(&mut self, volatile: Volatile, state: VolatileState) {
        self.0[volatile as usize] = state;
    }

    pub fn has(&self, volatile: Volatile) -> bool {
        self.0[volatile as usize].active
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|v| !v.active)
    }

    /// Active volatiles with their state, in [`Volatile::ALL`] order.
    pub fn iter(&self) -> impl Iterator<Item = (Volatile, VolatileState)> + '_ {
        Volatile::ALL
            .into_iter()
            .map(|v| (v, self.get(v)))
            .filter(|(_, s)| s.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::moves;

    #[test]
    fn ids_match_the_dex_conditions() {
        assert_eq!(Volatile::from_condition(ConditionId::NONE), None);
        for v in Volatile::ALL {
            if v.condition().is_none() {
                // Only kinds the dex never names: an ability's own condition (Flash Fire), Perish
                // Song (added by name) and engine state.
                assert!(matches!(
                    v,
                    Volatile::FlashFire
                        | Volatile::Unburden
                        | Volatile::Protosynthesis
                        | Volatile::QuarkDrive
                        | Volatile::PerishSong
                        | Volatile::ProteanUsed
                        | Volatile::AngerShellUnchecked
                        | Volatile::MicleBerry
                        | Volatile::ThroatChop
                        | Volatile::SolarBeam
                        | Volatile::SolarBlade
                        | Volatile::MeteorBeam
                        | Volatile::ElectroShot
                        | Volatile::SkyAttack
                        | Volatile::Fly
                        | Volatile::Bounce
                        | Volatile::Dig
                        | Volatile::Dive
                        | Volatile::PhantomForce
                        | Volatile::ShadowForce
                        | Volatile::ZenMode
                        | Volatile::AllySwitch
                        | Volatile::Trapped
                        | Volatile::Trapper
                        | Volatile::Counter
                        | Volatile::MirrorCoat
                        | Volatile::FocusPunch
                        | Volatile::BeakBlast
                        | Volatile::ShellTrap
                        | Volatile::SupremeOverlord
                        | Volatile::Commanding
                        | Volatile::Commanded
                        | Volatile::GorillaTactics
                        | Volatile::EjectPack
                        | Volatile::Metronome
                        | Volatile::SkullBash
                        | Volatile::RazorWind
                        | Volatile::FreezeShock
                        | Volatile::IceBurn
                        | Volatile::Geomancy
                        | Volatile::Rollout
                        | Volatile::IceBall
                        | Volatile::NeutralizingGasEnding
                        | Volatile::SlowStart
                        | Volatile::Truant
                        | Volatile::CudChew
                        | Volatile::RipenWeaken
                        | Volatile::Opportunist
                ));
                continue;
            }
            assert_eq!(v.condition().id(), v.id());
            assert_eq!(Volatile::from_condition(v.condition()), Some(v));
        }
        assert_eq!(Volatile::from_condition(ConditionId::NONE), None);
        assert_eq!(
            Volatile::ProteanUsed.showdown_state(VolatileState::NONE),
            None
        );
    }

    /// Durations and residual orders are the dex's (the move's `condition`).
    #[test]
    fn durations_and_residual_orders_match_the_moves() {
        for (volatile, id) in [
            (Volatile::Roost, moves::ROOST),
            (Volatile::Yawn, moves::YAWN),
            (Volatile::PerishSong, moves::PERISH_SONG),
            (Volatile::HelpingHand, moves::HELPING_HAND),
            (Volatile::Taunt, moves::TAUNT),
            (Volatile::Disable, moves::DISABLE),
            (Volatile::ThroatChop, moves::THROAT_CHOP),
            (Volatile::SpikyShield, moves::SPIKY_SHIELD),
            (Volatile::BanefulBunker, moves::BANEFUL_BUNKER),
            (Volatile::KingsShield, moves::KINGS_SHIELD),
            (Volatile::Obstruct, moves::OBSTRUCT),
            (Volatile::SilkTrap, moves::SILK_TRAP),
            (Volatile::BurningBulwark, moves::BURNING_BULWARK),
            (Volatile::LeechSeed, moves::LEECH_SEED),
            (Volatile::Substitute, moves::SUBSTITUTE),
            (Volatile::AllySwitch, moves::ALLY_SWITCH),
            (Volatile::SaltCure, moves::SALT_CURE),
            (Volatile::Ingrain, moves::INGRAIN),
            (Volatile::MagnetRise, moves::MAGNET_RISE),
            (Volatile::Counter, moves::COUNTER),
            (Volatile::MirrorCoat, moves::MIRROR_COAT),
            (Volatile::FocusPunch, moves::FOCUS_PUNCH),
            (Volatile::BeakBlast, moves::BEAK_BLAST),
            (Volatile::ShellTrap, moves::SHELL_TRAP),
            (Volatile::HealBlock, moves::HEAL_BLOCK),
            (Volatile::SmackDown, moves::SMACK_DOWN),
            (Volatile::Nightmare, moves::NIGHTMARE),
            (Volatile::Octolock, moves::OCTOLOCK),
            (Volatile::DragonCheer, moves::DRAGON_CHEER),
            (Volatile::LaserFocus, moves::LASER_FOCUS),
            (Volatile::AquaRing, moves::AQUA_RING),
            (Volatile::PowerTrick, moves::POWER_TRICK),
            (Volatile::PowerShift, moves::POWER_SHIFT),
            (Volatile::Stockpile, moves::STOCKPILE),
            (Volatile::Foresight, moves::FORESIGHT),
            (Volatile::MiracleEye, moves::MIRACLE_EYE),
            (Volatile::DefenseCurl, moves::DEFENSE_CURL),
            (Volatile::Rollout, moves::ROLLOUT),
            (Volatile::IceBall, moves::ICE_BALL),
        ] {
            let data = id.data();
            assert_eq!(
                data.condition_duration,
                volatile.initial_duration(),
                "{id:?}"
            );
            match volatile.residual_order() {
                Some(order) => assert!(
                    data.event_orders
                        .contains(&("condition.onResidualOrder", order as i16)),
                    "{id:?}"
                ),
                None => assert!(
                    !data
                        .event_orders
                        .iter()
                        .any(|(n, _)| *n == "condition.onResidualOrder"),
                    "{id:?}"
                ),
            }
        }
    }

    /// Micle Berry's condition lasts 2 turns; Focus Energy's has no duration.
    #[test]
    fn item_condition_durations_match_the_dex() {
        use crate::dex::items;
        assert_eq!(
            items::MICLE_BERRY.data().condition_duration,
            Volatile::MicleBerry.initial_duration()
        );
        assert_eq!(moves::FOCUS_ENERGY.data().condition_duration, 0);
        assert_eq!(Volatile::FocusEnergy.initial_duration(), 0);
    }

    /// Partial trapping is a named condition (`data/conditions.ts`): its duration, residual order
    /// and handler list are the implemented ones (`conditions::volatile_start`, `residual`,
    /// `conditions::trapped`; `onEnd` only logs).
    #[test]
    fn partial_trapping_matches_its_condition() {
        let data = conditions::PARTIALLYTRAPPED.data();
        assert_eq!(data.duration, Volatile::PartiallyTrapped.initial_duration());
        assert_eq!(data.event_orders, [("onResidualOrder", 13)]);
        assert_eq!(
            Volatile::PartiallyTrapped.residual_order(),
            Some(13),
            "residual order"
        );
        assert_eq!(
            data.handlers,
            [
                "durationCallback",
                "onEnd",
                "onResidual",
                "onStart",
                "onTrapPokemon"
            ]
        );
    }

    #[test]
    fn slots_and_pokemon_round_trip() {
        for side in [SideId::One, SideId::Two] {
            for index in 0..6 {
                let slot = SlotRef { side, slot: index };
                assert_eq!(decode_slot(encode_slot(slot)), slot);
                assert_ne!(encode_slot(slot), 0);
                let pokemon = PokemonRef { side, party: index };
                assert_eq!(decode_pokemon(encode_pokemon(pokemon)), pokemon);
            }
        }
    }

    #[test]
    fn types_round_trip() {
        for types in [
            [Type::Flying, Type::None],
            [Type::Normal, Type::Flying],
            [Type::Steel, Type::Flying],
            [Type::Water, Type::Stellar],
            [Type::Unknown, Type::Flying],
        ] {
            assert_eq!(decode_types(encode_types(types)), types);
            assert_ne!(encode_types(types), 0);
        }
    }
}
