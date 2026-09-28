//! Every search mode as a JSON report (board PY3b, the Python API's `Position.maximin`,
//! `nash`, `deep`, `deep_nash`, `plan` and `Scenario.believed`, `rollout`): the analysis with
//! its choices written as Showdown choice strings against the node's party order, its values
//! (from the searching side; `null` where a value is NaN, i.e. a pair never valued or refused),
//! the work counters ([`SearchStats`]) and the refusals met. The reports are plain JSON, so a
//! caller can store them as they are; `lab-plan` prints the same analyses as text.
//!
//! Values are evaluator scores (HP bar = 100, `WIN` for a won battle), not win rates.

use serde_json::{json, Value};

use lab_engine::eval::Evaluator;
use lab_engine::state::SideId;
use lab_engine::turn::EnumerateOptions;
use lab_scenario::{load_scenario_file, LoadedScenario, Position};

use crate::model::{
    analyse_positions, believed_analysis, check_observations, describe, observed_start,
    parse_observation, pick_position, side_name, BeliefSetup, Observation, PositionPick,
};
use crate::nash::{Equilibrium, Matrix};
use crate::node::{decision_name, Node, NodeError};
use crate::rollout::{run_games, team_files, wilson, winner_label, Ending, RolloutSettings};
use crate::solve::{Chance, Config, DeepLevel, MixedAnalysis, SearchStats, Solver};
use crate::{Choice, Decision};

/// A value as its shortest decimal form (`0.1`, not `0.10000000149011612`), `null` when it is
/// NaN or infinite.
fn num(v: f32) -> Value {
    if v.is_finite() {
        json!(v.to_string().parse::<f64>().unwrap_or(f64::from(v)))
    } else {
        Value::Null
    }
}

fn matrix_json(m: &Matrix) -> Value {
    Value::Array(
        (0..m.rows)
            .map(|r| Value::Array((0..m.cols).map(|c| num(m.at(r, c))).collect()))
            .collect(),
    )
}

fn probabilities(p: &[f32]) -> Value {
    Value::Array(p.iter().map(|&x| num(x)).collect())
}

/// The work counters of the solver's last analysis.
pub fn stats_json(s: &SearchStats) -> Value {
    json!({
        "tt_hits": s.tt_hits,
        "tt_misses": s.tt_misses,
        "nash_solves": s.nash_solves,
        "nash_iterations": s.nash_iterations,
        "nash_seconds": s.nash_seconds,
        "enumerate_seconds": s.enumerate_seconds,
        "deep_tt_hits": s.deep_tt_hits,
        "deep_tt_misses": s.deep_tt_misses,
        "split_cells": s.split_cells,
    })
}

/// The search settings a report was made with.
pub fn config_json(config: &Config) -> Value {
    json!({
        "side": side_name(config.us),
        "depth": config.depth,
        "chance": match config.chance { Chance::Expect => "expect", Chance::Worst => "worst" },
        "rolls": format!("{:?}", config.rolls),
        "pruning": format!("{:?}", config.pruning),
        "threads": config.threads,
        "transposition": config.transposition,
        "dominance": config.dominance,
        "double_oracle": config.double_oracle,
    })
}

fn equilibrium_json(eq: &Equilibrium) -> Value {
    json!({
        "rows": probabilities(&eq.rows),
        "cols": probabilities(&eq.cols),
        "value": num(eq.value),
        "exploitability": num(eq.exploitability),
        "iterations": eq.iterations,
    })
}

/// Choices with probability at least `min`, most likely first, as `[choice, p]` pairs.
fn support_json(choices: &[String], p: &[f32], min: f32) -> Value {
    let mut out: Vec<(&String, f32)> = choices
        .iter()
        .zip(p)
        .filter(|(_, &x)| x >= min)
        .map(|(c, &x)| (c, x))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    Value::Array(out.into_iter().map(|(c, x)| json!([c, x])).collect())
}

fn texts<const N: usize>(
    node: &Node<N>,
    decision: Decision,
    side: SideId,
    choices: &[Choice<N>],
) -> Vec<String> {
    choices
        .iter()
        .map(|c| node.describe(decision, side, c))
        .collect()
}

/// A matrix-game analysis (`nash`, the shallow game of `deep_nash`).
fn mixed_json<const N: usize>(node: &Node<N>, us: SideId, m: &MixedAnalysis<N>) -> Value {
    let ours = texts(node, m.decision, us, &m.ours);
    let theirs = texts(node, m.decision, us.other(), &m.theirs);
    json!({
        "decision": decision_name(m.decision),
        "ours": ours,
        "theirs": theirs,
        "matrix": matrix_json(&m.matrix),
        "equilibrium": equilibrium_json(&m.equilibrium),
        "value": num(m.equilibrium.value),
        "our_strategy": support_json(&ours, &m.equilibrium.rows, 0.01),
        "their_strategy": support_json(&theirs, &m.equilibrium.cols, 0.01),
        "maximin": {"row": m.maximin.0, "value": num(m.maximin.1)},
        "depth": m.depth,
        "nodes": m.nodes,
        "turns": m.turns,
        "elapsed_s": m.elapsed.as_secs_f64(),
        "unsupported": m.unsupported,
        "omitted_ours": m.omitted_ours,
        "omitted_theirs": m.omitted_theirs,
    })
}

/// Depth-limited maximin (`lab-plan --solve maximin`): every one of our choices with its value
/// against the reply that hurts us most (an upper bound when `exact` is false: the line was cut
/// off once it fell below the best, unless `config.exact_lines`).
pub fn maximin<const N: usize, E: Evaluator<N> + ?Sized + Sync>(
    node: &Node<N>,
    config: Config,
    evaluator: &E,
) -> Result<Value, NodeError> {
    let mut solver = Solver::new(config, evaluator);
    let mut state = node.state.clone();
    let a = solver.analyse(&mut state, node.suspension.as_ref())?;
    let us = config.us;
    let lines: Vec<Value> = a
        .lines
        .iter()
        .map(|l| {
            json!({
                "ours": node.describe(a.decision, us, &l.ours),
                "value": num(l.value),
                "exact": l.exact,
                "reply": l.reply.map(|r| node.describe(a.decision, us.other(), &r)),
            })
        })
        .collect();
    Ok(json!({
        "mode": "maximin",
        "config": config_json(&config),
        "decision": decision_name(a.decision),
        "value": num(a.value),
        "lines": lines,
        "depth": a.depth,
        "nodes": a.nodes,
        "turns": a.turns,
        "elapsed_s": a.elapsed.as_secs_f64(),
        "unsupported": a.unsupported,
        "omitted_pairs": a.omitted_pairs,
        "stats": stats_json(&solver.stats()),
    }))
}

/// The one-turn matrix game (`lab-plan --solve nash`; `lazy`: its root by double oracle,
/// `--lazy`, whose matrix has `null` for the pairs never valued).
pub fn nash<const N: usize, E: Evaluator<N> + ?Sized + Sync>(
    node: &Node<N>,
    config: Config,
    evaluator: &E,
    lazy: bool,
) -> Result<Value, NodeError> {
    let mut solver = Solver::new(config, evaluator);
    let mut state = node.state.clone();
    let m = if lazy {
        solver.analyse_mixed_lazy(&mut state, node.suspension.as_ref())?
    } else {
        solver.analyse_mixed(&mut state, node.suspension.as_ref())?
    };
    let mut out = mixed_json(node, config.us, &m);
    out["mode"] = json!(if lazy { "nash-lazy" } else { "nash" });
    out["config"] = config_json(&config);
    out["stats"] = stats_json(&solver.stats());
    Ok(out)
}

/// Two turns deep against the worst replies (`lab-plan --solve deep`): our `beam` best choices
/// by the one-turn maximin, each against its `beam` worst replies with the children worth
/// their next-turn equilibrium (`config.outcome_cap` outcomes).
pub fn deep<const N: usize, E: Evaluator<N> + ?Sized + Sync>(
    node: &Node<N>,
    config: Config,
    evaluator: &E,
    beam: usize,
) -> Result<Value, NodeError> {
    let mut solver = Solver::new(config, evaluator);
    let mut state = node.state.clone();
    let d = solver.analyse_deep(&mut state, node.suspension.as_ref(), beam)?;
    let us = config.us;
    let lines: Vec<Value> = d
        .lines
        .iter()
        .map(|l| {
            json!({
                "ours": node.describe(d.decision, us, &l.ours),
                "deep": num(l.deep),
                "shallow": num(l.shallow),
                "replies": l.replies.iter().map(|(b, v)| json!([node.describe(d.decision, us.other(), b), num(*v)])).collect::<Vec<_>>(),
            })
        })
        .collect();
    let rest: Vec<Value> = d
        .shallow_rest
        .iter()
        .map(|(c, v)| json!([node.describe(d.decision, us, c), num(*v)]))
        .collect();
    Ok(json!({
        "mode": "deep",
        "config": config_json(&config),
        "decision": decision_name(d.decision),
        "value": d.lines.first().map_or(Value::Null, |l| num(l.deep)),
        "beam": d.beam,
        "outcomes": d.outcome_cap,
        "lines": lines,
        "shallow_rest": rest,
        "nodes": d.nodes,
        "turns": d.turns,
        "elapsed_s": d.elapsed.as_secs_f64(),
        "unsupported": d.unsupported,
        "omitted_pairs": d.omitted_pairs,
        "stats": stats_json(&solver.stats()),
    }))
}

/// The deep mixed equilibrium over `levels.len() + 1` turns (`lab-plan --solve deep-nash`,
/// [`Solver::analyse_deep_mixed_levels`]; one level is depth 2, two are depth 3).
pub fn deep_nash<const N: usize, E: Evaluator<N> + ?Sized + Sync>(
    node: &Node<N>,
    config: Config,
    evaluator: &E,
    levels: &[DeepLevel],
) -> Result<Value, NodeError> {
    let mut solver = Solver::new(config, evaluator);
    let mut state = node.state.clone();
    let d = solver.analyse_deep_mixed_levels(&mut state, node.suspension.as_ref(), levels)?;
    let us = config.us;
    let ours = texts(node, d.decision, us, &d.ours);
    let theirs = texts(node, d.decision, us.other(), &d.theirs);
    Ok(json!({
        "mode": "deep-nash",
        "config": config_json(&config),
        "decision": decision_name(d.decision),
        "depth": d.levels.len() + 1,
        "levels": d.levels.iter().map(|l| json!({"beam": l.beam, "outcomes": l.outcomes})).collect::<Vec<_>>(),
        "ours": ours,
        "theirs": theirs,
        "matrix": matrix_json(&d.matrix),
        "equilibrium": equilibrium_json(&d.equilibrium),
        "value": num(d.equilibrium.value),
        "our_strategy": support_json(&ours, &d.equilibrium.rows, 0.01),
        "their_strategy": support_json(&theirs, &d.equilibrium.cols, 0.01),
        "maximin": {"row": d.maximin.0, "value": num(d.maximin.1)},
        "shallow": mixed_json(node, us, &d.shallow),
        "nodes": d.nodes,
        "turns": d.turns,
        "elapsed_s": d.elapsed.as_secs_f64(),
        "unsupported": d.unsupported,
        "omitted_ours": d.omitted_ours,
        "omitted_theirs": d.omitted_theirs,
        "stats": stats_json(&solver.stats()),
    }))
}

/// A fixed plan of our turn choices (`lab-plan --plan`, choice strings read against this
/// node's position) against the worst reply at every turn; with `config.child_nash` (one-turn
/// plans) the positions after it are also valued by their next-turn equilibrium
/// (`--child-nash`: the `config.reply_beam` worst replies, `config.outcome_cap` outcomes).
pub fn plan<const N: usize, E: Evaluator<N> + ?Sized + Sync>(
    node: &Node<N>,
    config: Config,
    evaluator: &E,
    plan: &[String],
) -> Result<Value, NodeError> {
    let us = config.us;
    if node.decision()? != Decision::Turn {
        return Err(NodeError::Invalid(
            "a plan starts at a turn decision (this position asks for a switch or is over)".into(),
        ));
    }
    if plan.is_empty() {
        return Err(NodeError::Invalid("an empty plan".into()));
    }
    let mut choices = Vec::with_capacity(plan.len());
    for (n, text) in plan.iter().enumerate() {
        let action =
            lab_scenario::parse_choice(&node.state, us, &node.order[us.index()], text.trim())
                .map_err(|e| NodeError::Invalid(format!("plan turn {}: {e}", n + 1)))?;
        choices.push(Choice::Turn(action));
    }
    let mut solver = Solver::new(config, evaluator);
    let mut state = node.state.clone();
    let r = solver.evaluate_plan(&mut state, node.suspension.as_ref(), &choices)?;
    let reply = |b: &Choice<N>| node.describe(r.decision, us.other(), b);
    Ok(json!({
        "mode": "plan",
        "config": config_json(&config),
        "decision": decision_name(r.decision),
        "plan": choices.iter().map(|c| node.describe(Decision::Turn, us, c)).collect::<Vec<_>>(),
        "value": num(r.value),
        "replies": r.replies.iter().map(|(b, v)| json!([reply(b), num(*v)])).collect::<Vec<_>>(),
        "broken": r.broken,
        "child": r.child.as_ref().map(|c| json!({
            "value": num(c.value),
            "replies": c.replies.iter().map(|(b, v)| json!([reply(b), num(*v)])).collect::<Vec<_>>(),
            "beam": c.beam,
            "outcomes": c.outcome_cap,
        })),
        "nodes": r.nodes,
        "turns": r.turns,
        "elapsed_s": r.elapsed.as_secs_f64(),
        "unsupported": r.unsupported,
        "omitted_pairs": r.omitted_pairs,
        "stats": stats_json(&solver.stats()),
    }))
}

/// What [`believed`] starts from: the scenario file, the opponent's beliefs about our team
/// (model ③) and what it observed after the setup turns (model ②).
#[derive(Clone, Debug, Default)]
pub struct BeliefRequest {
    /// The scenario file (believed teams replace our side's team file in it).
    pub scenario: String,
    /// Team files the opponent may believe we have; empty: model ② alone (the matrix game
    /// over the positions the observations cannot tell apart).
    pub believed_teams: Vec<String>,
    /// One weight per believed team (normalised); empty: uniform.
    pub believed_weights: Vec<f32>,
    /// `(setup turn, "Name:pct,Name:pct")`: what the opponent saw of our side after it.
    pub observations: Vec<(usize, String)>,
    pub tolerance: f32,
    pub setup: EnumerateOptions,
    /// Drop replayed branches in which a recorded setup choice is not legal.
    pub lenient: bool,
    /// Which start position (`None`: the only one, or the survivors' mixture).
    pub position: Option<usize>,
    pub most_probable: bool,
}

/// Opponent models ② and ③ (`lab-plan --believed-team ... --observed-turn ...`): with believed
/// teams, their equilibrium strategies mixed by (posterior) weight and our best responses on
/// the real position; without, the matrix game over the positions the observations cannot tell
/// apart. Errors are messages (`lab-plan`'s).
pub fn believed<E: Evaluator<2> + ?Sized + Sync>(
    request: &BeliefRequest,
    config: Config,
    evaluator: &E,
) -> Result<Value, String> {
    let us = config.us;
    let loaded = load_scenario_file(&request.scenario).map_err(|e| e.to_string())?;
    let mut observations: Vec<(usize, Observation)> = Vec::new();
    for (turn, text) in &request.observations {
        observations.push((*turn, parse_observation(text)?));
    }
    check_observations(&mut observations, loaded.setup_turns.len())?;
    let pick = PositionPick {
        index: request.position,
        most_probable: request.most_probable,
        before: None,
    };
    let (positions, trace, survivors) = observed_start(
        &loaded,
        us,
        &observations,
        request.tolerance,
        request.setup,
        request.lenient,
        pick.is_open(),
    )?;
    let position = pick_position(&loaded, positions, &pick, us)?;
    let trace: Vec<Value> = trace
        .iter()
        .map(|(turn, matched, total, share)| {
            json!({"turn": turn, "matched": matched, "total": total, "share": share})
        })
        .collect();
    let mut solver = Solver::new(config, evaluator);
    if request.believed_teams.is_empty() {
        let (m, note) = analyse_positions(&mut solver, &position, &survivors, "position")?;
        let ours: Vec<String> = m
            .ours
            .iter()
            .map(|c| describe(&position, m.decision, us, c))
            .collect();
        let theirs: Vec<String> = m
            .theirs
            .iter()
            .map(|c| describe(&position, m.decision, us.other(), c))
            .collect();
        return Ok(json!({
            "mode": "observed",
            "config": config_json(&config),
            "observations": trace,
            "survivors": survivors.len(),
            "note": note,
            "decision": decision_name(m.decision),
            "ours": ours,
            "theirs": theirs,
            "matrix": matrix_json(&m.matrix),
            "equilibrium": equilibrium_json(&m.equilibrium),
            "value": num(m.equilibrium.value),
            "our_strategy": support_json(&ours, &m.equilibrium.rows, 0.01),
            "their_strategy": support_json(&theirs, &m.equilibrium.cols, 0.01),
            "nodes": m.nodes,
            "turns": m.turns,
            "unsupported": m.unsupported,
            "stats": stats_json(&solver.stats()),
        }));
    }
    let setup = BeliefSetup {
        observations: observations.clone(),
        tolerance: request.tolerance,
        setup_options: request.setup,
        pick: pick.clone(),
    };
    let report = believed_analysis(
        &mut solver,
        &request.scenario,
        us,
        &position,
        &survivors,
        &request.believed_teams,
        &request.believed_weights,
        &setup,
    )?;
    let them = us.other();
    let mut mixture: Vec<(Choice<2>, f32)> = report.mixture.clone();
    mixture.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(json!({
        "mode": "believed",
        "config": config_json(&config),
        "observations": trace,
        "survivors": survivors.len(),
        "notes": report.notes,
        "teams": report.teams.iter().map(|t| json!({
            "team": t.team,
            "prior": t.prior,
            "posterior": t.posterior,
            "value": num(t.value),
        })).collect::<Vec<_>>(),
        "their_strategy": mixture.iter().map(|(c, p)| json!([describe(&report.believed, report.believed_decision, them, c), p])).collect::<Vec<_>>(),
        "real_equilibrium": num(report.real.equilibrium.value),
        "responses": report.response.iter().map(|(c, v)| json!([describe(&position, report.real.decision, us, c), num(*v)])).collect::<Vec<_>>(),
        "best_response": report.response.first().map_or(Value::Null, |r| num(r.1)),
        "decision": decision_name(report.real.decision),
        "nodes": report.real.nodes,
        "turns": report.real.turns,
        "unsupported": report.real.unsupported,
        "dropped": report.dropped,
        "elapsed_s": report.response_elapsed.as_secs_f64(),
        "stats": stats_json(&solver.stats()),
    }))
}

/// Self-play under an equilibrium policy (`lab-rollout`): `games` games from `positions`
/// (drawn by probability), `game_threads` at once. The report holds the tally (ties count
/// half in `p1_score`, cut-off and aborted games are outside it), the Wilson 95% interval and
/// every game's record. `scenario_path` adds the team files and their hashes.
#[allow(clippy::too_many_arguments)]
pub fn rollout<E: Evaluator<2> + ?Sized + Sync>(
    loaded: &LoadedScenario,
    scenario_path: Option<&str>,
    positions: &[Position],
    settings: &RolloutSettings,
    evaluator: &E,
    games: usize,
    game_threads: usize,
) -> Result<Value, String> {
    if games == 0 {
        return Err("games must be at least 1".into());
    }
    if positions.is_empty() {
        return Err("no start position".into());
    }
    let weights: Vec<f64> = positions.iter().map(|p| p.probability).collect();
    let started = std::time::Instant::now();
    let (records, cache) = run_games(
        loaded,
        positions,
        &weights,
        settings,
        evaluator,
        games,
        game_threads.max(1),
        &|_| {},
    );
    let count = |f: &dyn Fn(&Ending) -> bool| records.iter().filter(|r| f(&r.ending)).count();
    let p1 = count(&|e| *e == Ending::Win(SideId::One));
    let p2 = count(&|e| *e == Ending::Win(SideId::Two));
    let ties = count(&|e| *e == Ending::Tie);
    let cutoffs = count(&|e| *e == Ending::Cutoff);
    let aborted = count(&|e| matches!(e, Ending::Aborted(_)));
    let decided = p1 + p2 + ties;
    let (rate, lo, hi) = wilson(p1 as f64 + 0.5 * ties as f64, decided);
    let teams = match scenario_path {
        Some(path) => team_files(path)?
            .into_iter()
            .map(|(side, file, hash)| json!({"side": side, "file": file, "fnv1a64": format!("{hash:016x}")}))
            .collect(),
        None => Vec::new(),
    };
    let policy = match settings.policy {
        crate::rollout::Policy::Nash => {
            json!({"solve": "nash", "depth": 1, "double_oracle": settings.lazy})
        }
        crate::rollout::Policy::DeepNash => json!({
            "solve": "deep-nash",
            "depth": if settings.deep_rest.is_some() { 3 } else { 2 },
            "beam": settings.beam,
            "outcomes": settings.config.outcome_cap,
            "children_level": settings.deep_rest.map(|l| json!({"beam": l.beam, "outcomes": l.outcomes})),
        }),
    };
    Ok(json!({
        "mode": "rollout",
        "description": loaded.meta.description,
        "format": loaded.meta.format,
        "teams": teams,
        "games": games,
        "seed": settings.master_seed,
        "policy": policy,
        "config": config_json(&settings.config),
        "max_turns": settings.max_turns,
        "initial_states": positions.len(),
        "tally": {"p1": p1, "p2": p2, "tie": ties, "cutoff": cutoffs, "aborted": aborted, "decided": decided},
        "p1_score": if decided > 0 { json!(rate) } else { Value::Null },
        "wilson95": if decided > 0 { json!([lo, hi]) } else { Value::Null },
        "elapsed_s": started.elapsed().as_secs_f64(),
        "strategy_cache": {
            "hits": cache.hits.load(std::sync::atomic::Ordering::Relaxed),
            "solves": cache.misses.load(std::sync::atomic::Ordering::Relaxed),
        },
        "game_records": records.iter().map(|r| json!({
            "game": r.index,
            "seed": format!("{:016x}", r.seed),
            "start": r.start,
            "ending": r.ending.label(),
            "winner": winner_label(&r.ending),
            "aborted_reason": match &r.ending { Ending::Aborted(why) => Value::String(why.clone()), _ => Value::Null },
            "turns": r.turns,
            "elapsed_s": r.elapsed,
            "decisions": r.decisions,
        })).collect::<Vec<_>>(),
    }))
}
