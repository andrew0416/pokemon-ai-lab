//! The mutable battle context of one turn run and its primitive operations (Showdown's
//! `battle.damage`, `heal`, `setStatus`, `addVolatile`, `faintMessages`, `checkWin`, ...).
//!
//! Every state change goes through [`Battle::apply`], which records the reversible
//! instruction. Transient per-turn data that Showdown keeps on objects but that does not
//! survive the turn (the faint queue, what moved) lives here, not in `State`.

use crate::dex::{
    abilities, conditions, items, AbilityFlags, AbilityId, ItemId, MoveFlags, MoveId, Type,
    TypeImmunities, NO_BOOSTS,
};
use crate::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use crate::instruction::Instruction;
use crate::state::{
    BattleResult, Pokemon, PokemonRef, SideId, SlotRef, State, Status, BOOST_COUNT,
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
}

pub(crate) struct Battle<'a, const N: usize> {
    pub state: &'a mut State<N>,
    pub log: Vec<Instruction>,
    pub rng: &'a mut Chooser,
    /// Showdown `faintQueue`: Pokémon at 0 HP not yet processed, in the order they fell.
    faint_queue: Vec<(PokemonRef, SlotRef)>,
    /// The move in progress, if any (cleared when `runMove` ends).
    pub active_move: Option<ActiveMoveRef>,
    /// The actions of the turn not yet run (Showdown `queue.list`), see `queue.rs`.
    pub queue: Vec<super::queue::Action>,
}

impl<'a, const N: usize> Battle<'a, N> {
    pub fn new(state: &'a mut State<N>, rng: &'a mut Chooser) -> Battle<'a, N> {
        Battle {
            state,
            log: Vec::new(),
            rng,
            faint_queue: Vec::new(),
            active_move: None,
            queue: Vec::new(),
        }
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

    pub fn item(&self, slot: SlotRef) -> ItemId {
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

    /// Showdown `isGrounded` for the supported effects, in its order: Gravity, Iron Ball,
    /// Flying, Levitate, Air Balloon.
    pub fn is_grounded(&self, slot: SlotRef) -> bool {
        if self.field_active(FieldEffect::Gravity) {
            return true;
        }
        let item = self.item(slot);
        if super::items::grounds(item) {
            return true;
        }
        if self.has_type(slot, Type::Flying) {
            return false;
        }
        // `hasAbility('levitate') && !suppressingAbility(this)`.
        if self.ability(slot) == abilities::LEVITATE && !self.suppressing_ability(slot) {
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
            // Sand Rush: `onImmunity(type) { if (type === 'sandstorm') return false; }`.
            return mon.ability == abilities::SAND_RUSH || overcoat;
        }
        if immunity == TypeImmunities::POWDER {
            return overcoat;
        }
        if immunity == TypeImmunities::FRZ {
            // Harsh sunlight (`sunnyday.onImmunity`, hidden by Utility Umbrella) and Magma
            // Armor (breakable).
            return (matches!(self.effective_weather(), Weather::Sun | Weather::HarshSun)
                && mon.item != items::UTILITY_UMBRELLA)
                || self.ability_unless_broken(slot) == abilities::MAGMA_ARMOR;
        }
        // Ice Body's `onImmunity('hail')`: hail is not a supported weather.
        false
    }

    // ---- HP ----------------------------------------------------------------------------

    /// Showdown `spreadDamage` for one target: at least 1, Damage handlers, clamped to the
    /// target's HP, faint queued at 0 HP. Returns the HP removed.
    ///
    /// Damage handlers by priority: Rock Head and Magic Guard (0), Endure (-10), Sturdy (-30),
    /// Focus Sash and Focus Band (-40, `items::on_damage`). Rock Head (`effect.id === 'recoil'`)
    /// and Magic Guard (`effect.effectType !== 'Move'`) cancel the damage (neither is
    /// breakable). Endure, Sturdy and Focus Sash leave the target at 1 HP against a move's
    /// damage (Sturdy and the Sash only from full HP; Sturdy acts first, so the Sash then
    /// stays). Sturdy is breakable (ignored by Sunsteel Strike and the like).
    pub fn damage(&mut self, target: SlotRef, amount: f64, source: DamageSource) -> i32 {
        let Some(pokemon) = self.alive(target) else {
            return 0;
        };
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
        self.lose_hp(target, pokemon, amount)
    }

    fn lose_hp(&mut self, slot: SlotRef, pokemon: PokemonRef, amount: i32) -> i32 {
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
            self.queue_faint(pokemon, slot);
        }
        dealt
    }

    /// Showdown `battle.heal`: fractions below 1 become 1, then truncate; nothing on a fainted
    /// or full-HP Pokémon. Returns the HP restored.
    pub fn heal(&mut self, target: SlotRef, amount: f64) -> i32 {
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

    // ---- faint and win -------------------------------------------------------------------

    fn queue_faint(&mut self, pokemon: PokemonRef, slot: SlotRef) {
        if !self.faint_queue.iter().any(|&(p, _)| p == pokemon) {
            self.faint_queue.push((pokemon, slot));
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
        while !self.faint_queue.is_empty() {
            let (pokemon, slot) = self.faint_queue.remove(0);
            if self.occupant(slot) != Some(pokemon) {
                continue;
            }
            // clearVolatile: the ability and types revert; the slot empties (isActive = false).
            self.clear_volatile(pokemon);
            let previous = self.state.slot(slot).clone();
            self.apply(Instruction::Switch {
                slot,
                previous,
                party_index: None,
            });
            self.apply(Instruction::SetFaintedOccupant {
                slot,
                old: None,
                new: Some(pokemon.party),
            });
            last = Some(pokemon.side);
        }
        check_win && self.check_win(last)
    }

    /// The party-side part of Showdown `clearVolatile` when a Pokémon leaves the field: the
    /// ability reverts to its base and `setSpecies(baseSpecies)` restores the species' types
    /// (the species itself stays: Champions never regresses a forme). Slot state is reset by
    /// the caller's `Switch`.
    pub fn clear_volatile(&mut self, pokemon: PokemonRef) {
        let mon = self.mon(pokemon);
        if mon.ability != mon.base_ability {
            let (old, new) = (mon.ability, mon.base_ability);
            self.apply(Instruction::SetAbility {
                target: pokemon,
                old,
                new,
            });
        }
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
    ///   (everything), Leaf Guard (everything in harsh sunlight), Thermal Exchange (brn);
    /// - Sweet Veil on the target or an ally (`onAllySetStatus`, slp);
    /// - Misty Terrain (everything) and Electric Terrain (slp) for a grounded target.
    ///
    /// Flower Veil is refused (`onAllyTryBoost`).
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
            // `target.effectiveWeather()`: Utility Umbrella is not supported.
            a if a == abilities::LEAF_GUARD => self.effective_weather() == Weather::Sun,
            _ => false,
        };
        if blocked_by_own {
            return true;
        }
        // `onAllySetStatus` runs for every active ally and the target itself.
        if status == Status::Sleep
            && self
                .alive_slots(target.side)
                .into_iter()
                .any(|s| self.ability_unless_broken(s) == abilities::SWEET_VEIL)
        {
            return true;
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
    /// handlers of Insomnia, Vital Spirit, Purifying Salt and Leaf Guard (in sun) on the
    /// target block Yawn; Sweet Veil (Yawn) and Aroma Veil (Attract, Disable, Encore, Heal
    /// Block, Taunt, Torment) block for the whole side; Electric Terrain blocks Yawn on a
    /// grounded target; Safeguard blocks Yawn and confusion from another Pokémon. Of those
    /// volatiles only Yawn is implemented; the others (and Misty Terrain's confusion block)
    /// guard their future implementation.
    pub fn add_volatile_blocked(&self, target: SlotRef, volatile: Volatile) -> bool {
        let condition = volatile.condition();
        let yawn = condition == conditions::YAWN;
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
            a if a == abilities::LEAF_GUARD => yawn && self.effective_weather() == Weather::Sun,
            // Inner Focus: `if (status.id === 'flinch') return null;`
            a if a == abilities::INNER_FOCUS => condition == conditions::FLINCH,
            // Own Tempo: `if (status.id === 'confusion') return null;`
            a if a == abilities::OWN_TEMPO => condition == conditions::CONFUSION,
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
        // Electric Terrain's `onTryAddVolatile`: Yawn fails on a grounded target (Misty
        // Terrain's only blocks confusion). Safeguard: Yawn and confusion from the user of the
        // move in progress, if that is another Pokémon.
        let safeguard = (yawn || condition == conditions::CONFUSION)
            && self.safeguarded(target, self.active_move.map(|m| m.user));
        veiled
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
        self.lose_hp(slot, pokemon, hp);
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
                counter: if volatile == Volatile::Stall { 3 } else { 0 },
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
                _ => {}
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
        // onEnd.
        if volatile == Volatile::LockedMove && old.hidden <= 1 {
            self.add_volatile(target, Volatile::Confusion);
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
        let from_other = source.is_some_and(|s| s != target);
        let mut boost = *boosts;

        // ChangeBoost: the target's own ability.
        match self.ability_unless_broken(target) {
            a if a == abilities::CONTRARY => boost.iter_mut().for_each(|b| *b = -*b),
            a if a == abilities::SIMPLE => boost.iter_mut().for_each(|b| *b *= 2),
            _ => {}
        }
        // getCappedBoost.
        for (stat, b) in boost.iter_mut().enumerate() {
            let current = self.state.slot(target).boosts[stat];
            *b = (current + *b).clamp(-6, 6) - current;
        }
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
