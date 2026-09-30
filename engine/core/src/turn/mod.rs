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
#[cfg(feature = "experiment-slot-diff-observer")]
pub use diff::observer as slot_diff_observer;
mod field_events;
#[cfg(feature = "experiment-leaf-ending-states")]
mod final_states;
mod forme;
mod frontier;
mod history;
mod items;
mod lazy;
pub mod legal;
pub mod lock;
mod mega;
mod merge;
mod moves;
mod order;
#[cfg(feature = "experiment-prepared-turn")]
mod prepared;
#[cfg(feature = "experiment-prepared-turn")]
#[doc(hidden)]
pub use prepared::PreparedTurn;
#[cfg(feature = "experiment-prepared-turn-observe")]
#[doc(hidden)]
pub use prepared::{reset_validation_counts, validation_counts};
mod queue;
#[cfg(feature = "experiment-replay-action-keys")]
mod replay_action_keys;
#[cfg(feature = "experiment-replay-action-keys-observer")]
#[doc(hidden)]
pub use replay_action_keys::observer as replay_action_keys_observer;
mod residual;
#[cfg(feature = "experiment-stats-off-cost")]
mod stats_off_cost;
mod support;
mod switching;
#[cfg(feature = "experiment-stats-off-cost-observer")]
pub use stats_off_cost::observer as stats_cost_observer;
mod transform;
mod update;

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

#[cfg(feature = "experiment-inline-runstart-observer")]
#[doc(hidden)]
pub use battle::inline_runstart_observer;
use battle::{Battle, RunBuffers, RunStart};
use branch::Chooser;
use lazy::HpMark;
use merge::Merger;

/// A short list kept inline up to `K` entries (board P4a: the turn code's small per-event
/// lists were a large share of its heap allocations).
pub(crate) type Small<T, const K: usize> = smallvec::SmallVec<[T; K]>;

/// Target and position lists (at most both sides' active slots), inline.
pub(crate) type Slots = Small<SlotRef, 6>;

pub use branch::RollMode;
#[cfg(feature = "experiment-leaf-ending-observer")]
pub use final_states::observer as final_state_observer;
#[cfg(feature = "experiment-leaf-ending-states")]
pub use final_states::{try_enumerate_turn_final_states, FinalStates};
pub use frontier::{Factored, FactoredOptions, FactoredOutcome, FactoredScope};
use order::{
    ORDER_BEFORE_TURN, ORDER_BEFORE_TURN_MOVE, ORDER_MEGA, ORDER_MOVE, ORDER_PRIORITY_CHARGE,
    ORDER_SWITCH,
};
use queue::{Action, ActionKind};

pub use abilities::trapped;
pub use forme::temporary_forme_base;
pub use legal::{legal_joint_actions, legal_joint_actions_reference};
pub use lock::{locked_move, Locked, RECHARGE_INDEX, STRUGGLE_INDEX};
pub use moves::{choice_target, takes_target, valid_target_loc};
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

/// How a turn is enumerated (WORKPLAN F18). The default is the exact distribution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EnumerateOptions {
    /// Which damage rolls to branch on; anything but [`RollMode::Full`] approximates.
    pub rolls: RollMode,
}

/// Makes every enumeration and sampling of this process check each position hash it kept
/// incrementally against a full [`State::position_hash`] (a panic on a difference; board P3a).
/// For tests: the check costs a whole-state hash per merged position.
pub fn verify_position_hashes(on: bool) {
    merge::set_verify(on);
}

/// Every outcome of the turn in which the sides choose `choices` (side one first). `state`
/// is left unchanged. Probabilities sum to 1; outcomes are in first-reached order.
pub fn enumerate_turn<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
) -> Result<Vec<Outcome>, TurnError> {
    enumerate_turn_with(state, ruleset, choices, EnumerateOptions::default())
}

/// [`enumerate_turn`] with `options`.
pub fn enumerate_turn_with<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
    options: EnumerateOptions,
) -> Result<Vec<Outcome>, TurnError> {
    let choices = check_turn(state, ruleset, &choices)?;
    enumerate_checked(state, &choices, options)
}

fn enumerate_checked<const N: usize>(
    state: &mut State<N>,
    choices: &[JointAction<N>; 2],
    options: EnumerateOptions,
) -> Result<Vec<Outcome>, TurnError> {
    let start = Pending::new(initial_queue(state, choices));
    let endings = enumerate_stages(state, start, options, run_stage)?;
    Ok(outcomes(state, endings, Suspension))
}

/// [`enumerate_turn_with`] with the outcomes factored (WORKPLAN P1b; DESIGN.md "Full 모드의 HP
/// 인수분해"): each outcome lists the party members whose HP takes one of several values
/// independently of the others, instead of one outcome per combination. The same distribution;
/// for a turn whose flat outcome list is too large to hold (two spread moves under
/// [`RollMode::Full`]).
pub fn enumerate_turn_factored<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
    options: EnumerateOptions,
) -> Result<Vec<FactoredOutcome>, TurnError> {
    let options = FactoredOptions {
        rolls: options.rolls,
        max_support: None,
    };
    Ok(enumerate_turn_factored_with(state, ruleset, choices, options)?.outcomes)
}

/// [`enumerate_turn_factored`] with [`FactoredOptions`]: with `max_support`, each member's HP
/// distribution is cut to that many values after every stage (WORKPLAN P1c), and
/// [`Factored::tv_bound`] bounds the total variation distance from the exact distribution.
pub fn enumerate_turn_factored_with<const N: usize>(
    state: &mut State<N>,
    ruleset: Ruleset,
    choices: [JointAction<N>; 2],
    options: FactoredOptions,
) -> Result<Factored, TurnError> {
    let choices = check_turn(state, ruleset, &choices)?;
    let start = Pending::new(initial_queue(state, &choices));
    let (endings, tv_bound) = frontier::enumerate_factored(state, start, options, run_stage)?;
    Ok(Factored {
        outcomes: frontier::factored_outcomes(state, endings, Suspension),
        tv_bound,
    })
}

/// [`resume_turn_with`] with the outcomes factored ([`enumerate_turn_factored`]).
pub fn resume_turn_factored<const N: usize>(
    state: &mut State<N>,
    suspension: &Suspension,
    choices: [[Option<u8>; N]; 2],
    options: EnumerateOptions,
) -> Result<Vec<FactoredOutcome>, TurnError> {
    let switches = check_mid_turn_switches(state, &choices)?;
    let mut start = suspension.0.clone();
    start.switches = switches;
    let options = FactoredOptions {
        rolls: options.rolls,
        max_support: None,
    };
    let (endings, _) = frontier::enumerate_factored(state, start, options, run_stage)?;
    Ok(frontier::factored_outcomes(state, endings, Suspension))
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
    resume_turn_with(state, suspension, choices, EnumerateOptions::default())
}

/// [`resume_turn`] with `options`.
pub fn resume_turn_with<const N: usize>(
    state: &mut State<N>,
    suspension: &Suspension,
    choices: [[Option<u8>; N]; 2],
    options: EnumerateOptions,
) -> Result<Vec<Outcome>, TurnError> {
    let switches = check_mid_turn_switches(state, &choices)?;
    let mut start = suspension.0.clone();
    start.switches = switches;
    let endings = enumerate_stages(state, start, options, run_stage)?;
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
                s.party[i as usize].is_alive()
                    && !s.slots.iter().any(|slot| slot.party_index == Some(i))
            })
            .collect();
        let flagged: Vec<usize> = (0..N).filter(|&i| s.slots[i].must_switch_out()).collect();
        // Revival Blessing's slot asks for a fainted party member instead of a bench one.
        let reviving = |i: usize| {
            s.slot_conditions[i][crate::field::SlotCondition::RevivalBlessing as usize].is_active()
        };
        let fainted: Vec<u8> = (0..s.party.len() as u8)
            .filter(|&i| !s.party[i as usize].is_alive())
            .collect();
        let required = flagged
            .iter()
            .filter(|&&i| {
                if reviving(i) {
                    !fainted.is_empty()
                } else {
                    true
                }
            })
            .count()
            .min(bench.len() + flagged.iter().filter(|&&i| reviving(i)).count());
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
            let pool = if reviving(i) { &fainted } else { &bench };
            if !pool.contains(&party_index) || given.contains(&party_index) {
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

/// [`resume_turn`] played `samples` times at random (see [`sample_turn`]): one path of the
/// rest of the turn per sample after the mid-turn switches, merged by end state.
pub fn sample_resume_turn<const N: usize>(
    state: &mut State<N>,
    suspension: &Suspension,
    choices: [[Option<u8>; N]; 2],
    samples: usize,
    seed: u64,
) -> Result<Vec<Outcome>, TurnError> {
    let switches = check_mid_turn_switches(state, &choices)?;
    let mut start = suspension.0.clone();
    start.switches = switches;
    let endings = sample_stages(state, samples, seed, start, run_stage)?;
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
    let endings = enumerate_stages(state, (), EnumerateOptions::default(), |b, _| {
        for &slot in &leads {
            let pokemon = b.occupant(slot).expect("a lead");
            if let Some(why) = switching_problem_at_start(b, pokemon) {
                return Err(b.unsupported(why));
            }
        }
        // Each lead's `switchIn` runs its `BeforeSwitchIn` (Illusion, EE2) before the batched
        // `runSwitch`.
        abilities::illusion_leads(b, &leads);
        b.battle_start = true;
        switching::run_switch_in(b, &leads)?;
        b.battle_start = false;
        // `runAction('runSwitch')` ends with `eachEvent('Update')`.
        update::update_event(b)?;
        items::stage_end_check(b)?;
        refuse_switch_request(b, "the battle start")?;
        // Then `endTurn` starts turn 1 (the state's turn already is 1): the leads lose
        // `newlySwitched` (`activeTurns` becomes 1), which Payback and Stakeout read.
        b.end_turn_history();
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
/// counter advances — unless a newcomer's Emergency Exit asks for a switch first: that outcome
/// is suspended (`Outcome::suspension`, [`resume_turn`], which then only ends the turn).
/// `state` is left unchanged.
pub fn enumerate_replacements<const N: usize>(
    state: &mut State<N>,
    choices: [[Option<u8>; N]; 2],
) -> Result<Vec<Outcome>, TurnError> {
    check_replacements(state, &choices)?;
    // The replacement runs as the only stage; a newcomer's Emergency Exit (entry hazards took
    // it to half) suspends it for another switch, and the resumed turn only has `endTurn` left.
    let mut start = Pending::new(Vec::new());
    start.fractional_drawn = true;
    start.residual_done = true;
    let endings = enumerate_stages(state, start, EnumerateOptions::default(), |b, _| {
        let end = run_replacements(b, &choices)?;
        items::stage_end_check(b)?;
        Ok(end)
    })?;
    Ok(outcomes(state, endings, Suspension))
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
                s.party[i as usize].is_alive()
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
) -> Result<StageEnd, TurnError> {
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
        let hp_before = b.state.side(slot.side).party[usize::from(party_index)].hp_mark();
        switching::switch_in(b, slot, party_index, true)?;
        newcomers.push(Newcomer::queued(b, slot, hp_before));
    }
    // The last `instaswitch` action's `runAction` tail: `eachEvent('Update')` before the
    // queued `runSwitch` actions (see `switching::run_switch`).
    update::update_event(b)?;
    // `runSwitch` takes every queued `runSwitch` action: nothing is left in the queue.
    b.queue_done = true;
    let slots: Vec<SlotRef> = newcomers.iter().map(|n| n.slot).collect();
    switching::run_switch_in(b, &slots)?;
    if b.is_over() {
        return Ok(StageEnd::Finished);
    }
    refuse_switch_request(b, "a replacement")?;
    // `runAction`'s tail with nothing left in the queue: `checkFainted` (a newcomer that fainted
    // to entry hazards gets `fnt`), the Update, Emergency Exit for the `runSwitch` action's
    // Pokémon if the entry hazards took it to half (board R3), the switch request it raises
    // (the turn waits; `resume_turn` then only ends it), else `endTurn` (which waits for another
    // replacement if one is needed).
    residual::check_fainted(b);
    update::update_event(b)?;
    run_switch_emergency_exit(b, &newcomers);
    if request_switches(b) {
        return Ok(StageEnd::Suspended);
    }
    residual::end_turn(b);
    Ok(StageEnd::Finished)
}

/// A Pokémon a `switchIn` just put on the field, whose `runSwitch` is queued.
#[derive(Clone, Copy, Debug)]
struct Newcomer {
    slot: SlotRef,
    /// Its HP before the switch-in (entry hazards), for Emergency Exit.
    hp_before: HpMark,
    /// Its queued `runSwitch` action's Speed: `insertChoice` stores `getActionSpeed()` then.
    speed: i32,
}

impl Newcomer {
    /// A newcomer whose `runSwitch` action was just queued.
    fn queued<const N: usize>(b: &Battle<'_, N>, slot: SlotRef, hp_before: HpMark) -> Newcomer {
        Newcomer {
            slot,
            hp_before,
            speed: b.action_speed(slot),
        }
    }
}

/// `runAction('runSwitch')`'s tail: `if (pokemon.hp && pokemon.hp <= pokemon.maxhp / 2 &&
/// pokemonOriginalHP > pokemon.maxhp / 2) runEvent('EmergencyExit', pokemon)` for the action's
/// own Pokémon only, the first queued `runSwitch` (`actions.runSwitch` takes the batch's others
/// without a tail of their own). The `runSwitch` actions sort by their stored Speed;
/// `insertChoice` places one at random among its equals, which makes each tied newcomer first
/// with equal probability. Drawn only when a newcomer could trigger.
fn run_switch_emergency_exit<const N: usize>(b: &mut Battle<'_, N>, newcomers: &[Newcomer]) {
    if !newcomers
        .iter()
        .any(|n| switching::emergency_exit_would_trigger(b, n.slot, n.hp_before))
    {
        return;
    }
    let best = newcomers.iter().map(|n| n.speed).max().expect("non-empty");
    let tied: Vec<&Newcomer> = newcomers.iter().filter(|n| n.speed == best).collect();
    let first = if tied.len() == 1 {
        tied[0]
    } else {
        tied[b.rng.uniform(tied.len())]
    };
    switching::emergency_exit_check(b, first.slot, first.hp_before);
}

/// The end of Showdown `runAction` for a move, switch or Mega Evolution: faints (the turn ends
/// if the battle does), then `eachEvent('Update')`.
fn after_action<const N: usize>(
    b: &mut Battle<'_, N>,
    pending: &mut Pending,
    newcomers: &[Newcomer],
) -> Result<StageEnd, TurnError> {
    if b.faint_messages(true)? {
        pending.done = true;
        b.queue.clear();
        return Ok(StageEnd::Finished);
    }
    update::update_event(b)?;
    // `runSwitch`'s tail: Emergency Exit for its newcomer if the entry hazards took it to half.
    run_switch_emergency_exit(b, newcomers);
    if request_switches(b) {
        Ok(StageEnd::Suspended)
    } else {
        Ok(StageEnd::Continue)
    }
}

/// `runAction`'s phazing block right after a move: every Pokémon with `forceSwitchFlag`
/// (Roar, Whirlwind, Dragon Tail, Circle Throw, Red Card) still standing is dragged out
/// (`dragIn`: a uniformly random bench member switches in and runs its `runSwitch` at once),
/// then `clearActiveMove()`. The move's active move is still set during the drags (Opus DD unit
/// B26): a Mold Breaker user's phazing move, its user still active, suppresses the breakable
/// abilities of everyone else (`suppressingAbility`) in the second `DragOut` (Suction Cups),
/// the `SwitchIn` handlers (Flower Gift, Pastel Veil: `switching::run_switch_in`), the entry
/// hazards' grounding (Levitate) and the boosts there (Intimidate against Hyper Cutter, Sticky
/// Web against Clear Body). The attacker Red Card drags out is no longer active once replaced,
/// so its replacement is not affected.
pub(super) fn drag_outs<const N: usize>(b: &mut Battle<'_, N>) -> Result<(), TurnError> {
    let mut flagged = std::mem::take(&mut b.force_switch);
    // `for (const side of this.sides) for (const pokemon of side.active)`: position order.
    flagged.sort_by_key(|slot| (slot.side.index(), slot.slot));
    for slot in flagged {
        if b.alive(slot).is_some() {
            switching::drag_in(b, slot)?;
        }
    }
    b.active_move = None;
    Ok(())
}

/// The end of Showdown `runAction` after the Update: a side whose active Pokémon has
/// `switchFlag` gets a switch request if it can switch (`canSwitch`: a healthy bench member);
/// otherwise its flags are cleared. Showdown runs `BeforeSwitchOut` for the flagged Pokémon
/// here (no implemented handler) and sets `skipBeforeSwitchOutEventFlag`, so their switch-outs
/// skip it and the Update before it (`switching::instaswitch_in`). Returns whether the turn
/// must wait for a decision ([`resume_turn`]).
fn request_switches<const N: usize>(b: &mut Battle<'_, N>) -> bool {
    let mut any = false;
    for side in [SideId::One, SideId::Two] {
        // `side.active.some(pokemon => pokemon && !!pokemon.switchFlag)`: a fainted Pokémon
        // still holding its position counts (Emergency Exit after its own recoil).
        let flagged: Vec<SlotRef> = (0..N as u8)
            .map(|slot| SlotRef { side, slot })
            .filter(|&slot| b.state.slot(slot).must_switch_out())
            .collect();
        if flagged.is_empty() {
            continue;
        }
        // Revival Blessing asks for its fainted party member whatever the bench.
        let flagged: Vec<(SlotRef, bool)> = flagged
            .into_iter()
            .map(|slot| {
                let reviving = conditions::slot_condition(
                    b,
                    slot,
                    crate::field::SlotCondition::RevivalBlessing,
                )
                .is_active();
                (slot, reviving)
            })
            .collect();
        if residual::bench(b, side).next().is_none() {
            let mut kept = false;
            for (slot, reviving) in flagged {
                if reviving {
                    kept = true;
                } else {
                    b.clear_switch_flag(slot);
                }
            }
            any |= kept;
        } else {
            any = true;
        }
    }
    any
}

/// Whether `side` must send in a replacement before the next turn (Showdown `request:
/// switch` for it between turns): an empty active slot and a healthy bench member.
pub fn side_must_replace<const N: usize>(state: &State<N>, side: SideId) -> bool {
    let s = state.side(side);
    let empty = s.slots.iter().any(|slot| slot.party_index.is_none());
    let bench = (0..s.party.len() as u8).any(|i| {
        s.party[i as usize].is_alive() && !s.slots.iter().any(|slot| slot.party_index == Some(i))
    });
    empty && bench
}

/// Whether `side` must send in a mid-turn switch (Showdown `request: switch` with actions
/// still queued; see [`resume_turn`]): a slot whose occupant has its `switch_flag` set, or
/// whose flagged Pokémon fainted there (Emergency Exit after its own recoil:
/// [`Slot::must_switch_out`](crate::state::Slot::must_switch_out)).
pub fn side_must_switch<const N: usize>(state: &State<N>, side: SideId) -> bool {
    state
        .side(side)
        .slots
        .iter()
        .any(crate::state::Slot::must_switch_out)
}

/// A slot that must switch out ([`side_must_switch`]): the state is a suspended turn.
fn pending_mid_turn_switch<const N: usize>(state: &State<N>) -> Option<SlotRef> {
    State::<N>::slot_refs().find(|&r| state.slot(r).must_switch_out())
}

/// A decision that cannot suspend (the battle start, a replacement) refuses a switch request
/// raised inside it: an Eject Pack used in its switch-in batch (Intimidate, Sticky Web), which
/// Showdown answers with a `switch` request before the next turn.
fn refuse_switch_request<const N: usize>(b: &Battle<'_, N>, during: &str) -> Result<(), TurnError> {
    match pending_mid_turn_switch(b.state) {
        Some(slot) => Err(b.unsupported(format!(
            "a switch request for {:?} slot {} (Eject Pack) during {during}",
            slot.side, slot.slot
        ))),
        None => Ok(()),
    }
}

/// The mid-turn switch stage (`resume_turn`): the `instaswitch` actions by the outgoing
/// Pokémon's action Speed (ties uniformly at random; Showdown runs no Update between them, nor
/// before an outgoing Pokémon leaves: `switching::instaswitch_in`), then the newcomers' one
/// `runSwitch`, then `runAction`'s tail (faints, Update, switch requests).
fn run_mid_turn_switches<const N: usize>(
    b: &mut Battle<'_, N>,
    switches: Vec<(SlotRef, u8)>,
    pending: &mut Pending,
) -> Result<StageEnd, TurnError> {
    // The outgoing Pokémon's action Speed; a fainted one still holding its position (Emergency
    // Exit after its own recoil) has no boosts or handlers left.
    let mut switches: Vec<(SlotRef, u8, i32)> = switches
        .into_iter()
        .map(|(slot, party_index)| {
            let speed = match (b.occupant(slot), b.state.slot(slot).fainted_occupant) {
                (None, Some(party)) => switching::fainted_action_speed(
                    b,
                    PokemonRef {
                        side: slot.side,
                        party,
                    },
                ),
                _ => b.action_speed(slot),
            };
            (slot, party_index, speed)
        })
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
        if conditions::slot_condition(b, slot, crate::field::SlotCondition::RevivalBlessing)
            .is_active()
        {
            // `runAction('revivalblessing')`: the fainted party member comes back at half its
            // max HP with no status; one still holding an active position switches in at
            // once (`instaswitch`); the condition and the user's flag end.
            let party = PokemonRef {
                side: slot.side,
                party: party_index,
            };
            b.revive(party);
            b.clear_switch_flag(slot);
            conditions::remove_slot_condition(
                b,
                slot,
                crate::field::SlotCondition::RevivalBlessing,
            );
            let held = State::<N>::slot_refs().find(|&r| {
                r.side == slot.side && b.state.slot(r).fainted_occupant == Some(party_index)
            });
            if let Some(held) = held {
                // `switchIn` into its own position: `queue.cancelAction(oldActive)` drops the
                // revived Pokémon's queued actions.
                b.queue.retain(|a| a.pokemon != party);
                let hp_before = b.mon(party).hp_mark();
                switching::switch_in(b, held, party_index, false)?;
                newcomers.push(Newcomer::queued(b, held, hp_before));
            }
            continue;
        }
        let hp_before = b.state.side(slot.side).party[usize::from(party_index)].hp_mark();
        // The request set `skipBeforeSwitchOutEventFlag`: no BeforeSwitchOut and no Update
        // before the flagged Pokémon leaves.
        switching::instaswitch_in(b, slot, party_index, true)?;
        newcomers.push(Newcomer::queued(b, slot, hp_before));
    }
    // The last `instaswitch` action's `runAction` tail: `eachEvent('Update')` before the
    // queued `runSwitch` actions (see `switching::run_switch`).
    update::update_event(b)?;
    // `runSwitch` takes every queued `runSwitch` action: `queue.peek()` is empty from here on
    // when nothing else is left, i.e. for a batch requested after the residual (Emergency
    // Exit, Eject Pack at the residual). Cud Chew reads it.
    b.queue_done = pending.residual_done && b.queue.is_empty();
    let slots: Vec<SlotRef> = newcomers.iter().map(|n| n.slot).collect();
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

/// The final positions of a staged enumeration in first-reached order: the end state, the
/// remaining work if the turn suspended there, the probability (and the position hash).
type Endings<const N: usize, P> = merge::Chunks<N, Option<P>>;

/// The outcomes of `endings` from `start`; `suspend` wraps the remaining work of a suspended
/// one. The end states are read where they are (board P4b).
fn outcomes<const N: usize, P>(
    start: &State<N>,
    mut endings: Endings<N, P>,
    suspend: impl Fn(P) -> Suspension,
) -> Vec<Outcome> {
    endings
        .iter_mut()
        .flatten()
        .map(|(end, pending, probability, _)| {
            let instructions = diff::instructions(start, end);
            #[cfg(feature = "experiment-leaf-ending-observer")]
            final_states::observer::materialized(instructions.len());
            Outcome {
                probability: *probability,
                instructions,
                suspension: pending.take().map(&suspend),
            }
        })
        .collect()
}

fn enumerate_stages<const N: usize, P: Clone + Eq + Hash>(
    state: &mut State<N>,
    start: P,
    options: EnumerateOptions,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
) -> Result<Endings<N, P>, TurnError> {
    if frontier::factored_active() {
        return frontier::enumerate_expanded(state, start, options, stage);
    }
    #[cfg(feature = "experiment-stats-off-cost")]
    let stage_stats = stats_off_cost::StageStats::snapshot();
    // The turn runs in stages (one action, or the end of turn). After every stage identical
    // (state, remaining turn) pairs merge, so the work grows with the number of distinct
    // intermediate positions, not with the number of random paths. Within a stage every
    // random path is enumerated by replay.
    // Each position carries its `State::position_hash`; a run's instructions keep the hash of
    // the state they lead to (`Battle::hash_delta`), so merging hashes no state (board P3a).
    // The positions stay where the merge stored them: every run works on them in place (its
    // instructions are reversed after it; board P4b).
    let mut frontier: merge::Chunks<N, P> =
        vec![vec![(state.clone(), start, 1.0, state.position_hash())]];
    // Both merge in first-reached order, which keeps the output order deterministic.
    let mut finished: Merger<N, Option<P>> = Merger::new();
    // The runs' log and Speed snapshot buffers, handed from run to run.
    let mut buffers = RunBuffers::default();
    while !frontier.is_empty() {
        let mut next: Merger<N, P> = Merger::new();
        #[cfg(not(feature = "experiment-stats-off-cost"))]
        let stage_started = std::time::Instant::now();
        #[cfg(feature = "experiment-stats-off-cost")]
        let stage_started = stage_stats.start();
        let mut runs = 0usize;
        for (work, pending, probability, work_hash) in frontier.iter_mut().flatten() {
            let (probability, work_hash) = (*probability, *work_hash);
            let mut chooser = Chooser::with_rolls(options.rolls);
            // Every run starts from `work` (a run's instructions are reversed after it), so the
            // context `Battle::new` derives is the same for all of them: the first run derives
            // it and the others replay it, reusing the previous run's pending buffers too.
            let mut start: Option<RunStart> = None;
            let mut after = pending.clone();
            // P8d: this cache belongs to exactly this restored input and RunStart, never to
            // the next frontier entry/stage. Factored enumeration returned above.
            #[cfg(feature = "experiment-replay-action-keys")]
            let mut action_keys = replay_action_keys::ReplayActionKeys::default();
            loop {
                runs += 1;
                chooser.begin_run();
                after.clone_from(pending);
                let result = {
                    let mut b = match &start {
                        Some(start) => Battle::replay(work, &mut chooser, start, buffers),
                        None => {
                            buffers.clear_log();
                            Battle::recycle(work, &mut chooser, buffers)
                        }
                    };
                    if start.is_none() {
                        start = Some(b.run_start());
                    }
                    #[cfg(feature = "experiment-replay-action-keys")]
                    {
                        b.replay_action_keys = Some(&mut action_keys);
                    }
                    let result = stage(&mut b, &mut after);
                    buffers = b.into_buffers();
                    result
                };
                let end = result?;
                let p = probability * chooser.probability();
                let hash = work_hash.wrapping_add(buffers.hash_delta);
                match end {
                    StageEnd::Continue => next.add(work, hash, &after, p),
                    StageEnd::Finished | StageEnd::Suspended => {
                        let kept = (end == StageEnd::Suspended).then(|| after.clone());
                        finished.add(work, hash, &kept, p);
                    }
                }
                work.reverse(&buffers.log);
                if !chooser.advance() {
                    break;
                }
            }
        }
        #[cfg(not(feature = "experiment-stats-off-cost"))]
        if std::env::var_os("LAB_ENGINE_STATS").is_some() {
            eprintln!(
                "lab-engine: stage frontier {} states, {} finished; {} replays in {:.1} ms",
                next.len(),
                finished.len(),
                runs,
                stage_started.elapsed().as_secs_f64() * 1000.0
            );
        }
        #[cfg(feature = "experiment-stats-off-cost")]
        if let Some(elapsed) = stage_stats.finish(stage_started) {
            eprintln!(
                "lab-engine: stage frontier {} states, {} finished; {} replays in {:.1} ms",
                next.len(),
                finished.len(),
                runs,
                elapsed.as_secs_f64() * 1000.0
            );
        }
        frontier = next.into_chunks();
    }
    Ok(finished.into_chunks())
}

/// Monte Carlo counterpart of [`enumerate_stages`].
fn sample_stages<const N: usize, P: Clone + Eq + Hash>(
    state: &mut State<N>,
    samples: usize,
    seed: u64,
    start: P,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
) -> Result<Endings<N, P>, TurnError> {
    let mut chooser = Chooser::sampler(seed);
    let mut finished: Merger<N, Option<P>> = Merger::new();
    let weight = 1.0 / samples as f64;
    #[cfg(debug_assertions)]
    let begin = state.clone();
    // One buffer set for every stage: the stages of a sample log into one list.
    let mut buffers = RunBuffers::default();
    // The end positions' hashes are the start's plus the logs' changes; a single sample merges
    // nothing and needs none.
    let begin_hash = if samples > 1 {
        state.position_hash()
    } else {
        0
    };
    for _ in 0..samples {
        let mut pending = start.clone();
        buffers.clear_log();
        let mut result = Ok(StageEnd::Continue);
        while matches!(result, Ok(StageEnd::Continue)) {
            chooser.begin_run();
            let mut b = Battle::recycle(state, &mut chooser, buffers);
            result = stage(&mut b, &mut pending);
            buffers = b.into_buffers();
        }
        let end = match result {
            Ok(end) => end,
            Err(error) => {
                state.reverse(&buffers.log);
                return Err(error);
            }
        };
        let kept = (end == StageEnd::Suspended).then_some(pending);
        if samples == 1 {
            // A single path (what `lab-rollout` asks for every turn): nothing to merge.
            let ending = (state.clone(), kept, weight, 0);
            state.reverse(&buffers.log);
            #[cfg(debug_assertions)]
            debug_assert_eq!(*state, begin);
            return Ok(vec![vec![ending]]);
        }
        finished.add(
            state,
            begin_hash.wrapping_add(buffers.hash_delta),
            &kept,
            weight,
        );
        state.reverse(&buffers.log);
    }
    #[cfg(debug_assertions)]
    debug_assert_eq!(*state, begin);
    Ok(finished.into_chunks())
}

/// Validates the choices and that everything in play is implemented. Returns the choices as
/// the turn runs them: a locked Pokémon's move choice becomes its locked move (Showdown
/// `chooseMove` ignores what was picked), the `recharge` pseudo-move as `RECHARGE_INDEX`.
fn check_turn<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    choices: &[JointAction<N>; 2],
) -> Result<[JointAction<N>; 2], TurnError> {
    check_turn_parent(state)?;
    let normalized = [
        check_side(state, ruleset, SideId::One, &choices[0])?,
        check_side(state, ruleset, SideId::Two, &choices[1])?,
    ];
    check_turn_support(state)?;
    Ok(normalized)
}

fn check_turn_parent<const N: usize>(state: &State<N>) -> Result<(), TurnError> {
    #[cfg(feature = "experiment-prepared-turn-observe")]
    prepared::count(0);
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
        if side_must_replace(state, side) {
            return Err(TurnError::ReplacementPending(side));
        }
    }
    Ok(())
}

fn check_turn_support<const N: usize>(state: &State<N>) -> Result<(), TurnError> {
    #[cfg(feature = "experiment-prepared-turn-observe")]
    prepared::count(2);
    support::check_state(state).map_err(TurnError::Unsupported)
}

/// One side's part of [`check_turn`]: the ruleset's validation, then each slot's choice
/// against the state (locks, PP, disabled moves, targets, gimmicks, support), returned in the
/// form the turn runs it.
pub(crate) fn check_side<const N: usize>(
    state: &State<N>,
    ruleset: Ruleset,
    side: SideId,
    action: &JointAction<N>,
) -> Result<JointAction<N>, TurnError> {
    #[cfg(feature = "experiment-prepared-turn-observe")]
    prepared::count(1);
    let mut normalized = *action;
    ruleset
        .validate_joint_action(state, side, action)
        .map_err(|error| TurnError::Action { side, error })?;
    let mut switching_in = [u8::MAX; N];
    for (i, &slot_action) in action.iter().enumerate() {
        normalized[i] = check_slot(state, side, i as u8, slot_action)?;
        if let SlotAction::Switch { party_index } = slot_action {
            if switching_in[..i].contains(&party_index) {
                return Err(TurnError::InvalidChoice {
                    side,
                    slot: i as u8,
                    reason: format!("cannot switch to party {party_index}"),
                });
            }
            switching_in[i] = party_index;
        }
    }
    Ok(normalized)
}

/// One slot's part of [`check_side`] after the ruleset's validation: the choice against the
/// state (locks, PP, disabled moves, targets, gimmicks, support), returned in the form the turn
/// runs it. Independent of the other slots' choices but for two slots switching to the same
/// party member, which [`check_side`] checks (`legal_joint_actions` checks each slot's
/// candidates once with this; board P2a).
pub(crate) fn check_slot<const N: usize>(
    state: &State<N>,
    side: SideId,
    i: u8,
    slot_action: SlotAction,
) -> Result<SlotAction, TurnError> {
    let slot = SlotRef { side, slot: i };
    let invalid = |reason: String| TurnError::InvalidChoice {
        side,
        slot: i,
        reason,
    };
    let occupant = state.active(slot).filter(|p| p.is_alive());
    // A commanding Tatsugiri (Commander) passes: Showdown's `getChoiceIndex` skips it and
    // `choosePass` accepts it, whatever it is locked into.
    if occupant.is_some() && state.slot(slot).volatiles.has(Volatile::Commanding) {
        if slot_action != SlotAction::Pass {
            return Err(invalid("commanding (Commander): must pass".into()));
        }
        return Ok(slot_action);
    }
    // A locked Pokémon: any move choice stands for the locked move, nothing else is
    // allowed (`trapped`), no PP is needed.
    if let (Some(locked), Some(mon)) = (lock::locked_move(state, slot), occupant) {
        let SlotAction::Move { gimmick, .. } = slot_action else {
            return Err(invalid(format!("locked into {locked:?}; cannot switch")));
        };
        if !gimmick.is_none() {
            return Err(invalid(format!("locked into {locked:?}; no {gimmick:?}")));
        }
        // A lock onto a move the Pokémon does not know (a two-turn or locking move Copycat
        // called) keeps index 0: the queued action is the locked move whatever the index
        // (`lock::queued_move_id`).
        let index = match locked {
            Locked::Recharge => RECHARGE_INDEX,
            Locked::Move(id) | Locked::TwoTurn { id, .. } => {
                mon.moves.iter().position(|m| m.id == id).unwrap_or(0) as u8
            }
        };
        // A two-turn move keeps the target location it was aimed at.
        let target = match locked {
            Locked::TwoTurn { target, .. } => target,
            _ => 0,
        };
        return Ok(SlotAction::Move {
            index,
            target,
            gimmick: Gimmick::None,
        });
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
            if !target.is_alive() || active {
                return Err(invalid(format!("cannot switch to party {party_index}")));
            }
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
            let usable = mon
                .moves
                .iter()
                .any(|m| !m.id.is_none() && m.pp > 0 && disabled(state, slot, m.id).is_none());
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
                return Ok(SlotAction::Move {
                    index,
                    target: 0,
                    gimmick: Gimmick::None,
                });
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
            let target_type = choice_target(mon, id);
            let needs = takes_target(N, target_type);
            let ok = if needs {
                target != 0 && valid_target_loc(N, slot, target, target_type)
            } else {
                target == 0
            };
            if !ok {
                return Err(invalid(format!(
                    "target {target} for {} ({target_type:?})",
                    data.name
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
    Ok(slot_action)
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
    // Gorilla Tactics' `onDisableMove`.
    if let Some(why) = abilities::gorilla_disabled_move(state, slot, id) {
        return Some(why);
    }
    items::disabled_move(state, slot, id)
}

/// The rest of a turn between stages: the actions not yet run, a multi-hit move suspended
/// between two hits, and whether the turn is over. (Fainted Pokémon still holding a position
/// are in the state: `Slot::fainted_occupant`.)
#[derive(Debug, PartialEq, Eq, Hash)]
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

/// By hand for `clone_from`, which reuses the queue's allocation: the enumeration resets its
/// per-run copy of the pending work from the position's before every run (Opus GG).
impl Clone for Pending {
    fn clone(&self) -> Self {
        let Pending {
            queue,
            in_progress,
            done,
            fractional_drawn,
            switches,
            residual_done,
        } = self;
        Pending {
            queue: queue.clone(),
            in_progress: in_progress.clone(),
            done: *done,
            fractional_drawn: *fractional_drawn,
            switches: switches.clone(),
            residual_done: *residual_done,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Pending {
            queue,
            in_progress,
            done,
            fractional_drawn,
            switches,
            residual_done,
        } = source;
        self.queue.clone_from(queue);
        self.in_progress.clone_from(in_progress);
        self.done = *done;
        self.fractional_drawn = *fractional_drawn;
        self.switches.clone_from(switches);
        self.residual_done = *residual_done;
    }
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
    pub(crate) fn action_key(&self, action: &Action) -> (u32, i32, i32) {
        #[cfg(feature = "experiment-replay-action-keys-observer")]
        replay_action_keys::observer::update(|c| c.action_key_calls += 1);
        let in_slot = self.alive(action.slot) == Some(action.pokemon);
        let (order, priority) = match action.kind {
            ActionKind::Switch { .. } => (ORDER_SWITCH, 0),
            ActionKind::Mega => (ORDER_MEGA, 0),
            // Not a `move` choice: no priority (`getActionSpeed` only sets it for moves).
            ActionKind::BeforeTurn => (ORDER_BEFORE_TURN, 0),
            ActionKind::BeforeTurnMove { .. } => (ORDER_BEFORE_TURN_MOVE, 0),
            ActionKind::PriorityCharge { .. } => (ORDER_PRIORITY_CHARGE, 0),
            // The `recharge` pseudo-move: priority 0, but `getActionSpeed` runs
            // `FractionalPriority` for it too (Stall's and Mycelium Might's -0.1; Quick Claw and
            // Custap skip status moves), so a Stall holder recharges after everything at 0.
            ActionKind::Move {
                id,
                fractional_tenths,
                ..
            } if id.is_none() => (ORDER_MOVE, i32::from(fractional_tenths)),
            ActionKind::Move {
                id,
                fractional_tenths,
                ..
            } => {
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
            self.trick_room_speed(i32::from(self.mon(action.pokemon).stats[4]))
        };
        (action.order.unwrap_or(order), priority, speed)
    }
}

/// The turn's actions in choice order (side one first, slot order).
fn initial_queue<const N: usize>(state: &State<N>, choices: &[JointAction<N>; 2]) -> Vec<Action> {
    let mut queue = Vec::new();
    // Showdown's `beforeTurn` action: one per turn, on any active Pokémon (its tail is the
    // turn-start Update).
    if let Some((slot, pokemon)) = State::<N>::slot_refs()
        .find_map(|slot| state.active_ref(slot).map(|pokemon| (slot, pokemon)))
    {
        queue.push(Action {
            slot,
            pokemon,
            kind: ActionKind::BeforeTurn,
            order: None,
        });
    }
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
                    let id = lock::queued_move_id(state, slot, index);
                    // `resolveAction`: a move with a `beforeTurnCallback` also queues a
                    // `beforeTurnMove` action (the chosen move; Encore's override comes later).
                    if moves::has_before_turn_callback(id) {
                        queue.push(Action {
                            slot,
                            pokemon,
                            kind: ActionKind::BeforeTurnMove { id },
                            order: None,
                        });
                    }
                    // And a `priorityChargeMove` action for a `priorityChargeCallback`.
                    if moves::has_priority_charge_callback(id) {
                        queue.push(Action {
                            slot,
                            pokemon,
                            kind: ActionKind::PriorityCharge { id },
                            order: None,
                        });
                    }
                    ActionKind::Move {
                        id,
                        target,
                        original: (target != 0)
                            .then(|| state.active_ref(moves::at_loc(slot, target)))
                            .flatten(),
                        fractional_tenths: items::fractional_priority_tenths(state, slot, id),
                        round_source: None,
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
    #[cfg(feature = "experiment-replay-action-keys")]
    replay_action_keys::guard(b, pending);
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
    // Quick Draw's 3/10 and Quick Claw's 1/5 are drawn, and Custap Berry eaten, when the
    // actions are queued (first stage), in handler order for each action.
    if !pending.fractional_drawn {
        pending.fractional_drawn = true;
        for action in &mut pending.queue {
            if let ActionKind::Move {
                id,
                fractional_tenths,
                ..
            } = &mut action.kind
            {
                let id = *id;
                if let Some(t) = abilities::quick_draw(b, action.slot, action.pokemon, id) {
                    *fractional_tenths = t;
                }
                if let Some(t) =
                    items::quick_claw(b, action.slot, action.pokemon, *fractional_tenths, id)
                {
                    *fractional_tenths = t;
                }
                if let Some(t) =
                    items::custap(b, action.slot, action.pokemon, *fractional_tenths, id)
                {
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
    debug_assert!(
        b.called_suspension.is_none(),
        "a called move's suspension was not taken by its caller"
    );
    items::stage_end_check(b)?;
    Ok(end)
}

/// The queue index of the action that runs next, given the queue's sort keys: the best by
/// (order asc, priority desc, speed desc), uniformly at random among equals.
fn pick_action<const N: usize>(b: &mut Battle<'_, N>, keys: &[(u32, i32, i32)]) -> usize {
    let best = keys
        .iter()
        .copied()
        .min_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)).then(y.2.cmp(&x.2)))
        .expect("non-empty");
    let tied = keys.iter().filter(|&&k| k == best).count();
    let nth = b.rng.uniform(tied);
    keys.iter()
        .enumerate()
        .filter(|&(_, &k)| k == best)
        .nth(nth)
        .map(|(i, _)| i)
        .expect("a tied action")
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
        // Best action by (order asc, priority desc, speed desc), ties uniformly at random. The
        // keys stay on the stack for a queue of usual length (this runs for every action).
        #[cfg(feature = "experiment-replay-action-keys")]
        let pick = replay_action_keys::pick(b);
        #[cfg(not(feature = "experiment-replay-action-keys"))]
        let pick = {
            const INLINE: usize = 24;
            let n = b.queue.len();
            if n <= INLINE {
                let mut keys = [(0u32, 0i32, 0i32); INLINE];
                for (key, action) in keys.iter_mut().zip(&b.queue) {
                    *key = b.action_key(action);
                }
                pick_action(b, &keys[..n])
            } else {
                let keys: Vec<(u32, i32, i32)> = b.queue.iter().map(|a| b.action_key(a)).collect();
                pick_action(b, &keys)
            }
        };
        let action = b.queue.remove(pick);

        // `runAction` skips a Pokémon that is no longer active or has fainted.
        if b.alive(action.slot) == Some(action.pokemon) {
            let mut newcomers = Vec::new();
            match action.kind {
                ActionKind::Move {
                    id,
                    target,
                    original,
                    round_source,
                    ..
                } => {
                    let will_act = b.will_act();
                    let aim = moves::Aim {
                        loc: target,
                        original,
                    };
                    if let moves::MoveStep::Suspended(progress) =
                        moves::run_move(b, action.slot, id, aim, will_act, round_source)?
                    {
                        pending.in_progress = Some(progress);
                        return Ok(StageEnd::Continue);
                    }
                    drag_outs(b)?;
                }
                ActionKind::Switch { party_index } => {
                    let hp_before =
                        b.state.side(action.slot.side).party[usize::from(party_index)].hp_mark();
                    switching::run_switch(b, action.slot, party_index)?;
                    // Its own `runSwitch` ran: the only newcomer, its Speed unused.
                    newcomers.push(Newcomer {
                        slot: action.slot,
                        hp_before,
                        speed: 0,
                    });
                }
                ActionKind::Mega => {
                    mega::run_mega_evo(b, action.slot)?;
                }
                ActionKind::BeforeTurn => {}
                ActionKind::BeforeTurnMove { id } => {
                    moves::before_turn_move(b, action.slot, id);
                }
                ActionKind::PriorityCharge { id } => {
                    moves::priority_charge_move(b, action.slot, id);
                }
            }
            return after_action(b, pending, &newcomers);
        }
        return Ok(StageEnd::Continue);
    }

    if !pending.residual_done {
        // The residual action was the queue's last: `queue.peek()` is empty from here on.
        b.queue_done = true;
        // `residualPokemon`: every active Pokémon's HP before the residual damage, for
        // Emergency Exit.
        let before: Vec<(SlotRef, HpMark)> = b
            .all_alive()
            .into_iter()
            .map(|slot| (slot, b.slot_mon(slot).expect("alive").hp_mark()))
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
