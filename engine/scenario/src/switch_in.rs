//! Initial switch-in outcomes: the leads' start-of-battle effects, expanded into weighted
//! states. Runs once per scenario, outside the search hot path.
//!
//! The expansion itself is the turn engine's ([`lab_engine::turn::enumerate_start`]: Showdown
//! `runAction('start')` → `switchIn` for every lead → one `runSwitch` for all of them, start
//! handlers in Speed order with uniform ties, Trace's target uniform). This module checks that
//! `state` is a fresh start and turns the engine's refusals into errors that name the offending
//! ability, item or species handler (`SwitchInError`). Nothing that can change state is skipped
//! silently.

use std::fmt;

use lab_engine::dex::{AbilityId, Gender, ItemId, SpeciesId};
use lab_engine::field::Effect;
use lab_engine::state::{PokemonRef, SideId, SlotRef, State, Status, BOOST_COUNT, PARTY_SIZE};
use lab_engine::turn::{
    enumerate_start, item_start_handler, species_start_handler, start_handler, switch_in_supported,
    TurnError,
};

use crate::LoadedScenario;

/// One initial state and its probability. The probabilities of an expansion sum to 1.
#[derive(Clone, Debug, PartialEq)]
pub struct InitialOutcome<const N: usize> {
    pub probability: f64,
    pub state: State<N>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchInError {
    /// Only 1 and 2 active slots per side have Showdown's "every foe is adjacent" layout.
    UnsupportedSlotCount(usize),
    /// The state is not a fresh start (effects, boosts, volatiles, status, empty slots, ...).
    NotInitial { reason: String },
    UnsupportedAbility {
        slot: SlotRef,
        ability: AbilityId,
        handler: &'static str,
    },
    UnsupportedItem {
        slot: SlotRef,
        item: ItemId,
        handler: &'static str,
    },
    UnsupportedSpecies {
        side: SideId,
        party_index: u8,
        species: SpeciesId,
        handler: &'static str,
    },
    /// The turn engine refused something during the start (Trace without a traceable foe,
    /// next to No Ability or holding Ability Shield, a copied ability it cannot start, ...).
    Unsupported { what: String },
    /// The turn engine rejected the start for a reason that is not a missing mechanic (the
    /// battle is over, a replacement is pending, ...): an invariant the loader keeps, not
    /// something to implement (board A4-t3).
    Turn(TurnError),
}

impl fmt::Display for SwitchInError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SwitchInError::UnsupportedSlotCount(n) => {
                write!(f, "{n} active slots per side are not supported")
            }
            SwitchInError::NotInitial { reason } => {
                write!(f, "not an initial position: {reason}")
            }
            SwitchInError::UnsupportedAbility {
                slot,
                ability,
                handler,
            } => write!(
                f,
                "{}: ability {:?} has switch-in handler {handler} that is not implemented",
                slot_name(*slot),
                ability.data().name
            ),
            SwitchInError::UnsupportedItem {
                slot,
                item,
                handler,
            } => write!(
                f,
                "{}: item {:?} has switch-in handler {handler} that is not implemented",
                slot_name(*slot),
                item.data().name
            ),
            SwitchInError::UnsupportedSpecies {
                side,
                party_index,
                species,
                handler,
            } => write!(
                f,
                "{} party[{party_index}]: species {:?} has handler {handler} that is not implemented",
                side_name(*side),
                species.data().name
            ),
            SwitchInError::Unsupported { what } => write!(f, "not implemented: {what}"),
            SwitchInError::Turn(e) => write!(f, "the start was rejected: {e}"),
        }
    }
}

impl std::error::Error for SwitchInError {}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

fn slot_name(slot: SlotRef) -> String {
    format!("{} slot {}", side_name(slot.side), slot.slot)
}

fn not_initial(reason: String) -> SwitchInError {
    SwitchInError::NotInitial { reason }
}

/// Checks that `state` is leads-placed-nothing-started, and that every handler that could
/// fire during the start is implemented.
fn validate<const N: usize>(state: &State<N>) -> Result<(), SwitchInError> {
    if N == 0 || N > 2 {
        return Err(SwitchInError::UnsupportedSlotCount(N));
    }
    if state.field.iter().any(|e| *e != Effect::NONE) {
        return Err(not_initial("a field effect is set".into()));
    }
    for side_id in [SideId::One, SideId::Two] {
        let side = state.side(side_id);
        if side.effects.iter().any(|e| *e != Effect::NONE) {
            return Err(not_initial(format!(
                "{} has a side effect",
                side_name(side_id)
            )));
        }
        if !side.gimmicks_used.is_empty() {
            return Err(not_initial(format!(
                "{} already used a gimmick",
                side_name(side_id)
            )));
        }
        // `BattleStart` runs for every party member's species.
        for (i, mon) in side.party.iter().enumerate() {
            if mon.species.is_none() {
                continue;
            }
            let handlers = mon.species.data().handlers;
            if let Some(handler) = handlers.iter().copied().find(|h| *h == "onBattleStart") {
                return Err(SwitchInError::UnsupportedSpecies {
                    side: side_id,
                    party_index: i as u8,
                    species: mon.species,
                    handler,
                });
            }
            if mon.status != Status::None || mon.status_turns != 0 {
                return Err(not_initial(format!(
                    "{} party[{i}] has a status",
                    side_name(side_id)
                )));
            }
        }
    }

    let mut seen = [[false; PARTY_SIZE]; 2];
    for r in State::<N>::slot_refs() {
        let slot = state.slot(r);
        let index = match slot.party_index {
            Some(index) if (index as usize) < PARTY_SIZE => index,
            _ => return Err(not_initial(format!("{} has no Pokémon", slot_name(r)))),
        };
        let side_seen = &mut seen[r.side.index()][index as usize];
        if *side_seen {
            return Err(not_initial(format!(
                "{} party[{index}] is active twice",
                side_name(r.side)
            )));
        }
        *side_seen = true;
        if slot.boosts != [0; BOOST_COUNT]
            || !slot.volatiles.is_empty()
            || !slot.last_move.is_none()
            || slot.move_actions != 0
            || slot.substitute_hp != 0
            || slot.dynamax.is_active()
            || slot.fainted_occupant.is_some()
        {
            return Err(not_initial(format!(
                "{} has slot state (boosts, volatiles, substitute or Dynamax)",
                slot_name(r)
            )));
        }
        let mon = &state.side(r.side).party[index as usize];
        if mon.species.is_none() || !mon.is_alive() || mon.hp != mon.max_hp {
            return Err(not_initial(format!(
                "{} is not a healthy Pokémon",
                slot_name(r)
            )));
        }
        if !switch_in_supported(mon.ability) {
            return Err(SwitchInError::UnsupportedAbility {
                slot: r,
                ability: mon.ability,
                handler: start_handler(mon.ability.data().handlers).unwrap_or("suppressWeather"),
            });
        }
        if let Some(handler) = item_start_handler(mon.item) {
            return Err(SwitchInError::UnsupportedItem {
                slot: r,
                item: mon.item,
                handler,
            });
        }
        if let Some(handler) = species_start_handler(mon.species) {
            return Err(SwitchInError::UnsupportedSpecies {
                side: r.side,
                party_index: index,
                species: mon.species,
                handler,
            });
        }
    }
    Ok(())
}

/// Weighted states after the leads' start-of-battle effects. `state` must be a fresh start
/// (as [`crate::load_scenario_file`] builds it). Outcomes are in first-reached order.
///
/// Undecided genders are decided first where something in the battle reads them
/// ([`decide_genders`]): each gender assignment is its own start, weighted by its probability.
pub fn expand_switch_ins<const N: usize>(
    state: &State<N>,
) -> Result<Vec<InitialOutcome<N>>, SwitchInError> {
    validate(state)?;
    let mut out = Vec::new();
    for decided in decide_genders(state) {
        let mut work = decided.state.clone();
        let outcomes = enumerate_start(&mut work).map_err(start_error)?;
        debug_assert_eq!(work, decided.state);
        out.extend(outcomes.into_iter().map(|o| {
            let mut end = decided.state.clone();
            end.apply(&o.instructions);
            InitialOutcome {
                probability: decided.probability * o.probability,
                state: end,
            }
        }));
    }
    Ok(out)
}

/// The start's turn error as a switch-in error: only the turn engine's `Unsupported` is a
/// missing mechanic; any other rejection keeps its kind (board A4-t3).
fn start_error(e: TurnError) -> SwitchInError {
    match e {
        TurnError::Unsupported(what) => SwitchInError::Unsupported { what },
        other => SwitchInError::Turn(other),
    }
}

/// Board R13b: Showdown's Pokemon constructor gives a set without a gender and a species without
/// a fixed one `this.battle.sample(['M', 'F'])` (sim/pokemon.ts): a fair coin, not the species'
/// gender ratio, drawn when the players join and never changed. The loader leaves such a member
/// [`Gender::Random`]; if the battle can read a gender ([`gender_readers`]), every such member of
/// both parties is decided here, all 2^k assignments at 1/2^k each (p1's party, then p2's, in
/// party order; male before female). Otherwise nothing reads it and the state stays as it is (one
/// start: states that differ only in a gender nothing reads would split positions for nothing).
/// The oracle is `oracle/gender-mix.cjs` (enumerate.cjs keeps its seed's draw).
pub fn decide_genders<const N: usize>(state: &State<N>) -> Vec<InitialOutcome<N>> {
    let undecided: Vec<PokemonRef> = [SideId::One, SideId::Two]
        .into_iter()
        .flat_map(|side| {
            state
                .side(side)
                .party
                .iter()
                .enumerate()
                .filter(|(_, m)| !m.species.is_none() && m.gender == Gender::Random)
                .map(move |(i, _)| PokemonRef {
                    side,
                    party: i as u8,
                })
        })
        .collect();
    if undecided.is_empty() || !gender_readers(state) {
        return vec![InitialOutcome {
            probability: 1.0,
            state: state.clone(),
        }];
    }
    let variants = 1usize << undecided.len();
    (0..variants)
        .map(|bits| {
            let mut decided = state.clone();
            for (k, &pokemon) in undecided.iter().enumerate() {
                decided.pokemon_mut(pokemon).gender = if bits >> k & 1 == 1 {
                    Gender::Female
                } else {
                    Gender::Male
                };
            }
            InitialOutcome {
                probability: 1.0 / variants as f64,
                state: decided,
            }
        })
        .collect()
}

/// Whether anything in the battle can read a gender: Attract or Captivate among a party's moves
/// (Transform copies moves from Pokémon in the battle), Cute Charm or Rivalry as a party
/// member's ability (Skill Swap, Trace, Role Play, ... only move abilities between party
/// members) or as the ability of a Mega forme an item in the battle gives.
fn gender_readers<const N: usize>(state: &State<N>) -> bool {
    use lab_engine::dex::{abilities, moves};
    let reads = |a: AbilityId| a == abilities::CUTE_CHARM || a == abilities::RIVALRY;
    let mons = || state.sides.iter().flat_map(|side| side.party.iter());
    mons().any(|mon| {
        mon.moves
            .iter()
            .any(|s| s.id == moves::ATTRACT || s.id == moves::CAPTIVATE)
            || reads(mon.ability)
            || reads(mon.base_ability)
            || mons().any(|holder| {
                lab_engine::gimmick::mega_evolution(mon.species, holder.item)
                    .is_some_and(|mega| mega.data().abilities.iter().any(|&a| reads(a)))
            })
    })
}

/// [`expand_switch_ins`] for a loaded scenario.
pub fn initial_outcomes<const N: usize>(
    loaded: &LoadedScenario<N>,
) -> Result<Vec<InitialOutcome<N>>, SwitchInError> {
    expand_switch_ins(&loaded.state)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only `TurnError::Unsupported` becomes `SwitchInError::Unsupported`; the others keep
    /// their kind, and the scenario error of one is `Invalid`, not `Unsupported` (board A4-t3).
    #[test]
    fn a_start_error_keeps_its_kind() {
        assert_eq!(
            start_error(TurnError::Unsupported("x".into())),
            SwitchInError::Unsupported { what: "x".into() }
        );
        for other in [
            TurnError::BattleOver,
            TurnError::ReplacementPending(SideId::Two),
        ] {
            let e = start_error(other.clone());
            assert_eq!(e, SwitchInError::Turn(other));
            assert!(!crate::ScenarioError::from(e).is_unsupported());
        }
        assert!(
            crate::ScenarioError::from(start_error(TurnError::Unsupported("x".into())))
                .is_unsupported()
        );
    }
}
