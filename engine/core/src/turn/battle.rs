//! The mutable battle context of one turn run and its primitive operations (Showdown's
//! `battle.damage`, `heal`, `setStatus`, `addVolatile`, `faintMessages`, `checkWin`, ...).
//!
//! Every state change goes through [`Battle::apply`], which records the reversible
//! instruction. Transient per-turn data that Showdown keeps on objects but that does not
//! survive the turn (the faint queue, what moved) lives here, not in `State`.

use crate::dex::{
    abilities, conditions, items, AbilityFlags, AbilityId, ItemId, MoveCategory, MoveFlags, MoveId,
    Type, TypeImmunities, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{
    BattleResult, Pokemon, PokemonRef, SideId, SlotRef, State, Status, SwitchFlag, BOOST_COUNT,
};
use crate::volatile::{Volatile, VolatileState};

use super::branch::Chooser;
use super::TurnError;

/// What caused a loss of HP; decides which Damage handlers apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DamageSource {
    /// Direct damage of a move (Showdown effect type `Move`).
    Move,
    /// Recoil of a move (`recoil` array; Showdown effect id `recoil`), not Struggle's.
    Recoil,
    /// Everything else: weather, status, items (Life Orb), abilities.
    Indirect,
}

/// Active positions in order, kept inline: what [`Battle::alive_slots`] and
/// [`Battle::all_alive`] return. Every event walks them, and as `Vec`s their heap allocations
/// were a large share of a turn's cost (Opus GG). Derefs to a slice; iterates by value like a
/// `Vec`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotList {
    len: u8,
    slots: [SlotRef; SlotList::CAPACITY],
}

impl SlotList {
    /// Both sides' positions for up to three active Pokémon a side.
    pub const CAPACITY: usize = 6;
    const FILLER: SlotRef = SlotRef {
        side: SideId::One,
        slot: 0,
    };

    pub fn new() -> SlotList {
        SlotList {
            len: 0,
            slots: [Self::FILLER; Self::CAPACITY],
        }
    }

    pub fn push(&mut self, slot: SlotRef) {
        self.slots[usize::from(self.len)] = slot;
        self.len += 1;
    }
}

impl Default for SlotList {
    fn default() -> Self {
        SlotList::new()
    }
}

impl std::ops::Deref for SlotList {
    type Target = [SlotRef];

    fn deref(&self) -> &[SlotRef] {
        &self.slots[..usize::from(self.len)]
    }
}

impl std::ops::DerefMut for SlotList {
    fn deref_mut(&mut self) -> &mut [SlotRef] {
        &mut self.slots[..usize::from(self.len)]
    }
}

impl IntoIterator for SlotList {
    type Item = SlotRef;
    type IntoIter = std::iter::Take<std::array::IntoIter<SlotRef, { SlotList::CAPACITY }>>;

    fn into_iter(self) -> Self::IntoIter {
        self.slots.into_iter().take(usize::from(self.len))
    }
}

impl<'a> IntoIterator for &'a SlotList {
    type Item = &'a SlotRef;
    type IntoIter = std::slice::Iter<'a, SlotRef>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Extend<SlotRef> for SlotList {
    fn extend<I: IntoIterator<Item = SlotRef>>(&mut self, iter: I) {
        for slot in iter {
            self.push(slot);
        }
    }
}

impl FromIterator<SlotRef> for SlotList {
    fn from_iter<I: IntoIterator<Item = SlotRef>>(iter: I) -> Self {
        let mut out = SlotList::new();
        out.extend(iter);
        out
    }
}

impl From<SlotList> for Vec<SlotRef> {
    fn from(list: SlotList) -> Vec<SlotRef> {
        list.to_vec()
    }
}

/// The move being used (Showdown `activeMove` with `activePokemon`), set for the whole of
/// `runMove` and the action's phazing step after it; it decides whether breakable abilities are
/// suppressed (`suppressingAbility`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveMoveRef {
    pub user: SlotRef,
    pub pokemon: PokemonRef,
    pub id: MoveId,
    /// `activeMove.ignoreAbility`: the move's data flag (Sunsteel Strike), set by the user's
    /// Mold Breaker / Teravolt / Turboblaze in ModifyMove.
    pub ignore_ability: bool,
    /// `activeMove.category`: the move's own, or the one its ModifyMove chose (Photon Geyser,
    /// Shell Side Arm).
    pub category: MoveCategory,
    /// `activeMove.infiltrates`, set by the user's Infiltrator in ModifyMove: the move goes
    /// through a substitute, the screens, Safeguard and Mist (Pollen Puff's own, on an ally, is
    /// `handlers::infiltrates`).
    pub infiltrates: bool,
    /// `move.multihit` set by the user's Parental Bond in PrepareHit (Anger Shell and Berserk
    /// read `effect.multihit`).
    pub parental_bond: bool,
}

pub(crate) struct Battle<'a, const N: usize> {
    pub state: &'a mut State<N>,
    pub log: Vec<Instruction>,
    pub rng: &'a mut Chooser,
    /// Showdown `faintQueue`: Pokémon at 0 HP not yet processed, in the order they fell, with
    /// the Pokémon whose move's damage knocked them out (`faintData.source` when
    /// `faintData.effect` is a move; `None` otherwise), which Destiny Bond reads.
    faint_queue: Vec<(PokemonRef, SlotRef, Option<PokemonRef>)>,
    /// The move in progress, if any: Showdown `activeMove`, cleared by a failed move
    /// (`clearActiveMove(true)`) or after the action's phazing step (`clearActiveMove()`, the
    /// turn engine's `drag_outs`).
    pub active_move: Option<ActiveMoveRef>,
    /// The actions of the turn not yet run (Showdown `queue.list`), see `queue.rs`.
    pub queue: Vec<super::queue::Action>,
    /// The battle start's switch-ins are running (`enumerate_start`): no Pokémon has been on
    /// the field before, so the once-per-battle flags Showdown keeps on each Pokémon
    /// (`swordBoost`, `shieldBoost`, `syrupTriggered`), which the state does not record, are all
    /// unset.
    pub battle_start: bool,
    /// Showdown `target.getMoveHitData(move).typeMod` of the hit in progress, per side and
    /// slot: set by `getDamage` (`modifyDamage`) for every target it computes damage for,
    /// cleared at the start of every hit (`None`: not computed, as for fixed-damage and
    /// status moves). Read by Weakness Policy and Enigma Berry.
    pub hit_type_mod: [[Option<i8>; N]; 2],
    /// Showdown `target.getMoveHitData(move).crit` of the hit in progress, per side and slot:
    /// set by `getDamage` for every target it computes damage for, cleared with
    /// [`Battle::hit_type_mod`]. Read by Anger Point's `onHit`.
    pub hit_crit: [[bool; N]; 2],
    /// Mirror Herb's `effectState.boosts` per holder: the foes' raises it copied and has not
    /// used yet (`ready`). Showdown keeps them on the item across events; the engine keeps
    /// them only within a stage and refuses a stage that ends with one pending
    /// (`items::stage_end_check`).
    pub mirror_herb: Vec<(PokemonRef, [i8; BOOST_COUNT])>,
    /// Pokémon whose item state Utility Umbrella's End marked `inactive` (`takeItem` keeps the
    /// cleared state as `pokemon.itemState` until the next `setItem`): an umbrella given back
    /// silently then runs WeatherChange again at its next `onUpdate` (`items::umbrella_update`).
    /// Kept within a stage; a stage that ends with a living umbrella holder still marked is
    /// refused (`items::stage_end_check`).
    pub umbrella_inactive: Vec<PokemonRef>,
    /// Whether the move in flight switches its user out (`move.selfSwitch`); Parting Shot's
    /// `onHit` withdraws it (`delete move.selfSwitch`) when its drops failed.
    pub move_self_switch: bool,
    /// Showdown `forceSwitchFlag`: Pokémon a phazing move or Red Card drags out right after
    /// the action (`dragIn`, a uniformly random bench member), within the stage.
    pub force_switch: Vec<SlotRef>,
    /// Disguise's and Ice Face's `abilityState.busted` (`forme::absorbs_damage`): Pokémon whose
    /// ability absorbed a move's damage and changes forme at the next Update
    /// (`forme::on_update`), which always comes within the same stage.
    pub busted: Vec<PokemonRef>,
    /// Which damage-history fields anything in this battle can read (F18): fields nobody reads
    /// are not recorded, so positions that differ only in them merge (`history.rs`).
    pub history_readers: HistoryReaders,
    /// Pokémon whose species changed during the current action (Stance Change, Disguise, Mega
    /// Evolution, Transform, ...) with the `pokemon.speed` Showdown's `setSpecies` gave them (the
    /// raw stored Speed at that moment) until the next `updateSpeed()`, which comes after the
    /// action ([`Battle::event_speed`]). A stage is one action, so this starts empty with every
    /// stage; a multi-hit move suspended between hits carries it in its `MoveProgress`.
    pub raw_speed: Vec<(PokemonRef, i32)>,
    /// Showdown `pokemon.speed` of each active at the start of this stage (`updateSpeed()`
    /// between actions, at the residual and at the turn start): the Speed that sorts event
    /// handlers for the rest of the action ([`Battle::event_speed`]). A newcomer of this stage
    /// has no entry and is read live. Carried across a suspended action by `MoveProgress`.
    pub speed_snapshot: Vec<(PokemonRef, i32)>,
    /// A Pokémon switched in (`switching::switch_in`) and its `runSwitch` has not run yet:
    /// Showdown's `queue.peek()` is a `runSwitch` action (the Update that ends a switch action,
    /// or a batch of `instaswitch` actions, comes before it). Commander's `onUpdate` waits.
    pub awaiting_run_switch: bool,
    /// The Pokémon whose `runSwitch` has not run yet (the same window as
    /// [`Battle::awaiting_run_switch`], per Pokémon): their abilities have not started, so a
    /// handler that only acts once started (Unnerve's `effectState.unnerved`) ignores them.
    pub unstarted: Vec<PokemonRef>,
    /// Showdown's `queue.peek()` is empty: the turn's actions are all done (the residual action
    /// and what follows it in the same stage, or the `runSwitch` of a replacement batch or of a
    /// mid-turn switch batch requested after the residual). Cud Chew's `onEatItem` reads it.
    pub queue_done: bool,
    /// The position a future move's benched user stands in for its hit (`moves::AbsentUser`):
    /// inactive in Showdown (`source.isActive` is false), which Red Card checks.
    pub absent_user: Option<SlotRef>,
    /// The move in progress is external (`move.isExternal`: Dancer's copy): no Pressure PP, and
    /// no Dancer after it.
    pub external_move: bool,
    /// The move another move's handler called (`moves::call_move`: Sleep Talk, Copycat, Mirror
    /// Move, Nature Power) as it ended, until the caller's `runMove` takes it: Showdown's
    /// `if (this.battle.activeMove) move = this.battle.activeMove;` after `useMove`, so the
    /// AfterMove events (the move's own, Charge's) see the called move.
    pub(crate) called_move: Option<super::moves::ActiveMove>,
    /// A multi-hit move Copycat or Sleep Talk called (`moves::call_move`) that suspended after its
    /// first hit, until the caller's hit loop takes it and suspends with it (R14). `None` at the
    /// end of every stage.
    pub(crate) called_suspension: Option<super::moves::MoveProgress>,
    /// Showdown `battle.activeTarget` of the move that just ran (the (redirected) target it was
    /// used at; its user for a self-targeting move) with `useMove`'s result
    /// (`moveDidSomething`); `None` until `useMove` got that far. Dancer reads both.
    pub active_target: Option<(SlotRef, bool)>,
    /// Whether an ability can be suppressed in this battle (`abilities::suppression_possible`):
    /// without it [`Battle::ability`] skips the `ignoringAbility` check.
    pub suppression: bool,
}

/// The context [`Battle::new`] derives from the state before a run (see [`Battle::replay`]).
#[derive(Clone, Debug)]
pub(crate) struct RunStart {
    history_readers: HistoryReaders,
    suppression: bool,
    speed_snapshot: Vec<(PokemonRef, i32)>,
}

/// Buffers a finished run hands to the next ([`Battle::into_buffers`], [`Battle::replay`]).
#[derive(Debug, Default)]
pub(crate) struct RunBuffers {
    /// The finished run's instructions (the next run clears them).
    pub log: Vec<Instruction>,
    speed_snapshot: Vec<(PokemonRef, i32)>,
}

/// The readers of the hidden damage history present in a battle (any party member's moves;
/// see `history::record_attack`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryReaders {
    /// Metal Burst, Comeuppance (`lastDamagedBy`).
    pub last_damaged_by: bool,
    /// Rage Fist (`timesAttacked`).
    pub times_attacked: bool,
    /// Stomping Tantrum, Temper Flare, the Metronome item (`moveLastTurnResult`).
    pub move_last_turn_result: bool,
    /// Retaliate (`faintedLastTurn`).
    pub fainted_last_turn: bool,
    /// Burning Jealousy, Alluring Voice (`statsRaisedThisTurn`).
    pub stats_raised: bool,
    /// Lash Out (`statsLoweredThisTurn`).
    pub stats_lowered: bool,
    /// Belch (`ateBerry`).
    pub ate_berry: bool,
    /// Last Resort (`moveSlot.used`).
    pub moves_used: bool,
    /// Copycat (`battle.lastMove`: `State::last_move`).
    pub last_move: bool,
    /// Instruct (`lastMoveTargetLoc`: `Slot::last_move_target_loc`).
    pub last_move_target_loc: bool,
    /// Pickup (`usedItemThisTurn`).
    pub used_item: bool,
    /// A redirection tie (`abilityState.effectOrder`: `Slot::ability_order`): Follow Me, Rage
    /// Powder, Spotlight, Lightning Rod or Storm Drain, which only come from a party (moves are
    /// only copied from Pokémon in the battle, abilities only move between them), or a Mega
    /// forme with one of the abilities that a Mega Stone in the battle gives.
    pub ability_order: bool,
    /// Beat Up (`side.pokemon` order: `Side::party_order`), which only comes from a party's
    /// moves (Transform copies a Pokémon in the battle; Metronome and Assist are `Past`).
    pub party_order: bool,
}

impl HistoryReaders {
    pub fn of<const N: usize>(state: &State<N>) -> HistoryReaders {
        let mut readers = HistoryReaders::default();
        for side in &state.sides {
            for mon in &side.party {
                // The Metronome item's condition reads it; items only change hands, so a battle
                // without one never gets one.
                if mon.item == items::METRONOME {
                    readers.move_last_turn_result = true;
                }
                // Pickup: an ability only moves between party members (Skill Swap, Trace,
                // Receiver, ...), so a battle without a holder never gets one.
                if mon.ability == abilities::PICKUP || mon.base_ability == abilities::PICKUP {
                    readers.used_item = true;
                }
                for slot in &mon.moves {
                    use crate::dex::moves as m;
                    if slot.id == m::METAL_BURST || slot.id == m::COMEUPPANCE {
                        readers.last_damaged_by = true;
                    } else if slot.id == m::RAGE_FIST {
                        readers.times_attacked = true;
                    } else if slot.id == m::STOMPING_TANTRUM || slot.id == m::TEMPER_FLARE {
                        readers.move_last_turn_result = true;
                    } else if slot.id == m::RETALIATE {
                        readers.fainted_last_turn = true;
                    } else if slot.id == m::BURNING_JEALOUSY || slot.id == m::ALLURING_VOICE {
                        readers.stats_raised = true;
                    } else if slot.id == m::LASH_OUT {
                        readers.stats_lowered = true;
                    } else if slot.id == m::BELCH {
                        readers.ate_berry = true;
                    } else if slot.id == m::LAST_RESORT {
                        readers.moves_used = true;
                    } else if slot.id == m::COPYCAT {
                        readers.last_move = true;
                    } else if slot.id == m::INSTRUCT {
                        readers.last_move_target_loc = true;
                    }
                }
                let own = mon.transformed.map(|base| base.moves);
                if mon
                    .moves
                    .iter()
                    .chain(own.iter().flatten())
                    .any(|s| s.id == crate::dex::moves::BEAT_UP)
                {
                    readers.party_order = true;
                }
            }
        }
        readers.ability_order = Self::redirector_possible(state);
        readers
    }

    /// Whether a `RedirectTarget` handler of the same priority can be on two Pokémon
    /// ([`HistoryReaders::ability_order`]).
    fn redirector_possible<const N: usize>(state: &State<N>) -> bool {
        use crate::dex::moves as m;
        let redirects = |a: AbilityId| a == abilities::LIGHTNING_ROD || a == abilities::STORM_DRAIN;
        let mons = || state.sides.iter().flat_map(|side| side.party.iter());
        mons().any(|mon| {
            let own = mon.transformed.map(|base| base.moves);
            mon.moves
                .iter()
                .chain(own.iter().flatten())
                .any(|s| matches!(s.id, m::FOLLOW_ME | m::RAGE_POWDER | m::SPOTLIGHT))
                || redirects(mon.ability)
                || redirects(mon.base_ability)
                || mons().any(|holder| {
                    crate::gimmick::mega_evolution(mon.untransformed_species(), holder.item)
                        .is_some_and(|mega| mega.data().abilities.iter().any(|&a| redirects(a)))
                })
        })
    }
}

impl<'a, const N: usize> Battle<'a, N> {
    pub fn new(state: &'a mut State<N>, rng: &'a mut Chooser) -> Battle<'a, N> {
        Battle::recycle(state, rng, RunBuffers::default())
    }

    /// [`Battle::new`] with an earlier run's buffers ([`Battle::into_buffers`]): the log is kept
    /// as it is (a sampled turn's stages log into one), the Speed snapshot is taken again.
    pub fn recycle(
        state: &'a mut State<N>,
        rng: &'a mut Chooser,
        mut buffers: RunBuffers,
    ) -> Battle<'a, N> {
        let history_readers = HistoryReaders::of(state);
        let suppression = super::abilities::suppression_possible(state);
        buffers.speed_snapshot.clear();
        let mut b = Battle::with(state, rng, history_readers, suppression, buffers);
        b.snapshot_speeds();
        b
    }

    /// A battle for another run from the position `start` was taken from (the state restored to
    /// it): the same context as [`Battle::new`] would derive, without deriving it again, and the
    /// previous run's buffers for the log and the Speed snapshot (a staged enumeration replays
    /// many runs from each position; Opus GG).
    pub fn replay(
        state: &'a mut State<N>,
        rng: &'a mut Chooser,
        start: &RunStart,
        mut buffers: RunBuffers,
    ) -> Battle<'a, N> {
        buffers.log.clear();
        buffers.speed_snapshot.clear();
        buffers
            .speed_snapshot
            .extend_from_slice(&start.speed_snapshot);
        Battle::with(
            state,
            rng,
            start.history_readers,
            start.suppression,
            buffers,
        )
    }

    /// What [`Battle::replay`] reuses: the context this battle started with. Call it before the
    /// run changes anything.
    pub fn run_start(&self) -> RunStart {
        RunStart {
            history_readers: self.history_readers,
            suppression: self.suppression,
            speed_snapshot: self.speed_snapshot.clone(),
        }
    }

    /// The log (the run's instructions) and the Speed snapshot's buffer, for the next
    /// [`Battle::replay`].
    pub fn into_buffers(self) -> RunBuffers {
        RunBuffers {
            log: self.log,
            speed_snapshot: self.speed_snapshot,
        }
    }

    fn with(
        state: &'a mut State<N>,
        rng: &'a mut Chooser,
        history_readers: HistoryReaders,
        suppression: bool,
        buffers: RunBuffers,
    ) -> Battle<'a, N> {
        Battle {
            state,
            log: buffers.log,
            rng,
            faint_queue: Vec::new(),
            active_move: None,
            queue: Vec::new(),
            battle_start: false,
            hit_type_mod: [[None; N]; 2],
            hit_crit: [[false; N]; 2],
            mirror_herb: Vec::new(),
            umbrella_inactive: Vec::new(),
            move_self_switch: false,
            force_switch: Vec::new(),
            busted: Vec::new(),
            history_readers,
            raw_speed: Vec::new(),
            speed_snapshot: buffers.speed_snapshot,
            awaiting_run_switch: false,
            unstarted: Vec::new(),
            queue_done: false,
            absent_user: None,
            external_move: false,
            called_move: None,
            called_suspension: None,
            active_target: None,
            suppression,
        }
    }

    /// The category of `id` as the move in flight has it (`move.category` after ModifyMove:
    /// Photon Geyser and Shell Side Arm can become physical), else the dex's.
    pub fn move_category(&self, id: MoveId) -> MoveCategory {
        match self.active_move {
            Some(m) if m.id == id => m.category,
            _ => id.data().category,
        }
    }

    /// The hit's `typeMod` against `target` ([`Battle::hit_type_mod`]).
    pub fn type_mod_of(&self, target: SlotRef) -> Option<i32> {
        self.hit_type_mod[target.side.index()][usize::from(target.slot)].map(i32::from)
    }

    /// Whether the hit in progress was a critical hit on `target` ([`Battle::hit_crit`]).
    pub fn hit_was_crit(&self, target: SlotRef) -> bool {
        self.hit_crit[target.side.index()][usize::from(target.slot)]
    }

    pub fn apply(&mut self, instruction: Instruction) {
        self.state.apply_one(&instruction);
        self.log.push(instruction);
    }

    // ---- lookups ------------------------------------------------------------------------

    /// The party member in `slot`, fainted or not.
    pub fn occupant(&self, slot: SlotRef) -> Option<PokemonRef> {
        self.state.active_ref(slot)
    }

    pub fn mon(&self, pokemon: PokemonRef) -> &Pokemon {
        self.state.pokemon(pokemon)
    }

    /// The occupant of `slot` if it still has HP.
    pub fn alive(&self, slot: SlotRef) -> Option<PokemonRef> {
        self.occupant(slot).filter(|&p| self.mon(p).hp > 0)
    }

    pub fn slot_mon(&self, slot: SlotRef) -> Option<&Pokemon> {
        self.occupant(slot).map(|p| self.mon(p))
    }

    pub fn slots(side: SideId) -> impl Iterator<Item = SlotRef> {
        (0..N as u8).map(move |slot| SlotRef { side, slot })
    }

    /// Showdown `side.allies()` + self order is slot order; foes are the other side's slots.
    pub fn alive_slots(&self, side: SideId) -> SlotList {
        Self::slots(side)
            .filter(|&s| self.alive(s).is_some())
            .collect()
    }

    pub fn all_alive(&self) -> SlotList {
        let mut out = self.alive_slots(SideId::One);
        out.extend(self.alive_slots(SideId::Two));
        out
    }

    /// The ability whose handlers act for the occupant of `slot` (`hasAbility`, `runEvent`):
    /// `NONE` while it is suppressed by Gastro Acid or Neutralizing Gas
    /// (`abilities::ignoring_ability`; Showdown `ignoringAbility`).
    pub fn ability(&self, slot: SlotRef) -> AbilityId {
        if self.suppression {
            super::abilities::effective_ability(self.state, slot)
        } else {
            self.raw_ability(slot)
        }
    }

    /// Showdown `pokemon.ignoringAbility()` for the occupant of `slot`
    /// (`abilities::ignoring_ability`; checked only when suppression is possible at all).
    pub fn ignoring_ability(&self, slot: SlotRef) -> bool {
        if self.suppression {
            super::abilities::ignoring_ability(self.state, slot)
        } else {
            self.occupant(slot).is_none()
        }
    }

    /// Showdown `pokemon.ability` itself, suppressed or not (`getAbility()`): Skill Swap, Role
    /// Play, Entrainment, Mummy, Wandering Spirit, Trace's target, an ability's `End`.
    pub fn raw_ability(&self, slot: SlotRef) -> AbilityId {
        self.slot_mon(slot).map_or(AbilityId::NONE, |m| m.ability)
    }

    /// Showdown `suppressingAbility(target)`: the move in progress ignores abilities
    /// (`ignoreAbility`: Sunsteel Strike, or any move of a Mold Breaker user after ModifyMove),
    /// its user is still active, and `target` is not the user (Gen 8+) and holds no Ability
    /// Shield.
    pub fn suppressing_ability(&self, target: SlotRef) -> bool {
        let Some(active) = self.active_move else {
            return false;
        };
        active.ignore_ability
            && self.occupant(active.user) == Some(active.pokemon)
            && active.user != target
            && self.item(target) != items::ABILITY_SHIELD
    }

    /// The ability whose handlers run for `slot` in an event: none if it is breakable and the
    /// move in progress suppresses it (Showdown's `runEvent` skip for `flags.breakable`).
    pub fn ability_unless_broken(&self, slot: SlotRef) -> AbilityId {
        let ability = self.ability(slot);
        if ability.data().flags.contains(AbilityFlags::BREAKABLE) && self.suppressing_ability(slot)
        {
            AbilityId::NONE
        } else {
            ability
        }
    }

    /// The item whose effects apply (`hasItem`, item handlers): `NONE` while the holder is
    /// ignoring its item (`items::ignoring_item`: Magic Room, Klutz).
    pub fn item(&self, slot: SlotRef) -> ItemId {
        if super::items::ignoring_item(self.state, slot) {
            return ItemId::NONE;
        }
        self.raw_item(slot)
    }

    /// Showdown `pokemon.item` itself, suppressed or not (Knock Off, Trick, Acrobatics,
    /// Unburden, Mega Evolution).
    pub fn raw_item(&self, slot: SlotRef) -> ItemId {
        self.slot_mon(slot).map_or(ItemId::NONE, |m| m.item)
    }

    /// Showdown `pokemon.hasType(type)`: the types, and the added type (Forest's Curse,
    /// Trick-or-Treat).
    pub fn has_type(&self, slot: SlotRef, ty: Type) -> bool {
        self.slot_mon(slot).is_some_and(|m| m.types.contains(&ty))
            || (ty != Type::None && self.added_type(slot) == ty)
    }

    /// Showdown `pokemon.addedType` ([`Volatile::AddedType`]); `Type::None` without one.
    pub fn added_type(&self, slot: SlotRef) -> Type {
        super::conditions::added_type(self.state, slot)
    }

    /// Showdown `pokemon.getTypes()`: the types, then the added type (`Type::None` fills the
    /// rest).
    pub fn types(&self, slot: SlotRef) -> [Type; 3] {
        super::conditions::all_types(self.state, slot)
    }

    /// The weather on the field (`field.weather`), whether or not it is suppressed: what
    /// setting a weather, its duration and its residual countdown see. Effects of the weather
    /// read [`Battle::effective_weather`].
    pub fn weather(&self) -> Weather {
        let e = self.state.field[FieldEffect::Weather as usize];
        if !e.is_active() {
            return Weather::None;
        }
        weather_from(e.value)
    }

    /// Showdown `field.effectiveWeather()`: no weather while it is suppressed
    /// ([`Battle::weather_suppressed`]), otherwise the field's weather. Every effect of a weather
    /// reads this (Showdown's `isWeather` and `effectiveWeather`, and the weather condition's own
    /// handlers, which `runEvent` skips while the weather is suppressed).
    pub fn effective_weather(&self) -> Weather {
        if self.weather_suppressed() {
            Weather::None
        } else {
            self.weather()
        }
    }

    /// Showdown `field.suppressingWeather()`: an active Pokémon not processed as fainted (it may
    /// be at 0 HP) has an ability with `suppressWeather` (Air Lock, Cloud Nine) that it is not
    /// ignoring (`!pokemon.ignoringAbility()`: Gastro Acid, Neutralizing Gas). Its
    /// `abilityState.ending` flag only matters inside its own `End` event, whose
    /// `WeatherChange` has no implemented handler.
    pub fn weather_suppressed(&self) -> bool {
        State::<N>::slot_refs().any(|slot| self.ability(slot).data().suppress_weather)
    }

    pub fn terrain(&self) -> Terrain {
        let e = self.state.field[FieldEffect::Terrain as usize];
        if !e.is_active() {
            return Terrain::None;
        }
        terrain_from(e.value)
    }

    pub fn field_active(&self, effect: FieldEffect) -> bool {
        self.state.field[effect as usize].is_active()
    }

    pub fn side_effect_active(&self, side: SideId, effect: SideEffect) -> bool {
        self.state.side(side).effects[effect as usize].is_active()
    }

    /// Showdown `isGrounded` for the supported effects, in its order: Gravity, Ingrain, Smack
    /// Down, Iron Ball, Flying, Levitate, Magnet Rise, Air Balloon.
    pub fn is_grounded(&self, slot: SlotRef) -> bool {
        if self.field_active(FieldEffect::Gravity)
            || self.volatile(slot, Volatile::Ingrain).active
            || self.volatile(slot, Volatile::SmackDown).active
        {
            return true;
        }
        let item = self.item(slot);
        if super::items::grounds(item) {
            return true;
        }
        if self.has_type(slot, Type::Flying) {
            return false;
        }
        // `hasAbility(['levitate', 'eelevate']) && !suppressingAbility(this)`.
        let floats = [abilities::LEVITATE, abilities::EELEVATE].contains(&self.ability(slot));
        if floats && !self.suppressing_ability(slot) {
            return false;
        }
        if self.volatile(slot, Volatile::MagnetRise).active {
            return false;
        }
        !super::items::lifts(item)
    }

    pub fn volatile(&self, slot: SlotRef, volatile: Volatile) -> VolatileState {
        self.state.slot(slot).volatiles.get(volatile)
    }

    /// Showdown `dex.getImmunity(status, pokemon)`: the immunity the Pokémon's types give,
    /// without `Immunity` handlers (the powder and Prankster checks of `hitStepTryImmunity`).
    pub fn natural_immune(&self, slot: SlotRef, immunity: TypeImmunities) -> bool {
        self.slot_mon(slot).is_none()
            || self
                .types(slot)
                .iter()
                .any(|t| t.immunities().contains(immunity))
    }

    /// Showdown `dex.getImmunity(status, pokemon)` plus the supported `Immunity` handlers
    /// (`runStatusImmunity`).
    pub fn status_immune(&self, slot: SlotRef, immunity: TypeImmunities) -> bool {
        if self.slot_mon(slot).is_none() || self.natural_immune(slot, immunity) {
            return true;
        }
        // The item's `onImmunity` (Safety Goggles: sandstorm, powder), skipped by `runEvent`
        // while the holder ignores its item (Klutz, Magic Room): the effective item.
        if super::items::grants_immunity(self.item(slot), immunity) {
            return true;
        }
        // Immunity handlers; each returns false for one immunity id, so order is irrelevant.
        // Overcoat (breakable): `if (type === 'sandstorm' || type === 'hail' || type ===
        // 'powder') return false;` (hail is not a supported weather).
        let overcoat = self.ability_unless_broken(slot) == abilities::OVERCOAT;
        if immunity == TypeImmunities::SANDSTORM {
            // Sand Rush, Sand Force, Sand Veil (breakable): `onImmunity(type) { if (type ===
            // 'sandstorm') return false; }`.
            let sand_ability =
                [abilities::SAND_RUSH, abilities::SAND_FORCE].contains(&self.ability(slot));
            let sand_veil = self.ability_unless_broken(slot) == abilities::SAND_VEIL;
            return sand_ability || sand_veil || overcoat;
        }
        if immunity == TypeImmunities::POWDER {
            return overcoat;
        }
        if immunity == TypeImmunities::FRZ {
            // Harsh sunlight (`sunnyday.onImmunity`, only while the field's weather is sun:
            // `pokemon.effectiveWeather() === 'sunnyday'`, which Utility Umbrella hides and a
            // Mega Sol user's move shows, `Battle::move_weather`) and Magma Armor (breakable).
            let sun = match self.effective_weather() {
                Weather::Sun => self.move_weather(slot) == Weather::Sun,
                Weather::HarshSun => self.move_weather(slot) == Weather::HarshSun,
                _ => false,
            };
            return sun || self.ability_unless_broken(slot) == abilities::MAGMA_ARMOR;
        }
        // Ice Body's `onImmunity('hail')`: hail is not a supported weather.
        false
    }

    // ---- HP ----------------------------------------------------------------------------

    /// Showdown `spreadDamage` for one target: at least 1, Damage handlers, clamped to the
    /// target's HP, faint queued at 0 HP. Returns the HP removed.
    ///
    /// Damage handlers by priority: Disguise and Ice Face (1, `forme::absorbs_damage`), Rock
    /// Head and Magic Guard (0), Endure (-10), Sturdy (-30),
    /// Focus Sash and Focus Band (-40, `items::on_damage`). Rock Head (`effect.id === 'recoil'`)
    /// and Magic Guard (`effect.effectType !== 'Move'`) cancel the damage (neither is
    /// breakable). Endure, Sturdy and Focus Sash leave the target at 1 HP against a move's
    /// damage (Sturdy and the Sash only from full HP; Sturdy acts first, so the Sash then
    /// stays). Sturdy is breakable (ignored by Sunsteel Strike and the like).
    pub fn damage(&mut self, target: SlotRef, amount: f64, source: DamageSource) -> i32 {
        let Some(pokemon) = self.alive(target) else {
            return 0;
        };
        // Disguise / Ice Face `onDamage` (priority 1, the first handler): a move's damage becomes
        // 0, which ends the event.
        if source == DamageSource::Move && super::forme::absorbs_damage(self, target, pokemon) {
            return 0;
        }
        // Anger Shell / Berserk `onDamage` (priority 0, before every handler below that could
        // change the damage; it changes nothing itself).
        let multihit = self
            .active_move
            .is_some_and(|m| super::moves::is_multihit(m.id) || m.parental_bond);
        super::abilities::on_damage(self, target, source == DamageSource::Move, multihit);
        let mut amount = (amount.floor() as i32).max(1);
        let mon = self.mon(pokemon);
        let cancelled = match self.ability(target) {
            a if a == abilities::ROCK_HEAD => source == DamageSource::Recoil,
            a if a == abilities::MAGIC_GUARD => source != DamageSource::Move,
            _ => false,
        };
        if cancelled {
            return 0;
        }
        // Endure (`onDamagePriority: -10`): a move's damage leaves at least 1 HP; Sturdy and
        // Focus Sash then see damage below the HP and keep quiet.
        if source == DamageSource::Move
            && amount >= i32::from(mon.hp)
            && self.volatile(target, Volatile::Endure).active
        {
            amount = i32::from(mon.hp) - 1;
        }
        // False Swipe, Hold Back (the move's own `onDamage`, priority -20: after Endure, before
        // Sturdy): `if (damage >= target.hp) return target.hp - 1;` for the move's damage to its
        // target (not a confusion self-hit, whose effect is not the move).
        let holds_back = self.active_move.is_some_and(|m| {
            [crate::dex::moves::FALSE_SWIPE, crate::dex::moves::HOLD_BACK].contains(&m.id)
                && m.user != target
        });
        if source == DamageSource::Move && holds_back && amount >= i32::from(mon.hp) {
            amount = i32::from(mon.hp) - 1;
        }
        if source == DamageSource::Move
            && mon.hp == mon.max_hp
            && amount >= i32::from(mon.hp)
            && self.ability_unless_broken(target) == abilities::STURDY
        {
            amount = i32::from(mon.hp) - 1;
        }
        let amount = super::items::on_damage(self, target, amount, source);
        // A move's damage has the move's user as its source (Destiny Bond).
        let attacker = self
            .active_move
            .filter(|_| source == DamageSource::Move)
            .map(|m| m.pokemon);
        let dealt = self.lose_hp(target, pokemon, amount, attacker);
        // `if (targetDamage !== 0) target.hurtThisTurn = target.hp`.
        if dealt != 0 {
            self.record_hurt(target);
        }
        dealt
    }

    /// Showdown `directDamage`: at least 1 HP, no Damage handlers (Struggle's recoil). Returns the
    /// HP removed.
    pub fn direct_damage(&mut self, target: SlotRef, amount: i32) -> i32 {
        let Some(pokemon) = self.alive(target) else {
            return 0;
        };
        if amount == 0 {
            return 0;
        }
        self.lose_hp(target, pokemon, amount.max(1), None)
    }

    fn lose_hp(
        &mut self,
        slot: SlotRef,
        pokemon: PokemonRef,
        amount: i32,
        attacker: Option<PokemonRef>,
    ) -> i32 {
        let hp = i32::from(self.mon(pokemon).hp);
        if amount <= 0 || hp == 0 {
            return 0;
        }
        let dealt = amount.min(hp);
        self.apply(Instruction::Damage {
            target: pokemon,
            amount: dealt as i16,
        });
        if dealt == hp {
            self.queue_faint(pokemon, slot, attacker);
        }
        dealt
    }

    /// Showdown `battle.heal`: fractions below 1 become 1, then truncate; nothing on a fainted
    /// or full-HP Pokémon; `runEvent('TryHeal')`: Heal Block on the target stops every heal
    /// (`return false`, or `null` for an ally's Pollen Puff). Returns the HP restored.
    pub fn heal(&mut self, target: SlotRef, amount: f64) -> i32 {
        if self.volatile(target, Volatile::HealBlock).active {
            return 0;
        }
        self.heal_unblocked(target, amount)
    }

    /// Showdown `pokemon.heal` (no TryHeal event: Heal Block does not stop it): Regenerator,
    /// Healing Wish.
    pub fn heal_unblocked(&mut self, target: SlotRef, amount: f64) -> i32 {
        let Some(pokemon) = self.alive(target) else {
            return 0;
        };
        let mut amount = amount;
        if amount > 0.0 && amount <= 1.0 {
            amount = 1.0;
        }
        let amount = amount.trunc() as i32;
        let mon = self.mon(pokemon);
        if amount <= 0 || mon.hp >= mon.max_hp {
            return 0;
        }
        let healed = amount.min(i32::from(mon.max_hp - mon.hp));
        self.apply(Instruction::Heal {
            target: pokemon,
            amount: healed as i16,
        });
        healed
    }

    /// `battle.heal` for the heals whose effect Big Root's `onTryHeal` (priority 1) lists:
    /// `drain`, `leechseed`, `ingrain`, `aquaring`, `strengthsap`. The amount is normalized as
    /// in `heal` (at least 1, truncated), then `runEvent('TryHeal')` chains `[5324, 4096]` on a
    /// holder of Big Root (Heal Block stops it in `heal`; Ripen only doubles a berry's heal,
    /// `update::berry_heal`). Returns the HP restored.
    pub fn heal_rooted(&mut self, target: SlotRef, amount: f64) -> i32 {
        self.heal_rooted_from(target, amount, None)
    }

    /// [`Battle::heal_rooted`] for a heal whose source is the Pokémon in `source` and whose
    /// effect Liquid Ooze lists (`drain`, `leechseed`, `strengthsap`). Big Root (priority 1) only
    /// chains its modifier, applied to what the event returns at its end; the TryHeal handlers
    /// at priority 0 run by `pokemon.speed`, then sub-order — Heal Block on the healed Pokémon (a
    /// condition, 2: `return false`, which ends the event) and the source's Liquid Ooze
    /// (`onSourceTryHeal`, an ability, 7; not breakable, and it acts for a source at 0 HP not yet
    /// fainted): `this.damage(damage)` to the healed Pokémon with the amount before Big Root's
    /// modifier (not a move's damage: Magic Guard stops it) and `return 0`. TryHeal comes before
    /// `heal`'s full-HP check, so the ooze hurts a healer at full HP too.
    pub fn heal_rooted_from(
        &mut self,
        target: SlotRef,
        amount: f64,
        source: Option<SlotRef>,
    ) -> i32 {
        let amount = if amount > 0.0 && amount <= 1.0 {
            1.0
        } else {
            amount
        };
        let amount = amount.trunc() as i32;
        let ooze = source.filter(|&s| {
            s != target && self.occupant(s).is_some() && self.ability(s) == abilities::LIQUID_OOZE
        });
        if let Some(ooze) = ooze {
            let heal_block_first = self.volatile(target, Volatile::HealBlock).active
                && self.event_speed(target) >= self.event_speed(ooze);
            if !heal_block_first {
                if amount > 0 && self.alive(target).is_some() {
                    self.damage(target, f64::from(amount), DamageSource::Indirect);
                }
                return 0;
            }
        }
        let amount = if self.item(target) == items::BIG_ROOT {
            super::order::modify(amount, 5324)
        } else {
            amount
        };
        self.heal(target, f64::from(amount))
    }

    // ---- faint and win -------------------------------------------------------------------

    /// Whether a Pokémon at 0 HP waits for `faintMessages` (the faint queue is not empty).
    pub fn faint_pending(&self) -> bool {
        !self.faint_queue.is_empty()
    }

    fn queue_faint(&mut self, pokemon: PokemonRef, slot: SlotRef, attacker: Option<PokemonRef>) {
        if !self.faint_queue.iter().any(|&(p, _, _)| p == pokemon) {
            // `faint()`: `this.switchFlag = false` (only a flag set after this survives the
            // faint: Emergency Exit after the user's own recoil).
            self.clear_switch_flag(slot);
            self.faint_queue.push((pokemon, slot, attacker));
        }
    }

    /// Showdown `for (const pokemon of this.getAllActive()) if (pokemon.switchFlag === true)`
    /// (Eject Button, Eject Pack): an active Pokémon with an Eject Button, Eject Pack or
    /// Emergency Exit flag, a 0-HP one whose faint is not processed yet included (it is still in
    /// its slot: Emergency Exit after its own recoil); a processed faint has left the slot.
    pub fn any_active_switch_flag_true(&self) -> bool {
        State::<N>::slot_refs().any(|slot| {
            self.occupant(slot).is_some() && self.state.slot(slot).switch_flag == SwitchFlag::Effect
        })
    }

    /// Showdown `faintMessages(lastFirst = false, forceCheck = false, checkWin)`. Returns
    /// whether the battle is over; an error when a fainted Neutralizing Gas holder's `End`
    /// restarts an ability the engine cannot start.
    pub fn faint_messages(&mut self, check_win: bool) -> Result<bool, TurnError> {
        if self.state.result.is_over() {
            return Ok(true);
        }
        if self.faint_queue.is_empty() {
            return Ok(false);
        }
        let mut check_win = check_win;
        // `const length = this.faintQueue.length`, and `faintData`: the last entry taken from
        // the queue, processed or not: AfterFaint's source, and `checkWin(faintData)`'s winner
        // when no side has a Pokémon left (the side of that entry's Pokémon).
        let length = self.faint_queue.len();
        let mut last_source = None;
        let mut last = None;
        while !self.faint_queue.is_empty() {
            let queue_left = self.faint_queue.len();
            let (pokemon, slot, attacker) = self.faint_queue.remove(0);
            last_source = attacker;
            last = Some(pokemon.side);
            if self.occupant(slot) != Some(pokemon) {
                continue;
            }
            // runEvent('Faint'): Destiny Bond takes its attacker down (`if
            // (this.faintQueue.length >= faintQueueLeft) checkWin = true;`).
            super::conditions::destiny_bond_faint(self, slot, attacker);
            if self.faint_queue.len() >= queue_left {
                check_win = true;
            }
            // Soul-Heart's `onAnyFaint` belongs to the Faint event, before the fainted Pokémon's
            // ability `End`: its holders are the ones acting while a fainting Neutralizing Gas
            // still suppresses them.
            let hearts = super::abilities::soul_heart_holders(self);
            // `singleEvent('End', ability)`: Neutralizing Gas's `onEnd` (unless it already ran:
            // `abilityState.ending`) restarts the other abilities; it runs once the holder has
            // left below (at 0 HP it is in no target list, and its `ending` excludes it).
            // A transformed holder's `onEnd` returns at once (`if (source.transformed) return`).
            let gas_ends = self.raw_ability(slot) == abilities::NEUTRALIZING_GAS
                && !self.volatile(slot, Volatile::NeutralizingGasEnding).active
                && self.mon(pokemon).transformed.is_none();
            // Receiver / Power of Alchemy (`onAllyFaint`) take `target.getAbility()`, the one it
            // has before `clearVolatile` reverts it.
            let fainted_ability = self.raw_ability(slot);
            // Power Construct's `formeRegression`: a fainting Zygarde-Complete goes back to its
            // set's species (50% or 10%) and ability with `updateMaxHp`; the state does not keep
            // which forme the set had.
            if self.mon(pokemon).species == crate::dex::species::ZYGARDE_COMPLETE {
                return Err(self.unsupported(
                    "Zygarde-Complete fainting (Power Construct's formeRegression to the set's forme)",
                ));
            }
            // clearVolatile: the ability and types revert; the slot empties (isActive = false).
            self.clear_volatile(pokemon);
            // `pokemon.illusion = null` (and Illusion's `onFaint`, EE2).
            super::abilities::illusion_end(self, pokemon);
            let previous = self.state.slot(slot).clone();
            let flag = previous.switch_flag;
            self.apply(Instruction::Switch {
                slot,
                previous: Box::new(previous),
                party_index: None,
            });
            self.apply(Instruction::SetFaintedOccupant {
                slot,
                old: None,
                new: Some(pokemon.party),
            });
            // `clearVolatile(false)` keeps `switchFlag`: the fainted Pokémon Emergency Exit
            // flagged after its own recoil still asks for a mid-turn replacement
            // (`Slot::must_switch_out`).
            self.set_switch_flag(slot, flag);
            // The rest of runEvent('Faint'): Soul-Heart (priority 1, before Destiny Bond, which
            // only faints its attacker: a holder it knocks out gets no boost either way), run
            // once the faint counts as processed (`pokemonLeft` dropped: `boost` needs
            // `foePokemonLeft()`).
            super::abilities::soul_heart(self, &hearts);
            super::abilities::receiver(self, slot, fainted_ability)?;
            self.record_faint(pokemon.side);
            if gas_ends {
                super::abilities::neutralizing_gas_end(self, None)?;
            }
        }
        if check_win && self.check_win(last) {
            return Ok(true);
        }
        // `runEvent('AfterFaint', faintData.target, faintData.source, faintData.effect,
        // length)`: only the source's `onSourceAfterFaint` handlers exist, and they need a move's
        // damage (`effect.effectType === 'Move'`), which is when the queue records a source.
        if let Some(source) = last_source {
            super::abilities::after_faint(self, source, length);
        }
        Ok(false)
    }

    /// The party-side part of Showdown `clearVolatile` when a Pokémon leaves the field: the
    /// ability reverts to its base and `setSpecies(baseSpecies)` restores the species' types
    /// (a permanent forme stays: Champions never regresses one; a temporary forme returns to its
    /// base species, `forme::revert_on_leave`; a transformed Pokémon gets its base species and
    /// own move slots back, `transform::revert_on_leave`). Slot state is reset by the caller's
    /// `Switch`.
    pub fn clear_volatile(&mut self, pokemon: PokemonRef) {
        // `removeLinkedVolatiles` for its linked volatiles (Mean Look's `trapped` / `trapper`),
        // while it still holds its slot.
        if let Some(slot) = State::<N>::slot_refs().find(|&s| self.occupant(s) == Some(pokemon)) {
            super::conditions::remove_linked_volatiles(self, pokemon, slot);
        }
        let mon = self.mon(pokemon);
        if mon.ability != mon.base_ability {
            let (old, new) = (mon.ability, mon.base_ability);
            self.apply(Instruction::SetAbility {
                target: pokemon,
                old,
                new,
            });
        }
        super::transform::revert_on_leave(self, pokemon);
        super::forme::revert_on_leave(self, pokemon);
        let mon = self.mon(pokemon);
        let species_types = mon.species.data().types;
        if mon.types != species_types {
            let old = mon.types;
            self.apply(Instruction::SetTypes {
                target: pokemon,
                old,
                new: species_types,
            });
        }
        // `setSpecies` also resets the weight (Autotomize) and recalculates the stored stats
        // (Speed Swap's exchange ends).
        self.reset_autotomize(pokemon);
        let mon = self.mon(pokemon);
        let stats = mon.forme_as(mon.species).stats;
        if mon.stats != stats {
            let old = mon.forme();
            self.apply(Instruction::SetForme {
                target: pokemon,
                old,
                new: crate::state::Forme { stats, ..old },
            });
        }
    }

    /// Showdown `checkWin(faintData)`: with every side out, the side of the last faint-queue
    /// entry `faintMessages` dequeued wins (Gen 5+; Explosion that takes out everyone: the user
    /// is queued first, so the last target's side wins); `None` (`checkWin()` without an entry)
    /// makes it a tie.
    pub fn check_win(&mut self, last_faint: Option<SideId>) -> bool {
        if self.state.result.is_over() {
            return true;
        }
        let left = [SideId::One, SideId::Two].map(|side| self.pokemon_left(side));
        let result = if left == [0, 0] {
            last_faint.map_or(BattleResult::Tie, BattleResult::Win)
        } else if left[1] == 0 {
            BattleResult::Win(SideId::One)
        } else if left[0] == 0 {
            BattleResult::Win(SideId::Two)
        } else {
            return false;
        };
        self.apply(Instruction::SetResult {
            old: BattleResult::Ongoing,
            new: result,
        });
        true
    }

    pub fn pokemon_left(&self, side: SideId) -> usize {
        self.state
            .side(side)
            .party
            .iter()
            .filter(|p| p.hp > 0)
            .count()
    }

    pub fn is_over(&self) -> bool {
        self.state.result.is_over()
    }

    // ---- status --------------------------------------------------------------------------

    /// Showdown `trySetStatus` → `setStatus` for a status inflicted by the move in progress:
    /// its user is the status's source (moves pass `source` explicitly), none outside a move.
    /// Other sources (a contact ability's holder, the holder itself for Toxic / Flame Orb) must
    /// use [`Battle::try_set_status_from`].
    pub fn try_set_status(&mut self, target: SlotRef, status: Status) -> bool {
        let source = self
            .active_move
            .filter(|m| self.occupant(m.user) == Some(m.pokemon))
            .map(|m| m.user);
        // Safeguard's `if (effect.effectType === 'Move' && effect.infiltrates &&
        // !target.isAlly(source)) return;`: an infiltrating move's status on a foe.
        let infiltrates = self.active_move.is_some_and(|m| m.infiltrates)
            && source.is_some_and(|s| s.side != target.side);
        self.try_set_status_inner(target, status, source, infiltrates, true, true)
    }

    /// Showdown `trySetStatus(status, source)` → `setStatus` for the supported handlers: fails
    /// on a fainted target, an existing status, status immunity (`runStatusImmunity`), and the
    /// `SetStatus` handlers (see [`Battle::set_status_blocked`]); a status that is set runs the
    /// `AfterSetStatus` handlers ([`Battle::after_set_status`]).
    pub fn try_set_status_from(
        &mut self,
        target: SlotRef,
        status: Status,
        source: Option<SlotRef>,
    ) -> bool {
        self.try_set_status_inner(target, status, source, false, false, true)
    }

    /// [`Battle::try_set_status_from`] for Toxic Spikes' poison (`pokemon.trySetStatus(status,
    /// pokemon.side.foe.active[0])`, the condition as the effect): Synchronize's
    /// `onAfterSetStatus` ignores it (`if (effect && effect.id === 'toxicspikes') return;`;
    /// oracle `rr-toxic-spikes-synchronize`).
    pub fn try_set_status_from_toxic_spikes(
        &mut self,
        target: SlotRef,
        status: Status,
        source: Option<SlotRef>,
    ) -> bool {
        self.try_set_status_inner(target, status, source, false, false, false)
    }

    /// [`Battle::try_set_status_from`]; `infiltrates`: the status is an infiltrating move's
    /// effect on a foe, which Safeguard lets through; `by_move`: the status's effect is the move
    /// in progress (`effect.effectType === 'Move'`, Poison Puppeteer); `synchronize`: the effect
    /// is not Toxic Spikes, so Synchronize may pass the status back.
    fn try_set_status_inner(
        &mut self,
        target: SlotRef,
        status: Status,
        source: Option<SlotRef>,
        infiltrates: bool,
        by_move: bool,
        synchronize: bool,
    ) -> bool {
        let Some(pokemon) = self.alive(target) else {
            return false;
        };
        // Safeguard (`onSetStatus` of the target's side): blocks a status from another Pokémon.
        if !infiltrates && self.safeguarded(target, source) {
            return false;
        }
        if self.mon(pokemon).status != Status::None {
            return false;
        }
        let immunity = match status {
            Status::Burn => TypeImmunities::BRN,
            Status::Freeze => TypeImmunities::FRZ,
            Status::Paralyze => TypeImmunities::PAR,
            Status::Poison | Status::Toxic => TypeImmunities::PSN,
            Status::Sleep => TypeImmunities::EMPTY,
            Status::None | Status::Fainted => return false,
        };
        // `!(source?.hasAbility('corrosion') && ['tox', 'psn'].includes(status.id))`: a
        // Corrosion source's poison skips `runStatusImmunity` (the Poison and Steel types). With
        // no source Showdown takes the target itself (`if (!source) source = this`: Toxic Orb).
        let corrosion = matches!(status, Status::Poison | Status::Toxic)
            && self.ability(source.unwrap_or(target)) == abilities::CORROSION;
        if immunity != TypeImmunities::EMPTY && !corrosion && self.status_immune(target, immunity) {
            return false;
        }
        if self.set_status_blocked(target, status) {
            return false;
        }
        if super::abilities::blocks_status(self.ability_unless_broken(target), status) {
            return false;
        }
        // Flower Veil's `onAllySetStatus`: `source && target !== source && effect.id !==
        // 'yawn'` (Yawn's end passes no source here).
        if source.is_some_and(|s| s != target)
            && super::abilities::flower_veil_holder(self, target).is_some()
        {
            return false;
        }
        let turns = match status {
            // Champions `slp`: `sample([2, 3, 3])`.
            Status::Sleep => {
                if self.rng.weighted(&[1.0 / 3.0, 2.0 / 3.0]) == 0 {
                    2
                } else {
                    3
                }
            }
            // Champions `frz`: thaws after at most 3 turns.
            Status::Freeze => 3,
            _ => 0,
        };
        self.apply(Instruction::ChangeStatus {
            target: pokemon,
            old: Status::None,
            new: status,
        });
        self.set_status_turns(pokemon, turns);
        // The `slp` condition's `onStart`: `target.removeVolatile('nightmare')` (one a Comatose
        // holder kept after losing the ability).
        if status == Status::Sleep {
            self.end_nightmare(pokemon);
        }
        if synchronize {
            self.after_set_status(target, status, source);
        }
        // Poison Puppeteer's `onAnyAfterSetStatus` (like Synchronize, priority 0; each changes
        // a different Pokémon and Synchronize cannot poison the Poison-type source).
        if by_move {
            super::abilities::poison_puppeteer(self, target, status, source);
        }
        // Lum Berry's `onAfterSetStatus` (priority -1: after Synchronize).
        super::update::after_set_status(self, target);
        true
    }

    /// `runEvent('AfterSetStatus', target, source, effect, status)`. The only implemented
    /// handler is Synchronize on the target (not breakable, not modded in Champions): a burn,
    /// paralysis or (bad) poison from another Pokémon is passed back to it
    /// (`source.trySetStatus(status, target)`), which fails if the source already has a status
    /// or is immune. Toxic Spikes' poison skips it ([`Battle::try_set_status_from_toxic_spikes`]).
    fn after_set_status(&mut self, target: SlotRef, status: Status, source: Option<SlotRef>) {
        let Some(source) = source else {
            return;
        };
        if source == target || self.ability(target) != abilities::SYNCHRONIZE {
            return;
        }
        if matches!(status, Status::Sleep | Status::Freeze) {
            return;
        }
        self.try_set_status_from(source, status, Some(target));
    }

    /// Showdown `runEvent('SetStatus')` for a status set on `target` by another Pokémon's move.
    /// Every implemented handler only returns `false`/`null` (plus a message), so whether the
    /// status is blocked does not depend on their order:
    /// - the target's own ability (`onSetStatus`; breakable ones are skipped while an
    ///   ability-ignoring move is in progress): Water Veil (brn), Immunity (psn, tox),
    ///   Insomnia and Vital Spirit (slp), Limber (par), Comatose (everything), Purifying Salt
    ///   (everything), Leaf Guard (everything in harsh sunlight), Thermal Exchange (brn), Shields
    ///   Down (everything on Minior-Meteor, not breakable);
    /// - Sweet Veil (slp) and Pastel Veil (psn, tox) on the target or an ally
    ///   (`onAllySetStatus`; Pastel Veil's own `onSetStatus` is the same block);
    /// - Misty Terrain (everything) and Electric Terrain (slp) for a grounded target.
    ///
    /// Flower Veil, which also needs the source, is checked in [`Battle::try_set_status_from`].
    pub fn set_status_blocked(&self, target: SlotRef, status: Status) -> bool {
        let own = self.ability_unless_broken(target);
        let blocked_by_own = match own {
            a if a == abilities::WATER_VEIL || a == abilities::THERMAL_EXCHANGE => {
                status == Status::Burn
            }
            a if a == abilities::IMMUNITY => matches!(status, Status::Poison | Status::Toxic),
            a if a == abilities::INSOMNIA || a == abilities::VITAL_SPIRIT => {
                status == Status::Sleep
            }
            a if a == abilities::LIMBER => status == Status::Paralyze,
            a if a == abilities::COMATOSE || a == abilities::PURIFYING_SALT => true,
            // `target.effectiveWeather()` (Utility Umbrella hides the sun).
            a if a == abilities::LEAF_GUARD => self.weather_for(target) == Weather::Sun,
            // Shields Down (not breakable): every status on Minior-Meteor.
            a if a == abilities::SHIELDS_DOWN => super::forme::shields_up(self, target),
            _ => false,
        };
        if blocked_by_own {
            return true;
        }
        // `onAllySetStatus` runs for every active ally and the target itself: Sweet Veil (slp),
        // Pastel Veil (psn, tox; its own `onSetStatus` blocks the same for the holder).
        let veil = match status {
            Status::Sleep => Some(abilities::SWEET_VEIL),
            Status::Poison | Status::Toxic => Some(abilities::PASTEL_VEIL),
            _ => None,
        };
        if let Some(veil) = veil {
            if self
                .alive_slots(target.side)
                .into_iter()
                .any(|s| self.ability_unless_broken(s) == veil)
            {
                return true;
            }
        }
        if self.is_grounded(target) {
            match self.terrain() {
                Terrain::Misty => return true,
                Terrain::Electric if status == Status::Sleep => return true,
                _ => {}
            }
        }
        // Uproar's `onAnySetStatus` (`if (status.id === 'slp') return null;`) on any active
        // Pokémon with HP (`alliesAndSelf()` / `foes()`), the holder included.
        status == Status::Sleep
            && self
                .all_alive()
                .into_iter()
                .any(|s| self.volatile(s, Volatile::Uproar).active)
    }

    /// Safeguard on `target`'s side against an effect from `source` (its `onSetStatus` and
    /// `onTryAddVolatile`): only another Pokémon's effects are blocked (`target !== source`),
    /// and nothing without a source (`if (!effect || !source) return;`). An infiltrating move
    /// (Infiltrator) on a foe passes: the callers check it.
    fn safeguarded(&self, target: SlotRef, source: Option<SlotRef>) -> bool {
        source.is_some_and(|s| s != target)
            && self.side_effect_active(target.side, SideEffect::Safeguard)
    }

    /// Showdown `runEvent('TryAddVolatile')` for a new volatile on `target`: the ability
    /// handlers of Insomnia, Vital Spirit, Purifying Salt, Leaf Guard (in sun) and Shields Down
    /// (Minior-Meteor) on the target block Yawn; Sweet Veil (Yawn) and Aroma Veil (Attract, Disable, Encore, Heal
    /// Block, Taunt, Torment) block for the whole side; Electric Terrain blocks Yawn on a
    /// grounded target; Safeguard blocks Yawn and confusion from another Pokémon. Of those
    /// volatiles only Yawn is implemented; the others (and Misty Terrain's confusion block)
    /// guard their future implementation.
    pub fn add_volatile_blocked(&self, target: SlotRef, volatile: Volatile) -> bool {
        let condition = volatile.condition();
        let yawn = condition == conditions::YAWN;
        // Focus Punch's condition: `onTryAddVolatile(status) { if (status.id === 'flinch')
        // return null; }`.
        if volatile == Volatile::Flinch && self.volatile(target, Volatile::FocusPunch).active {
            return true;
        }
        // Misty Terrain: `if (status.id === 'confusion' && target.isGrounded()) return false`.
        if volatile == Volatile::Confusion
            && self.terrain() == Terrain::Misty
            && self.is_grounded(target)
        {
            return true;
        }
        let blocked_by_own = match self.ability_unless_broken(target) {
            a if a == abilities::INSOMNIA
                || a == abilities::VITAL_SPIRIT
                || a == abilities::PURIFYING_SALT =>
            {
                yawn
            }
            a if a == abilities::LEAF_GUARD => yawn && self.weather_for(target) == Weather::Sun,
            // Inner Focus: `if (status.id === 'flinch') return null;`
            a if a == abilities::INNER_FOCUS => condition == conditions::FLINCH,
            // Own Tempo: `if (status.id === 'confusion') return null;`
            a if a == abilities::OWN_TEMPO => condition == conditions::CONFUSION,
            // Shields Down: Yawn on Minior-Meteor.
            a if a == abilities::SHIELDS_DOWN => yawn && super::forme::shields_up(self, target),
            _ => false,
        };
        if blocked_by_own {
            return true;
        }
        let aroma = [
            conditions::ATTRACT,
            conditions::DISABLE,
            conditions::ENCORE,
            conditions::HEALBLOCK,
            conditions::TAUNT,
            conditions::TORMENT,
        ]
        .contains(&condition);
        let veiled = self.alive_slots(target.side).into_iter().any(|s| {
            let ability = self.ability_unless_broken(s);
            (ability == abilities::SWEET_VEIL && yawn)
                || (ability == abilities::AROMA_VEIL && aroma)
        });
        // Flower Veil's `onAllyTryAddVolatile`: Yawn on a Grass type.
        let flower_veiled = yawn && super::abilities::flower_veil_holder(self, target).is_some();
        // Electric Terrain's `onTryAddVolatile`: Yawn fails on a grounded target (Misty
        // Terrain's only blocks confusion). Safeguard: Yawn and confusion from the user of the
        // move in progress, if that is another Pokémon.
        // An infiltrating move's volatile on a foe passes (`effect.infiltrates &&
        // !target.isAlly(source)`).
        let infiltrates = self
            .active_move
            .is_some_and(|m| m.infiltrates && m.user.side != target.side);
        let safeguard = (yawn || condition == conditions::CONFUSION)
            && !infiltrates
            && self.safeguarded(target, self.active_move.map(|m| m.user));
        veiled
            || flower_veiled
            || safeguard
            || (yawn && self.terrain() == Terrain::Electric && self.is_grounded(target))
    }

    /// Showdown `pokemon.faint()`: HP drops to 0 at once, without Damage handlers (Focus Sash,
    /// Sturdy and Endure do not apply), and the Pokémon is queued to faint.
    pub fn faint(&mut self, slot: SlotRef) {
        let Some(pokemon) = self.alive(slot) else {
            return;
        };
        let hp = i32::from(self.mon(pokemon).hp);
        self.lose_hp(slot, pokemon, hp, None);
    }

    pub fn set_status_turns(&mut self, pokemon: PokemonRef, turns: i8) {
        let old = self.mon(pokemon).status_turns;
        if old != turns {
            self.apply(Instruction::SetStatusTurns {
                target: pokemon,
                old,
                new: turns,
            });
        }
    }

    /// Showdown `cureStatus` / `clearStatus`: a sleeping Pokémon also loses Nightmare (`if
    /// (this.status === 'slp' && this.removeVolatile('nightmare'))`).
    pub fn cure_status(&mut self, pokemon: PokemonRef) {
        let old = self.mon(pokemon).status;
        if self.mon(pokemon).hp == 0 || old == Status::None {
            return;
        }
        if old == Status::Sleep {
            self.end_nightmare(pokemon);
        }
        self.apply(Instruction::ChangeStatus {
            target: pokemon,
            old,
            new: Status::None,
        });
        self.set_status_turns(pokemon, 0);
    }

    /// `pokemon.removeVolatile('nightmare')` for an active `pokemon` (its sleep ends, or a new
    /// sleep starts: the `slp` condition's `onStart`).
    pub fn end_nightmare(&mut self, pokemon: PokemonRef) {
        if let Some(slot) = State::<N>::slot_refs().find(|&s| self.occupant(s) == Some(pokemon)) {
            self.remove_volatile(slot, Volatile::Nightmare);
        }
    }

    // ---- volatiles -----------------------------------------------------------------------

    /// Showdown `addVolatile` for the implemented volatiles (with Stall's `onRestart`).
    pub fn add_volatile(&mut self, target: SlotRef, volatile: Volatile) -> bool {
        self.add_volatile_from(target, volatile, MoveId::NONE)
    }

    /// Showdown `addVolatile(status, source, sourceEffect)`: `TryAddVolatile`, then the
    /// condition's `onStart` (or `onRestart` when it is already up; conditions without one
    /// fail). `source_move` is the move that adds it (a locked move remembers it).
    pub fn add_volatile_from(
        &mut self,
        target: SlotRef,
        volatile: Volatile,
        source_move: MoveId,
    ) -> bool {
        let Some(pokemon) = self.alive(target) else {
            return false;
        };
        let old = self.volatile(target, volatile);
        let new = if old.active {
            match volatile {
                Volatile::Stall => VolatileState {
                    active: true,
                    duration: 2,
                    counter: if old.counter < STALL_COUNTER_MAX {
                        old.counter * 3
                    } else {
                        old.counter
                    },
                    ..VolatileState::NONE
                },
                // `if (this.effectState.trueDuration >= 2) this.effectState.duration = 2`.
                Volatile::LockedMove => VolatileState {
                    duration: if old.hidden >= 2 { 2 } else { old.duration },
                    ..old
                },
                // Helping Hand's `onRestart`: `this.effectState.multiplier *= 1.5`.
                Volatile::HelpingHand => VolatileState {
                    counter: old.counter + 1,
                    ..old
                },
                // Charge's `onRestart` only announces it (the move, Electromorphosis, Wind Power):
                // it returns nothing, a success, and the state stays.
                Volatile::Charge => return true,
                // Laser Focus's `onRestart`: `this.effectState.duration = 2`.
                Volatile::LaserFocus => VolatileState { duration: 2, ..old },
                // Power Trick's and Power Shift's `onRestart`: `pokemon.removeVolatile(...)` (its
                // `onEnd` swaps back); it returns nothing, a success.
                Volatile::PowerTrick | Volatile::PowerShift => {
                    self.remove_volatile(target, volatile);
                    return true;
                }
                // Stockpile's `onRestart`: `if (this.effectState.layers >= 3) return false;` then
                // one more layer and its raises.
                Volatile::Stockpile => {
                    if old.counter >= 3 {
                        return false;
                    }
                    super::conditions::stockpile_raise(self, target, old)
                }
                // Smack Down's `onRestart`: a holder in the air again (Fly, Bounce) comes down
                // (`conditions::smack_down_lands`); it returns nothing.
                Volatile::SmackDown => {
                    super::conditions::smack_down_lands(self, target);
                    return true;
                }
                // Heal Block's `onRestart`: nothing from Psychic Noise; otherwise `if
                // (!source.moveThisTurnResult) source.moveThisTurnResult = false;`. Either way it
                // returns nothing, so `addVolatile` succeeds without changing the volatile.
                Volatile::HealBlock => {
                    let source = self
                        .active_move
                        .filter(|m| self.occupant(m.user) == Some(m.pokemon));
                    if let Some(source) =
                        source.filter(|m| m.id != crate::dex::moves::PSYCHIC_NOISE)
                    {
                        let result = self.state.slot(source.user).history.move_this_turn_result;
                        if result != crate::state::MoveResult::Succeeded {
                            self.set_move_result(source.user, crate::state::MoveResult::Failed);
                        }
                    }
                    return true;
                }
                // Ally Switch's `onRestart`: `randomChance(1, counter)`, else `delete
                // pokemon.volatiles['allyswitch']` (no `onEnd`) and fail; on success the counter
                // triples below `counterMax` (729) and the duration is 2 again.
                Volatile::AllySwitch => {
                    if !self.rng.chance(1, u32::from(old.counter.max(1))) {
                        self.delete_volatile(target, volatile);
                        return false;
                    }
                    VolatileState {
                        duration: 2,
                        counter: if old.counter < STALL_COUNTER_MAX {
                            old.counter * 3
                        } else {
                            old.counter
                        },
                        ..old
                    }
                }
                // No onRestart.
                _ => return false,
            }
        } else {
            // TryAddVolatile handlers (the new volatile only; a restart skips them).
            if self.add_volatile_blocked(target, volatile) {
                return false;
            }
            let mut new = VolatileState {
                active: true,
                duration: volatile.initial_duration(),
                // Stall's first counter; Helping Hand's `onStart`: `multiplier = 1.5` (one
                // application).
                counter: match volatile {
                    // Stall; Ally Switch's `onStart`: `this.effectState.counter = 3`.
                    Volatile::Stall | Volatile::AllySwitch => 3,
                    Volatile::HelpingHand => 1,
                    _ => 0,
                },
                ..VolatileState::NONE
            };
            match volatile {
                // `this.effectState.time = this.random(2, 6)`.
                Volatile::Confusion => new.time = 2 + self.rng.uniform(4) as u8,
                // `trueDuration = this.random(2, 4)`, the move that locked.
                Volatile::LockedMove => {
                    new.hidden = 2 + self.rng.uniform(2) as u8;
                    new.mv = source_move;
                }
                // Encore's `onStart`: the target's last move must be usable and encorable;
                // one more turn if the target already moved.
                Volatile::Encore => {
                    let last = self.state.slot(target).last_move;
                    if last.is_none() || last.data().flags.contains(MoveFlags::FAILENCORE) {
                        return false;
                    }
                    let slot_move = self.mon(pokemon).moves.iter().find(|m| m.id == last);
                    if !slot_move.is_some_and(|m| m.pp > 0) {
                        return false;
                    }
                    new.mv = last;
                    if self.will_move(target).is_none() {
                        new.duration += 1;
                    }
                }
                // The other conditions' `onStart` (`conditions::volatile_start`).
                _ => {
                    if !super::conditions::volatile_start(self, target, volatile, &mut new) {
                        return false;
                    }
                }
            }
            new
        };
        self.apply(Instruction::SetVolatile {
            target,
            volatile,
            old,
            new,
        });
        if volatile == Volatile::Encore && !old.active {
            self.encore_change_action(target, new.mv);
        }
        true
    }

    /// The rest of the Champions mod's `encore.condition.onStart`: when the target still has a
    /// move action queued with another move and does not hold Mental Herb, `queue.changeAction`
    /// replaces that action (FF-parity-harness; the base game only swaps the move when it runs,
    /// `onOverrideAction`). The new action (`resolveAction`) keeps the `order` (After You,
    /// Quash), uses the encored move with the target `getRandomTarget` draws now (for a move
    /// that takes a chosen target: `moves::resolved_target_loc`; it becomes the user's
    /// `lastMoveTargetLoc`, and the move re-draws when it runs only if that target is gone), and
    /// runs `FractionalPriority` again (the constants, Quick Draw, Quick Claw, Custap Berry).
    /// From the next re-sort on its priority is the encored move's (an encored Protect jumps to
    /// +4), which `action_key` reads from the action's move. The random insertion point among
    /// equal actions does not matter: the queue is re-sorted before the next move.
    /// `resolveAction` also queues the encored move's `beforeTurnMove` (Counter, Mirror Coat) or
    /// `priorityChargeMove` (Focus Punch, Beak Blast, Chilly Reception) action, which
    /// `insertChoice` puts ahead of every move action: it runs next and adds its condition (oracle
    /// `nn-encore-counter-hit`, `nn-encore-focus-punch-hit`, `nn-encore-beak-blast-hit`,
    /// `rr-encore-counter`). (With the target's own action moved up by After You, order 3, the
    /// moved action would come first in the engine but after the callback in Showdown, whose
    /// `insertChoice` places the pair by the callback's order; not modelled.)
    fn encore_change_action(&mut self, target: SlotRef, encored: MoveId) {
        use super::queue::{Action, ActionKind};
        let Some(i) = self.will_move(target) else {
            return;
        };
        let action = self.queue[i];
        let ActionKind::Move { id, .. } = action.kind else {
            return;
        };
        let pokemon = action.pokemon;
        if id == encored || self.item(target) == items::MENTAL_HERB {
            return;
        }
        let mut fractional = super::items::fractional_priority_tenths(self.state, target, encored);
        if let Some(t) = super::abilities::quick_draw(self, target, pokemon, encored) {
            fractional = t;
        }
        if let Some(t) = super::items::quick_claw(self, target, pokemon, fractional, encored) {
            fractional = t;
        }
        if let Some(t) = super::items::custap(self, target, pokemon, fractional, encored) {
            fractional = t;
        }
        let target_loc = super::moves::resolved_target_loc(self, target, encored);
        if let Some(i) = self.will_move(target) {
            self.queue[i].kind = ActionKind::Move {
                id: encored,
                target: target_loc,
                original: None,
                fractional_tenths: fractional,
                round_source: None,
            };
        }
        let callback = if super::moves::has_before_turn_callback(encored) {
            Some(ActionKind::BeforeTurnMove { id: encored })
        } else if super::moves::has_priority_charge_callback(encored) {
            Some(ActionKind::PriorityCharge { id: encored })
        } else {
            None
        };
        if let Some(kind) = callback {
            self.queue.push(Action {
                slot: target,
                pokemon,
                kind,
                order: None,
            });
        }
    }

    /// Showdown `removeVolatile`: the condition's `onEnd` (a locked move that ends by fatigue
    /// confuses its user), then it is gone.
    pub fn remove_volatile(&mut self, target: SlotRef, volatile: Volatile) -> bool {
        let old = self.volatile(target, volatile);
        if !old.active || self.alive(target).is_none() {
            return false;
        }
        self.apply(Instruction::SetVolatile {
            target,
            volatile,
            old,
            new: VolatileState::NONE,
        });
        // The substitute's HP goes with it (its `onEnd` only logs).
        if volatile == Volatile::Substitute {
            self.set_substitute_hp(target, 0);
        }
        // onEnd.
        if volatile == Volatile::LockedMove && old.hidden <= 1 {
            self.add_volatile(target, Volatile::Confusion);
        }
        // Power Trick, Power Shift: the stored Attack and Defense trade places back.
        if matches!(volatile, Volatile::PowerTrick | Volatile::PowerShift) {
            super::conditions::swap_stored_stats(self, target, 0, 1);
        }
        // Stockpile: the raises that took are taken back.
        if volatile == Volatile::Stockpile {
            super::conditions::stockpile_end(self, target, old);
        }
        // `twoturnmove.onEnd`: the move's own volatile goes with it (an aborted second turn).
        if volatile == Volatile::TwoTurnMove {
            if let Some(own) = super::conditions::charge_volatile(old.mv) {
                self.remove_volatile(target, own);
            }
        }
        true
    }

    /// `delete pokemon.volatiles[id]`: gone without its `onEnd`.
    pub fn delete_volatile(&mut self, target: SlotRef, volatile: Volatile) {
        let old = self.volatile(target, volatile);
        if old.active {
            self.apply(Instruction::SetVolatile {
                target,
                volatile,
                old,
                new: VolatileState::NONE,
            });
            if volatile == Volatile::Substitute {
                self.set_substitute_hp(target, 0);
            }
        }
    }

    /// Whether the Pokémon in `slot` is behind a substitute (`volatiles['substitute']`).
    pub fn has_substitute(&self, slot: SlotRef) -> bool {
        self.volatile(slot, Volatile::Substitute).active
    }

    /// The substitute's `effectState.hp` ([`crate::state::Slot::substitute_hp`]).
    pub fn set_substitute_hp(&mut self, slot: SlotRef, hp: i16) {
        let old = self.state.slot(slot).substitute_hp;
        if old != hp {
            self.apply(Instruction::SetSubstituteHp {
                target: slot,
                old,
                new: hp,
            });
        }
    }

    pub fn set_volatile_state(&mut self, target: SlotRef, volatile: Volatile, new: VolatileState) {
        let old = self.volatile(target, volatile);
        if old != new {
            self.apply(Instruction::SetVolatile {
                target,
                volatile,
                old,
                new,
            });
        }
    }

    // ---- boosts ----------------------------------------------------------------------------

    /// Showdown `side.pokemonLeft > 0`: `pokemonLeft` only drops when `faintMessages` processes
    /// a faint, so a Pokémon at 0 HP still in its slot (its faint is queued) counts, and a
    /// processed one has left its slot.
    pub fn has_pokemon_left(&self, side: SideId) -> bool {
        let s = self.state.side(side);
        s.party.iter().any(|m| m.hp > 0) || s.slots.iter().any(|slot| slot.party_index.is_some())
    }

    /// Showdown `battle.boost(boost, target, source, effect)` with its events (WORKPLAN F16):
    /// `ChangeBoost` (Contrary, Simple), the ±6 cap, `TryBoost` (Clear Body family, Hyper
    /// Cutter, Big Pecks, Mirror Armor, Guard Dog), each stage change with `AfterEachBoost`
    /// (Competitive, Defiant), then `AfterBoost` (Rattled). Returns whether any stage changed.
    ///
    /// `source` is who caused it (the user of a move, the Intimidate holder, the boosted
    /// Pokémon itself for its own ability); `effect` what caused it. Handlers that block only
    /// look at negative changes from someone else, as in Showdown.
    pub fn boost_by(
        &mut self,
        target: SlotRef,
        boosts: &[i8; BOOST_COUNT],
        source: Option<SlotRef>,
        effect: BoostEffect,
    ) -> bool {
        if self.alive(target).is_none() {
            return false;
        }
        // `if (this.gen > 5 && !target.side.foePokemonLeft()) return false;`
        if !self.has_pokemon_left(target.side.other()) {
            return false;
        }
        let from_other = source.is_some_and(|s| s != target);
        let mut boost = *boosts;

        // ChangeBoost: the target's own ability (Ripen: a berry's boosts, `effect.isBerry`).
        match self.ability_unless_broken(target) {
            a if a == abilities::CONTRARY => boost.iter_mut().for_each(|b| *b = -*b),
            a if a == abilities::SIMPLE => boost.iter_mut().for_each(|b| *b *= 2),
            a if a == abilities::RIPEN
                && matches!(effect, BoostEffect::Item(item) if item.data().is_berry) =>
            {
                boost.iter_mut().for_each(|b| *b *= 2)
            }
            _ => {}
        }
        // getCappedBoost.
        let requested_atk = boost[0];
        for (stat, b) in boost.iter_mut().enumerate() {
            let current = self.state.slot(target).boosts[stat];
            *b = (current + *b).clamp(-6, 6) - current;
        }
        // Showdown keeps a capped stat's key at 0 (`boost.atk === 0`), unlike a TryBoost
        // handler's `delete`; Adrenaline Orb tells them apart.
        let atk_capped_to_zero = requested_atk != 0 && boost[0] == 0;
        // TryBoost (abilities in `resolvePriority` order: Guard Dog's priority 2 first; the
        // rest only delete, so their order is moot).
        let ability = self.ability_unless_broken(target);
        if ability == abilities::GUARD_DOG
            && effect == BoostEffect::Ability(abilities::INTIMIDATE)
            && boost[0] != 0
        {
            boost[0] = 0;
            let mut up = NO_BOOSTS;
            up[0] = 1;
            self.boost_by(
                target,
                &up,
                Some(target),
                BoostEffect::Ability(abilities::GUARD_DOG),
            );
        }
        // Inner Focus, Own Tempo, Oblivious (breakable), Scrappy: `if (effect.name ===
        // 'Intimidate' && boost.atk) delete boost.atk`.
        if [
            abilities::INNER_FOCUS,
            abilities::OWN_TEMPO,
            abilities::OBLIVIOUS,
            abilities::SCRAPPY,
        ]
        .contains(&ability)
            && effect == BoostEffect::Ability(abilities::INTIMIDATE)
        {
            boost[0] = 0;
        }
        // Mist on the target's side (`onTryBoost`): another Pokémon's drops are blocked, except
        // an infiltrating move's on a foe (`effect.effectType === 'Move' && effect.infiltrates &&
        // !target.isAlly(source)`: the effect is the move in progress).
        let infiltrates = self.active_move.is_some_and(|m| {
            m.infiltrates
                && effect == BoostEffect::Move(m.id)
                && source.is_some_and(|s| s.side != target.side)
        });
        if from_other && !infiltrates && self.side_effect_active(target.side, SideEffect::Mist) {
            boost.iter_mut().filter(|b| **b < 0).for_each(|b| *b = 0);
        }
        // `if (source && target === source) return;` — no source counts as "from another".
        let blocks_drops = source.is_none_or(|s| s != target);
        if blocks_drops {
            // Clear Amulet (the target's item, `onTryBoostPriority: 1`: before every
            // priority-0 handler, so Mirror Armor finds nothing to reflect) deletes every drop.
            if self.item(target) == items::CLEAR_AMULET {
                boost.iter_mut().filter(|b| **b < 0).for_each(|b| *b = 0);
            }
            // Flower Veil (`onAllyTryBoost`) deletes a Grass target's drops; it runs before or
            // after the target's own Mirror Armor by Speed (`abilities::flower_veil_first`).
            if super::abilities::flower_veil_first(self, target, &boost, source, effect) {
                boost.iter_mut().filter(|b| **b < 0).for_each(|b| *b = 0);
            }
            match ability {
                a if a == abilities::CLEAR_BODY
                    || a == abilities::WHITE_SMOKE
                    || a == abilities::FULL_METAL_BODY =>
                {
                    boost.iter_mut().filter(|b| **b < 0).for_each(|b| *b = 0);
                }
                a if a == abilities::HYPER_CUTTER => boost[0] = boost[0].max(0),
                a if a == abilities::BIG_PECKS => boost[1] = boost[1].max(0),
                // Keen Eye, Illuminate, Mind's Eye (breakable): accuracy drops.
                a if a == abilities::KEEN_EYE
                    || a == abilities::ILLUMINATE
                    || a == abilities::MINDS_EYE =>
                {
                    boost[5] = boost[5].max(0);
                }
                a if a == abilities::MIRROR_ARMOR
                    && source.is_some()
                    && effect != BoostEffect::Ability(abilities::MIRROR_ARMOR) =>
                {
                    let source = source.expect("checked");
                    for stat in 0..BOOST_COUNT {
                        if boost[stat] >= 0 || self.state.slot(target).boosts[stat] == -6 {
                            continue;
                        }
                        let mut reflected = NO_BOOSTS;
                        reflected[stat] = boost[stat];
                        boost[stat] = 0;
                        if self.alive(source).is_some() {
                            self.boost_by(
                                source,
                                &reflected,
                                Some(target),
                                BoostEffect::Ability(abilities::MIRROR_ARMOR),
                            );
                        }
                    }
                }
                _ => {}
            }
            // Flower Veil after Mirror Armor: whatever drop Mirror Armor left (at -6).
            if super::abilities::flower_veil_holder(self, target).is_some() {
                boost.iter_mut().filter(|b| **b < 0).for_each(|b| *b = 0);
            }
        }

        let mut changed = false;
        for (stat, &amount) in boost.iter().enumerate() {
            if amount == 0 {
                continue;
            }
            if self.alive(target).is_none() {
                break;
            }
            self.apply(Instruction::Boost {
                target,
                stat: stat as u8,
                amount,
            });
            changed = true;
            // AfterEachBoost: Competitive / Defiant react to each drop from a foe.
            if amount < 0 && from_other && source.is_some_and(|s| s.side != target.side) {
                let reacting = self.ability_unless_broken(target);
                let raised = if reacting == abilities::COMPETITIVE {
                    Some(2)
                } else if reacting == abilities::DEFIANT {
                    Some(0)
                } else {
                    None
                };
                if let Some(index) = raised {
                    let mut up = NO_BOOSTS;
                    up[index] = 2;
                    self.boost_by(target, &up, Some(target), BoostEffect::Ability(reacting));
                }
            }
        }
        // AfterBoost: Rattled after Intimidate's Attack drop.
        if effect == BoostEffect::Ability(abilities::INTIMIDATE)
            && boosts[0] != 0
            && self.alive(target).is_some()
            && self.ability_unless_broken(target) == abilities::RATTLED
        {
            let mut up = NO_BOOSTS;
            up[4] = 1;
            self.boost_by(
                target,
                &up,
                Some(target),
                BoostEffect::Ability(abilities::RATTLED),
            );
        }
        // AfterBoost of items (after the target's ability): Adrenaline Orb, the foes' Mirror
        // Herbs; the foes' Opportunist (`onFoeAfterBoost`; each only adds to its own copies).
        super::items::after_boost(self, target, &boost, effect, atk_capped_to_zero);
        super::abilities::opportunist_after_boost(self, target, &boost, effect);
        // `if (success)`: `statsRaisedThisTurn` / `statsLoweredThisTurn` from the boost table
        // that was applied (after the cap and TryBoost), while a move reads them.
        if changed {
            self.record_stat_changes(target, &boost);
        }
        changed
    }

    /// Showdown `getStat`'s `ModifyBoost`: Unaware (`onAnyModifyBoost`) zeroes the boosts an
    /// attack does not see. `viewer` is the other party of the attack (the target when the
    /// attacker's stats are read, the attacker when the target's are): if it has Unaware, the
    /// attacker's Atk/Def/SpA/accuracy or the target's Def/SpD/evasion count as 0.
    pub fn boost_seen(
        &self,
        holder: SlotRef,
        stat: usize,
        viewer: SlotRef,
        as_attacker: bool,
    ) -> i8 {
        let mut boost = self.state.slot(holder).boosts[stat];
        // Foresight's and Miracle Eye's `onModifyBoost` on the holder: positive evasion is 0.
        if stat == 6
            && boost > 0
            && (self.volatile(holder, Volatile::Foresight).active
                || self.volatile(holder, Volatile::MiracleEye).active)
        {
            boost = 0;
        }
        if viewer == holder || self.ability_unless_broken(viewer) != abilities::UNAWARE {
            return boost;
        }
        let ignored = if as_attacker {
            // Unaware target: atk, def, spa, accuracy of the attacker.
            matches!(stat, 0 | 1 | 2 | 5)
        } else {
            // Unaware attacker: def, spd, evasion of the target.
            matches!(stat, 1 | 3 | 6)
        };
        if ignored {
            0
        } else {
            boost
        }
    }

    // ---- items ------------------------------------------------------------------------------

    /// Showdown `useItem`: the held item is consumed and becomes `lastItem`.
    pub fn use_item(&mut self, slot: SlotRef) -> bool {
        let Some(pokemon) = self.alive(slot) else {
            return false;
        };
        let (item, last) = (self.mon(pokemon).item, self.mon(pokemon).last_item);
        if item.is_none() {
            return false;
        }
        self.apply(Instruction::SetLastItem {
            target: pokemon,
            old: last,
            new: item,
        });
        self.apply(Instruction::SetItem {
            target: pokemon,
            old: item,
            new: ItemId::NONE,
        });
        // `clearEffectState(itemState)`: Eject Pack's flag goes with the item.
        self.delete_volatile(slot, Volatile::EjectPack);
        // `this.usedItemThisTurn = true` (Pickup).
        self.record_used_item(slot);
        // The only berries consumed through here are the resist berries, which Showdown eats
        // (`eatItem`: `runEvent('EatItem')` after their empty `onEat`, then `ateBerry = true`,
        // Belch). EatItem comes before the item is gone; nothing it runs reads the item.
        if item.data().is_berry {
            super::abilities::eat_item_event(self, slot, item, false);
            self.record_ate_berry(pokemon);
        }
        // AfterUseItem: Unburden, an ally's Symbiosis.
        super::abilities::unburden(self, slot);
        super::abilities::symbiosis(self, slot);
        true
    }

    /// Whether the item's own `TakeItem` handler lets it be removed from its holder.
    pub fn item_can_be_taken(&self, slot: SlotRef) -> bool {
        let Some(mon) = self.slot_mon(slot) else {
            return false;
        };
        let item = mon.item.data();
        if mon.item.is_none() || item.cannot_be_taken {
            return false;
        }
        // Booster Energy stays with a Paradox Pokémon (its `onTakeItem`).
        // (`source.baseSpecies`: a transformed holder's own species.)
        let own = mon.untransformed_species();
        if mon.item == items::BOOSTER_ENERGY && super::abilities::booster_energy_kept(own) {
            return false;
        }
        // Mega Stones: `onTakeItem(item, source) { return !item.megaStone?.[source.baseSpecies.baseSpecies]; }`
        let base = own.data().base_species;
        let base = if base.is_none() { own } else { base };
        !item.mega_stone.iter().any(|&(from, _)| from == base)
    }

    /// Sticky Hold's `onTakeItem` (breakable) for the holder in `slot`: `if (!pokemon.hp ||
    /// pokemon.item === 'stickybarb') return; if ((source && source !== pokemon) ||
    /// this.activeMove.id === 'knockoff') return false;` — another Pokémon cannot take its item,
    /// nor can Knock Off remove it, while it has HP (a Sticky Barb goes).
    pub fn sticky_hold_keeps(&self, slot: SlotRef, source: Option<SlotRef>) -> bool {
        let Some(mon) = self.slot_mon(slot) else {
            return false;
        };
        let knock_off = self
            .active_move
            .is_some_and(|m| m.id == crate::dex::moves::KNOCK_OFF);
        self.ability_unless_broken(slot) == abilities::STICKY_HOLD
            && mon.hp > 0
            && mon.item != items::STICKY_BARB
            && (source.is_some_and(|s| s != slot) || knock_off)
    }

    /// Showdown `takeItem()` without a source (the holder itself: Knock Off, Sticky Barb): see
    /// [`Battle::take_item_by`].
    pub fn take_item(&mut self, slot: SlotRef) -> bool {
        self.take_item_by(slot, None)
    }

    /// Showdown `takeItem(source)`: removed without becoming `lastItem`. Works on a target at 0
    /// HP that has not been processed as fainted yet, as in Showdown. `source` is who takes it
    /// (Thief, Bug Bite, Magician, Pickpocket, ...); `None` is the holder itself.
    pub fn take_item_by(&mut self, slot: SlotRef, source: Option<SlotRef>) -> bool {
        let Some(pokemon) = self.occupant(slot) else {
            return false;
        };
        if self.mon(pokemon).item.is_none() {
            return false;
        }
        // TakeItem: the holder's ability (Sticky Hold, Unburden) before the item's own handler;
        // Sticky Hold's `false` ends the event.
        if self.sticky_hold_keeps(slot, source) {
            return false;
        }
        super::abilities::unburden(self, slot);
        if !self.item_can_be_taken(slot) {
            return false;
        }
        let old = self.mon(pokemon).item;
        self.apply(Instruction::SetItem {
            target: pokemon,
            old,
            new: ItemId::NONE,
        });
        // The item's state ends with it (Eject Pack's flag).
        self.delete_volatile(slot, Volatile::EjectPack);
        // `singleEvent('End', item, oldItemState, this)` on the holder, which holds nothing now:
        // Utility Umbrella's WeatherChange (its `inactive` mark stays on the cleared state).
        if super::items::umbrella_end(self, slot, old) {
            self.umbrella_inactive.push(pokemon);
        }
        true
    }

    // ---- per-slot counters -------------------------------------------------------------------

    /// Showdown `pokemon.activeTurns > 0` during a turn: the Pokémon was already active when the
    /// turn started (`endTurn` counts every active Pokémon; `switchIn` resets it to 0).
    ///
    /// `State` has no such counter, but `activeTurns` is 0 exactly while the slot history's
    /// `newlySwitched` is set (both reset by `switchIn`, both cleared by `endTurn`, the battle
    /// start's included), which a move from Dancer after switching in does not change (it counts
    /// `activeMoveActions`).
    pub fn active_since_turn_start(&self, slot: SlotRef) -> bool {
        self.occupant(slot).is_some() && !self.state.slot(slot).history.newly_switched
    }

    /// `pokemon.switchFlag = <move id | true>` (F6): the occupant must switch out at the next
    /// request.
    pub fn set_switch_flag(&mut self, slot: SlotRef, flag: SwitchFlag) {
        let old = self.state.slot(slot).switch_flag;
        if old != flag {
            self.apply(Instruction::SetSwitchFlag {
                target: slot,
                old,
                new: flag,
            });
        }
    }

    /// Revival Blessing's revival: `hp = 1; sethp(maxhp / 2)` (truncated), `status = ''`,
    /// `fainted = false` (the side's `totalFainted` stays).
    pub fn revive(&mut self, pokemon: PokemonRef) {
        let mon = self.mon(pokemon);
        if mon.hp > 0 {
            return;
        }
        let (old, half) = (mon.status, mon.max_hp / 2);
        if old != Status::None {
            self.apply(Instruction::ChangeStatus {
                target: pokemon,
                old,
                new: Status::None,
            });
        }
        self.apply(Instruction::Heal {
            target: pokemon,
            amount: half.max(1),
        });
    }

    /// `pokemon.illusion` (EE2), as a flag.
    pub fn set_illusion(&mut self, pokemon: PokemonRef, illusion: bool) {
        let old = self.mon(pokemon).illusion;
        if old != illusion {
            self.apply(Instruction::SetIllusion {
                target: pokemon,
                old,
                new: illusion,
            });
        }
    }

    /// `pokemon.switchFlag = false`.
    pub fn clear_switch_flag(&mut self, slot: SlotRef) {
        self.set_switch_flag(slot, SwitchFlag::None);
    }

    /// The end of an action that used a move (`clearActiveMove()` after `runAction`, not a failed
    /// one): `battle.lastMove = activeMove` (`State::last_move`), recorded only while a Copycat
    /// is in a party.
    pub fn record_battle_last_move(&mut self, id: MoveId) {
        if !self.history_readers.last_move || id.is_none() {
            return;
        }
        let old = self.state.last_move;
        if old != id {
            self.apply(Instruction::SetBattleLastMove { old, new: id });
        }
    }

    /// `moveUsed(move, targetLoc)`'s `lastMoveTargetLoc`, recorded only while an Instruct is in a
    /// party.
    pub fn set_last_move_target_loc(&mut self, slot: SlotRef, loc: i8) {
        if !self.history_readers.last_move_target_loc {
            return;
        }
        let old = self.state.slot(slot).last_move_target_loc;
        if old != loc {
            self.apply(Instruction::SetLastMoveTargetLoc {
                target: slot,
                old,
                new: loc,
            });
        }
    }

    pub fn set_last_move(&mut self, slot: SlotRef, id: MoveId) {
        let old = self.state.slot(slot).last_move;
        if old != id {
            self.apply(Instruction::SetLastMove {
                target: slot,
                old,
                new: id,
            });
        }
    }

    pub fn increment_move_actions(&mut self, slot: SlotRef) {
        let old = self.state.slot(slot).move_actions;
        self.apply(Instruction::SetMoveActions {
            target: slot,
            old,
            new: old.saturating_add(1),
        });
    }

    pub fn set_field(&mut self, effect: FieldEffect, new: Effect) {
        let old = self.state.field[effect as usize];
        if old != new {
            self.apply(Instruction::SetField { effect, old, new });
        }
    }

    /// Sets a side condition. An entry hazard that starts or ends also updates the side's
    /// [`crate::field::HazardOrder`] (Showdown `effectOrder`: the order its `onSwitchIn` runs).
    pub fn set_side_effect(&mut self, side: SideId, effect: SideEffect, new: Effect) {
        let old = self.state.side(side).effects[effect as usize];
        if old != new {
            if old.is_active() != new.is_active() {
                let mut history = self.state.side(side).history;
                history.hazard_order = history.hazard_order.changed(
                    &self.state.side(side).effects,
                    effect,
                    new.is_active(),
                );
                self.set_side_history(side, history);
            }
            self.apply(Instruction::SetSideEffect {
                side,
                effect,
                old,
                new,
            });
        }
    }

    pub fn unsupported(&self, what: impl Into<String>) -> TurnError {
        TurnError::Unsupported(what.into())
    }
}

/// Showdown `stall.counterMax`.
const STALL_COUNTER_MAX: u16 = 729;

/// Whether `ability`'s `onUpdate` cures `status` (Water Veil, Thermal Exchange, Water Bubble:
/// brn; Immunity: psn, tox; Insomnia, Vital Spirit: slp; Limber: par; Magma Armor: frz).
///
/// The cure runs in the `Update` event (`abilities::on_update`). The same abilities block the
/// status from being set, so a holder only has it when something got past them: a move that
/// ignores abilities (they are breakable, and skipped at the hit's Update while the move is in
/// progress; the Update after the action cures), or gaining the ability while statused (Mega
/// Evolution, Trace, a switch-in with a status from before).
pub(crate) fn cured_on_update(ability: AbilityId, status: Status) -> bool {
    match ability {
        a if a == abilities::WATER_VEIL
            || a == abilities::THERMAL_EXCHANGE
            || a == abilities::WATER_BUBBLE =>
        {
            status == Status::Burn
        }
        a if a == abilities::IMMUNITY => matches!(status, Status::Poison | Status::Toxic),
        a if a == abilities::INSOMNIA || a == abilities::VITAL_SPIRIT => status == Status::Sleep,
        a if a == abilities::LIMBER => status == Status::Paralyze,
        a if a == abilities::MAGMA_ARMOR => status == Status::Freeze,
        a if a == abilities::PASTEL_VEIL => matches!(status, Status::Poison | Status::Toxic),
        _ => false,
    }
}

/// What caused a boost (Showdown's `effect` in `boost()`), for the handlers that look at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoostEffect {
    Move(MoveId),
    Ability(AbilityId),
    #[allow(dead_code)]
    Item(ItemId),
}

pub(crate) fn weather_from(value: u8) -> Weather {
    match value {
        v if v == Weather::Sun as u8 => Weather::Sun,
        v if v == Weather::Rain as u8 => Weather::Rain,
        v if v == Weather::Sand as u8 => Weather::Sand,
        v if v == Weather::Snow as u8 => Weather::Snow,
        v if v == Weather::HarshSun as u8 => Weather::HarshSun,
        v if v == Weather::HeavyRain as u8 => Weather::HeavyRain,
        v if v == Weather::StrongWinds as u8 => Weather::StrongWinds,
        _ => Weather::None,
    }
}

pub(crate) fn terrain_from(value: u8) -> Terrain {
    match value {
        v if v == Terrain::Electric as u8 => Terrain::Electric,
        v if v == Terrain::Grassy as u8 => Terrain::Grassy,
        v if v == Terrain::Misty as u8 => Terrain::Misty,
        v if v == Terrain::Psychic as u8 => Terrain::Psychic,
        _ => Terrain::None,
    }
}
