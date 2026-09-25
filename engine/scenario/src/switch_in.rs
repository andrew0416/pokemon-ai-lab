//! Initial switch-in outcomes: the leads' start-of-battle effects, expanded into weighted
//! states. Runs once per scenario, outside the search hot path and outside `State`.
//!
//! This reproduces what Showdown does between team preview and the first decision
//! (`runAction('start')` → `switchIn` for every lead → one `runSwitch` for all of them):
//!
//! 1. `runSwitch` speed-sorts the actives once (`speedSort`: speed high to low, ties shuffled
//!    uniformly). At battle start `pokemon.speed` is the stored Speed stat.
//! 2. `fieldEvent('SwitchIn')` collects every switch-in handler first (abilities and items
//!    run their `onStart` there) and runs them in that order. A handler whose ability changed
//!    before it ran is skipped.
//! 3. Trace samples uniformly among adjacent foes whose current ability lacks `notrace`, and
//!    `setAbility` then runs the copied ability's `onStart` immediately.
//! 4. Weather and terrain abilities call `setWeather`/`setTerrain`; the same weather set by an
//!    ability again fails (gen > 5), the same terrain always fails. Duration is 5, or 8 when
//!    the source holds Smooth Rock (sand) / Terrain Extender (terrain).
//!
//! Only the abilities in `IMPLEMENTED` have behaviour here (Trace, Sand Stream, Grassy Surge,
//! and Sand Rush and Quick Feet as verified inert). Any other ability, item or species handler that can fire
//! during this sequence is rejected with an error naming it; nothing that can change state is
//! skipped silently. Random calls that cannot change the outcome (speed-tie shuffles in
//! `eachEvent` with no listeners) are not branched on. Format/rule handlers (`onBegin`,
//! `onSwitchIn` of a rule) are not in the dex export; the oracle fixture covers the one format
//! the loader accepts.

use std::fmt;

use lab_engine::dex::{abilities, items, AbilityFlags, AbilityId, ItemId, SpeciesId};
use lab_engine::field::{Effect, FieldEffect, Terrain, Weather};
use lab_engine::state::{SideId, SlotRef, State, Status, BOOST_COUNT, PARTY_SIZE};

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
    /// The dex handler list of an implemented ability no longer matches what is implemented.
    HandlersChanged {
        ability: AbilityId,
        expected: &'static [&'static str],
    },
    /// Trace with no valid target keeps seeking on later updates; that pending state is not
    /// representable in `State`.
    TraceWithoutTarget { slot: SlotRef },
    /// `No Ability` next to Trace (Hackmons-only interaction).
    TraceNextToNoAbility { slot: SlotRef },
}

impl fmt::Display for SwitchInError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SwitchInError::UnsupportedSlotCount(n) => {
                write!(
                    f,
                    "{n} active slots per side: only singles and doubles are supported"
                )
            }
            SwitchInError::NotInitial { reason } => {
                write!(f, "not a fresh battle start: {reason}")
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
                "{} party[{party_index}]: species {:?} has handler {handler} that is not \
                 implemented",
                side_name(*side),
                species.data().name
            ),
            SwitchInError::HandlersChanged { ability, expected } => write!(
                f,
                "ability {:?} now has handlers {:?}, implemented for {expected:?}; regenerate \
                 and re-verify",
                ability.data().name,
                ability.data().handlers
            ),
            SwitchInError::TraceWithoutTarget { slot } => write!(
                f,
                "{}: Trace has no traceable foe and would keep seeking; not representable",
                slot_name(*slot)
            ),
            SwitchInError::TraceNextToNoAbility { slot } => {
                write!(f, "{}: Trace next to No Ability", slot_name(*slot))
            }
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

/// What an ability does when it starts (switch-in or copied by Trace).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartBehavior {
    /// No handler that can fire before the first decision.
    Inert,
    Trace,
    Weather(Weather),
    Terrain(Terrain),
}

/// Events that can fire between team preview and the first decision: `SwitchIn` (with the
/// `onStart` fallback for abilities and items), `BeforeSwitchIn`, `BattleStart` (species),
/// `Update`, and what Trace/weather/terrain setting triggers (`SetAbility`, `SetWeather`,
/// `WeatherChange`, `TerrainChange`). Trace's `setAbility` also fires `End`, but only on the
/// holder's Trace, which has no `onEnd`. `ModifySpe` would matter if the start order used
/// modified Speed; only handlers verified inert at start are allowed.
const START_EVENTS: [&str; 10] = [
    "Start",
    "SwitchIn",
    "BeforeSwitchIn",
    "BattleStart",
    "Update",
    "SetAbility",
    "SetWeather",
    "WeatherChange",
    "TerrainChange",
    "ModifySpe",
];

/// The first handler in `handlers` that can fire during the start sequence. Handlers of an
/// effect's own condition (`condition.on*`) belong to a volatile that does not exist yet.
fn start_handler(handlers: &'static [&'static str]) -> Option<&'static str> {
    handlers.iter().copied().find(|h| {
        let Some(event) = h.strip_prefix("on") else {
            return false;
        };
        let event = ["Ally", "Foe", "Any", "Source"]
            .iter()
            .find_map(|p| event.strip_prefix(*p).filter(|e| START_EVENTS.contains(e)))
            .unwrap_or(event);
        START_EVENTS.contains(&event)
    })
}

/// Implemented abilities with the exact handler lists they were implemented against.
const IMPLEMENTED: [(AbilityId, &[&str], StartBehavior); 5] = [
    (
        abilities::TRACE,
        &["onStart", "onUpdate"],
        StartBehavior::Trace,
    ),
    (
        abilities::SAND_STREAM,
        &["onStart"],
        StartBehavior::Weather(Weather::Sand),
    ),
    (
        abilities::GRASSY_SURGE,
        &["onStart"],
        StartBehavior::Terrain(Terrain::Grassy),
    ),
    // `onModifySpe` doubles Speed only in sand, which cannot be up before the start
    // handlers run, and the start order is fixed before any of them.
    (
        abilities::SAND_RUSH,
        &["onImmunity", "onModifySpe"],
        StartBehavior::Inert,
    ),
    // `onModifySpe` needs a status, and no lead has one at the start (`validate`).
    (
        abilities::QUICK_FEET,
        &["onModifySpe"],
        StartBehavior::Inert,
    ),
];

fn start_behavior(slot: SlotRef, ability: AbilityId) -> Result<StartBehavior, SwitchInError> {
    let data = ability.data();
    if let Some(&(_, expected, behavior)) = IMPLEMENTED.iter().find(|(id, ..)| *id == ability) {
        if data.handlers != expected {
            return Err(SwitchInError::HandlersChanged { ability, expected });
        }
        return Ok(behavior);
    }
    if let Some(handler) = start_handler(data.handlers) {
        return Err(SwitchInError::UnsupportedAbility {
            slot,
            ability,
            handler,
        });
    }
    if data.suppress_weather {
        return Err(SwitchInError::UnsupportedAbility {
            slot,
            ability,
            handler: "suppressWeather",
        });
    }
    Ok(StartBehavior::Inert)
}

fn check_item(slot: SlotRef, item: ItemId) -> Result<(), SwitchInError> {
    match start_handler(item.data().handlers) {
        Some(handler) => Err(SwitchInError::UnsupportedItem {
            slot,
            item,
            handler,
        }),
        None => Ok(()),
    }
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
        start_behavior(r, mon.ability)?;
        check_item(r, mon.item)?;
        if let Some(handler) = start_handler(mon.species.data().handlers) {
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

/// Every order `speedSort` can produce for the actives, with its probability: Speed high to
/// low, each group of ties in each of its `k!` orders with probability `1/k!`.
fn start_orders<const N: usize>(state: &State<N>) -> Vec<(f64, Vec<SlotRef>)> {
    let speed = |r: &SlotRef| state.active(*r).map_or(0, |mon| mon.stats[4]);
    let mut actives: Vec<SlotRef> = State::<N>::slot_refs().collect();
    actives.sort_by_key(|r| std::cmp::Reverse(speed(r)));

    let mut orders = vec![(1.0, Vec::new())];
    let mut start = 0;
    while start < actives.len() {
        let mut end = start + 1;
        while end < actives.len() && speed(&actives[end]) == speed(&actives[start]) {
            end += 1;
        }
        let group = permutations(&actives[start..end]);
        let p = 1.0 / group.len() as f64;
        orders = orders
            .into_iter()
            .flat_map(|(q, prefix)| {
                group.iter().map(move |perm| {
                    let mut order = prefix.clone();
                    order.extend_from_slice(perm);
                    (q * p, order)
                })
            })
            .collect();
        start = end;
    }
    orders
}

fn permutations(items: &[SlotRef]) -> Vec<Vec<SlotRef>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, first);
            out.push(tail);
        }
    }
    out
}

fn ability_of<const N: usize>(state: &State<N>, r: SlotRef) -> AbilityId {
    state.active(r).map_or(AbilityId::NONE, |p| p.ability)
}

/// Showdown `setWeather` from an ability: fails if the same weather is up.
fn set_weather<const N: usize>(state: &mut State<N>, weather: Weather, source: SlotRef) {
    let current = state.field[FieldEffect::Weather as usize];
    if current.is_active() && current.value == weather as u8 {
        return;
    }
    let rock = match weather {
        Weather::Sand => items::SMOOTH_ROCK,
        // Only abilities that set sand are implemented (see `IMPLEMENTED`).
        _ => unreachable!("weather {weather:?} has no implemented setter"),
    };
    let holds_rock = state.active(source).is_some_and(|mon| mon.item == rock);
    state.field[FieldEffect::Weather as usize] = Effect {
        value: weather as u8,
        turns: if holds_rock { 8 } else { 5 },
    };
}

/// Showdown `setTerrain`: fails if the same terrain is up.
fn set_terrain<const N: usize>(state: &mut State<N>, terrain: Terrain, source: SlotRef) {
    let current = state.field[FieldEffect::Terrain as usize];
    if current.is_active() && current.value == terrain as u8 {
        return;
    }
    let extender = state
        .active(source)
        .is_some_and(|mon| mon.item == items::TERRAIN_EXTENDER);
    state.field[FieldEffect::Terrain as usize] = Effect {
        value: terrain as u8,
        turns: if extender { 8 } else { 5 },
    };
}

struct Expansion<'a, const N: usize> {
    order: &'a [SlotRef],
    /// The ability each handler was collected for (`fieldEvent` skips it if it changed).
    collected: &'a [AbilityId],
    out: &'a mut Vec<InitialOutcome<N>>,
}

impl<const N: usize> Expansion<'_, N> {
    fn run(&mut self, state: State<N>, step: usize, p: f64) -> Result<(), SwitchInError> {
        let Some(&holder) = self.order.get(step) else {
            self.push(state, p);
            return Ok(());
        };
        let ability = ability_of(&state, holder);
        if ability != self.collected[step] {
            return self.run(state, step + 1, p);
        }
        self.start(state, holder, ability, step, p)
    }

    /// Runs `ability`'s start for `holder`, then the remaining handlers.
    fn start(
        &mut self,
        mut state: State<N>,
        holder: SlotRef,
        ability: AbilityId,
        step: usize,
        p: f64,
    ) -> Result<(), SwitchInError> {
        match start_behavior(holder, ability)? {
            StartBehavior::Inert => {}
            StartBehavior::Weather(weather) => set_weather(&mut state, weather, holder),
            StartBehavior::Terrain(terrain) => set_terrain(&mut state, terrain, holder),
            StartBehavior::Trace => return self.trace(state, holder, step, p),
        }
        self.run(state, step + 1, p)
    }

    fn trace(
        &mut self,
        state: State<N>,
        holder: SlotRef,
        step: usize,
        p: f64,
    ) -> Result<(), SwitchInError> {
        // With at most two slots per side every living foe is adjacent.
        let foes: Vec<SlotRef> = (0..N as u8)
            .map(|slot| SlotRef {
                side: holder.side.other(),
                slot,
            })
            .filter(|r| state.active(*r).is_some_and(|mon| mon.is_alive()))
            .collect();
        if foes
            .iter()
            .any(|r| ability_of(&state, *r) == abilities::NO_ABILITY)
        {
            return Err(SwitchInError::TraceNextToNoAbility { slot: holder });
        }
        if state
            .active(holder)
            .is_some_and(|mon| mon.item == items::ABILITY_SHIELD)
        {
            // Rejected by `check_item` already (`onSetAbility`); kept as a guard.
            return Err(SwitchInError::UnsupportedItem {
                slot: holder,
                item: items::ABILITY_SHIELD,
                handler: "onSetAbility",
            });
        }
        let targets: Vec<AbilityId> = foes
            .iter()
            .map(|r| ability_of(&state, *r))
            .filter(|a| !a.data().flags.contains(AbilityFlags::NOTRACE))
            .collect();
        if targets.is_empty() {
            return Err(SwitchInError::TraceWithoutTarget { slot: holder });
        }
        let share = p / targets.len() as f64;
        for copied in targets {
            // setAbility fails on `cantsuppress` and would leave Trace seeking.
            if copied.data().flags.contains(AbilityFlags::CANTSUPPRESS) {
                return Err(SwitchInError::UnsupportedAbility {
                    slot: holder,
                    ability: copied,
                    handler: "cantsuppress (Trace copy)",
                });
            }
            let mut next = state.clone();
            if let Some(mon) = next.active_mut(holder) {
                mon.ability = copied;
            }
            // Trace has no `onEnd`; the copied ability starts at once.
            self.start(next, holder, copied, step, share)?;
        }
        Ok(())
    }

    fn push(&mut self, state: State<N>, p: f64) {
        match self.out.iter_mut().find(|o| o.state == state) {
            Some(existing) => existing.probability += p,
            None => self.out.push(InitialOutcome {
                probability: p,
                state,
            }),
        }
    }
}

/// Weighted states after the leads' start-of-battle effects. `state` must be a fresh start
/// (as [`crate::load_scenario_file`] builds it). Outcomes are in first-reached order.
pub fn expand_switch_ins<const N: usize>(
    state: &State<N>,
) -> Result<Vec<InitialOutcome<N>>, SwitchInError> {
    validate(state)?;
    let mut out = Vec::new();
    for (p, order) in start_orders(state) {
        let collected: Vec<AbilityId> = order.iter().map(|r| ability_of(state, *r)).collect();
        Expansion {
            order: &order,
            collected: &collected,
            out: &mut out,
        }
        .run(state.clone(), 0, p)?;
    }
    Ok(out)
}

/// [`expand_switch_ins`] for a loaded scenario.
pub fn initial_outcomes(loaded: &LoadedScenario) -> Result<Vec<InitialOutcome<2>>, SwitchInError> {
    expand_switch_ins(&loaded.state)
}
