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

/// The move being used (Showdown `activeMove` with `activePokemon`), set for the whole of
/// `runMove`; it decides whether breakable abilities are suppressed (`suppressingAbility`).
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
}

pub(crate) struct Battle<'a, const N: usize> {
    pub state: &'a mut State<N>,
    pub log: Vec<Instruction>,
    pub rng: &'a mut Chooser,
    /// Showdown `faintQueue`: Pokémon at 0 HP not yet processed, in the order they fell, with
    /// the Pokémon whose move's damage knocked them out (`faintData.source` when
    /// `faintData.effect` is a move; `None` otherwise), which Destiny Bond reads.
    faint_queue: Vec<(PokemonRef, SlotRef, Option<PokemonRef>)>,
    /// The move in progress, if any (cleared when `runMove` ends).
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
    /// Mirror Herb's `effectState.boosts` per holder: the foes' raises it copied and has not
    /// used yet (`ready`). Showdown keeps them on the item across events; the engine keeps
    /// them only within a stage and refuses a stage that ends with one pending
    /// (`items::stage_end_check`).
    pub mirror_herb: Vec<(PokemonRef, [i8; BOOST_COUNT])>,
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
    /// Evolution, ...): Showdown's `setSpecies` sets their `pokemon.speed` to the raw stored
    /// Speed until the next `updateSpeed()`, which comes after the action
    /// ([`Battle::event_speed`]). A stage is one action, so this starts empty with every stage;
    /// a multi-hit move suspended between hits carries it in its `MoveProgress`.
    pub raw_speed: Vec<PokemonRef>,
}

/// The readers of the hidden damage history present in a battle (any party member's moves;
/// see `history::record_attack`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryReaders {
    /// Metal Burst, Comeuppance (`lastDamagedBy`).
    pub last_damaged_by: bool,
    /// Rage Fist (`timesAttacked`).
    pub times_attacked: bool,
    /// Stomping Tantrum, Temper Flare (`moveLastTurnResult`).
    pub move_last_turn_result: bool,
    /// Retaliate (`faintedLastTurn`; unsupported, so never set today).
    pub fainted_last_turn: bool,
    /// Burning Jealousy, Alluring Voice (`statsRaisedThisTurn`).
    pub stats_raised: bool,
    /// Lash Out (`statsLoweredThisTurn`).
    pub stats_lowered: bool,
    /// Belch (`ateBerry`).
    pub ate_berry: bool,
    /// Last Resort (`moveSlot.used`).
    pub moves_used: bool,
}

impl HistoryReaders {
    pub fn of<const N: usize>(state: &State<N>) -> HistoryReaders {
        let mut readers = HistoryReaders::default();
        for side in &state.sides {
            for mon in &side.party {
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
                    }
                }
            }
        }
        readers
    }
}

impl<'a, const N: usize> Battle<'a, N> {
    pub fn new(state: &'a mut State<N>, rng: &'a mut Chooser) -> Battle<'a, N> {
        let history_readers = HistoryReaders::of(state);
        Battle {
            state,
            log: Vec::new(),
            rng,
            faint_queue: Vec::new(),
            active_move: None,
            queue: Vec::new(),
            battle_start: false,
            hit_type_mod: [[None; N]; 2],
            mirror_herb: Vec::new(),
            move_self_switch: false,
            force_switch: Vec::new(),
            busted: Vec::new(),
            history_readers,
            raw_speed: Vec::new(),
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
    pub fn alive_slots(&self, side: SideId) -> Vec<SlotRef> {
        Self::slots(side)
            .filter(|&s| self.alive(s).is_some())
            .collect()
    }

    pub fn all_alive(&self) -> Vec<SlotRef> {
        let mut out = self.alive_slots(SideId::One);
        out.extend(self.alive_slots(SideId::Two));
        out
    }

    pub fn ability(&self, slot: SlotRef) -> AbilityId {
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

    pub fn has_type(&self, slot: SlotRef, ty: Type) -> bool {
        self.slot_mon(slot).is_some_and(|m| m.types.contains(&ty))
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
    /// be at 0 HP) has an ability with `suppressWeather` (Air Lock, Cloud Nine). Its
    /// `abilityState.ending` flag only matters inside its own `End` event, whose
    /// `WeatherChange` has no implemented handler; Gastro Acid and Neutralizing Gas are not
    /// supported.
    pub fn weather_suppressed(&self) -> bool {
        State::<N>::slot_refs().any(|slot| {
            self.slot_mon(slot)
                .is_some_and(|mon| mon.ability.data().suppress_weather)
        })
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

    /// Showdown `isGrounded` for the supported effects, in its order: Gravity, Ingrain, Iron
    /// Ball, Flying, Levitate, Magnet Rise, Air Balloon.
    pub fn is_grounded(&self, slot: SlotRef) -> bool {
        if self.field_active(FieldEffect::Gravity) || self.volatile(slot, Volatile::Ingrain).active
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
        self.slot_mon(slot)
            .is_none_or(|mon| mon.types.iter().any(|t| t.immunities().contains(immunity)))
    }

    /// Showdown `dex.getImmunity(status, pokemon)` plus the supported `Immunity` handlers
    /// (`runStatusImmunity`).
    pub fn status_immune(&self, slot: SlotRef, immunity: TypeImmunities) -> bool {
        let Some(mon) = self.slot_mon(slot) else {
            return true;
        };
        if self.natural_immune(slot, immunity) {
            return true;
        }
        // The item's `onImmunity` (Safety Goggles: sandstorm, powder).
        if super::items::grants_immunity(mon.item, immunity) {
            return true;
        }
        // Immunity handlers; each returns false for one immunity id, so order is irrelevant.
        // Overcoat (breakable): `if (type === 'sandstorm' || type === 'hail' || type ===
        // 'powder') return false;` (hail is not a supported weather).
        let overcoat = self.ability_unless_broken(slot) == abilities::OVERCOAT;
        if immunity == TypeImmunities::SANDSTORM {
            // Sand Rush, Sand Force, Sand Veil (breakable): `onImmunity(type) { if (type ===
            // 'sandstorm') return false; }`.
            let sand_ability = [abilities::SAND_RUSH, abilities::SAND_FORCE].contains(&mon.ability);
            let sand_veil = self.ability_unless_broken(slot) == abilities::SAND_VEIL;
            return sand_ability || sand_veil || overcoat;
        }
        if immunity == TypeImmunities::POWDER {
            return overcoat;
        }
        if immunity == TypeImmunities::FRZ {
            // Harsh sunlight (`sunnyday.onImmunity`, hidden by Utility Umbrella) and Magma
            // Armor (breakable).
            return matches!(self.weather_for(slot), Weather::Sun | Weather::HarshSun)
                || self.ability_unless_broken(slot) == abilities::MAGMA_ARMOR;
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
            .is_some_and(|m| m.id.data().multihit.is_some());
        super::abilities::on_damage(self, target, source == DamageSource::Move, multihit);
        let mut amount = (amount.floor() as i32).max(1);
        let mon = self.mon(pokemon);
        let cancelled = match mon.ability {
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
    /// holder of Big Root (nothing else supported answers TryHeal: Heal Block, Liquid Ooze and
    /// Ripen are refused). Returns the HP restored.
    pub fn heal_rooted(&mut self, target: SlotRef, amount: f64) -> i32 {
        let amount = if amount > 0.0 && amount <= 1.0 {
            1.0
        } else {
            amount
        };
        let mut amount = amount.trunc() as i32;
        if self.item(target) == items::BIG_ROOT {
            amount = super::order::modify(amount, 5324);
        }
        self.heal(target, f64::from(amount))
    }

    // ---- faint and win -------------------------------------------------------------------

    fn queue_faint(&mut self, pokemon: PokemonRef, slot: SlotRef, attacker: Option<PokemonRef>) {
        if !self.faint_queue.iter().any(|&(p, _, _)| p == pokemon) {
            self.faint_queue.push((pokemon, slot, attacker));
        }
    }

    /// Showdown `faintMessages(lastFirst = false, forceCheck = false, checkWin)`. Returns
    /// whether the battle is over.
    pub fn faint_messages(&mut self, check_win: bool) -> bool {
        if self.state.result.is_over() {
            return true;
        }
        if self.faint_queue.is_empty() {
            return false;
        }
        let mut last = None;
        let mut check_win = check_win;
        // `const length = this.faintQueue.length`, and `faintData`: the last entry taken from
        // the queue, processed or not (for AfterFaint).
        let length = self.faint_queue.len();
        let mut last_source = None;
        while !self.faint_queue.is_empty() {
            let queue_left = self.faint_queue.len();
            let (pokemon, slot, attacker) = self.faint_queue.remove(0);
            last_source = attacker;
            if self.occupant(slot) != Some(pokemon) {
                continue;
            }
            // runEvent('Faint'): Destiny Bond takes its attacker down (`if
            // (this.faintQueue.length >= faintQueueLeft) checkWin = true;`).
            super::conditions::destiny_bond_faint(self, slot, attacker);
            if self.faint_queue.len() >= queue_left {
                check_win = true;
            }
            // clearVolatile: the ability and types revert; the slot empties (isActive = false).
            self.clear_volatile(pokemon);
            let previous = self.state.slot(slot).clone();
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
            self.record_faint(pokemon.side);
            last = Some(pokemon.side);
        }
        if check_win && self.check_win(last) {
            return true;
        }
        // `runEvent('AfterFaint', faintData.target, faintData.source, faintData.effect,
        // length)`: only the source's `onSourceAfterFaint` handlers exist, and they need a move's
        // damage (`effect.effectType === 'Move'`), which is when the queue records a source.
        if let Some(source) = last_source {
            super::abilities::after_faint(self, source, length);
        }
        false
    }

    /// The party-side part of Showdown `clearVolatile` when a Pokémon leaves the field: the
    /// ability reverts to its base and `setSpecies(baseSpecies)` restores the species' types
    /// (a permanent forme stays: Champions never regresses one; a temporary forme returns to its
    /// base species, `forme::revert_on_leave`). Slot state is reset by the caller's `Switch`.
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
        // `setSpecies` also recalculates the stored stats (Speed Swap's exchange ends).
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

    /// Showdown `checkWin(faintData)`: with every side out, the side of the last processed
    /// faint wins (Gen 5+); `None` makes it a tie.
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
        self.try_set_status_from(target, status, source)
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
        let Some(pokemon) = self.alive(target) else {
            return false;
        };
        // Safeguard (`onSetStatus` of the target's side): blocks a status from another Pokémon.
        if self.safeguarded(target, source) {
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
        if immunity != TypeImmunities::EMPTY && self.status_immune(target, immunity) {
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
        self.after_set_status(target, status, source);
        // Lum Berry's `onAfterSetStatus` (priority -1: after Synchronize).
        super::update::after_set_status(self, target);
        true
    }

    /// `runEvent('AfterSetStatus', target, source, effect, status)`. The only implemented
    /// handler is Synchronize on the target (not breakable, not modded in Champions): a burn,
    /// paralysis or (bad) poison from another Pokémon is passed back to it
    /// (`source.trySetStatus(status, target)`), which fails if the source already has a status
    /// or is immune. Toxic Spikes (excluded by Synchronize) is not supported.
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
        false
    }

    /// Safeguard on `target`'s side against an effect from `source` (its `onSetStatus` and
    /// `onTryAddVolatile`): only another Pokémon's effects are blocked (`target !== source`),
    /// and nothing without a source (`if (!effect || !source) return;`). Infiltrator, which
    /// bypasses it, is refused.
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
        let safeguard = (yawn || condition == conditions::CONFUSION)
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

    /// Showdown `cureStatus` / `clearStatus`.
    pub fn cure_status(&mut self, pokemon: PokemonRef) {
        let old = self.mon(pokemon).status;
        if self.mon(pokemon).hp == 0 || old == Status::None {
            return;
        }
        self.apply(Instruction::ChangeStatus {
            target: pokemon,
            old,
            new: Status::None,
        });
        self.set_status_turns(pokemon, 0);
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
        true
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

        // ChangeBoost: the target's own ability.
        match self.ability_unless_broken(target) {
            a if a == abilities::CONTRARY => boost.iter_mut().for_each(|b| *b = -*b),
            a if a == abilities::SIMPLE => boost.iter_mut().for_each(|b| *b *= 2),
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
        // Mist on the target's side (`onTryBoost`): another Pokémon's drops are blocked
        // (Infiltrator, which ignores it, is refused).
        if from_other && self.side_effect_active(target.side, SideEffect::Mist) {
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
        // Herbs.
        super::items::after_boost(self, target, &boost, effect, atk_capped_to_zero);
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
        let boost = self.state.slot(holder).boosts[stat];
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
        // The only berries consumed through here are the resist berries, which Showdown eats
        // (`eatItem`: `ateBerry = true`, Belch).
        if item.data().is_berry {
            self.record_ate_berry(pokemon);
        }
        // AfterUseItem: Unburden.
        super::abilities::unburden(self, slot);
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
        if mon.item == items::BOOSTER_ENERGY && super::abilities::booster_energy_kept(mon.species) {
            return false;
        }
        // Mega Stones: `onTakeItem(item, source) { return !item.megaStone?.[source.baseSpecies.baseSpecies]; }`
        let base = mon.species.data().base_species;
        let base = if base.is_none() { mon.species } else { base };
        !item.mega_stone.iter().any(|&(from, _)| from == base)
    }

    /// Showdown `takeItem` (Knock Off): removed without becoming `lastItem`. Works on a
    /// target at 0 HP that has not been processed as fainted yet, as in Showdown.
    pub fn take_item(&mut self, slot: SlotRef) -> bool {
        let Some(pokemon) = self.occupant(slot) else {
            return false;
        };
        if self.mon(pokemon).item.is_none() {
            return false;
        }
        // TakeItem: the holder's ability (Unburden) before the item's own handler.
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
        true
    }

    // ---- per-slot counters -------------------------------------------------------------------

    /// Showdown `pokemon.activeTurns > 0` during a turn: the Pokémon was already active when the
    /// turn started (`endTurn` counts every active Pokémon; `switchIn` resets it to 0).
    ///
    /// `State` has no such counter. During a turn it equals `move_actions > 0`
    /// (`activeMoveActions`, also reset by `switchIn`): every Pokémon active at the start of
    /// the turn has a move action that runs `runMove` (which counts it) unless it switches
    /// out or faints first, while a Pokémon that switched in during the turn cannot act again.
    /// Mechanics that break this (a move from Dancer or Instruct after switching in, a
    /// skipped action while staying in) must replace this with a real counter.
    pub fn active_since_turn_start(&self, slot: SlotRef) -> bool {
        self.state.slot(slot).move_actions > 0
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

    /// `pokemon.switchFlag = false`.
    pub fn clear_switch_flag(&mut self, slot: SlotRef) {
        self.set_switch_flag(slot, SwitchFlag::None);
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

    pub fn set_side_effect(&mut self, side: SideId, effect: SideEffect, new: Effect) {
        let old = self.state.side(side).effects[effect as usize];
        if old != new {
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
