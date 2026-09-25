//! The mutable battle context of one turn run and its primitive operations (Showdown's
//! `battle.damage`, `heal`, `setStatus`, `addVolatile`, `faintMessages`, `checkWin`, ...).
//!
//! Every state change goes through [`Battle::apply`], which records the reversible
//! instruction. Transient per-turn data that Showdown keeps on objects but that does not
//! survive the turn (the faint queue, what moved) lives here, not in `State`.

use crate::dex::{abilities, items, AbilityId, ItemId, MoveId, Type, TypeImmunities};
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
    /// Direct damage of a move.
    Move,
    /// Everything else: recoil, weather, status, items.
    Indirect,
}

pub(crate) struct Battle<'a, const N: usize> {
    pub state: &'a mut State<N>,
    pub log: Vec<Instruction>,
    pub rng: &'a mut Chooser,
    /// Showdown `faintQueue`: Pokémon at 0 HP not yet processed, in the order they fell.
    faint_queue: Vec<(PokemonRef, SlotRef)>,
    /// Pokémon that fainted in an active position this turn (`checkFainted` marks them).
    pub fainted_positions: Vec<(SlotRef, PokemonRef)>,
}

impl<'a, const N: usize> Battle<'a, N> {
    pub fn new(state: &'a mut State<N>, rng: &'a mut Chooser) -> Battle<'a, N> {
        Battle {
            state,
            log: Vec::new(),
            rng,
            faint_queue: Vec::new(),
            fainted_positions: Vec::new(),
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
        self.ability(slot) != abilities::LEVITATE
    }

    pub fn volatile(&self, slot: SlotRef, volatile: Volatile) -> VolatileState {
        self.state.slot(slot).volatiles.get(volatile)
    }

    /// Showdown `dex.getImmunity(status, pokemon)` plus the supported `Immunity` handlers.
    pub fn status_immune(&self, slot: SlotRef, immunity: TypeImmunities) -> bool {
        let Some(mon) = self.slot_mon(slot) else {
            return true;
        };
        if mon.types.iter().any(|t| t.immunities().contains(immunity)) {
            return true;
        }
        // Sand Rush: `onImmunity(type) { if (type === 'sandstorm') return false; }`.
        immunity == TypeImmunities::SANDSTORM && mon.ability == abilities::SAND_RUSH
    }

    // ---- HP ----------------------------------------------------------------------------

    /// Showdown `spreadDamage` for one target: at least 1, Damage handlers (Focus Sash),
    /// clamped to the target's HP, faint queued at 0 HP. Returns the HP removed.
    pub fn damage(&mut self, target: SlotRef, amount: f64, source: DamageSource) -> i32 {
        let Some(pokemon) = self.alive(target) else {
            return 0;
        };
        let mut amount = (amount.floor() as i32).max(1);
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
            // clearVolatile: the ability reverts; the slot empties (isActive = false).
            let mon = self.mon(pokemon);
            if mon.ability != mon.base_ability {
                let (old, new) = (mon.ability, mon.base_ability);
                self.apply(Instruction::SetAbility {
                    target: pokemon,
                    old,
                    new,
                });
            }
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
    /// target, an existing status, type immunity, and the Electric/Misty Terrain rules.
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
        // SetStatus handlers.
        if self.is_grounded(target) {
            match self.terrain() {
                Terrain::Misty => return false,
                Terrain::Electric if status == Status::Sleep => return false,
                _ => {}
            }
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
