//! Conservative one-turn bridge. No per-world continuation search is performed.
//! The caller supplies compatible hypotheses and their prior, not the true world ID.
//! Unsupported intermediate decisions reject the whole game instead of dropping worlds.

use super::{Error, Game, World};
use crate::budgeted::{Domain, EngineDomain, Phase, Position};
use crate::{Choice, Pruning};
use lab_engine::eval::Evaluator;
use lab_engine::rules::Ruleset;
use lab_engine::state::{Pokemon, SideId, State, PARTY_SIZE};
use lab_engine::turn::EnumerateOptions;

#[derive(Clone, Debug)]
pub struct EngineWorld<const N: usize> {
    pub id: String,
    pub weight: f64,
    pub position: Position<N>,
}

/// Explicit information boundary; this is NOT an observation-history inference system.
#[derive(Clone, Debug, Default)]
pub struct Knowledge {
    /// May differ in attack/defense/sp.atk/sp.def/speed and stat-point allocations.
    /// Exact current/max HP and every other field still have to agree (conservative).
    pub hidden_stats: bool,
    /// Zero-based opponent party slots asserted by the caller to be unrevealed reserves.
    /// Only accepted at the opening decision (turn 1), never for active occupants.
    /// The caller must validate these hypotheses against the open team sheet.
    pub unrevealed_reserves: Vec<usize>,
}

pub struct BuiltGame<const N: usize> {
    pub game: Game,
    pub ours: Vec<Choice<N>>,
    pub theirs: Vec<Vec<Choice<N>>>,
    pub transitions: usize,
    pub outcomes: usize,
}

pub(crate) fn visible<const N: usize>(
    state: &State<N>,
    us: SideId,
    knowledge: &Knowledge,
) -> Result<State<N>, Error> {
    let mut result = state.clone();
    let other = result.side_mut(us.other());
    let mut used = [false; PARTY_SIZE];
    for &index in &knowledge.unrevealed_reserves {
        if index >= PARTY_SIZE
            || used[index]
            || state.turn != 1
            || other.slots.iter().any(|s| {
                s.party_index == Some(index as u8) || s.fainted_occupant == Some(index as u8)
            })
        {
            return Err(Error(
                "unrevealed reserves must be unique inactive party slots at turn 1".into(),
            ));
        }
        used[index] = true;
        other.party[index] = Pokemon::default();
    }
    if knowledge.hidden_stats {
        for p in &mut other.party {
            p.stats = [0; 5];
            p.stat_points = Default::default();
        }
    }
    Ok(result)
}

/// Enumerates all legal joint actions and full chance, then evaluates terminal/next-turn
/// states with the supplied Evaluator. The returned certificate concerns this horizon-one
/// heuristic game only. KO replacements / U-turn / other mid-turn choices require a later
/// public-information tree implementation and currently return an error.
/// `max_cells` bounds enumerator calls, NOT the allocations/time inside one enumeration.
pub fn one_turn<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    max_cells: usize,
) -> Result<BuiltGame<N>, Error> {
    // Validate every prior and ID even if a world has zero probability.
    Game::new(
        vec!["validate".into()],
        worlds
            .iter()
            .map(|w| World {
                id: w.id.clone(),
                weight: w.weight,
                columns: vec!["validate".into()],
                payoffs: vec![0.0],
            })
            .collect(),
    )?;
    let domain = EngineDomain {
        ruleset,
        options: EnumerateOptions::default(),
        pruning: Pruning::All,
        us,
        evaluator,
    };
    let reference = visible(&worlds[0].position.state, us, knowledge)?;
    let mut ours = Vec::new();
    let mut theirs = Vec::new();
    let mut total = 0usize;
    for (i, w) in worlds.iter().enumerate() {
        if domain.phase(&w.position)? != Phase::Turn || w.position.suspension.is_some() {
            return Err(Error(format!(
                "world {}: root must be a normal simultaneous turn",
                w.id
            )));
        }
        if visible(&w.position.state, us, knowledge)? != reference {
            return Err(Error(format!(
                "world {}: known state differs across the information set",
                w.id
            )));
        }
        let rows = domain.actions(&w.position, 0)?;
        if i == 0 {
            ours = rows;
        } else if rows != ours {
            return Err(Error(format!(
                "world {}: our shared action identities differ",
                w.id
            )));
        }
        let columns = domain.actions(&w.position, 1)?;
        let count = ours
            .len()
            .checked_mul(columns.len())
            .ok_or_else(|| Error("cell count overflow".into()))?;
        total = total
            .checked_add(count)
            .filter(|n| *n <= max_cells)
            .ok_or_else(|| {
                Error(format!(
                    "one-turn game exceeds max_cells={max_cells}; no partial matrix returned"
                ))
            })?;
        theirs.push(columns);
    }
    let mut matrices = Vec::new();
    let mut outcomes = 0usize;
    for (w, columns) in worlds.iter().zip(&theirs) {
        let mut payoffs = Vec::with_capacity(ours.len() * columns.len());
        for row in &ours {
            for column in columns {
                let children = domain
                    .transitions(&w.position, [row, column])
                    .map_err(|e| Error(format!("world {}: {e}", w.id)))?;
                let mut mass = 0.0;
                let mut value = 0.0;
                for (p, child) in &children {
                    if !p.is_finite() || *p < 0.0 {
                        return Err(Error("invalid chance probability".into()));
                    }
                    if domain.phase(child)? == Phase::Switch {
                        return Err(Error(format!("world {}: intermediate switch requires a belief-aware continuation; no world was dropped", w.id)));
                    }
                    let v = f64::from(domain.value(child));
                    if !v.is_finite() {
                        return Err(Error("nonfinite leaf evaluation".into()));
                    }
                    mass += p;
                    value += p * v;
                }
                if (mass - 1.0).abs() > 1e-8 {
                    return Err(Error(format!(
                        "world {}: chance mass {mass} is not one",
                        w.id
                    )));
                }
                outcomes = outcomes
                    .checked_add(children.len())
                    .ok_or_else(|| Error("outcome count overflow".into()))?;
                payoffs.push(value);
            }
        }
        matrices.push(World {
            id: w.id.clone(),
            weight: w.weight,
            columns: columns.iter().map(|a| format!("{a:?}")).collect(),
            payoffs,
        });
    }
    let game = Game::new(ours.iter().map(|a| format!("{a:?}")).collect(), matrices)?;
    Ok(BuiltGame {
        game,
        ours,
        theirs,
        transitions: total,
        outcomes,
    })
}
