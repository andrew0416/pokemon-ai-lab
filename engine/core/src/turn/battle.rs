//! The mutable battle context of one turn run and its primitive operations (Showdown's
//! `battle.damage`, `heal`, `setStatus`, `addVolatile`, `faintMessages`, `checkWin`, ...).
//!
//! Every state change goes through [`Battle::apply`], which records the reversible
//! instruction. Transient per-turn data that Showdown keeps on objects but that does not
//! survive the turn (the faint queue, what moved) lives here, not in `State`.

use crate::dex::{
    abilities, conditions, items, AbilityFlags, AbilityId, ItemId, MoveId, Type, TypeImmunities,
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
}

pub(crate) struct Battle<'a, const N: usize> {
    pub state: &'a mut State<N>,
    pub log: Vec<Instruction>,
    pub rng: &'a mut Chooser,
    /// Showdown `faintQueue`: Pokémon at 0 HP not yet processed, in the order they fell.
    faint_queue: Vec<(PokemonRef, SlotRef)>,
    /// Pokémon that fainted in an active position this turn (`checkFainted` marks them).
    pub fainted_positions: Vec<(SlotRef, PokemonRef)>,
    /// The move in progress, if any (cleared when `runMove` ends).
    pub active_move: Option<ActiveMoveRef>,
}

impl<'a, const N: usize> Battle<'a, N> {
    pub fn new(state: &'a mut State<N>, rng: &'a mut Chooser) -> Battle<'a, N> {
        Battle {
            state,
            log: Vec::new(),
            rng,
            faint_queue: Vec::new(),
            fainted_positions: Vec::new(),
            active_move: None,
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
    /// (`ignoreAbility`, e.g. Sunsteel Strike), its user is still active, and `target` is not
    /// the user (Gen 8+) and holds no Ability Shield.
    pub fn suppressing_ability(&self, target: SlotRef) -> bool {
        let Some(active) = self.active_move else {
            return false;
        };
        active.id.data().ignore_ability
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

    pub fn weather(&self) -> Weather {
        let e = self.state.field[FieldEffect::Weather as usize];
        if !e.is_active() {
            return Weather::None;
        }
        weather_from(e.value)
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

    /// Showdown `isGrounded` for the supported effects (Gravity, Flying, Levitate).
    pub fn is_grounded(&self, slot: SlotRef) -> bool {
        if self.field_active(FieldEffect::Gravity) {
            return true;
        }
        if self.has_type(slot, Type::Flying) {
            return false;
        }
        // `hasAbility('levitate') && !suppressingAbility(this)`.
        self.ability(slot) != abilities::LEVITATE || self.suppressing_ability(slot)
    }

    pub fn volatile(&self, slot: SlotRef, volatile: Volatile) -> VolatileState {
        self.state.slot(slot).volatiles.get(volatile)
    }

    /// Showdown `dex.getImmunity(status, pokemon)` plus the supported `Immunity` handlers
    /// (`runStatusImmunity`).
    pub fn status_immune(&self, slot: SlotRef, immunity: TypeImmunities) -> bool {
        let Some(mon) = self.slot_mon(slot) else {
            return true;
        };
        if mon.types.iter().any(|t| t.immunities().contains(immunity)) {
            return true;
        }
        // Immunity handlers; each returns false for one immunity id, so order is irrelevant.
        if immunity == TypeImmunities::SANDSTORM {
            // Sand Rush: `onImmunity(type) { if (type === 'sandstorm') return false; }`.
            return mon.ability == abilities::SAND_RUSH;
        }
        if immunity == TypeImmunities::FRZ {
            // Harsh sunlight (`sunnyday.onImmunity`, hidden by Utility Umbrella) and Magma
            // Armor (breakable).
            return (matches!(self.weather(), Weather::Sun | Weather::HarshSun)
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
    /// Damage handlers by priority: Rock Head and Magic Guard (0), Sturdy (-30), Focus Sash
    /// (-40). Rock Head (`effect.id === 'recoil'`) and Magic Guard (`effect.effectType !==
    /// 'Move'`) cancel the damage (neither is breakable). Sturdy and Focus Sash leave a full-HP
    /// target at 1 HP against a move's damage; Sturdy acts first, so the Sash then stays.
    /// Sturdy is breakable (ignored by Sunsteel Strike and the like).
    pub fn damage(&mut self, target: SlotRef, amount: f64, source: DamageSource) -> i32 {
        let Some(pokemon) = self.alive(target) else {
            return 0;
        };
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
        if source == DamageSource::Move
            && mon.hp == mon.max_hp
            && amount >= i32::from(mon.hp)
            && self.ability_unless_broken(target) == abilities::STURDY
        {
            amount = i32::from(mon.hp) - 1;
        }
        let mon = self.mon(pokemon);
        if source == DamageSource::Move
            && mon.item == items::FOCUS_SASH
            && mon.hp == mon.max_hp
            && amount >= i32::from(mon.hp)
            && self.use_item(target)
        {
            amount = i32::from(self.mon(pokemon).hp) - 1;
        }
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
            self.fainted_positions.push((slot, pokemon));
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

    /// Showdown `trySetStatus` → `setStatus` for the supported handlers: fails on a fainted
    /// target, an existing status, status immunity (`runStatusImmunity`), and the `SetStatus`
    /// handlers (see [`Battle::set_status_blocked`]).
    pub fn try_set_status(&mut self, target: SlotRef, status: Status) -> bool {
        let Some(pokemon) = self.alive(target) else {
            return false;
        };
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
        true
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
    /// Purifying Salt and Thermal Exchange are still refused by `support` (their damage
    /// handlers are not implemented here); Flower Veil is refused (`onAllyTryBoost`).
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
            a if a == abilities::LEAF_GUARD => self.weather() == Weather::Sun,
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

    /// Showdown `runEvent('TryAddVolatile')` for a new volatile on `target`: the ability
    /// handlers of Insomnia, Vital Spirit, Purifying Salt and Leaf Guard (in sun) on the
    /// target block Yawn; Sweet Veil (Yawn) and Aroma Veil (Attract, Disable, Encore, Heal
    /// Block, Taunt, Torment) block for the whole side. None of those volatiles is
    /// representable yet, so this only guards their future implementation. The terrains'
    /// `onTryAddVolatile` (Yawn, confusion) belong with those volatiles too.
    pub fn add_volatile_blocked(&self, target: SlotRef, volatile: Volatile) -> bool {
        let condition = volatile.condition();
        let yawn = condition == conditions::YAWN;
        let blocked_by_own = match self.ability_unless_broken(target) {
            a if a == abilities::INSOMNIA
                || a == abilities::VITAL_SPIRIT
                || a == abilities::PURIFYING_SALT =>
            {
                yawn
            }
            a if a == abilities::LEAF_GUARD => yawn && self.weather() == Weather::Sun,
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
        self.alive_slots(target.side).into_iter().any(|s| {
            let ability = self.ability_unless_broken(s);
            (ability == abilities::SWEET_VEIL && yawn)
                || (ability == abilities::AROMA_VEIL && aroma)
        })
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
        if self.alive(target).is_none() {
            return false;
        }
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
                },
                // No onRestart.
                _ => return false,
            }
        } else {
            // TryAddVolatile handlers (the new volatile only; a restart skips them).
            if self.add_volatile_blocked(target, volatile) {
                return false;
            }
            VolatileState {
                active: true,
                duration: volatile.initial_duration(),
                counter: if volatile == Volatile::Stall { 3 } else { 0 },
            }
        };
        self.apply(Instruction::SetVolatile {
            target,
            volatile,
            old,
            new,
        });
        true
    }

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
        true
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

    /// Showdown `battle.boost` without boost-modifying abilities (those are rejected up
    /// front). Returns whether any stage changed.
    pub fn boost(&mut self, target: SlotRef, boosts: &[i8; BOOST_COUNT]) -> bool {
        if self.alive(target).is_none() {
            return false;
        }
        let mut changed = false;
        for (stat, &amount) in boosts.iter().enumerate() {
            if amount == 0 {
                continue;
            }
            let current = self.state.slot(target).boosts[stat];
            let delta = (current + amount).clamp(-6, 6) - current;
            if delta != 0 {
                self.apply(Instruction::Boost {
                    target,
                    stat: stat as u8,
                    amount: delta,
                });
                changed = true;
            }
        }
        changed
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

/// Whether `ability`'s `onUpdate` would cure `status` (Water Veil, Thermal Exchange: brn;
/// Immunity: psn, tox; Insomnia, Vital Spirit: slp; Limber: par; Magma Armor: frz).
///
/// The engine has no `Update` event yet (Showdown runs it after every action), so a holder
/// must never be active with that status: `support::check_state` refuses such a state and a
/// switch-in of such a Pokémon is refused. With those guards the cure is unreachable, because
/// the same abilities block the status from being set (`set_status_blocked`, `status_immune`)
/// and nothing implemented bypasses them. Anything that gives one of these abilities to a
/// Pokémon that already has the status (Mega Evolution into Mewtwo-Mega-Y's Insomnia, Trace
/// mid-turn, Skill Swap) must check this too.
pub(crate) fn cured_on_update(ability: AbilityId, status: Status) -> bool {
    match ability {
        a if a == abilities::WATER_VEIL || a == abilities::THERMAL_EXCHANGE => {
            status == Status::Burn
        }
        a if a == abilities::IMMUNITY => matches!(status, Status::Poison | Status::Toxic),
        a if a == abilities::INSOMNIA || a == abilities::VITAL_SPIRIT => status == Status::Sleep,
        a if a == abilities::LIMBER => status == Status::Paralyze,
        a if a == abilities::MAGMA_ARMOR => status == Status::Freeze,
        _ => false,
    }
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
