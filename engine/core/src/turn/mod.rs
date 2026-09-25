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
mod history;
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
use crate::state::{PokemonRef, SideId, SlotRef, State, SwitchFlag};
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
    let start = Pending::new(initial_queue(state, &choices));
    let endings = enumerate_stages(state, start, run_stage)?;
    Ok(outcomes(state, endings, Suspension))
}

/// Continues a turn that stopped for a mid-turn switch decision (`Outcome::suspension`):
/// `choices[side][slot]` is the party index switching into each slot whose
/// [`Slot::switch_flag`] is set (as many as the side has bench members; `None` elsewhere).
/// `state` is the state the suspended outcome's instructions lead to; it is left unchanged
/// and the outcomes' instructions start from it. Showdown: the `instaswitch` actions in the
/// outgoing Pokémon's action-Speed order (ties uniformly at random), the newcomers' batched
/// `runSwitch`, then the rest of the queue; the turn can suspend again.
pub fn resume_turn<const N: usize>(
    state: &mut State<N>,
    suspension: &Suspension,
    choices: [[Option<u8>; N]; 2],
) -> Result<Vec<Outcome>, TurnError> {
    let switches = check_mid_turn_switches(state, &choices)?;
    let mut start = suspension.0.clone();
    start.switches = switches;
    let endings = enumerate_stages(state, start, run_stage)?;
    Ok(outcomes(state, endings, Suspension))
}

/// Validates a mid-turn switch decision (see [`resume_turn`]) and lists the switches.
fn check_mid_turn_switches<const N: usize>(
    state: &State<N>,
    choices: &[[Option<u8>; N]; 2],
) -> Result<Vec<(SlotRef, u8)>, TurnError> {
    if state.result.is_over() {
        return Err(TurnError::BattleOver);
    }
    let mut out = Vec::new();
    let mut any = false;
    for (side, choice) in [SideId::One, SideId::Two].into_iter().zip(choices) {
        let s = state.side(side);
        let bench: Vec<u8> = (0..s.party.len() as u8)
            .filter(|&i| {
                s.party[i as usize].hp > 0
                    && !s.slots.iter().any(|slot| slot.party_index == Some(i))
            })
            .collect();
        let flagged: Vec<usize> = (0..N)
            .filter(|&i| {
                s.slots[i].switch_flag != SwitchFlag::None && s.slots[i].party_index.is_some()
            })
            .collect();
        let required = flagged.len().min(bench.len());
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
            if !flagged.contains(&i) {
                return Err(invalid("the slot is not switching out".into()));
            }
            if !bench.contains(&party_index) || given.contains(&party_index) {
                return Err(invalid(format!("cannot switch to party {party_index}")));
            }
            given.push(party_index);
            out.push((
                SlotRef {
                    side,
                    slot: i as u8,
                },
                party_index,
            ));
        }
        if given.len() != required {
            return Err(TurnError::InvalidChoice {
                side,
                slot: 0,
                reason: format!("{required} mid-turn switches needed, {} given", given.len()),
            });
        }
        any |= required > 0;
    }
    if !any {
        return Err(TurnError::InvalidChoice {
            side: SideId::One,
            slot: 0,
            reason: "no mid-turn switch is pending".into(),
        });
    }
    Ok(out)
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
    let start = Pending::new(initial_queue(state, &choices));
    let endings = sample_stages(state, samples, seed, start, |b, pending| {
        run_stage(b, pending)
    })?;
    Ok(outcomes(state, endings, Suspension))
}

/// Every outcome of the battle start (Showdown `runAction('start')` → `switchIn` for every
/// lead → one batched `runSwitch`): the leads' start handlers in Speed order, Speed ties
/// uniformly at random, Trace's target uniformly at random. `state` must hold the leads in
/// their slots with nothing started yet; it is left unchanged.
pub fn enumerate_start<const N: usize>(state: &mut State<N>) -> Result<Vec<Outcome>, TurnError> {
    let leads: Vec<SlotRef> = State::<N>::slot_refs()
        .filter(|&r| state.active_ref(r).is_some())
        .collect();
    let endings = enumerate_stages(state, (), |b, _| {
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
        Ok(StageEnd::Finished)
    })?;
    Ok(outcomes(state, endings, |()| {
        unreachable!("a start never suspends")
    }))
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
    let endings = enumerate_stages(state, (), |b, _| {
        run_replacements(b, &choices)?;
        items::stage_end_check(b)?;
        Ok(StageEnd::Finished)
    })?;
    Ok(outcomes(state, endings, |()| {
        unreachable!("a replacement never suspends")
    }))
}

/// Validates a replacement decision (see [`enumerate_replacements`]).
fn check_replacements<const N: usize>(
    state: &State<N>,
    choices: &[[Option<u8>; N]; 2],
) -> Result<(), TurnError> {
    if state.result.is_over() {
        return Err(TurnError::BattleOver);
    }
    if let Some(slot) = pending_mid_turn_switch(state) {
        return Err(TurnError::InvalidChoice {
            side: slot.side,
            slot: slot.slot,
            reason: "a mid-turn switch is pending (resume_turn)".into(),
        });
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
        let hp_before = b.state.side(slot.side).party[usize::from(party_index)].hp;
        switching::switch_in(b, slot, party_index, true)?;
        newcomers.push((slot, hp_before));
    }
    let slots: Vec<SlotRef> = newcomers.iter().map(|n| n.0).collect();
    switching::run_switch_in(b, &slots)?;
    if b.is_over() {
        return Ok(());
    }
    // A replacement's Emergency Exit (entry hazards took it to half) would ask for another
    // switch before the turn ends; the replacement decision cannot suspend.
    for &(slot, hp_before) in &newcomers {
        if switching::emergency_exit_would_trigger(b, slot, hp_before) {
            return Err(b.unsupported("Emergency Exit of a replacement hit by entry hazards"));
        }
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
    newcomers: &[(SlotRef, i16)],
) -> Result<StageEnd, TurnError> {
    if b.faint_messages(true) {
        pending.done = true;
        b.queue.clear();
        return Ok(StageEnd::Finished);
    }
    update::update_event(b)?;
    // `runSwitch`'s tail: Emergency Exit for a newcomer the entry hazards took to half.
    for &(slot, hp_before) in newcomers {
        switching::emergency_exit_check(b, slot, hp_before);
    }
    if request_switches(b) {
        Ok(StageEnd::Suspended)
    } else {
        Ok(StageEnd::Continue)
    }
}

/// `runAction`'s phazing block right after a move: every Pokémon with `forceSwitchFlag`
/// (Roar, Whirlwind, Dragon Tail, Circle Throw, Red Card) still standing is dragged out
/// (`dragIn`: a uniformly random bench member switches in and runs its `runSwitch` at once).
fn drag_outs<const N: usize>(b: &mut Battle<'_, N>) -> Result<(), TurnError> {
    let flagged = std::mem::take(&mut b.force_switch);
    for slot in flagged {
        if b.alive(slot).is_some() {
            switching::drag_in(b, slot)?;
        }
    }
    Ok(())
}

/// The end of Showdown `runAction` after the Update: a side whose active Pokémon has
/// `switchFlag` gets a switch request if it can switch (`canSwitch`: a healthy bench member);
/// otherwise its flags are cleared. `BeforeSwitchOut` has no implemented handler. Returns
/// whether the turn must wait for a decision ([`resume_turn`]).
fn request_switches<const N: usize>(b: &mut Battle<'_, N>) -> bool {
    let mut any = false;
    for side in [SideId::One, SideId::Two] {
        let flagged: Vec<SlotRef> = (0..N as u8)
            .map(|slot| SlotRef { side, slot })
            .filter(|&slot| {
                b.state.slot(slot).switch_flag != SwitchFlag::None && b.alive(slot).is_some()
            })
            .collect();
        if flagged.is_empty() {
            continue;
        }
        if residual::bench(b, side).next().is_none() {
            for slot in flagged {
                b.clear_switch_flag(slot);
            }
        } else {
            any = true;
        }
    }
    any
}

/// A slot whose living occupant has `switch_flag` set: the state is a suspended turn.
fn pending_mid_turn_switch<const N: usize>(state: &State<N>) -> Option<SlotRef> {
    State::<N>::slot_refs()
        .find(|&r| state.slot(r).switch_flag != SwitchFlag::None && state.active_ref(r).is_some())
}

/// The mid-turn switch stage (`resume_turn`): the `instaswitch` actions by the outgoing
/// Pokémon's action Speed (ties uniformly at random; Showdown runs no Update between them),
/// then the newcomers' one `runSwitch`, then `runAction`'s tail (faints, Update, switch
/// requests).
fn run_mid_turn_switches<const N: usize>(
    b: &mut Battle<'_, N>,
    switches: Vec<(SlotRef, u8)>,
    pending: &mut Pending,
) -> Result<StageEnd, TurnError> {
    let mut switches: Vec<(SlotRef, u8, i32)> = switches
        .into_iter()
        .map(|(slot, party_index)| (slot, party_index, b.action_speed(slot)))
        .collect();
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
        let hp_before = b.state.side(slot.side).party[usize::from(party_index)].hp;
        switching::switch_in(b, slot, party_index, true)?;
        newcomers.push((slot, hp_before));
    }
    let slots: Vec<SlotRef> = newcomers.iter().map(|n| n.0).collect();
    switching::run_switch_in(b, &slots)?;
    after_action(b, pending, &newcomers)
}

/// Runs `stage` repeatedly from `start` until it reports completion, merging identical
/// (state, pending) pairs after every stage and enumerating every random path within a stage
/// by replay. Returns the merged end states as outcomes; `state` is left unchanged.
/// How a stage ended: more stages follow; the turn is over; or the turn waits for a mid-turn
/// switch decision ([`resume_turn`]) with its remaining work kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StageEnd {
    Continue,
    Finished,
    Suspended,
}

/// A final position of a staged enumeration: the end state, the remaining work if the turn
/// suspended there, and the probability.
struct Ending<const N: usize, P> {
    end: State<N>,
    pending: Option<P>,
    probability: f64,
}

/// The outcomes of `endings` from `start`; `suspend` wraps the remaining work of a suspended
/// one.
fn outcomes<const N: usize, P>(
    start: &State<N>,
    endings: Vec<Ending<N, P>>,
    suspend: impl Fn(P) -> Suspension,
) -> Vec<Outcome> {
    endings
        .into_iter()
        .map(|ending| Outcome {
            probability: ending.probability,
            instructions: diff::instructions(start, &ending.end),
            suspension: ending.pending.map(&suspend),
        })
        .collect()
}

fn enumerate_stages<const N: usize, P: Clone + Eq + Hash>(
    state: &mut State<N>,
    start: P,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
) -> Result<Vec<Ending<N, P>>, TurnError> {
    // The turn runs in stages (one action, or the end of turn). After every stage identical
    // (state, remaining turn) pairs merge, so the work grows with the number of distinct
    // intermediate positions, not with the number of random paths. Within a stage every
    // random path is enumerated by replay.
    let mut frontier: Vec<(State<N>, P, f64)> = vec![(state.clone(), start, 1.0)];
    let mut finished: Vec<Ending<N, P>> = Vec::new();
    let mut finished_index: HashMap<(State<N>, Option<P>), usize> = HashMap::new();
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
                let end = result?;
                let p = probability * chooser.probability();
                match end {
                    StageEnd::Continue => {
                        let order = next.len();
                        next.entry((work.clone(), after)).or_insert((order, 0.0)).1 += p;
                    }
                    StageEnd::Finished | StageEnd::Suspended => {
                        let kept = (end == StageEnd::Suspended).then_some(after);
                        let key = (work.clone(), kept);
                        match finished_index.get(&key) {
                            Some(&i) => finished[i].probability += p,
                            None => {
                                finished_index.insert(key.clone(), finished.len());
                                finished.push(Ending {
                                    end: work.clone(),
                                    pending: key.1,
                                    probability: p,
                                });
                            }
                        }
                    }
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
    Ok(finished)
}

/// Monte Carlo counterpart of [`enumerate_stages`].
fn sample_stages<const N: usize, P: Clone + Eq + Hash>(
    state: &mut State<N>,
    samples: usize,
    seed: u64,
    start: P,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
) -> Result<Vec<Ending<N, P>>, TurnError> {
    let mut chooser = Chooser::sampler(seed);
    let mut finished: Vec<Ending<N, P>> = Vec::new();
    let mut index: HashMap<(State<N>, Option<P>), usize> = HashMap::new();
    let weight = 1.0 / samples as f64;
    let begin = state.clone();
    for _ in 0..samples {
        let mut pending = start.clone();
        let mut log = Vec::new();
        let mut result = Ok(StageEnd::Continue);
        while matches!(result, Ok(StageEnd::Continue)) {
            chooser.begin_run();
            let mut b = Battle::new(state, &mut chooser);
            result = stage(&mut b, &mut pending);
            log.append(&mut b.log);
        }
        let end = match result {
            Ok(end) => end,
            Err(error) => {
                state.reverse(&log);
                return Err(error);
            }
        };
        let key = (
            state.clone(),
            (end == StageEnd::Suspended).then_some(pending),
        );
        match index.get(&key) {
            Some(&i) => finished[i].probability += weight,
            None => {
                index.insert(key.clone(), finished.len());
                finished.push(Ending {
                    end: key.0,
                    pending: key.1,
                    probability: weight,
                });
            }
        }
        state.reverse(&log);
    }
    debug_assert_eq!(*state, begin);
    Ok(finished)
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
    if let Some(slot) = pending_mid_turn_switch(state) {
        return Err(TurnError::InvalidChoice {
            side: slot.side,
            slot: slot.slot,
            reason: "a mid-turn switch is pending (resume_turn)".into(),
        });
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
                    // A trapped Pokémon (abilities, No Retreat, partial trapping) was rejected
                    // by the ruleset above (`ActionError::Trapped`).
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
    // `endTurn`: `if (activeMove.flags['cantusetwice'] && pokemon.lastMove?.id === moveSlot.id)
    // pokemon.disableMove(...)` (Gigaton Hammer, Blood Moon). The hint volatile `runMove` adds
    // when such a move is forced twice in a row is removed within the same `runMove`.
    if data.flags.contains(MoveFlags::CANTUSETWICE) && state.slot(slot).last_move == id {
        return Some(format!("{} cannot be used twice in a row", data.name));
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
    /// Mid-turn switches decided for a suspended turn (`resume_turn`): the slot switching
    /// out and the party member coming in. Run by the next stage before anything else.
    switches: Vec<(SlotRef, u8)>,
    /// The residual phase ran (the turn suspended after it for an Emergency Exit switch): the
    /// end of the turn only remains.
    residual_done: bool,
}

impl Pending {
    fn new(queue: Vec<Action>) -> Pending {
        Pending {
            queue,
            in_progress: None,
            done: false,
            fractional_drawn: false,
            switches: Vec::new(),
            residual_done: false,
        }
    }
}

/// A turn stopped for a mid-turn switch decision (F6; Showdown `request: switch` while the
/// action queue is not empty: U-turn, Parting Shot, ...): the remaining turn, opaque to the
/// caller, which [`resume_turn`] continues once the switching side has chosen.
/// [`Slot::switch_flag`] marks the slots that must switch.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Suspension(Pending);

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
                        fractional_tenths: items::fractional_priority_tenths(state, slot),
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
) -> Result<StageEnd, TurnError> {
    // A resumed turn: the decided mid-turn switches first.
    if !pending.switches.is_empty() {
        let switches = std::mem::take(&mut pending.switches);
        b.queue = std::mem::take(&mut pending.queue);
        let result = run_mid_turn_switches(b, switches, pending);
        pending.queue = std::mem::take(&mut b.queue);
        let end = result?;
        items::stage_end_check(b)?;
        return Ok(end);
    }
    // Quick Claw's 1/5 is drawn, and Custap Berry eaten, when the actions are queued (first
    // stage).
    if !pending.fractional_drawn {
        pending.fractional_drawn = true;
        for action in &mut pending.queue {
            if let ActionKind::Move {
                fractional_tenths, ..
            } = &mut action.kind
            {
                if let Some(t) =
                    items::quick_claw(b, action.slot, action.pokemon, *fractional_tenths)
                {
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
    let end = result?;
    items::stage_end_check(b)?;
    Ok(end)
}

fn run_stage_inner<const N: usize>(
    b: &mut Battle<'_, N>,
    pending: &mut Pending,
) -> Result<StageEnd, TurnError> {
    // A multi-hit move continues with its next hit before anything else.
    if let Some(progress) = pending.in_progress.take() {
        return match moves::resume_move(b, progress)? {
            moves::MoveStep::Suspended(progress) => {
                pending.in_progress = Some(progress);
                Ok(StageEnd::Continue)
            }
            moves::MoveStep::Done => {
                drag_outs(b)?;
                after_action(b, pending, &[])
            }
        };
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
            let mut newcomers = Vec::new();
            match action.kind {
                ActionKind::Move { index, target, .. } => {
                    let will_act = b.will_act();
                    if let moves::MoveStep::Suspended(progress) =
                        moves::run_move(b, action.slot, index, target, will_act)?
                    {
                        pending.in_progress = Some(progress);
                        return Ok(StageEnd::Continue);
                    }
                    drag_outs(b)?;
                }
                ActionKind::Switch { party_index } => {
                    let hp_before =
                        b.state.side(action.slot.side).party[usize::from(party_index)].hp;
                    switching::run_switch(b, action.slot, party_index)?;
                    newcomers.push((action.slot, hp_before));
                }
                ActionKind::Mega => {
                    mega::run_mega_evo(b, action.slot)?;
                }
            }
            return after_action(b, pending, &newcomers);
        }
        return Ok(StageEnd::Continue);
    }

    if !pending.residual_done {
        // `residualPokemon`: every active Pokémon's HP before the residual damage, for
        // Emergency Exit.
        let before: Vec<(SlotRef, i16)> = b
            .all_alive()
            .into_iter()
            .map(|slot| (slot, b.slot_mon(slot).expect("alive").hp))
            .collect();
        residual::residual(b)?;
        pending.residual_done = true;
        if b.is_over() {
            pending.done = true;
            return Ok(StageEnd::Finished);
        }
        // `runAction`'s tail for the residual action: `checkFainted` (the queue is empty), the
        // Update, Emergency Exit for a Pokémon the residual damage took to half, then the
        // switch requests (the turn ends once they are resolved).
        residual::check_fainted(b);
        update::update_event(b)?;
        for (slot, hp_before) in before {
            switching::emergency_exit_check(b, slot, hp_before);
        }
        if request_switches(b) {
            return Ok(StageEnd::Suspended);
        }
    }
    residual::end_turn(b);
    pending.done = true;
    Ok(StageEnd::Finished)
}
