//! The turn engine: every outcome of one turn with its probability.
//!
//! [`enumerate_turn`] takes a state and both sides' joint actions and returns the exact
//! outcome distribution as reversible instruction lists ([`Outcome`]); outcomes that end in
//! the same state are merged. It follows Showdown's Champions mechanics for what is
//! implemented and refuses the rest: before anything runs, [`support`] checks the chosen
//! moves and every ability, item, volatile and field effect in play, and anything the
//! engine does not implement is a [`TurnError::Unsupported`] instead of a silently wrong
//! result. The oracle (`engine/oracle`) is the reference for parity.
//!
//! Structure of one turn (Showdown `turnLoop`): the actions run one at a time, best first by
//! (order, priority, Speed) with random tie-breaks and re-sorting after each action; faints
//! are processed after each action; then the residual (end-of-turn) handlers; then
//! `checkFainted`/`endTurn`. Replacements after a faint are the next decision, not part of
//! the turn.

mod abilities;
mod battle;
mod branch;
mod conditions;
pub mod coverage;
mod diff;
mod field_events;
mod items;
pub mod lock;
mod mega;
mod moves;
mod order;
mod queue;
mod residual;
mod support;
mod switching;
mod update;

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use crate::action::{JointAction, SlotAction};
use crate::dex::{moves as move_ids, MoveFlags, MoveId};
use crate::field::FieldEffect;
use crate::gimmick::Gimmick;
use crate::instruction::Outcome;
use crate::rules::{ActionError, Ruleset};
use crate::state::{PokemonRef, SideId, SlotRef, State};
use crate::volatile::Volatile;

use battle::Battle;
use branch::Chooser;
use order::{ORDER_MEGA, ORDER_MOVE, ORDER_SWITCH};
use queue::{Action, ActionKind};

pub use abilities::trapped;
pub use lock::{locked_move, Locked, RECHARGE_INDEX, STRUGGLE_INDEX};
pub use moves::{takes_target, valid_target_loc};
pub use switching::{
    item_start_handler, species_start_handler, start_handler, switch_in_supported,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnError {
    /// The battle is over.
    BattleOver,
    /// A side must first replace a fainted Pokémon (a switch decision, not a turn).
    ReplacementPending(SideId),
    /// Rejected by the ruleset.
    Action { side: SideId, error: ActionError },
    /// Not a legal choice in this state (empty slot, no PP, disabled move, bad target, ...).
    InvalidChoice {
        side: SideId,
        slot: u8,
        reason: String,
    },
    /// Something in play is not implemented by the turn engine.
    Unsupported(String),
}

impl fmt::Display for TurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TurnError::BattleOver => write!(f, "the battle is over"),
            TurnError::ReplacementPending(side) => {
                write!(f, "{side:?} must replace a fainted Pokémon first")
            }
            TurnError::Action { side, error } => write!(f, "{side:?}: {error}"),
            TurnError::InvalidChoice { side, slot, reason } => {
                write!(f, "{side:?} slot {slot}: {reason}")
            }
            TurnError::Unsupported(what) => write!(f, "not implemented: {what}"),
        }
    }
}

impl std::error::Error for TurnError {}

/// Every outcome of the turn in which the sides choose `choices` (side one first). `state`
/// is left unchanged. Probabilities sum to 1; outcomes are in first-reached order.
pub fn enumerate_turn<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
) -> Result<Vec<Outcome>, TurnError> {
    let choices = check_turn(state, ruleset, &choices)?;
    let start = Pending {
        queue: initial_queue(state, &choices),
        in_progress: None,
        done: false,
        fractional_drawn: false,
    };
    enumerate_stages(state, start, |b, pending| {
        run_stage(b, pending)?;
        Ok(pending.done)
    })
}

/// `samples` random playthroughs of the turn (Monte Carlo), merged by end state; each
/// outcome's probability is its frequency. For cross-checking turns whose exact distribution
/// is too large to enumerate. Deterministic for a given `seed`.
pub fn sample_turn<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
    samples: usize,
    seed: u64,
) -> Result<Vec<Outcome>, TurnError> {
    let choices = check_turn(state, ruleset, &choices)?;
    let start = Pending {
        queue: initial_queue(state, &choices),
        in_progress: None,
        done: false,
        fractional_drawn: false,
    };
    sample_stages(state, samples, seed, start, |b, pending| {
        run_stage(b, pending)?;
        Ok(pending.done)
    })
}

/// Every outcome of the battle start (Showdown `runAction('start')` → `switchIn` for every
/// lead → one batched `runSwitch`): the leads' start handlers in Speed order, Speed ties
/// uniformly at random, Trace's target uniformly at random. `state` must hold the leads in
/// their slots with nothing started yet; it is left unchanged.
pub fn enumerate_start<const N: usize>(state: &mut State<N>) -> Result<Vec<Outcome>, TurnError> {
    let leads: Vec<SlotRef> = State::<N>::slot_refs()
        .filter(|&r| state.active_ref(r).is_some())
        .collect();
    enumerate_stages(state, (), |b, _| {
        for &slot in &leads {
            let pokemon = b.occupant(slot).expect("a lead");
            if let Some(why) = switching_problem_at_start(b, pokemon) {
                return Err(b.unsupported(why));
            }
        }
        b.battle_start = true;
        switching::run_switch_in(b, &leads)?;
        b.battle_start = false;
        // `runAction('runSwitch')` ends with `eachEvent('Update')`.
        update::update_event(b)?;
        items::stage_end_check(b)?;
        Ok(true)
    })
}

fn switching_problem_at_start<const N: usize>(
    b: &Battle<'_, N>,
    pokemon: PokemonRef,
) -> Option<String> {
    let mon = b.mon(pokemon);
    let name = mon.species.data().name;
    if let Some(handler) = switching::item_start_handler(mon.item) {
        return Some(format!(
            "{name}: item {} switch-in handler {handler}",
            mon.item.data().name
        ));
    }
    if let Some(handler) = switching::species_start_handler(mon.species) {
        return Some(format!("{name}: species switch-in handler {handler}"));
    }
    if !switching::switch_in_supported(mon.ability) {
        return Some(format!(
            "{name}: ability {} switch-in handler ({:?})",
            mon.ability.data().name,
            mon.ability.data().handlers
        ));
    }
    None
}

/// The replacement decision after faints (Showdown `request: switch` → `instaswitch` actions →
/// one batched `runSwitch` → `endTurn`): every outcome of both sides sending in `choices`
/// (for each side, per active slot, the party index that fills it, `None` where nothing
/// changes). A side that must replace gives exactly as many switches as it can (empty slots,
/// bounded by its bench); a side that need not gives none. The fainted occupants lose `fnt`,
/// the newcomers' start handlers run in Speed order (ties uniformly at random), and the turn
/// counter advances. `state` is left unchanged.
pub fn enumerate_replacements<const N: usize>(
    state: &mut State<N>,
    choices: [[Option<u8>; N]; 2],
) -> Result<Vec<Outcome>, TurnError> {
    check_replacements(state, &choices)?;
    enumerate_stages(state, (), |b, _| {
        run_replacements(b, &choices)?;
        items::stage_end_check(b)?;
        Ok(true)
    })
}

/// Validates a replacement decision (see [`enumerate_replacements`]).
fn check_replacements<const N: usize>(
    state: &State<N>,
    choices: &[[Option<u8>; N]; 2],
) -> Result<(), TurnError> {
    if state.result.is_over() {
        return Err(TurnError::BattleOver);
    }
    let mut any = false;
    for (side, choice) in [SideId::One, SideId::Two].into_iter().zip(choices) {
        let s = state.side(side);
        let bench: Vec<u8> = (0..s.party.len() as u8)
            .filter(|&i| {
                s.party[i as usize].hp > 0
                    && !s.slots.iter().any(|slot| slot.party_index == Some(i))
            })
            .collect();
        let empty = s
            .slots
            .iter()
            .filter(|slot| slot.party_index.is_none())
            .count();
        let required = empty.min(bench.len());
        let mut given = Vec::new();
        for (i, &c) in choice.iter().enumerate() {
            let invalid = |reason: String| TurnError::InvalidChoice {
                side,
                slot: i as u8,
                reason,
            };
            let Some(party_index) = c else {
                continue;
            };
            if s.slots[i].party_index.is_some() {
                return Err(invalid("the slot is occupied".into()));
            }
            if !bench.contains(&party_index) || given.contains(&party_index) {
                return Err(invalid(format!("cannot switch to party {party_index}")));
            }
            given.push(party_index);
        }
        if given.len() != required {
            return Err(TurnError::InvalidChoice {
                side,
                slot: 0,
                reason: format!("{required} replacements needed, {} given", given.len()),
            });
        }
        any |= required > 0;
    }
    if !any {
        return Err(TurnError::InvalidChoice {
            side: SideId::One,
            slot: 0,
            reason: "no replacement is pending".into(),
        });
    }
    Ok(())
}

/// The replacement stage: `instaswitch` actions in order (the fainted occupant's Speed,
/// ties uniformly at random), then the newcomers' batched `runSwitch`, then `endTurn`.
fn run_replacements<const N: usize>(
    b: &mut Battle<'_, N>,
    choices: &[[Option<u8>; N]; 2],
) -> Result<(), TurnError> {
    let mut switches: Vec<(SlotRef, u8, i32)> = Vec::new();
    for (side, choice) in [SideId::One, SideId::Two].into_iter().zip(choices) {
        for (i, &c) in choice.iter().enumerate() {
            let Some(party_index) = c else {
                continue;
            };
            let slot = SlotRef {
                side,
                slot: i as u8,
            };
            let speed = match b.state.slot(slot).fainted_occupant {
                Some(party) => switching::fainted_action_speed(b, PokemonRef { side, party }),
                // An empty slot with no fainted occupant (a scenario built by hand): speed 1
                // like Showdown's pokemon-less actions.
                None => 1,
            };
            switches.push((slot, party_index, speed));
        }
    }
    let mut newcomers = Vec::with_capacity(switches.len());
    while !switches.is_empty() {
        let best = switches.iter().map(|s| s.2).max().expect("non-empty");
        let tied: Vec<usize> = (0..switches.len())
            .filter(|&i| switches[i].2 == best)
            .collect();
        let pick = if tied.len() == 1 {
            tied[0]
        } else {
            tied[b.rng.uniform(tied.len())]
        };
        let (slot, party_index, _) = switches.remove(pick);
        switching::switch_in(b, slot, party_index, true)?;
        newcomers.push(slot);
    }
    switching::run_switch_in(b, &newcomers)?;
    if b.is_over() {
        return Ok(());
    }
    // `runAction`'s tail with nothing left in the queue: `checkFainted` (a newcomer that fainted
    // to entry hazards gets `fnt`), the Update, then `endTurn` (which waits for another
    // replacement if one is needed).
    residual::check_fainted(b);
    update::update_event(b)?;
    residual::end_turn(b);
    Ok(())
}

/// The end of Showdown `runAction` for a move, switch or Mega Evolution: faints (the turn ends
/// if the battle does), then `eachEvent('Update')`.
fn after_action<const N: usize>(
    b: &mut Battle<'_, N>,
    pending: &mut Pending,
) -> Result<(), TurnError> {
    if b.faint_messages(true) {
        pending.done = true;
        b.queue.clear();
    } else {
        update::update_event(b)?;
    }
    Ok(())
}

/// Runs `stage` repeatedly from `start` until it reports completion, merging identical
/// (state, pending) pairs after every stage and enumerating every random path within a stage
/// by replay. Returns the merged end states as outcomes; `state` is left unchanged.
fn enumerate_stages<const N: usize, P: Clone + Eq + Hash>(
    state: &mut State<N>,
    start: P,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<bool, TurnError>,
) -> Result<Vec<Outcome>, TurnError> {
    // The turn runs in stages (one action, or the end of turn). After every stage identical
    // (state, remaining turn) pairs merge, so the work grows with the number of distinct
    // intermediate positions, not with the number of random paths. Within a stage every
    // random path is enumerated by replay.
    let mut frontier: Vec<(State<N>, P, f64)> = vec![(state.clone(), start, 1.0)];
    let mut finished: Vec<(State<N>, f64)> = Vec::new();
    let mut finished_index: HashMap<State<N>, usize> = HashMap::new();
    while !frontier.is_empty() {
        // Value: (first-reached index, probability); keeps the output order deterministic.
        let mut next: HashMap<(State<N>, P), (usize, f64)> = HashMap::new();
        for (mut work, pending, probability) in frontier {
            let mut chooser = Chooser::new();
            loop {
                chooser.begin_run();
                let mut after = pending.clone();
                let (result, log) = {
                    let mut b = Battle::new(&mut work, &mut chooser);
                    let result = stage(&mut b, &mut after);
                    (result, std::mem::take(&mut b.log))
                };
                let done = result?;
                let p = probability * chooser.probability();
                if done {
                    match finished_index.get(&work) {
                        Some(&i) => finished[i].1 += p,
                        None => {
                            finished_index.insert(work.clone(), finished.len());
                            finished.push((work.clone(), p));
                        }
                    }
                } else {
                    let order = next.len();
                    next.entry((work.clone(), after)).or_insert((order, 0.0)).1 += p;
                }
                work.reverse(&log);
                if !chooser.advance() {
                    break;
                }
            }
        }
        if std::env::var_os("LAB_ENGINE_STATS").is_some() {
            eprintln!(
                "lab-engine: stage frontier {} states, {} finished",
                next.len(),
                finished.len()
            );
        }
        let mut staged: Vec<_> = next.into_iter().collect();
        staged.sort_unstable_by_key(|(_, (order, _))| *order);
        frontier = staged
            .into_iter()
            .map(|((s, q), (_, p))| (s, q, p))
            .collect();
    }

    Ok(finished
        .into_iter()
        .map(|(end, probability)| Outcome {
            probability,
            instructions: diff::instructions(state, &end),
        })
        .collect())
}

/// Monte Carlo counterpart of [`enumerate_stages`].
fn sample_stages<const N: usize, P: Clone>(
    state: &mut State<N>,
    samples: usize,
    seed: u64,
    start: P,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<bool, TurnError>,
) -> Result<Vec<Outcome>, TurnError> {
    let mut chooser = Chooser::sampler(seed);
    let mut finished: Vec<(State<N>, f64)> = Vec::new();
    let mut index: HashMap<State<N>, usize> = HashMap::new();
    let weight = 1.0 / samples as f64;
    let begin = state.clone();
    for _ in 0..samples {
        let mut pending = start.clone();
        let mut log = Vec::new();
        let mut result = Ok(false);
        while matches!(result, Ok(false)) {
            chooser.begin_run();
            let mut b = Battle::new(state, &mut chooser);
            result = stage(&mut b, &mut pending);
            log.append(&mut b.log);
        }
        if let Err(error) = result {
            state.reverse(&log);
            return Err(error);
        }
        match index.get(state) {
            Some(&i) => finished[i].1 += weight,
            None => {
                index.insert(state.clone(), finished.len());
                finished.push((state.clone(), weight));
            }
        }
        state.reverse(&log);
    }
    debug_assert_eq!(*state, begin);
    Ok(finished
        .into_iter()
        .map(|(end, probability)| Outcome {
            probability,
            instructions: diff::instructions(&begin, &end),
        })
        .collect())
}

/// Validates the choices and that everything in play is implemented. Returns the choices as
/// the turn runs them: a locked Pokémon's move choice becomes its locked move (Showdown
/// `chooseMove` ignores what was picked), the `recharge` pseudo-move as `RECHARGE_INDEX`.
fn check_turn<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    choices: &[JointAction<N>; 2],
) -> Result<[JointAction<N>; 2], TurnError> {
    let mut normalized = *choices;
    if state.result.is_over() {
        return Err(TurnError::BattleOver);
    }
    for side in [SideId::One, SideId::Two] {
        let s = state.side(side);
        let empty = s.slots.iter().any(|slot| slot.party_index.is_none());
        let bench = (0..s.party.len() as u8).any(|i| {
            s.party[i as usize].hp > 0 && !s.slots.iter().any(|slot| slot.party_index == Some(i))
        });
        if empty && bench {
            return Err(TurnError::ReplacementPending(side));
        }
    }
    for (side, action) in [SideId::One, SideId::Two].into_iter().zip(choices) {
        ruleset
            .validate_joint_action(state, side, action)
            .map_err(|error| TurnError::Action { side, error })?;
        let mut switching_in = Vec::new();
        for (i, &slot_action) in action.iter().enumerate() {
            let slot = SlotRef {
                side,
                slot: i as u8,
            };
            let invalid = |reason: String| TurnError::InvalidChoice {
                side,
                slot: i as u8,
                reason,
            };
            let occupant = state.active(slot).filter(|p| p.hp > 0);
            // A locked Pokémon: any move choice stands for the locked move, nothing else is
            // allowed (`trapped`), no PP is needed.
            if let (Some(locked), Some(mon)) = (lock::locked_move(state, slot), occupant) {
                let SlotAction::Move { gimmick, .. } = slot_action else {
                    return Err(invalid(format!("locked into {locked:?}; cannot switch")));
                };
                if !gimmick.is_none() {
                    return Err(invalid(format!("locked into {locked:?}; no {gimmick:?}")));
                }
                let index = match locked {
                    Locked::Recharge => RECHARGE_INDEX,
                    Locked::Move(id) => {
                        mon.moves.iter().position(|m| m.id == id).ok_or_else(|| {
                            invalid(format!("locked move {} not known", id.data().name))
                        })? as u8
                    }
                };
                normalized[side.index()][i] = SlotAction::Move {
                    index,
                    target: 0,
                    gimmick: Gimmick::None,
                };
                continue;
            }
            match (slot_action, occupant) {
                (SlotAction::Pass, None) => {}
                (SlotAction::Pass, Some(_)) => return Err(invalid("must act".into())),
                (_, None) => return Err(invalid("empty or fainted slot must pass".into())),
                (SlotAction::Switch { party_index }, Some(_)) => {
                    let target = &state.side(side).party[party_index as usize];
                    let active = state
                        .side(side)
                        .slots
                        .iter()
                        .any(|s| s.party_index == Some(party_index));
                    if target.hp <= 0 || active || switching_in.contains(&party_index) {
                        return Err(invalid(format!("cannot switch to party {party_index}")));
                    }
                    switching_in.push(party_index);
                }
                (
                    SlotAction::Move {
                        index,
                        target,
                        gimmick,
                    },
                    Some(mon),
                ) => {
                    // Without a usable move the only choice is Struggle (`move 1` names it).
                    let usable = mon.moves.iter().any(|m| {
                        !m.id.is_none() && m.pp > 0 && disabled(state, slot, m.id).is_none()
                    });
                    let index = if !usable && index == 0 {
                        STRUGGLE_INDEX
                    } else {
                        index
                    };
                    if index == STRUGGLE_INDEX {
                        if usable {
                            return Err(invalid("Struggle while a move is usable".into()));
                        }
                        if target != 0 || !gimmick.is_none() {
                            return Err(invalid("Struggle takes no target or gimmick".into()));
                        }
                        if let Some(why) = support::move_unsupported(move_ids::STRUGGLE) {
                            return Err(TurnError::Unsupported(why));
                        }
                        normalized[side.index()][i] = SlotAction::Move {
                            index,
                            target: 0,
                            gimmick: Gimmick::None,
                        };
                        continue;
                    }
                    let slot_move = mon.moves[index as usize];
                    let id = slot_move.id;
                    if id.is_none() {
                        return Err(invalid(format!("no move in slot {index}")));
                    }
                    if slot_move.pp == 0 {
                        return Err(invalid(format!("{} has no PP", id.data().name)));
                    }
                    if let Some(reason) = disabled(state, slot, id) {
                        return Err(invalid(reason));
                    }
                    let data = id.data();
                    let needs = takes_target(N, data.target);
                    let ok = if needs {
                        target != 0 && valid_target_loc(N, slot, target, data.target)
                    } else {
                        target == 0
                    };
                    if !ok {
                        return Err(invalid(format!(
                            "target {target} for {} ({:?})",
                            data.name, data.target
                        )));
                    }
                    match gimmick {
                        Gimmick::None => {}
                        Gimmick::Mega => {
                            mega::mega_target(mon).map_err(TurnError::Unsupported)?;
                        }
                        other => {
                            return Err(TurnError::Unsupported(format!(
                                "{other:?} activation (effects not implemented)"
                            )));
                        }
                    }
                    if let Some(why) = support::move_unsupported(id) {
                        return Err(TurnError::Unsupported(why));
                    }
                    if id == move_ids::SLEEP_TALK {
                        let known = mon.moves.map(|m| m.id);
                        if let Some(why) = support::sleep_talk_problem(&known) {
                            return Err(TurnError::Unsupported(why));
                        }
                    }
                }
            }
        }
    }
    support::check_state(state).map_err(TurnError::Unsupported)?;
    Ok(normalized)
}

/// Why a move cannot be chosen now (Showdown `DisableMove` handlers that are implemented).
fn disabled<const N: usize>(state: &State<N>, slot: SlotRef, id: MoveId) -> Option<String> {
    let data = id.data();
    // Champions Fake Out, First Impression: disabled once the user has acted since switching in.
    if (id == move_ids::FAKE_OUT || id == move_ids::FIRST_IMPRESSION)
        && state.slot(slot).move_actions > 0
    {
        return Some(format!("{} only works on the first turn out", data.name));
    }
    if state.field[FieldEffect::Gravity as usize].is_active()
        && data.flags.contains(MoveFlags::GRAVITY)
    {
        return Some(format!("{} is disabled by Gravity", data.name));
    }
    // Encore's `onDisableMove`: only the encored move can be chosen.
    let encore = state.slot(slot).volatiles.get(Volatile::Encore);
    if encore.active
        && encore.mv != id
        && state
            .active(slot)
            .is_some_and(|m| m.moves.iter().any(|s| s.id == encore.mv))
    {
        return Some(format!("Encore locks it into {}", encore.mv.data().name));
    }
    // Taunt and the other conditions' `onDisableMove`.
    if let Some(why) = conditions::disabled_move(state, slot, id) {
        return Some(why);
    }
    items::disabled_move(state, slot, id)
}

/// The rest of a turn between stages: the actions not yet run, a multi-hit move suspended
/// between two hits, and whether the turn is over. (Fainted Pokémon still holding a position
/// are in the state: `Slot::fainted_occupant`.)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Pending {
    queue: Vec<Action>,
    in_progress: Option<moves::MoveProgress>,
    done: bool,
    /// Whether the random fractional priorities (Quick Claw) were drawn: Showdown draws them
    /// when the actions are queued, so the first stage does.
    fractional_drawn: bool,
}

impl<const N: usize> Battle<'_, N> {
    /// Showdown's sort key of a queued action: (order, priority in tenths including the
    /// fractional priority, speed).
    fn action_key(&self, action: &Action) -> (u32, i32, i32) {
        let in_slot = self.alive(action.slot) == Some(action.pokemon);
        let (order, priority) = match action.kind {
            ActionKind::Switch { .. } => (ORDER_SWITCH, 0),
            ActionKind::Mega => (ORDER_MEGA, 0),
            ActionKind::Move {
                index: RECHARGE_INDEX,
                ..
            } => (ORDER_MOVE, 0),
            ActionKind::Move {
                index,
                fractional_tenths,
                ..
            } => {
                let id = lock::action_move_id(self.mon(action.pokemon), index);
                let priority = if in_slot {
                    self.move_priority(action.slot, id)
                } else {
                    // A fainted Pokémon's action stays queued with its base priority.
                    i32::from(id.data().priority)
                };
                (ORDER_MOVE, priority * 10 + i32::from(fractional_tenths))
            }
        };
        let speed = if in_slot {
            self.action_speed(action.slot)
        } else {
            // No handlers run for an inactive Pokémon: stored Speed, Trick Room applies.
            let spe = i32::from(self.mon(action.pokemon).stats[4]);
            if self.field_active(FieldEffect::TrickRoom) {
                -spe
            } else {
                spe
            }
        };
        (action.order.unwrap_or(order), priority, speed)
    }
}

/// The turn's actions in choice order (side one first, slot order).
fn initial_queue<const N: usize>(state: &State<N>, choices: &[JointAction<N>; 2]) -> Vec<Action> {
    let mut queue = Vec::new();
    for (side, action) in [SideId::One, SideId::Two].into_iter().zip(choices) {
        for (i, &slot_action) in action.iter().enumerate() {
            let slot = SlotRef {
                side,
                slot: i as u8,
            };
            let Some(pokemon) = state.active_ref(slot) else {
                continue;
            };
            let kind = match slot_action {
                SlotAction::Pass => continue,
                SlotAction::Move {
                    index,
                    target,
                    gimmick,
                } => {
                    if gimmick == Gimmick::Mega {
                        queue.push(Action {
                            slot,
                            pokemon,
                            kind: ActionKind::Mega,
                            order: None,
                        });
                    }
                    ActionKind::Move {
                        index,
                        target,
                        fractional_tenths: items::fractional_priority_tenths(
                            state.pokemon(pokemon),
                        ),
                    }
                }
                SlotAction::Switch { party_index } => ActionKind::Switch { party_index },
            };
            queue.push(Action {
                slot,
                pokemon,
                kind,
                order: None,
            });
        }
    }
    queue
}

/// One stage of the turn: the next action (with its faints), or, once no action is left,
/// the end of turn. Sets `pending.done` when the turn is over.
fn run_stage<const N: usize>(
    b: &mut Battle<'_, N>,
    pending: &mut Pending,
) -> Result<(), TurnError> {
    // Quick Claw's 1/5 is drawn, and Custap Berry eaten, when the actions are queued (first
    // stage).
    if !pending.fractional_drawn {
        pending.fractional_drawn = true;
        for action in &mut pending.queue {
            if let ActionKind::Move {
                fractional_tenths, ..
            } = &mut action.kind
            {
                if let Some(t) = items::quick_claw(b, action.pokemon, *fractional_tenths) {
                    *fractional_tenths = t;
                }
                if let Some(t) = items::custap(b, action.slot, action.pokemon, *fractional_tenths) {
                    *fractional_tenths = t;
                }
            }
        }
    }
    // The remaining queue is visible to handlers through the Battle while the stage runs.
    b.queue = std::mem::take(&mut pending.queue);
    let result = run_stage_inner(b, pending);
    pending.queue = std::mem::take(&mut b.queue);
    result?;
    items::stage_end_check(b)
}

fn run_stage_inner<const N: usize>(
    b: &mut Battle<'_, N>,
    pending: &mut Pending,
) -> Result<(), TurnError> {
    // A multi-hit move continues with its next hit before anything else.
    if let Some(progress) = pending.in_progress.take() {
        match moves::resume_move(b, progress)? {
            moves::MoveStep::Suspended(progress) => pending.in_progress = Some(progress),
            moves::MoveStep::Done => after_action(b, pending)?,
        }
        return Ok(());
    }
    if !b.queue.is_empty() {
        // Best action by (order asc, priority desc, speed desc), ties uniformly at random.
        let keys: Vec<(u32, i32, i32)> = b.queue.iter().map(|a| b.action_key(a)).collect();
        let best = keys
            .iter()
            .copied()
            .min_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)).then(y.2.cmp(&x.2)))
            .expect("non-empty");
        let tied: Vec<usize> = (0..b.queue.len()).filter(|&i| keys[i] == best).collect();
        let pick = tied[b.rng.uniform(tied.len())];
        let action = b.queue.remove(pick);

        // `runAction` skips a Pokémon that is no longer active or has fainted.
        if b.alive(action.slot) == Some(action.pokemon) {
            match action.kind {
                ActionKind::Move { index, target, .. } => {
                    let will_act = b.will_act();
                    if let moves::MoveStep::Suspended(progress) =
                        moves::run_move(b, action.slot, index, target, will_act)?
                    {
                        pending.in_progress = Some(progress);
                        return Ok(());
                    }
                }
                ActionKind::Switch { party_index } => {
                    switching::run_switch(b, action.slot, party_index)?;
                }
                ActionKind::Mega => {
                    mega::run_mega_evo(b, action.slot)?;
                }
            }
            after_action(b, pending)?;
        }
        return Ok(());
    }

    residual::residual(b)?;
    if !b.is_over() {
        residual::check_fainted(b);
        update::update_event(b)?;
        residual::end_turn(b);
    }
    pending.done = true;
    Ok(())
}
