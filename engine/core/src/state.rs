//! Battle state, generic over the number of active slots per side.
//!
//! Persistent data (HP, status, item, PP) lives on the party [`Pokemon`]; data that resets
//! on switch-out (boosts, volatiles, substitute) lives on the [`Slot`]. poke-engine keeps the
//! latter on the side, which only works with one active Pokémon.

use crate::dex::{AbilityId, Gender, ItemId, MoveId, Nature, SpeciesId, Type};
use crate::field::{
    Effect, SlotEffect, FIELD_EFFECT_COUNT, SIDE_EFFECT_COUNT, SLOT_CONDITION_COUNT,
};
use crate::gimmick::{DynamaxState, GimmickSet};
use crate::stats::{champions_stats_unchecked, StatPoints};
use crate::volatile::Volatiles;

pub const PARTY_SIZE: usize = 6;
pub const BOOST_COUNT: usize = 7; // atk, def, spa, spd, spe, accuracy, evasion

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SideId {
    One = 0,
    Two = 1,
}

impl SideId {
    pub fn other(self) -> SideId {
        match self {
            SideId::One => SideId::Two,
            SideId::Two => SideId::One,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// An active position on the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotRef {
    pub side: SideId,
    pub slot: u8,
}

/// A party member, wherever it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PokemonRef {
    pub side: SideId,
    pub party: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Status {
    #[default]
    None,
    Burn,
    Freeze,
    Paralyze,
    Poison,
    Toxic,
    Sleep,
    /// Showdown's `fnt`: set on fainted Pokémon still in an active position when the turn
    /// ends (`checkFainted`). A Pokémon that fainted in a turn that ended the battle keeps its
    /// previous status, as in Showdown.
    Fainted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveSlot {
    pub id: MoveId,
    pub pp: u8,
    pub disabled: bool,
}

impl MoveSlot {
    /// A move slot at full Champions PP (see [`champions_max_pp`]).
    pub fn full(id: MoveId) -> MoveSlot {
        MoveSlot {
            id,
            pp: champions_max_pp(id),
            disabled: false,
        }
    }
}

/// Max PP in Champions. The dex already caps base PP at 20; Showdown's Champions
/// `calculatePP` then gives `(pp / 5 + 1) * 4` (5 → 8, 10 → 12, 15 → 16, 20 → 20), except for
/// moves without PP boosts, which keep their base PP.
pub fn champions_max_pp(id: MoveId) -> u8 {
    let data = id.data();
    if data.no_pp_boosts {
        data.pp
    } else {
        (data.pp / 5 + 1) * 4
    }
}

/// Party member state that survives switching.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pokemon {
    /// Current species (forme). Champions never reverts a Mega Evolution, not even on
    /// fainting (`formeRegression` is not set in its `formeChange`).
    pub species: SpeciesId,
    pub level: u8,
    /// Current types. They reset to the species' types when the Pokémon leaves the field
    /// (Showdown `clearVolatile` → `setSpecies`).
    pub types: [Type; 2],
    pub hp: i16,
    pub max_hp: i16,
    /// atk, def, spa, spd, spe (before boosts).
    pub stats: [i16; 5],
    /// Set data the stats are recalculated from on a forme change (Showdown `spreadModify`).
    pub nature: Nature,
    pub stat_points: StatPoints,
    /// Showdown `pokemon.gender`: the set's (`M`, `F`, `N`), else the species' fixed one;
    /// [`Gender::Random`] when neither decides (Showdown then draws `M` or `F` with the battle's
    /// PRNG, which the state cannot know: what reads the gender refuses it). Never changes, so
    /// it does not split positions.
    pub gender: Gender,
    pub status: Status,
    /// Showdown `statusState.time` for sleep and freeze, `statusState.stage` for toxic.
    pub status_turns: i8,
    pub item: ItemId,
    /// The item this Pokémon last consumed (Showdown `lastItem`); knocked-off items are not
    /// recorded.
    pub last_item: ItemId,
    /// Autotomize's weight reductions since the last `setSpecies` (Showdown lowers `weighthg`
    /// itself, `max(1, weighthg - 1000)` while it is above 1): the weight is the forme's minus
    /// 1000 hg per reduction, at least 1 hg ([`Pokemon::weight_hg`]). `setSpecies` resets it:
    /// leaving the field (switching out, fainting) and every forme change. Hidden from the
    /// canonical output, as Showdown's `weighthg` is.
    pub autotomized: u8,
    /// Current ability; changes in battle (Trace) and reverts to `base_ability` on
    /// switch-out or fainting.
    pub ability: AbilityId,
    pub base_ability: AbilityId,
    /// The current move slots (Showdown `moveSlots`): the Pokémon's own, or while it is
    /// transformed the 5-PP virtual copies of its target's ([`Pokemon::transformed`]).
    pub moves: [MoveSlot; 4],
    /// Showdown `transformed`, with what `clearVolatile` restores when the Pokémon leaves the
    /// field (switching out, fainting): `setSpecies(baseSpecies)` and `moveSlots =
    /// baseMoveSlots.slice()` (EE1: Transform, Imposter). `None` while not transformed; only an
    /// active Pokémon can be transformed.
    pub transformed: Option<TransformBase>,
    /// Showdown `pokemon.illusion` as a flag (EE2: Illusion): set by Illusion's
    /// `onBeforeSwitchIn` when a Pokémon that has not fainted comes after this one in party
    /// order, cleared by the ability's `End` (a damaging hit, Neutralizing Gas coming in, Gastro
    /// Acid, losing the ability; not switching out, when `beingCalledBack` keeps it) and by
    /// fainting. Only `transformInto` reads it (it fails while either Pokémon is under Illusion);
    /// which Pokémon the disguise shows changes nothing the engine models.
    pub illusion: bool,
    /// Activation modes this individual can use (Mega Stone, Z-Crystal, Tera type, ...),
    /// filled in from species/item data. The ruleset and the side's usage further restrict it.
    pub gimmicks: GimmickSet,
    /// Dynamaxing produces [`DynamaxState::Gigantamax`] instead of plain Dynamax.
    pub gigantamax_factor: bool,
}

impl Pokemon {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    /// Showdown `pokemon.weighthg`: the current forme's weight, lowered by Autotomize
    /// ([`Pokemon::autotomized`]), before the `ModifyWeight` handlers.
    pub fn weight_hg(&self) -> i32 {
        let weight = i32::from(self.species.data().weight_hg);
        (weight - 1000 * i32::from(self.autotomized)).max(1)
    }

    /// The fields a forme change rewrites, as they are now.
    pub fn forme(&self) -> Forme {
        Forme {
            species: self.species,
            types: self.types,
            max_hp: self.max_hp,
            stats: self.stats,
            ability: self.ability,
            base_ability: self.base_ability,
        }
    }

    /// The forme this Pokémon takes as `species` (Showdown `formeChange` with a permanent
    /// change): the species' types, its stats from this set's nature and SP, and its first
    /// ability. HP is not part of it; see [`Forme::hp_after`].
    pub fn forme_as(&self, species: SpeciesId) -> Forme {
        let data = species.data();
        let stats = champions_stats_unchecked(species, self.nature, self.stat_points);
        Forme {
            species,
            types: data.types,
            max_hp: stats[0],
            stats: [stats[1], stats[2], stats[3], stats[4], stats[5]],
            ability: data.abilities[0],
            base_ability: data.abilities[0],
        }
    }

    pub fn set_forme(&mut self, forme: Forme) {
        self.species = forme.species;
        self.types = forme.types;
        self.max_hp = forme.max_hp;
        self.stats = forme.stats;
        self.ability = forme.ability;
        self.base_ability = forme.base_ability;
    }

    /// The species Showdown's `pokemon.baseSpecies` checks see: while transformed, the species
    /// the Pokémon returns to (a transformed Ditto is still a Ditto to Light Ball, Eviolite,
    /// Mega Stones, Commander, ...); otherwise its current species, whose `baseSpecies` the
    /// callers look up in the dex.
    pub fn untransformed_species(&self) -> SpeciesId {
        self.transformed.map_or(self.species, |base| base.species)
    }
}

/// What a transformed Pokémon gets back when it leaves the field (Showdown `baseSpecies` and
/// `baseMoveSlots` while `transformed`): its base species (types and stored stats follow from it
/// and the set) and its own move slots with the PP they had when it transformed (the virtual
/// copies' PP is spent instead).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TransformBase {
    pub species: SpeciesId,
    pub moves: [MoveSlot; 4],
}

/// What a forme change rewrites at once (Showdown `setSpecies` + the ability part of
/// `formeChange`): species, types, max HP and stats, and the ability with its base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Forme {
    pub species: SpeciesId,
    pub types: [Type; 2],
    pub max_hp: i16,
    pub stats: [i16; 5],
    pub ability: AbilityId,
    pub base_ability: AbilityId,
}

impl Forme {
    /// Showdown `updateMaxHp`: when max HP changes, the HP lost so far is kept
    /// (`max(1, new_max - (old_max - hp))`); a fainted Pokémon stays at 0.
    pub fn hp_after(&self, old_max_hp: i16, hp: i16) -> i16 {
        if self.max_hp == old_max_hp || hp <= 0 {
            hp
        } else {
            (self.max_hp - (old_max_hp - hp)).max(1)
        }
    }
}

/// Showdown `moveThisTurnResult` / `moveLastTurnResult`: `undefined` (no move yet), `null`
/// (neither success nor failure: a recharge turn), `false`, `true`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MoveResult {
    #[default]
    Undefined,
    Null,
    Failed,
    Succeeded,
}

/// The last `attackedBy` entry with a numeric damage from a foe (`getLastDamagedBy(true)`),
/// this turn: who hit, from which slot (`getAtSlot(lastDamagedBy.slot)`), for how much.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DamagedBy {
    pub source: PokemonRef,
    pub slot: SlotRef,
    pub damage: i16,
}

/// Showdown's damage-history bookkeeping on an active Pokémon (WORKPLAN F13), reduced to
/// what the implemented consumers read. It resets on switch-out like the rest of the slot
/// (Champions also resets `timesAttacked` there) and is not part of the canonical output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotHistory {
    /// `hurtThisTurn`: the HP left after the latest damage this turn (`spreadDamage`), `None`
    /// until then and again at the end of the turn. Read by Assurance.
    pub hurt_this_turn: Option<i16>,
    /// `getLastDamagedBy(true)` restricted to this turn (its only readers require `thisTurn`):
    /// Metal Burst, Comeuppance.
    pub last_damaged_by: Option<DamagedBy>,
    /// One bit per Pokémon (`attacker_bit`) whose move damaged this one this turn
    /// (`attackedBy.some(p => p.source === target && p.damage > 0 && p.thisTurn)`): Avalanche,
    /// Revenge.
    pub damaged_by_this_turn: u16,
    /// `timesAttacked`: hits by damaging moves since switching in (Rage Fist).
    pub times_attacked: u8,
    /// `moveThisTurnResult`, copied to `move_last_turn_result` at the end of the turn
    /// (Stomping Tantrum, Temper Flare).
    pub move_this_turn_result: MoveResult,
    pub move_last_turn_result: MoveResult,
    /// `newlySwitched`: set when the Pokémon comes in (also at the start of the battle),
    /// cleared at the end of the turn (Payback).
    pub newly_switched: bool,
    /// `statsRaisedThisTurn` / `statsLoweredThisTurn`: a `boost()` that changed a stage raised /
    /// lowered one this turn (Burning Jealousy, Alluring Voice / Lash Out). Cleared at the end
    /// of every turn but the one that starts the battle (Intimidate at the start still counts
    /// on turn 1) and when the Pokémon leaves the field. Recorded only while a reader is in the
    /// battle (`HistoryReaders`).
    pub stats_raised_this_turn: bool,
    pub stats_lowered_this_turn: bool,
    /// `usedItemThisTurn`: the Pokémon used or ate its item this turn (`useItem`, `eatItem`).
    /// Cleared like the stat-change flags (every `endTurn` but the battle start's, and on
    /// leaving the field). Read by Pickup; recorded only while a Pickup holder is in the battle
    /// (`HistoryReaders`).
    pub used_item_this_turn: bool,
    /// `moveSlot.used` per move index (bit `1 << index`): `deductPP` marks a move slot (its own
    /// use, Pressure's extra PP, Spite on it); cleared on switch-in. Read by Last Resort;
    /// recorded only while it is in the battle (`HistoryReaders`).
    pub moves_used: u8,
}

impl Default for SlotHistory {
    fn default() -> Self {
        SlotHistory {
            hurt_this_turn: None,
            last_damaged_by: None,
            damaged_by_this_turn: 0,
            times_attacked: 0,
            move_this_turn_result: MoveResult::Undefined,
            move_last_turn_result: MoveResult::Undefined,
            newly_switched: true,
            stats_raised_this_turn: false,
            stats_lowered_this_turn: false,
            used_item_this_turn: false,
            moves_used: 0,
        }
    }
}

impl SlotHistory {
    /// The bit of `damaged_by_this_turn` for an attacker.
    pub fn attacker_bit(attacker: PokemonRef) -> u16 {
        1 << (attacker.side.index() * PARTY_SIZE + usize::from(attacker.party))
    }

    /// Whether `attacker`'s move damaged this Pokémon this turn.
    pub fn damaged_by(&self, attacker: PokemonRef) -> bool {
        self.damaged_by_this_turn & Self::attacker_bit(attacker) != 0
    }
}

/// Showdown `switchFlag` values the engine tells apart: `false`, a move id (a self-switching
/// move), `true` (Eject Button, Emergency Exit). Eject Button and Emergency Exit read the
/// distinction. The move id also decides the switch's `sourceEffect` (`resolveAction`): a move
/// whose `selfSwitch` is `'copyvolatile'` (Baton Pass) or `'shedtail'` (Shed Tail) makes the
/// newcomer copy the outgoing Pokémon's boosts and volatiles (`copyVolatileFrom`), which the
/// engine keeps as their own variants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SwitchFlag {
    #[default]
    None,
    Move,
    Effect,
    /// A move with `selfSwitch: 'copyvolatile'` (Baton Pass).
    CopyVolatile,
    /// A move with `selfSwitch: 'shedtail'` (Shed Tail).
    ShedTail,
}

/// A side's faint counters: `totalFainted` (capped at 100; Last Respects), `faintedThisTurn`
/// and `faintedLastTurn` (Retaliate), as booleans since only their truth is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SideHistory {
    pub total_fainted: u8,
    pub fainted_this_turn: bool,
    pub fainted_last_turn: bool,
    /// `pokemon.ateBerry` per party member (bit `1 << party index`): it ate a berry at some point
    /// in the battle (`eatItem`, or Bug Bite / Pluck's stolen one). Never cleared. Read by Belch;
    /// recorded only while it is in the battle (`HistoryReaders`).
    pub ate_berry: u8,
}

/// Active-position state that resets on switch-out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Index into the side's party; `None` for an empty slot (fainted, not yet replaced).
    pub party_index: Option<u8>,
    /// The party member that fainted here and has not been replaced (Showdown keeps it in
    /// `side.active[pos]` with `isActive = false`): it gets `fnt` at the end of the turn and
    /// its status clears when a replacement switches in. `None` while the slot is occupied.
    pub fainted_occupant: Option<u8>,
    pub boosts: [i8; BOOST_COUNT],
    pub volatiles: Volatiles,
    /// Showdown `lastMove`: the last move this Pokémon used since switching in.
    pub last_move: MoveId,
    /// Showdown `activeMoveActions`: moves attempted since switching in (Fake Out).
    pub move_actions: u8,
    /// Damage history (F13); hidden from the canonical output.
    pub history: SlotHistory,
    /// Showdown `switchFlag`: the occupant must be switched out by a mid-turn decision. Set
    /// when a self-switching move lands (`Move`, Showdown's move id) or by Eject Button /
    /// Emergency Exit (`Effect`, Showdown's `true`), cleared by the switch (the slot resets),
    /// by a faint (`faint()`) or when the side has no bench to switch to. A fainted Pokémon
    /// keeps it (`clearVolatile(false)`): Emergency Exit flags a user its own recoil knocked
    /// out, and its replacement is then asked for mid-turn. While [`Slot::must_switch_out`] the
    /// side's canonical `request` is `switch` (F6).
    pub switch_flag: SwitchFlag,
    pub substitute_hp: i16,
    pub dynamax: DynamaxState,
}

impl Slot {
    /// Whether this position asks for a mid-turn switch (Showdown's `switchFlag` on
    /// `side.active[pos]`): its occupant has [`Slot::switch_flag`], or the flagged Pokémon that
    /// fainted here still holds the position (Emergency Exit after its own recoil).
    pub fn must_switch_out(&self) -> bool {
        self.switch_flag != SwitchFlag::None
            && (self.party_index.is_some() || self.fainted_occupant.is_some())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Side<const N: usize> {
    pub slots: [Slot; N],
    pub party: [Pokemon; PARTY_SIZE],
    pub effects: [Effect; SIDE_EFFECT_COUNT],
    /// Once-per-battle activations this side has spent. Each kind has its own budget
    /// (Mega and Ultra Burst are separate, as in Showdown).
    pub gimmicks_used: GimmickSet,
    /// Faint counters (F13); hidden from the canonical output.
    pub history: SideHistory,
    /// Slot conditions per position (F12): Wish, Healing Wish, Revival Blessing.
    pub slot_conditions: [[SlotEffect; SLOT_CONDITION_COUNT]; N],
}

impl<const N: usize> Default for Side<N> {
    fn default() -> Self {
        Side {
            slots: std::array::from_fn(|_| Slot::default()),
            party: Default::default(),
            effects: [Effect::NONE; SIDE_EFFECT_COUNT],
            gimmicks_used: GimmickSet::EMPTY,
            history: SideHistory::default(),
            slot_conditions: [[SlotEffect::NONE; SLOT_CONDITION_COUNT]; N],
        }
    }
}

/// How the battle stands. Showdown decides it in `checkWin`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BattleResult {
    #[default]
    Ongoing,
    Win(SideId),
    Tie,
}

impl BattleResult {
    pub fn is_over(self) -> bool {
        self != BattleResult::Ongoing
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State<const N: usize> {
    pub sides: [Side<N>; 2],
    pub field: [Effect; FIELD_EFFECT_COUNT],
    pub turn: u16,
    pub result: BattleResult,
    /// Showdown `battle.lastMove`: the active move at the end of the last action that used one
    /// (a called move, not its caller; never a future move's hit), kept across turns.
    /// Read by Copycat. Hidden from the canonical output; recorded only while a Copycat is in a
    /// party (`turn::battle::HistoryReaders`), so it does not split positions otherwise.
    pub last_move: MoveId,
}

impl<const N: usize> Default for State<N> {
    fn default() -> Self {
        State {
            sides: [Side::default(), Side::default()],
            field: [Effect::NONE; FIELD_EFFECT_COUNT],
            turn: 0,
            result: BattleResult::Ongoing,
            last_move: MoveId::NONE,
        }
    }
}

impl<const N: usize> State<N> {
    pub const SLOTS: usize = N;

    pub fn side(&self, side: SideId) -> &Side<N> {
        &self.sides[side.index()]
    }

    pub fn side_mut(&mut self, side: SideId) -> &mut Side<N> {
        &mut self.sides[side.index()]
    }

    pub fn slot(&self, r: SlotRef) -> &Slot {
        &self.sides[r.side.index()].slots[r.slot as usize]
    }

    pub fn slot_mut(&mut self, r: SlotRef) -> &mut Slot {
        &mut self.sides[r.side.index()].slots[r.slot as usize]
    }

    /// The Pokémon in an active slot, if any.
    pub fn active(&self, r: SlotRef) -> Option<&Pokemon> {
        let side = &self.sides[r.side.index()];
        side.slots[r.slot as usize]
            .party_index
            .map(|i| &side.party[i as usize])
    }

    pub fn active_mut(&mut self, r: SlotRef) -> Option<&mut Pokemon> {
        let side = &mut self.sides[r.side.index()];
        let index = side.slots[r.slot as usize].party_index?;
        Some(&mut side.party[index as usize])
    }

    /// The party member in an active slot, if any.
    pub fn active_ref(&self, r: SlotRef) -> Option<PokemonRef> {
        self.slot(r).party_index.map(|party| PokemonRef {
            side: r.side,
            party,
        })
    }

    pub fn pokemon(&self, r: PokemonRef) -> &Pokemon {
        &self.sides[r.side.index()].party[r.party as usize]
    }

    pub fn pokemon_mut(&mut self, r: PokemonRef) -> &mut Pokemon {
        &mut self.sides[r.side.index()].party[r.party as usize]
    }

    /// All slot references in a fixed order (side one first).
    pub fn slot_refs() -> impl Iterator<Item = SlotRef> {
        [SideId::One, SideId::Two]
            .into_iter()
            .flat_map(|side| (0..N as u8).map(move |slot| SlotRef { side, slot }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::moves;

    #[test]
    fn champions_pp_follows_the_showdown_formula() {
        assert_eq!(champions_max_pp(moves::HYPNOSIS), 20);
        assert_eq!(champions_max_pp(moves::WOOD_HAMMER), 16);
        // Champions lowers Protect to base PP 5.
        assert_eq!(champions_max_pp(moves::PROTECT), 8);
        assert_eq!(champions_max_pp(moves::FOCUS_BLAST), 8);
        assert_eq!(
            MoveSlot::full(moves::FAKE_OUT),
            MoveSlot {
                id: moves::FAKE_OUT,
                pp: 12,
                disabled: false
            }
        );
        // The formula is only exact for multiples of 5; every boostable move must be one, and
        // nothing may exceed the Champions cap.
        for id in MoveId::all() {
            let data = id.data();
            let pp = data.pp;
            if !data.no_pp_boosts {
                assert_eq!(pp % 5, 0, "{id:?} has base PP {pp}");
            }
            assert!(champions_max_pp(id) <= 20, "{id:?}");
        }
    }
}
