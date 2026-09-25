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

mod battle;
mod branch;
pub mod coverage;
mod diff;
mod moves;
mod order;
mod residual;
mod support;
mod switching;

use std::collections::HashMap;
use std::fmt;

use crate::action::{JointAction, SlotAction};
use crate::dex::{moves as move_ids, MoveFlags, MoveId};
use crate::field::FieldEffect;
use crate::gimmick::Gimmick;
use crate::instruction::Outcome;
use crate::rules::{ActionError, Ruleset};
use crate::state::{PokemonRef, SideId, SlotRef, State};

use battle::Battle;
use branch::Chooser;
use order::{ORDER_MOVE, ORDER_SWITCH};

pub use moves::{takes_target, valid_target_loc};

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
    check_turn(state, ruleset, &choices)?;

    // The turn runs in stages (one action, or the end of turn). After every stage identical
    // (state, remaining turn) pairs merge, so the work grows with the number of distinct
    // intermediate positions, not with the number of random paths. Within a stage every
    // random path is enumerated by replay.
    let start = Pending {
        queue: initial_queue(state, &choices),
        fainted: Vec::new(),
        done: false,
    };
    let mut frontier: Vec<(State<N>, Pending, f64)> = vec![(state.clone(), start, 1.0)];
    let mut finished: Vec<(State<N>, f64)> = Vec::new();
    let mut finished_index: HashMap<State<N>, usize> = HashMap::new();
    let mut chooser = Chooser::new();
    while !frontier.is_empty() {
        // Value: (first-reached index, probability); keeps the output order deterministic.
        let mut next: HashMap<(State<N>, Pending), (usize, f64)> = HashMap::new();
        for (mut work, pending, probability) in frontier {
            chooser = Chooser::new();
            loop {
                chooser.begin_run();
                let mut after = pending.clone();
                let (result, log) = {
                    let mut b = Battle::new(&mut work, &mut chooser);
                    b.fainted_positions = std::mem::take(&mut after.fainted);
                    let result = run_stage(&mut b, &mut after);
                    after.fainted = std::mem::take(&mut b.fainted_positions);
                    (result, std::mem::take(&mut b.log))
                };
                result?;
                let p = probability * chooser.probability();
                if after.done {
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
    drop(chooser);

    Ok(finished
        .into_iter()
        .map(|(end, probability)| Outcome {
            probability,
            instructions: diff::instructions(state, &end),
        })
        .collect())
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
    check_turn(state, ruleset, &choices)?;
    let mut chooser = Chooser::sampler(seed);
    let mut finished: Vec<(State<N>, f64)> = Vec::new();
    let mut index: HashMap<State<N>, usize> = HashMap::new();
    let weight = 1.0 / samples as f64;
    let start = state.clone();
    for _ in 0..samples {
        let mut pending = Pending {
            queue: initial_queue(state, &choices),
            fainted: Vec::new(),
            done: false,
        };
        let mut log = Vec::new();
        let mut result = Ok(());
        while !pending.done && result.is_ok() {
            chooser.begin_run();
            let mut b = Battle::new(state, &mut chooser);
            b.fainted_positions = std::mem::take(&mut pending.fainted);
            result = run_stage(&mut b, &mut pending);
            pending.fainted = std::mem::take(&mut b.fainted_positions);
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
    debug_assert_eq!(*state, start);
    Ok(finished
        .into_iter()
        .map(|(end, probability)| Outcome {
            probability,
            instructions: diff::instructions(&start, &end),
        })
        .collect())
}

/// Validates the choices and that everything in play is implemented.
fn check_turn<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    choices: &[JointAction<N>; 2],
) -> Result<(), TurnError> {
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
                    if !gimmick.is_none() {
                        return Err(TurnError::Unsupported(format!(
                            "{gimmick:?} activation (effects not implemented)"
                        )));
                    }
                    if let Some(why) = support::move_unsupported(id) {
                        return Err(TurnError::Unsupported(why));
                    }
                }
            }
        }
    }
    support::check_state(state).map_err(TurnError::Unsupported)?;
    let _ = Gimmick::None;
    Ok(())
}

/// Why a move cannot be chosen now (Showdown `DisableMove` handlers that are implemented).
fn disabled<const N: usize>(state: &State<N>, slot: SlotRef, id: MoveId) -> Option<String> {
    let data = id.data();
    // Champions Fake Out: disabled once the user has acted since switching in.
    if id == move_ids::FAKE_OUT && state.slot(slot).move_actions > 0 {
        return Some("Fake Out only works on the first turn out".into());
    }
    if state.field[FieldEffect::Gravity as usize].is_active()
        && data.flags.contains(MoveFlags::GRAVITY)
    {
        return Some(format!("{} is disabled by Gravity", data.name));
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Action {
    slot: SlotRef,
    pokemon: PokemonRef,
    kind: ActionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ActionKind {
    Move { index: u8, target: i8 },
    Switch { party_index: u8 },
}

/// The rest of a turn between stages: the actions not yet run, the Pokémon that fainted in
/// an active position so far (`checkFainted` marks them at the end), and whether the turn
/// is over.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Pending {
    queue: Vec<Action>,
    fainted: Vec<(SlotRef, PokemonRef)>,
    done: bool,
}

impl<const N: usize> Battle<'_, N> {
    /// Showdown's sort key of a queued action: (order, priority, speed).
    fn action_key(&self, action: &Action) -> (u32, i32, i32) {
        let in_slot = self.alive(action.slot) == Some(action.pokemon);
        let (order, priority) = match action.kind {
            ActionKind::Switch { .. } => (ORDER_SWITCH, 0),
            ActionKind::Move { index, .. } => {
                let id = self.mon(action.pokemon).moves[index as usize].id;
                let priority = if in_slot {
                    self.move_priority(action.slot, id)
                } else {
                    // A fainted Pokémon's action stays queued with its base priority.
                    i32::from(id.data().priority)
                };
                (ORDER_MOVE, priority)
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
        (order, priority, speed)
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
                SlotAction::Move { index, target, .. } => ActionKind::Move { index, target },
                SlotAction::Switch { party_index } => ActionKind::Switch { party_index },
            };
            queue.push(Action {
                slot,
                pokemon,
                kind,
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
    let queue = &mut pending.queue;
    if !queue.is_empty() {
        // Best action by (order asc, priority desc, speed desc), ties uniformly at random.
        let keys: Vec<(u32, i32, i32)> = queue.iter().map(|a| b.action_key(a)).collect();
        let best = keys
            .iter()
            .copied()
            .min_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)).then(y.2.cmp(&x.2)))
            .expect("non-empty");
        let tied: Vec<usize> = (0..queue.len()).filter(|&i| keys[i] == best).collect();
        let pick = tied[b.rng.uniform(tied.len())];
        let action = queue.remove(pick);

        // `runAction` skips a Pokémon that is no longer active or has fainted.
        if b.alive(action.slot) == Some(action.pokemon) {
            match action.kind {
                ActionKind::Move { index, target } => {
                    let will_act = !queue.is_empty();
                    moves::run_move(b, action.slot, index, target, will_act)?;
                }
                ActionKind::Switch { party_index } => {
                    switching::run_switch(b, action.slot, party_index)?;
                }
            }
            if b.faint_messages(true) {
                pending.done = true;
                queue.clear();
            }
        }
        return Ok(());
    }

    residual::residual(b)?;
    if !b.is_over() {
        residual::end_turn(b);
    }
    pending.done = true;
    Ok(())
}
