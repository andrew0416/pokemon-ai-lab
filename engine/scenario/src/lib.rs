//! Oracle scenario/team JSON → `lab_engine::State<2>` plus sidecar identity data.
//!
//! This crate is the only place serde touches the engine. It runs once per scenario, off the
//! search hot path; the `State` it produces holds no strings or JSON values. Names and other
//! set data the rules do not need live in [`meta::ScenarioMeta`] beside it.
//!
//! Scope: initial states only. A scenario is two teams, their team preview order and the
//! decision to check, in the custom game ([`DOUBLES_FORMAT`], every member brought) or VGC
//! ([`VGC_FORMAT`], team preview keeps 4). The loaded state is the one right after leads are placed and **before
//! switch-in effects**. [`switch_in::initial_outcomes`] expands it into the weighted states
//! after the leads' start effects (the implemented subset: Trace, Sand Stream, Grassy Surge;
//! anything else that could act is rejected); one of those is Showdown's `before` snapshot.
//! [`canonical::canonical_json`] writes a state in the oracle's canonical form (schema 1).
//! [`decision`] applies the oracle's `patch` after the switch-ins ([`scenario_states`]) and
//! parses the `turn` choice strings. `setupTurns` are replayed with the turn engine,
//! never skipped.

pub mod canonical;
pub mod decision;
pub mod error;
pub mod json;
pub mod meta;
pub mod parity;
pub mod switch_in;
pub mod team;

use std::path::Path;

use serde_json::Value;

use lab_engine::action::JointAction;
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, State, PARTY_SIZE};
use lab_engine::turn::{
    enumerate_replacements, enumerate_turn_with, resume_turn_with, EnumerateOptions, RollMode,
    TurnError,
};
use lab_engine::Doubles;

pub use canonical::{canonical_json, canonical_value, CanonicalError};
pub use decision::{
    advance_order, apply_patch, initial_order, parse_choice, parse_mid_turn, parse_replacement,
    PartyOrder, PatchJson,
};
pub use error::{LoadError, SetProblem, TeamProblem};
pub use json::{ScenarioJson, TeamSet};
pub use meta::{MemberMeta, ScenarioMeta, SideMeta};
pub use switch_in::{expand_switch_ins, initial_outcomes, InitialOutcome, SwitchInError};
pub use team::{build_picked_side, build_pokemon, build_side, picked_order, preview_order};

/// The oracle's doubles format: Champions mechanics, every member brought, level 50.
pub const DOUBLES_FORMAT: &str = "gen9championsdoublescustomgame";

/// Champions VGC 2026 Reg M-C (`[Gen 9 Champions] VGC 2026 Reg M-C`): the same battle
/// mechanics, but Flat Rules' team preview keeps 4 of the (up to 6) members (`Picked Team
/// Size = Auto` in doubles), and its `Adjust Level = 50` sets every set to level 50 (the team
/// validator does it; the oracle's `loadTeam` applies it too). Its other rules (clauses, team
/// size, legality) belong to the team validator, which neither the oracle nor this loader
/// runs; the timer and Open Team Sheets only add messages. (The custom game's `battle.trunc`
/// override only differs for values beyond 16 bits and Speed above 10000.)
pub const VGC_FORMAT: &str = "gen9championsvgc2026regmc";

/// How many members a supported doubles format's team preview keeps (Showdown
/// `ruleTable.pickedTeamSize`, capped by the team size): every member in the custom game, 4 in
/// VGC. `None` for a format the loader does not support.
pub fn picked_team_size(format: &str) -> Option<usize> {
    match format {
        DOUBLES_FORMAT => Some(PARTY_SIZE),
        VGC_FORMAT => Some(4),
        _ => None,
    }
}

/// Showdown's `battle.turn` at the first decision after team preview.
const FIRST_TURN: u16 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedScenario {
    pub state: Doubles,
    pub meta: ScenarioMeta,
    /// Turns played before the decision (`[p1 choice, p2 choice]` or `[p1, p2, {"p1": [...],
    /// "p2": [...]}]` with mid-turn switch choices), replayed by [`scenario_positions`] after
    /// the switch-ins and before the patch, as `enumerate.cjs` does.
    pub setup_turns: Vec<SetupTurn>,
    /// Applied after the setup turns, by [`scenario_positions`].
    pub patch: Option<PatchJson>,
    /// The decision turn's mid-turn switch choices per side (`midTurn`), consumed in order as
    /// the turn asks that side (see [`run_decision_mid_turn`]).
    pub mid_turn: [Vec<String>; 2],
    /// `startState`: only initial outcomes with this canonical state are replayed.
    pub start_state: Option<Value>,
    /// `setupStates`: per setup turn, the canonical state its replay must end in (`None` keeps
    /// every outcome). See [`scenario_positions_filtered`].
    pub setup_states: Vec<Option<Value>>,
    /// `setupRolls`: the damage-roll mode the setup turns are replayed with, overriding the
    /// caller's.
    pub setup_rolls: Option<RollMode>,
}

/// A turn played before the decision: both sides' choices and the mid-turn switch choices
/// the turn asks for (U-turn, Parting Shot, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupTurn {
    pub p1: String,
    pub p2: String,
    pub mid_turn: [Vec<String>; 2],
}

fn parse_setup_turn(value: serde_json::Value) -> Result<SetupTurn, LoadError> {
    if let Ok((p1, p2)) = serde_json::from_value::<(String, String)>(value.clone()) {
        return Ok(SetupTurn {
            p1,
            p2,
            mid_turn: [Vec::new(), Vec::new()],
        });
    }
    let (p1, p2, mid): (String, String, crate::json::MidTurnJson) =
        serde_json::from_value(value).map_err(|error| LoadError::Json {
            what: "setupTurns".into(),
            error,
        })?;
    Ok(SetupTurn {
        p1,
        p2,
        mid_turn: [mid.p1, mid.p2],
    })
}

/// A position the scenario's decision can be made in, with Showdown's party order per side
/// (what `switch N` in a choice string counts).
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub probability: f64,
    pub state: Doubles,
    pub order: [PartyOrder; 2],
}

/// A scenario's decision: a turn, or the replacement of fainted Pokémon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Turn([JointAction<2>; 2]),
    Replacement([[Option<u8>; 2]; 2]),
}

/// Loads a scenario file; team paths resolve relative to its directory.
pub fn load_scenario_file(path: impl AsRef<Path>) -> Result<LoadedScenario, LoadError> {
    let path = path.as_ref();
    let text = read_text(path)?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    load_scenario_str(&text, base_dir)
}

/// Loads scenario JSON; team paths resolve relative to `base_dir`.
pub fn load_scenario_str(json: &str, base_dir: &Path) -> Result<LoadedScenario, LoadError> {
    let scenario: ScenarioJson =
        serde_json::from_str(strip_bom(json)).map_err(|error| LoadError::Json {
            what: "scenario".into(),
            error,
        })?;

    let Some(picked) = picked_team_size(&scenario.format) else {
        return Err(LoadError::UnsupportedFormat(scenario.format));
    };
    let setup_turns = match scenario.setup_turns {
        Some(value) if !is_empty(Some(&value)) => {
            let turns: Vec<serde_json::Value> =
                serde_json::from_value(value).map_err(|error| LoadError::Json {
                    what: "setupTurns".into(),
                    error,
                })?;
            turns
                .into_iter()
                .map(parse_setup_turn)
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => Vec::new(),
    };
    let patch = match scenario.patch {
        Some(value) if !is_empty(Some(&value)) => Some(serde_json::from_value(value).map_err(
            |error| LoadError::Json {
                what: "patch".into(),
                error,
            },
        )?),
        _ => None,
    };

    let mut state = Doubles::default();
    let mut sides: [SideMeta; 2] = Default::default();
    for (side, spec) in [(SideId::One, &scenario.p1), (SideId::Two, &scenario.p2)] {
        let mut team = resolve_team(side, &spec.team, base_dir)?;
        if scenario.format == VGC_FORMAT {
            // `Adjust Level = 50`.
            for set in &mut team {
                set.level = Some(50);
            }
        } else {
            // The custom game adjusts nothing: Showdown plays a set without `level` at level
            // 100 (the Champions stats stay the same, damage about doubles), so such a set
            // must not load silently as level 50 (board B31). It is refused like any other
            // level-100 set; the file has to say `"level": 50`.
            for set in &mut team {
                if set.level.is_none() {
                    set.level = Some(100);
                }
            }
        }
        let (built, meta) = build_picked_side::<2>(side, &team, spec.order.as_deref(), picked)?;
        state.sides[side.index()] = built;
        sides[side.index()] = meta;
    }
    state.turn = FIRST_TURN;

    let mid_turn = scenario.mid_turn.unwrap_or_default();
    let setup_states: Vec<Option<Value>> = scenario
        .setup_states
        .unwrap_or_default()
        .into_iter()
        .map(|v| if v.is_null() { None } else { Some(v) })
        .collect();
    if setup_states.len() > setup_turns.len() {
        return Err(LoadError::Unsupported {
            field: "setupStates",
            reason: "more pinned states than setup turns",
        });
    }
    let setup_rolls = match scenario.setup_rolls.as_deref() {
        None => None,
        Some("full") => Some(RollMode::Full),
        Some("extremes") => Some(RollMode::Extremes),
        Some(_) => {
            return Err(LoadError::Unsupported {
                field: "setupRolls",
                reason: "expected \"full\" or \"extremes\"",
            })
        }
    };
    Ok(LoadedScenario {
        state,
        meta: ScenarioMeta {
            format: scenario.format,
            description: scenario.description.unwrap_or_default(),
            sides,
            turn: scenario.turn,
        },
        setup_turns,
        patch,
        mid_turn: [mid_turn.p1, mid_turn.p2],
        start_state: scenario.start_state.filter(|v| !v.is_null()),
        setup_states,
        setup_rolls,
    })
}

/// Whether `side` must send in a replacement (Showdown `request: switch` for it): an empty
/// active slot and a healthy bench member.
pub fn side_must_replace<const N: usize>(state: &State<N>, side: SideId) -> bool {
    lab_engine::turn::side_must_replace(state, side)
}

/// Parses both sides' choice strings for the decision `state` is waiting for: replacements
/// if either side must replace a fainted Pokémon, a turn otherwise.
pub fn parse_decision(
    state: &Doubles,
    order: &[PartyOrder; 2],
    p1: &str,
    p2: &str,
) -> Result<Decision, String> {
    let replacing = [SideId::One, SideId::Two]
        .into_iter()
        .any(|side| side_must_replace(state, side));
    if replacing {
        Ok(Decision::Replacement([
            parse_replacement(state, SideId::One, &order[0], p1)?,
            parse_replacement(state, SideId::Two, &order[1], p2)?,
        ]))
    } else {
        Ok(Decision::Turn([
            parse_choice(state, SideId::One, &order[0], p1)?,
            parse_choice(state, SideId::Two, &order[1], p2)?,
        ]))
    }
}

/// Every outcome of `decision` from `state` (left unchanged). A turn's outcome can be
/// suspended for a mid-turn switch (`Outcome::suspension`); see [`run_decision_mid_turn`].
pub fn run_decision(state: &mut Doubles, decision: &Decision) -> Result<Vec<Outcome>, TurnError> {
    run_decision_with(state, decision, EnumerateOptions::default())
}

/// [`run_decision`] with enumeration `options` (damage-roll mode).
pub fn run_decision_with(
    state: &mut Doubles,
    decision: &Decision,
    options: EnumerateOptions,
) -> Result<Vec<Outcome>, TurnError> {
    match decision {
        Decision::Turn(choices) => {
            enumerate_turn_with(state, Ruleset::CHAMPIONS_MC, *choices, options)
        }
        Decision::Replacement(choices) => enumerate_replacements(state, *choices),
    }
}

/// Whether `side` must send in a mid-turn switch (Showdown `request: switch` with actions
/// still queued): `Slot::must_switch_out` (an occupant with `Slot::switch_flag`, or the
/// flagged Pokémon that fainted there after its own recoil).
pub fn side_must_switch<const N: usize>(state: &State<N>, side: SideId) -> bool {
    lab_engine::turn::side_must_switch(state, side)
}

/// [`run_decision`], then every suspended outcome is resumed with the next `mid_turn` choice
/// of each side it asks (`"switch N"` against Showdown's party order at that point), as
/// `enumerate.cjs` does with `midTurn`. A turn asking a side that has no choice left stays
/// suspended in the result. The outcomes' instructions run from `state`.
pub fn run_decision_mid_turn(
    state: &mut Doubles,
    order: &[PartyOrder; 2],
    decision: &Decision,
    mid_turn: &[Vec<String>; 2],
) -> Result<Vec<Outcome>, String> {
    run_decision_mid_turn_with(
        state,
        order,
        decision,
        mid_turn,
        EnumerateOptions::default(),
    )
}

/// [`run_decision_mid_turn`] with enumeration `options`.
pub fn run_decision_mid_turn_with(
    state: &mut Doubles,
    order: &[PartyOrder; 2],
    decision: &Decision,
    mid_turn: &[Vec<String>; 2],
    options: EnumerateOptions,
) -> Result<Vec<Outcome>, String> {
    let outcomes = run_decision_with(state, decision, options).map_err(|e| e.to_string())?;
    let mut done = Vec::new();
    let mut work: Vec<(Outcome, [usize; 2])> = outcomes.into_iter().map(|o| (o, [0, 0])).collect();
    while let Some((outcome, used)) = work.pop() {
        let Some(suspension) = outcome.suspension.clone() else {
            done.push(outcome);
            continue;
        };
        let mut paused = state.clone();
        paused.apply(&outcome.instructions);
        let mut order = order.clone();
        advance_order(&mut order, &outcome.instructions);
        let mut choices = [[None; 2]; 2];
        let mut next_used = used;
        let mut missing = false;
        for side in [SideId::One, SideId::Two] {
            if !side_must_switch(&paused, side) {
                continue;
            }
            match mid_turn[side.index()].get(used[side.index()]) {
                Some(text) => {
                    choices[side.index()] =
                        parse_mid_turn(&paused, side, &order[side.index()], text)?;
                    next_used[side.index()] += 1;
                }
                None => missing = true,
            }
        }
        if missing {
            done.push(outcome);
            continue;
        }
        let resumed = resume_turn_with(&mut paused, &suspension, choices, options)
            .map_err(|e| e.to_string())?;
        for r in resumed {
            let mut instructions = outcome.instructions.clone();
            instructions.extend(r.instructions);
            work.push((
                Outcome {
                    probability: outcome.probability * r.probability,
                    instructions,
                    suspension: r.suspension,
                },
                next_used,
            ));
        }
    }
    Ok(done)
}

/// The positions the scenario's decision is made in: every initial outcome (the leads'
/// switch-in effects), then every outcome of the setup turns, then the patch. Positions
/// with the same state and party order merge.
pub fn scenario_positions(loaded: &LoadedScenario) -> Result<Vec<Position>, String> {
    scenario_positions_with(loaded, EnumerateOptions::default())
}

/// [`scenario_positions`] with enumeration `options` for the setup turns (a reduced damage-roll
/// mode keeps the position count small at the cost of exactness; the initial switch-ins and the
/// patch are unaffected).
pub fn scenario_positions_with(
    loaded: &LoadedScenario,
    options: EnumerateOptions,
) -> Result<Vec<Position>, String> {
    scenario_positions_filtered(loaded, options, &mut |_, positions| positions)
}

/// [`scenario_positions_with`] with `filter(n, positions)` applied to the positions after each
/// setup turn (`n` counts from 1); only what it returns is carried into the next turn. Positions
/// carry joint probabilities, so the total probability of what survives every filter is the
/// probability that the setup turns produced the observations the filters encode: the
/// likelihood the opponent models of DESIGN.md "모델 ③·② 구현" weigh believed teams by. The
/// scenario's patch is applied after the last turn, to what survives.
pub fn scenario_positions_filtered(
    loaded: &LoadedScenario,
    options: EnumerateOptions,
    filter: &mut dyn FnMut(usize, Vec<Position>) -> Vec<Position>,
) -> Result<Vec<Position>, String> {
    replay_setup_turns(loaded, options, false, filter)
}

/// [`scenario_positions_filtered`] for a replay of turns that were actually played on a
/// scenario whose teams are only believed: a position in which a setup turn's choices are not
/// legal (a Pokémon the choice moves with had already fainted there, say) is contradicted by
/// the fact that they were made, so it is dropped with its probability instead of failing the
/// replay. What remains sums to the probability that the believed teams produce the observed
/// choices and observations. A choice illegal in every position leaves nothing, not an error.
pub fn scenario_positions_consistent(
    loaded: &LoadedScenario,
    options: EnumerateOptions,
    filter: &mut dyn FnMut(usize, Vec<Position>) -> Vec<Position>,
) -> Result<Vec<Position>, String> {
    replay_setup_turns(loaded, options, true, filter)
}

fn replay_setup_turns(
    loaded: &LoadedScenario,
    options: EnumerateOptions,
    drop_illegal: bool,
    filter: &mut dyn FnMut(usize, Vec<Position>) -> Vec<Position>,
) -> Result<Vec<Position>, String> {
    let mut positions: Vec<Position> = initial_outcomes(loaded)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|o| Position {
            order: [
                initial_order(&o.state, SideId::One),
                initial_order(&o.state, SideId::Two),
            ],
            probability: o.probability,
            state: o.state,
        })
        .collect();
    if let Some(pin) = &loaded.start_state {
        positions = pinned(&loaded.meta, positions, pin).map_err(|e| format!("startState: {e}"))?;
    }
    let options = EnumerateOptions {
        rolls: loaded.setup_rolls.unwrap_or(options.rolls),
    };
    for (n, turn) in loaded.setup_turns.iter().enumerate() {
        let pin = loaded.setup_states.get(n).and_then(|pin| pin.as_ref());
        let mut next: Vec<Position> = Vec::new();
        // Whether an outcome of this turn still waits for a mid-turn switch (board B29).
        let mut paused = false;
        for position in &positions {
            let decision =
                match parse_decision(&position.state, &position.order, &turn.p1, &turn.p2) {
                    Ok(decision) => decision,
                    Err(_) if drop_illegal => continue,
                    Err(e) => return Err(format!("setup turn {}: {e}", n + 1)),
                };
            let mut state = position.state.clone();
            let outcomes = match run_decision_mid_turn_with(
                &mut state,
                &position.order,
                &decision,
                &turn.mid_turn,
                options,
            ) {
                Ok(outcomes) => outcomes,
                Err(_) if drop_illegal => continue,
                Err(e) => return Err(format!("setup turn {}: {e}", n + 1)),
            };
            for outcome in outcomes {
                // A setup turn that still waits for a mid-turn switch is not a position: the
                // suspension cannot be carried into the next turn (board B29). On believed
                // teams the choices that were made contradict such an outcome, which is dropped.
                let mut end = state.clone();
                end.apply(&outcome.instructions);
                if outcome.suspension.is_some() {
                    // With a pinned outcome the turn's other branches are not the scenario's
                    // (the U-turn that hit where the game's missed), so a paused one is only
                    // refused if it is the pinned state itself.
                    let refused = match pin {
                        Some(pin) => {
                            canonical_value(&end, &loaded.meta).map_err(|e| e.to_string())? == *pin
                        }
                        None => !drop_illegal,
                    };
                    if refused {
                        paused = true;
                    }
                    continue;
                }
                let mut order = position.order.clone();
                advance_order(&mut order, &outcome.instructions);
                let p = position.probability * outcome.probability;
                match next.iter_mut().find(|q| q.state == end && q.order == order) {
                    Some(existing) => existing.probability += p,
                    None => next.push(Position {
                        probability: p,
                        state: end,
                        order,
                    }),
                }
            }
        }
        if paused {
            return Err(format!(
                "setup turn {}: the turn pauses for a mid-turn switch that has no choice; give                  it in the setup turn's third element (`midTurn`)",
                n + 1
            ));
        }
        if let Some(pin) = pin {
            next = pinned(&loaded.meta, next, pin)
                .map_err(|e| format!("setup turn {}: {e}", n + 1))?;
        }
        positions = filter(n + 1, next);
    }
    if let Some(patch) = &loaded.patch {
        for position in &mut positions {
            apply_patch(&mut position.state, &loaded.meta, patch)?;
        }
    }
    Ok(positions)
}

/// The positions whose canonical state is `pin` (compared as JSON values, so key order does
/// not matter), with their probabilities renormalized: a pinned outcome is part of the
/// scenario's definition, not an observation. None matching is an error naming the first
/// canonical fields in which the closest candidate differs.
fn pinned(
    meta: &ScenarioMeta,
    positions: Vec<Position>,
    pin: &Value,
) -> Result<Vec<Position>, String> {
    let candidates = positions.len();
    let mut kept = Vec::new();
    let mut closest: Option<(usize, Vec<String>)> = None;
    for position in positions {
        let value = canonical_value(&position.state, meta).map_err(|e| e.to_string())?;
        if value == *pin {
            kept.push(position);
            continue;
        }
        let diffs = parity::json_diff(pin, &value, 5);
        if closest.as_ref().is_none_or(|(n, _)| diffs.len() < *n) {
            closest = Some((diffs.len(), diffs));
        }
    }
    if kept.is_empty() {
        return Err(format!(
            "none of {candidates} position(s) has the pinned canonical state{}",
            match closest {
                Some((_, diffs)) => format!("; closest differs at {}", diffs.join(", ")),
                None => String::new(),
            }
        ));
    }
    let total: f64 = kept.iter().map(|p| p.probability).sum();
    if total > 0.0 {
        for p in &mut kept {
            p.probability /= total;
        }
    }
    Ok(kept)
}

/// The scenario's `turn` choices parsed for `position`.
pub fn scenario_decision(loaded: &LoadedScenario, position: &Position) -> Result<Decision, String> {
    let turn = loaded
        .meta
        .turn
        .as_ref()
        .ok_or("the scenario has no turn")?;
    parse_decision(&position.state, &position.order, &turn.p1, &turn.p2)
}

/// [`scenario_positions`] without the party orders.
pub fn scenario_states(loaded: &LoadedScenario) -> Result<Vec<InitialOutcome<2>>, String> {
    Ok(scenario_positions(loaded)?
        .into_iter()
        .map(|p| InitialOutcome {
            probability: p.probability,
            state: p.state,
        })
        .collect())
}

/// The scenario's `turn` choices as a turn decision, parsed against `state` with the initial
/// party order (valid for scenarios without setup turns).
pub fn scenario_choices(
    loaded: &LoadedScenario,
    state: &Doubles,
) -> Result<[JointAction<2>; 2], String> {
    let turn = loaded
        .meta
        .turn
        .as_ref()
        .ok_or("the scenario has no turn")?;
    let order = [
        initial_order(state, SideId::One),
        initial_order(state, SideId::Two),
    ];
    Ok([
        parse_choice(state, SideId::One, &order[0], &turn.p1)?,
        parse_choice(state, SideId::Two, &order[1], &turn.p2)?,
    ])
}

/// Parses a team JSON array.
pub fn parse_team(json: &str) -> Result<Vec<TeamSet>, LoadError> {
    serde_json::from_str(strip_bom(json)).map_err(|error| LoadError::Json {
        what: "team".into(),
        error,
    })
}

/// Builds a state from teams and team preview orders directly (no scenario file).
pub fn state_from_teams<const N: usize>(
    teams: [(&[TeamSet], Option<&str>); 2],
) -> Result<(State<N>, [SideMeta; 2]), LoadError> {
    let mut state = State::<N>::default();
    let mut sides: [SideMeta; 2] = Default::default();
    for (side, (team, order)) in [SideId::One, SideId::Two].into_iter().zip(teams) {
        let (built, meta) = build_side::<N>(side, team, order)?;
        state.sides[side.index()] = built;
        sides[side.index()] = meta;
    }
    state.turn = FIRST_TURN;
    Ok((state, sides))
}

fn resolve_team(side: SideId, spec: &Value, base_dir: &Path) -> Result<Vec<TeamSet>, LoadError> {
    let what = format!("{} team", if side == SideId::One { "p1" } else { "p2" });
    match spec {
        Value::String(relative) => {
            let path = base_dir.join(relative);
            serde_json::from_str(strip_bom(&read_text(&path)?)).map_err(|error| LoadError::Json {
                what: format!("{what} {}", path.display()),
                error,
            })
        }
        Value::Array(_) => {
            serde_json::from_value(spec.clone()).map_err(|error| LoadError::Json { what, error })
        }
        _ => Err(LoadError::Unsupported {
            field: "team",
            reason: "expected a path string or an inline team array",
        }),
    }
}

fn read_text(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|error| LoadError::Io {
        path: path.to_owned(),
        error,
    })
}

/// Team files written by PowerShell may start with a byte order mark.
fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Absent, `null`, `[]` and `{}` all mean "nothing to do".
fn is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
        Some(_) => false,
    }
}
