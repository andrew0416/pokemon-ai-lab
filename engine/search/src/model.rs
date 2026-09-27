//! Opponent models ② and ③ over scenario files (DESIGN.md "모델 ③·② 구현"), the position a
//! search starts from, and the child-position dump for evaluator fitting — the library side of
//! `lab-plan` (board PY3a: the binary only parses arguments and prints).
//!
//! - Model ③ (fixed belief): [`believed_analysis`] solves the matrix game on each team the
//!   opponent believes we have ([`believed_scenario`]: the scenario with our team file
//!   replaced), mixes their equilibrium strategies by weight and values our choices against the
//!   mixture on the real position ([`matrix_best_response`]).
//! - Model ② (belief updated by observation): the setup turns are replayed and filtered by what
//!   the opponent saw after each of them ([`observed_positions`]); each believed team's weight
//!   is multiplied by the likelihood of the observations under its own replay; the positions the
//!   observations cannot tell apart form one matrix game ([`mixed_over_positions`]).

use serde_json::Value;

use lab_engine::eval::{
    features, Evaluator, Heuristic, Material, Weighted, FEATURE_COUNT, FEATURE_NAMES,
};
use lab_engine::state::SideId;
use lab_engine::turn::EnumerateOptions;
use lab_scenario::{
    canonical_json, scenario_positions_consistent, scenario_positions_filtered, LoadedScenario,
    Position,
};

use crate::game::{asked_slots, Decision};
use crate::nash::{self, Matrix};
use crate::solve::{DeepAnalysis, MixedAnalysis, Solver};
use crate::{format_choice, format_switches, Choice};

/// What the opponent saw of our side after a setup turn: our Pokémon's HP percentages by name
/// (a fainted Pokémon is 0).
pub type Observation = Vec<(String, f32)>;

/// Per observed setup turn: `(turn, matched, total, share)`, see [`observed_positions`].
pub type ObservationTrace = Vec<(usize, usize, usize, f64)>;

/// A leaf evaluator by name: `material`, `heuristic` or `file:<weights.json>` (the weights
/// `engine/scripts/fit_eval.py` writes, one per [`FEATURE_NAMES`] entry).
pub fn load_evaluator(spec: &str) -> Result<Box<dyn Evaluator<2> + Sync>, String> {
    if spec == "material" {
        Ok(Box::new(Material))
    } else if let Some(path) = spec.strip_prefix("file:") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut weights = [0.0f32; FEATURE_COUNT];
        for (i, name) in FEATURE_NAMES.iter().enumerate() {
            weights[i] = value["weights"][*name]
                .as_f64()
                .ok_or_else(|| format!("{path}: weights.{name} missing"))?
                as f32;
        }
        Ok(Box::new(Weighted { weights }))
    } else if spec == "heuristic" {
        Ok(Box::new(Heuristic))
    } else {
        Err("--eval needs material, heuristic or file:<weights.json>".into())
    }
}

/// `Name:pct,Name:pct`: what the opponent saw of our side's Pokémon after a setup turn.
pub fn parse_observation(text: &str) -> Result<Observation, String> {
    text.split(',')
        .map(|part| {
            let (name, pct) = part
                .trim()
                .rsplit_once(':')
                .ok_or_else(|| format!("--observed: {part:?} is not Name:pct"))?;
            let pct: f32 = pct
                .trim()
                .trim_end_matches('%')
                .parse()
                .map_err(|_| format!("--observed: {pct:?} is not a percentage"))?;
            Ok((name.trim().to_owned(), pct))
        })
        .collect()
}

/// Sorts `(setup turn, observation)` pairs by turn and checks that each belongs to one of the
/// scenario's `setup_turns` turns, one per turn.
pub fn check_observations(
    observations: &mut [(usize, Observation)],
    setup_turns: usize,
) -> Result<(), String> {
    observations.sort_by_key(|(turn, _)| *turn);
    if let Some(pair) = observations.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(format!(
            "two observations for setup turn {}; give one --observed-turn per turn",
            pair[0].0
        ));
    }
    if let Some((turn, _)) = observations
        .iter()
        .find(|(turn, _)| *turn == 0 || *turn > setup_turns)
    {
        return Err(format!(
            "--observed-turn {turn}: the scenario has {setup_turns} setup turns (an observation belongs to one of them)"
        ));
    }
    Ok(())
}

/// Whose teams a replay of the setup turns runs on: the real ones, where a choice that is not
/// legal is a scenario error, or a believed team's, where it contradicts the belief in that
/// position and the position is dropped (`scenario_positions_consistent`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replay {
    Real,
    Believed,
}

/// The positions after the setup turns that survive every observation, each applied as soon as
/// its turn is played, with one `(turn, matched, total, share)` per observed turn: how many of
/// the positions reaching that turn matched and the share of their probability they hold. The
/// total probability of the result is the likelihood of the observations under this scenario's
/// teams.
pub fn observed_positions(
    loaded: &LoadedScenario,
    us: SideId,
    observations: &[(usize, Observation)],
    tolerance: f32,
    setup_options: EnumerateOptions,
    replay: Replay,
) -> Result<(Vec<Position>, ObservationTrace), String> {
    let mut trace = Vec::new();
    let replay_fn = match replay {
        Replay::Real => scenario_positions_filtered,
        Replay::Believed => scenario_positions_consistent,
    };
    let positions = replay_fn(loaded, setup_options, &mut |turn, positions| {
        let Some((_, observation)) = observations.iter().find(|(t, _)| *t == turn) else {
            return positions;
        };
        let total = positions.len();
        let reaching: f64 = positions.iter().map(|p| p.probability).sum();
        let matching = matching_positions(loaded, positions, us, observation, tolerance);
        let kept: f64 = matching.iter().map(|p| p.probability).sum();
        let share = if reaching > 0.0 { kept / reaching } else { 0.0 };
        trace.push((turn, matching.len(), total, share));
        matching
    })?;
    Ok((positions, trace))
}

/// The start positions of a search on `loaded` under opponent model ②: the setup turns
/// replayed (on the real teams; `lenient` drops branches in which a recorded choice is not
/// legal instead of failing) and filtered by `observations`. Returns the positions (most
/// probable first when there are observations), the per-turn trace and, when several survive
/// and `open` (no `--position`), all of them as the mixture's survivors with only the most
/// probable left in the first vector. Errors when nothing survives.
pub fn observed_start(
    loaded: &LoadedScenario,
    us: SideId,
    observations: &[(usize, Observation)],
    tolerance: f32,
    setup_options: EnumerateOptions,
    lenient: bool,
    open: bool,
) -> Result<(Vec<Position>, ObservationTrace, Vec<Position>), String> {
    let (mut positions, trace) = observed_positions(
        loaded,
        us,
        observations,
        tolerance,
        setup_options,
        if lenient {
            Replay::Believed
        } else {
            Replay::Real
        },
    )?;
    if positions.is_empty() {
        return Err("no position survives the setup turns (with --setup-lenient every replayed branch made a recorded choice illegal)".into());
    }
    if !observations.is_empty() {
        positions.sort_by(|a, b| b.probability.total_cmp(&a.probability));
    }
    // The positions the observations cannot tell apart: `--solve nash` and the believed-team
    // analysis play the matrix game over their mixture (where the choice lists coincide); the
    // other modes take the most probable one.
    let survivors = if !observations.is_empty() && positions.len() > 1 && open {
        let keep = positions.clone();
        positions.truncate(1);
        keep
    } else {
        Vec::new()
    };
    Ok((positions, trace, survivors))
}

/// The positions whose HP percentages of our named Pokémon match the observation within
/// `tolerance` points (a fainted Pokémon is 0%).
pub fn matching_positions(
    loaded: &LoadedScenario,
    positions: Vec<Position>,
    us: SideId,
    observation: &[(String, f32)],
    tolerance: f32,
) -> Vec<Position> {
    let meta = &loaded.meta.sides[us.index()];
    positions
        .into_iter()
        .filter(|p| {
            let side = p.state.side(us);
            observation.iter().all(|(name, pct)| {
                let Some(party) = meta.party_index(name) else {
                    return false;
                };
                let mon = &side.party[party as usize];
                let actual = if mon.hp <= 0 {
                    0.0
                } else {
                    100.0 * f32::from(mon.hp) / f32::from(mon.max_hp.max(1))
                };
                (actual - pct).abs() <= tolerance
            })
        })
        .collect()
}

/// The scenario with our side's team file replaced by `team_path`, loaded (the callers replay
/// its setup turns under the observations and pick).
pub fn believed_scenario(
    scenario: &str,
    us: SideId,
    team_path: &str,
) -> Result<LoadedScenario, String> {
    let text = std::fs::read_to_string(scenario).map_err(|e| format!("{scenario}: {e}"))?;
    let mut json: Value = serde_json::from_str(&text).map_err(|e| format!("{scenario}: {e}"))?;
    let side = side_name(us);
    let team_abs = std::fs::canonicalize(team_path).map_err(|e| format!("{team_path}: {e}"))?;
    json[side]["team"] = Value::String(
        team_abs
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned(),
    );
    if let Some(desc) = json["description"].as_str() {
        json["description"] = Value::String(format!("{desc} [believed {side} team: {team_path}]"));
    }
    let base_dir = std::path::Path::new(scenario)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    lab_scenario::load_scenario_str(&json.to_string(), base_dir)
        .map_err(|e| format!("believed scenario: {e}"))
}

/// Which of several start positions a search runs on.
#[derive(Clone, Debug, Default)]
pub struct PositionPick {
    /// `--position i`.
    pub index: Option<usize>,
    /// `--position max`: the most probable one.
    pub most_probable: bool,
    /// `--before <oracle report>`: the one whose canonical JSON is the report's `before`.
    pub before: Option<String>,
}

impl PositionPick {
    /// Neither an index nor `max`: a caller may play the mixture of the survivors.
    pub fn is_open(&self) -> bool {
        self.index.is_none() && !self.most_probable
    }
}

/// The position `pick` selects; with several and no selector, an error listing them.
pub fn pick_position(
    loaded: &LoadedScenario,
    positions: Vec<Position>,
    pick: &PositionPick,
    us: SideId,
) -> Result<Position, String> {
    if pick.most_probable {
        return positions
            .into_iter()
            .max_by(|a, b| a.probability.total_cmp(&b.probability))
            .ok_or_else(|| "no initial state".to_owned());
    }
    if let Some(i) = pick.index {
        let n = positions.len();
        return positions
            .into_iter()
            .nth(i)
            .ok_or_else(|| format!("--position {i}: only {n} initial states"));
    }
    let wanted = match &pick.before {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let report: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            Some(report["before"].clone())
        }
        None => None,
    };
    match wanted {
        None if positions.len() == 1 => Ok(positions.into_iter().next().unwrap()),
        None => {
            // Several start states (Trace, Speed ties among the leads): list them.
            let mut text = format!(
                "{} initial states; pass --position <index> or --before <oracle report>:
",
                positions.len()
            );
            for (i, p) in positions.iter().enumerate() {
                let actives: Vec<String> = [SideId::One, SideId::Two]
                    .into_iter()
                    .flat_map(|side| {
                        let meta = &loaded.meta.sides[side.index()];
                        p.state.side(side).slots.iter().filter_map(move |slot| {
                            let party = slot.party_index?;
                            let mon = &p.state.side(side).party[party as usize];
                            Some(format!(
                                "{} ({})",
                                meta.name(party).unwrap_or("?"),
                                mon.ability.data().name
                            ))
                        })
                    })
                    .collect();
                let hp: Vec<String> = {
                    let meta = &loaded.meta.sides[us.index()];
                    let side = p.state.side(us);
                    side.slots
                        .iter()
                        .filter_map(|slot| slot.party_index)
                        .map(|party| {
                            let mon = &side.party[party as usize];
                            format!(
                                "{}:{:.1}",
                                meta.name(party).unwrap_or("?"),
                                100.0 * f32::from(mon.hp) / f32::from(mon.max_hp.max(1))
                            )
                        })
                        .collect()
                };
                text.push_str(&format!(
                    "  {i}: p={:.4} {} | our HP% {}\n",
                    p.probability,
                    actives.join(", "),
                    hp.join(",")
                ));
            }
            Err(text)
        }
        Some(w) => positions
            .into_iter()
            .find(|p| {
                canonical_json(&p.state, &loaded.meta)
                    .ok()
                    .and_then(|key| serde_json::from_str::<Value>(&key).ok())
                    .is_some_and(|v| v == w)
            })
            .ok_or_else(|| "no initial state matches the report's `before`".to_owned()),
    }
}

/// `p1` / `p2`.
pub fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

/// A choice as a Showdown choice string against the position's party order (replacements as
/// `switch N` per asked slot, `(waits)` when the decision asks nothing of the side).
pub fn describe(
    position: &Position,
    decision: Decision,
    side: SideId,
    choice: &Choice<2>,
) -> String {
    let order = &position.order[side.index()];
    match choice {
        Choice::Turn(action) => format_choice(&position.state, side, order, action),
        Choice::Switches(switches) => {
            let slots = asked_slots(&position.state, decision, side);
            if slots.is_empty() {
                "(waits)".to_owned()
            } else {
                format_switches(order, &slots, switches)
            }
        }
    }
}

/// The one-turn matrix game of `single`, or, when `survivors` (positions the observations
/// cannot tell apart, `single` the most probable of them) has several, of their mixture; falls
/// back to `single` when the mixture is not well defined. The note (`None` for a single
/// position) says which it was, as `lab-plan` prints it.
pub fn analyse_positions<E: Evaluator<2> + ?Sized + Sync>(
    solver: &mut Solver<'_, 2, E>,
    single: &Position,
    survivors: &[Position],
    label: &str,
) -> Result<(MixedAnalysis<2>, Option<String>), String> {
    let mut note = None;
    if survivors.len() > 1 {
        match mixed_over_positions(solver, survivors)? {
            Ok(mixed) => {
                let note = format!(
                    "{label}: matrix game over the mixture of {} positions (total p={:.4})",
                    survivors.len(),
                    survivors.iter().map(|p| p.probability).sum::<f64>()
                );
                return Ok((mixed, Some(note)));
            }
            Err(why) => {
                note = Some(format!(
                    "{label}: {why}; using the most probable position alone"
                ))
            }
        }
    }
    let mut state = single.state.clone();
    let mixed = solver
        .analyse_mixed(&mut state, None)
        .map_err(|e| e.to_string())?;
    Ok((mixed, note))
}

/// Several positions the players cannot tell apart (chance the observations do not reveal:
/// sleep turns, hidden damage rolls) form one matrix game: the probability-weighted average of
/// their payoff matrices, over choice lists that must coincide (the same decision, the same
/// choices kept after dropping unsupported pairs). The inner `Err(why)` says when they do not.
pub fn mixed_over_positions<E: Evaluator<2> + ?Sized + Sync>(
    solver: &mut Solver<'_, 2, E>,
    positions: &[Position],
) -> Result<Result<MixedAnalysis<2>, String>, String> {
    let mut parts: Vec<(f64, MixedAnalysis<2>)> = Vec::with_capacity(positions.len());
    for p in positions {
        let mut state = p.state.clone();
        let analysis = solver
            .analyse_mixed(&mut state, None)
            .map_err(|e| e.to_string())?;
        parts.push((p.probability, analysis));
    }
    let first = &parts[0].1;
    for (_, m) in &parts[1..] {
        if m.decision != first.decision {
            return Ok(Err("the surviving positions ask different decisions".into()));
        }
        if m.ours != first.ours || m.theirs != first.theirs {
            return Ok(Err(
                "the surviving positions keep different choice lists (different legal moves or unsupported pairs)".into(),
            ));
        }
    }
    let total: f64 = parts.iter().map(|(p, _)| *p).sum();
    let mut values = vec![0.0f32; first.matrix.values.len()];
    for (p, m) in &parts {
        let w = (*p / total) as f32;
        for (v, x) in values.iter_mut().zip(&m.matrix.values) {
            *v += w * x;
        }
    }
    let matrix = Matrix::new(first.matrix.rows, first.matrix.cols, values);
    let equilibrium = nash::solve(&matrix, 20_000, 0.01);
    let maximin = matrix.maximin();
    let nodes = parts.iter().map(|(_, m)| m.nodes).sum();
    let turns = parts.iter().map(|(_, m)| m.turns).sum();
    let elapsed = parts.iter().map(|(_, m)| m.elapsed).sum();
    let mut unsupported: Vec<String> = parts
        .iter()
        .flat_map(|(_, m)| m.unsupported.iter().cloned())
        .collect();
    unsupported.sort();
    unsupported.dedup();
    let first = parts.swap_remove(0).1;
    Ok(Ok(MixedAnalysis {
        matrix,
        equilibrium,
        maximin,
        nodes,
        turns,
        elapsed,
        unsupported,
        ..first
    }))
}

/// Our choices' expected values on `analysis`'s matrix against their mixed `strategy` (choices
/// of theirs the matrix does not keep are dropped and the rest renormalised), best first.
pub fn matrix_best_response(
    analysis: &MixedAnalysis<2>,
    strategy: &[(Choice<2>, f32)],
) -> Vec<(Choice<2>, f32)> {
    let mut weights = vec![0.0f32; analysis.theirs.len()];
    for (choice, p) in strategy {
        if let Some(i) = analysis.theirs.iter().position(|x| x == choice) {
            weights[i] += p;
        }
    }
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut lines: Vec<(Choice<2>, f32)> = analysis
        .ours
        .iter()
        .enumerate()
        .map(|(r, choice)| {
            let value: f32 = weights
                .iter()
                .enumerate()
                .map(|(c, w)| w / total * analysis.matrix.at(r, c))
                .sum();
            (*choice, value)
        })
        .collect();
    lines.sort_by(|a, b| b.1.total_cmp(&a.1));
    lines
}

/// How the setup turns are replayed and observed for [`believed_analysis`].
#[derive(Clone, Debug)]
pub struct BeliefSetup {
    /// `(setup turn, observation)`, checked by [`check_observations`].
    pub observations: Vec<(usize, Observation)>,
    pub tolerance: f32,
    pub setup_options: EnumerateOptions,
    pub pick: PositionPick,
}

/// One believed team in a [`BelievedReport`].
#[derive(Clone, Debug)]
pub struct BelievedTeam {
    pub team: String,
    /// The weight given (normalised).
    pub prior: f32,
    /// After the observations (the prior when there are none; 0 when contradicted).
    pub posterior: f32,
    /// The equilibrium value on the believed position, from our side (NaN when contradicted).
    pub value: f32,
}

/// [`believed_analysis`]'s result.
#[derive(Clone, Debug)]
pub struct BelievedReport {
    /// Messages in the order the analysis met them (mixtures of surviving positions).
    pub notes: Vec<String>,
    pub teams: Vec<BelievedTeam>,
    /// Their strategy: the believed equilibria mixed by posterior weight.
    pub mixture: Vec<(Choice<2>, f32)>,
    /// The first believed position (their choices are described against it).
    pub believed: Position,
    pub believed_decision: Decision,
    /// The real position's analysis.
    pub real: MixedAnalysis<2>,
    /// Our best responses to `mixture` on the real matrix, best first.
    pub response: Vec<(Choice<2>, f32)>,
    /// Time of the real analysis and the response.
    pub response_elapsed: std::time::Duration,
    /// Unsupported reasons met on the believed positions.
    pub dropped: Vec<String>,
}

/// Opponent model ③ with a belief (and ② when `setup.observations` is not empty): for each
/// believed team (the scenario with our side's team file replaced) the opponent's equilibrium
/// strategy; the mixture by weight is what it plays; our best response is valued on the real
/// `position` (`survivors`: the real positions the observations cannot tell apart, see
/// [`analyse_positions`]). With observations, each believed team's weight is multiplied by the
/// probability that its own replay of the setup turns produced all of them (Bayes with the
/// engine as the likelihood); the believed position is then the most probable surviving one.
#[allow(clippy::too_many_arguments)]
pub fn believed_analysis<E: Evaluator<2> + ?Sized + Sync>(
    solver: &mut Solver<'_, 2, E>,
    scenario: &str,
    us: SideId,
    position: &Position,
    survivors: &[Position],
    believed_teams: &[String],
    believed_weights: &[f32],
    setup: &BeliefSetup,
) -> Result<BelievedReport, String> {
    let weights: Vec<f32> = if believed_weights.is_empty() {
        vec![1.0 / believed_teams.len() as f32; believed_teams.len()]
    } else if believed_weights.len() == believed_teams.len() {
        let total: f32 = believed_weights.iter().sum();
        if total <= 0.0 {
            return Err("--believed-weight must sum to a positive number".into());
        }
        believed_weights.iter().map(|w| w / total).collect()
    } else {
        return Err("--believed-weight needs one weight per --believed-team".into());
    };
    let observations = &setup.observations;
    let mut notes = Vec::new();
    let mut mixture: Vec<(Choice<2>, f32)> = Vec::new();
    let mut teams = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    let mut reference: Option<(Vec<Choice<2>>, Position, Decision)> = None;
    let mut posterior = weights.clone();
    let mut believed_positions = Vec::with_capacity(believed_teams.len());
    for (k, team_path) in believed_teams.iter().enumerate() {
        let bl = believed_scenario(scenario, us, team_path)?;
        let (mut bpositions, _) = observed_positions(
            &bl,
            us,
            observations,
            setup.tolerance,
            setup.setup_options,
            Replay::Believed,
        )?;
        let mut bsurvivors: Vec<Position> = Vec::new();
        if !observations.is_empty() {
            let likelihood: f64 = bpositions.iter().map(|p| p.probability).sum();
            posterior[k] *= likelihood as f32;
            if bpositions.is_empty() {
                believed_positions.push(None);
                continue;
            }
            bpositions.sort_by(|a, b| b.probability.total_cmp(&a.probability));
            if bpositions.len() > 1 && setup.pick.is_open() {
                bsurvivors = bpositions.clone();
            }
            bpositions.truncate(1);
        }
        let bp = pick_position(&bl, bpositions, &setup.pick, us)?;
        believed_positions.push(Some((bp, bsurvivors)));
    }
    let total: f32 = posterior.iter().sum();
    if total <= 0.0 {
        return Err("every believed team is contradicted by the observations".into());
    }
    for w in &mut posterior {
        *w /= total;
    }
    for (k, team_path) in believed_teams.iter().enumerate() {
        let w = posterior[k];
        let Some((believed, bsurvivors)) = believed_positions[k].clone() else {
            teams.push(BelievedTeam {
                team: team_path.clone(),
                prior: weights[k],
                posterior: w,
                value: f32::NAN,
            });
            continue;
        };
        let (mixed, note) = analyse_positions(
            solver,
            &believed,
            &bsurvivors,
            &format!("believed team {team_path}"),
        )
        .map_err(|e| format!("believed position {team_path}: {e}"))?;
        notes.extend(note);
        teams.push(BelievedTeam {
            team: team_path.clone(),
            prior: weights[k],
            posterior: w,
            value: mixed.equilibrium.value,
        });
        dropped.extend(mixed.unsupported.iter().cloned());
        match &reference {
            None => reference = Some((mixed.theirs.clone(), believed.clone(), mixed.decision)),
            Some((theirs, _, _)) if *theirs != mixed.theirs => {
                return Err(format!(
                    "believed team {team_path}: the opponent's choice list differs from the first believed team's (different species, moves or order); the beliefs must share it"
                ));
            }
            Some(_) => {}
        }
        for (c, &p) in mixed.theirs.iter().zip(&mixed.equilibrium.cols) {
            match mixture.iter_mut().find(|(x, _)| x == c) {
                Some(entry) => entry.1 += w * p,
                None => mixture.push((*c, w * p)),
            }
        }
    }
    let (_, believed, believed_decision) = reference.expect("at least one believed team");
    let started = std::time::Instant::now();
    let (real, note) = analyse_positions(solver, position, survivors, "real position")?;
    notes.extend(note);
    // Our best response to their mixed strategy, read off the (mixed) real matrix: their
    // choices the real position does not keep are dropped and the rest renormalised.
    let response = matrix_best_response(&real, &mixture);
    if response.is_empty() {
        return Err("none of their strategy's choices is kept on the real position".into());
    }
    Ok(BelievedReport {
        notes,
        teams,
        mixture,
        believed,
        believed_decision,
        real,
        response,
        response_elapsed: started.elapsed(),
        dropped,
    })
}

/// One row of [`dump_children`]: a position one turn ahead with its next-turn equilibrium.
#[derive(Clone, Debug)]
pub struct ChildRow {
    /// `lab_engine::eval::features`, signed from our side.
    pub features: Vec<f32>,
    /// The child's next-turn equilibrium value from our side.
    pub target: f32,
    /// The outcome's probability (after the outcome cap).
    pub probability: f64,
    pub ours: Choice<2>,
    pub theirs: Choice<2>,
}

/// Feature vectors of the positions one turn ahead, each with its next-turn equilibrium value
/// as the fitting target (WORKPLAN S12): our `beam` best choices by the root matrix, their
/// `beam` worst replies, the `Config::outcome_cap` most probable outcomes of each pair (the
/// deep analysis' children, cached). `state` is left unchanged.
pub fn dump_children<E: Evaluator<2> + ?Sized + Sync>(
    solver: &mut Solver<'_, 2, E>,
    state: &mut lab_engine::state::State<2>,
    beam: usize,
) -> Result<(DeepAnalysis<2>, Vec<ChildRow>), String> {
    let config = solver.config;
    let us = config.us;
    let deep = solver
        .analyse_deep(state, None, beam)
        .map_err(|e| e.to_string())?;
    let decision = deep.decision;
    let mut rows = Vec::new();
    // Only the beam's replies: their children were just valued by `analyse_deep`, so the
    // targets come from the solver's cache instead of hundreds of fresh matrix games.
    for line in &deep.lines {
        for &(b, _) in &line.replies {
            let pair = match us {
                SideId::One => [line.ours, b],
                SideId::Two => [b, line.ours],
            };
            let Ok(mut outcomes) = crate::transitions(
                state,
                config.ruleset,
                config.enumerate_options(),
                decision,
                None,
                pair,
            ) else {
                continue;
            };
            outcomes.sort_by(|x, y| y.probability.total_cmp(&x.probability));
            if let Some(cap) = config.outcome_cap {
                outcomes.truncate(cap);
            }
            for o in &outcomes {
                state.apply(&o.instructions);
                let target = solver.nash_value(state, o.suspension.as_ref());
                let f = features(state);
                let sign = if us == SideId::One { 1.0 } else { -1.0 };
                state.reverse(&o.instructions);
                let Ok(target) = target else { continue };
                if target.is_nan() {
                    continue;
                }
                rows.push(ChildRow {
                    features: f.iter().map(|x| x * sign).collect(),
                    target,
                    probability: o.probability,
                    ours: line.ours,
                    theirs: b,
                });
            }
        }
    }
    Ok((deep, rows))
}
