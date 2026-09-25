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
pub mod switch_in;
pub mod team;

use std::path::Path;

use serde_json::Value;

use lab_engine::action::JointAction;
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, State, PARTY_SIZE};
use lab_engine::turn::{enumerate_replacements, enumerate_turn, TurnError};
use lab_engine::Doubles;

pub use canonical::{canonical_json, canonical_value, CanonicalError};
pub use decision::{
    advance_order, apply_patch, initial_order, parse_choice, parse_replacement, PartyOrder,
    PatchJson,
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
    /// Turns played before the decision (`[p1 choice, p2 choice]` each), replayed by
    /// [`scenario_positions`] after the switch-ins and before the patch, as `enumerate.cjs`
    /// does.
    pub setup_turns: Vec<(String, String)>,
    /// Applied after the setup turns, by [`scenario_positions`].
    pub patch: Option<PatchJson>,
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
            let turns: Vec<(String, String)> =
                serde_json::from_value(value).map_err(|error| LoadError::Json {
                    what: "setupTurns".into(),
                    error,
                })?;
            turns
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
        }
        let (built, meta) = build_picked_side::<2>(side, &team, spec.order.as_deref(), picked)?;
        state.sides[side.index()] = built;
        sides[side.index()] = meta;
    }
    state.turn = FIRST_TURN;

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
    })
}

/// Whether `side` must send in a replacement (Showdown `request: switch` for it): an empty
/// active slot and a healthy bench member.
pub fn side_must_replace<const N: usize>(state: &State<N>, side: SideId) -> bool {
    let s = state.side(side);
    let empty = s.slots.iter().any(|slot| slot.party_index.is_none());
    let bench = (0..s.party.len() as u8).any(|i| {
        s.party[i as usize].hp > 0 && !s.slots.iter().any(|slot| slot.party_index == Some(i))
    });
    empty && bench
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

/// Every outcome of `decision` from `state` (left unchanged).
pub fn run_decision(state: &mut Doubles, decision: &Decision) -> Result<Vec<Outcome>, TurnError> {
    match decision {
        Decision::Turn(choices) => enumerate_turn(state, Ruleset::CHAMPIONS_MC, *choices),
        Decision::Replacement(choices) => enumerate_replacements(state, *choices),
    }
}

/// The positions the scenario's decision is made in: every initial outcome (the leads'
/// switch-in effects), then every outcome of the setup turns, then the patch. Positions
/// with the same state and party order merge.
pub fn scenario_positions(loaded: &LoadedScenario) -> Result<Vec<Position>, String> {
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
    for (n, (p1, p2)) in loaded.setup_turns.iter().enumerate() {
        let mut next: Vec<Position> = Vec::new();
        for position in &positions {
            let decision = parse_decision(&position.state, &position.order, p1, p2)
                .map_err(|e| format!("setup turn {}: {e}", n + 1))?;
            let mut state = position.state.clone();
            let outcomes = run_decision(&mut state, &decision)
                .map_err(|e| format!("setup turn {}: {e}", n + 1))?;
            for outcome in outcomes {
                let mut end = state.clone();
                end.apply(&outcome.instructions);
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
        positions = next;
    }
    if let Some(patch) = &loaded.patch {
        for position in &mut positions {
            apply_patch(&mut position.state, &loaded.meta, patch)?;
        }
    }
    Ok(positions)
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
