//! Values every choice of one side at an oracle scenario's decision (DESIGN.md "탐색의 용도와
//! 정보 모델": opponent model ①, chance averaged or at its worst).
//!
//! Usage: lab-plan <scenario.json> [--side p1|p2] [--depth n] [--rng expect|worst]
//!                 [--before <oracle-report.json>] [--top k] [--exact] [--all-targets]
//!                 [--max-turns n] [--rolls full|extremes|quartiles|median|pessimistic]
//!                 [--eval material|heuristic|file:<weights.json>] [--position i]
//!                 [--solve maximin|nash|deep] [--dump-children <out.jsonl> [--beam b] [--outcomes k]]
//!                 [--believed-team <team.json>]... [--believed-weight w1,w2,...]
//!                 [--observed "Name:pct,Name:pct"] [--observed-turn k "Name:pct,..."]... [--observed-tolerance 1.0]
//!                 [--setup-rolls full|median|extremes|quartiles]
//!
//! `--believed-team` is opponent model ③ at one turn (DESIGN.md): the opponent solves the
//! matrix game on the team it believes we have (the same species, moves and order as ours, but
//! the spreads, items and abilities it assumes) and plays that equilibrium strategy; our
//! choices are then valued on the real position against it. The gap to the real equilibrium is
//! what the concealed sets are worth this turn. Several `--believed-team` files with
//! `--believed-weight` make a belief: the opponent's strategy is the weighted mixture of its
//! equilibrium strategies on each believed position. `--observed` / `--observed-turn` are
//! opponent model ②: the scenario's `setupTurns` are the turns played so far, and an observation
//! is what the opponent saw of our side after one of them (our Pokémon's HP percentages,
//! `Name:pct`; a fainted Pokémon is 0). `--observed-turn k ...` attaches one to setup turn `k`
//! (repeatable, one per turn); `--observed ...` is the last setup turn's. The positions the
//! setup turns lead to are filtered by each observation as soon as its turn is played, the real
//! position is the most probable one that survives, and each believed team's weight is
//! multiplied by the probability that its own replay of the same turns produced every
//! observation (a belief an observation contradicts drops to 0; so does a believed position in
//! which the choices actually made were not legal). DESIGN.md "모델 ③·② 구현".
//!                 [--threads n] [--plan "<turn 1> / <turn 2> / ..."]
//!                 [--child-nash [--beam b] [--outcomes k]]
//!
//! `--plan` values a fixed sequence of our turn choices (Showdown choice strings parsed
//! against the starting position; a turn whose choice is no longer legal falls back to
//! maximin and is counted as broken) against the reply that hurts us most at every turn;
//! after the plan `--depth - 1` maximin turns follow. `--child-nash` (one-turn plans) also values
//! the positions after the plan by their own next-turn equilibrium, for the `--beam` worst
//! replies and the `--outcomes` most probable outcomes of each.
//!
//! The position is the scenario's (after switch-ins, setup turns and patch); with several
//! initial states `--before` picks the one matching an oracle report, as `lab-turn` does.
//! `--depth` counts turns (default 1: this turn, then the material evaluation). `--exact`
//! values every root choice fully instead of stopping once it falls below the best one.
//! `--all-targets` keeps damaging moves aimed at an ally. `--rolls` picks the damage rolls the
//! enumeration branches on (default `extremes`: min and max; `full` is exact but a turn with
//! two spread moves has millions of outcomes; `median` one roll; `pessimistic` the minimum for
//! our attacks and the maximum against us). `--eval` picks the leaf evaluation (default
//! `heuristic`: material plus status, stages, volatiles and side conditions). Choices print as Showdown choice
//! strings against the position's party order, so they paste into a scenario's `turn`.

use std::process::ExitCode;

use serde_json::Value;

use lab_engine::eval::{
    features, Evaluator, Heuristic, Material, Weighted, FEATURE_COUNT, FEATURE_NAMES,
};
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::RollMode;
use lab_scenario::{
    canonical_json, load_scenario_file, scenario_positions_consistent, scenario_positions_filtered,
    Position,
};
use lab_search::game::asked_slots;
use lab_search::{
    format_choice, format_switches, Chance, Choice, Config, Decision, Pruning, Solver,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-plan: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scenario = None;
    let mut before = None;
    let mut us = SideId::One;
    let mut top = 10usize;
    let mut position_index: Option<usize> = None;
    let mut eval = "heuristic".to_owned();
    let mut solve = "maximin".to_owned();
    let mut plan: Option<String> = None;
    let mut dump_children: Option<String> = None;
    let mut believed_teams: Vec<String> = Vec::new();
    let mut observed: Option<String> = None;
    let mut observed_turns: Vec<(usize, String)> = Vec::new();
    let mut observed_tolerance: f32 = 1.0;
    let mut setup_rolls = RollMode::Full;
    let mut believed_weights: Vec<f32> = Vec::new();
    let mut pessimistic = false;
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, us);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--before" => {
                i += 1;
                before = args.get(i).cloned();
            }
            "--side" => {
                i += 1;
                us = match args.get(i).map(String::as_str) {
                    Some("p1") => SideId::One,
                    Some("p2") => SideId::Two,
                    _ => return Err("--side needs p1 or p2".into()),
                };
            }
            "--depth" => {
                i += 1;
                config.depth = args
                    .get(i)
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|&d| d > 0)
                    .ok_or("--depth needs a positive number of turns")?;
            }
            "--rng" => {
                i += 1;
                config.chance = match args.get(i).map(String::as_str) {
                    Some("expect") => Chance::Expect,
                    Some("worst") => Chance::Worst,
                    _ => return Err("--rng needs expect or worst".into()),
                };
            }
            "--top" => {
                i += 1;
                top = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--top needs a number")?;
            }
            "--position" => {
                i += 1;
                position_index = Some(
                    args.get(i)
                        .and_then(|s| s.parse::<usize>().ok())
                        .ok_or("--position needs an index")?,
                );
            }
            "--max-turns" => {
                i += 1;
                config.max_turns = Some(
                    args.get(i)
                        .and_then(|s| s.parse().ok())
                        .ok_or("--max-turns needs a number")?,
                );
            }
            "--rolls" => {
                i += 1;
                config.rolls = match args.get(i).map(String::as_str) {
                    Some("full") => RollMode::Full,
                    Some("extremes") => RollMode::Extremes,
                    Some("quartiles") => RollMode::Quartiles,
                    Some("median") => RollMode::Median,
                    Some("pessimistic") => {
                        pessimistic = true;
                        RollMode::Extremes
                    }
                    _ => {
                        return Err(
                            "--rolls needs full, extremes, quartiles, median or pessimistic".into(),
                        )
                    }
                };
            }
            "--eval" => {
                i += 1;
                eval = match args.get(i).map(String::as_str) {
                    Some(e @ ("material" | "heuristic")) => e.to_owned(),
                    Some(e) if e.starts_with("file:") => e.to_owned(),
                    _ => {
                        return Err("--eval needs material, heuristic or file:<weights.json>".into())
                    }
                };
            }
            "--solve" => {
                i += 1;
                solve = match args.get(i).map(String::as_str) {
                    Some(s @ ("maximin" | "nash" | "deep")) => s.to_owned(),
                    _ => return Err("--solve needs maximin, nash or deep".into()),
                };
            }
            "--plan" => {
                i += 1;
                plan = Some(args.get(i).cloned().ok_or("--plan needs the turns")?);
            }
            "--dump-children" => {
                i += 1;
                dump_children = Some(args.get(i).cloned().ok_or("--dump-children needs a file")?);
            }
            "--believed-team" => {
                i += 1;
                believed_teams.push(
                    args.get(i)
                        .cloned()
                        .ok_or("--believed-team needs a team file")?,
                );
            }
            "--observed" => {
                i += 1;
                observed = Some(
                    args.get(i)
                        .cloned()
                        .ok_or("--observed needs Name:pct,...")?,
                );
            }
            "--setup-rolls" => {
                i += 1;
                setup_rolls = match args.get(i).map(String::as_str) {
                    Some("full") => RollMode::Full,
                    Some("median") => RollMode::Median,
                    Some("extremes") => RollMode::Extremes,
                    Some("quartiles") => RollMode::Quartiles,
                    _ => {
                        return Err("--setup-rolls needs full, median, extremes or quartiles".into())
                    }
                };
            }
            "--observed-turn" => {
                i += 1;
                let turn: usize = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--observed-turn needs a setup turn number and Name:pct,...")?;
                i += 1;
                let text = args
                    .get(i)
                    .cloned()
                    .ok_or("--observed-turn needs a setup turn number and Name:pct,...")?;
                observed_turns.push((turn, text));
            }
            "--observed-tolerance" => {
                i += 1;
                observed_tolerance = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--observed-tolerance needs a number")?;
            }
            "--believed-weight" => {
                i += 1;
                let text = args.get(i).ok_or("--believed-weight needs w1,w2,...")?;
                believed_weights = text
                    .split(',')
                    .map(|w| w.trim().parse::<f32>())
                    .collect::<Result<Vec<f32>, _>>()
                    .map_err(|e| format!("--believed-weight: {e}"))?;
            }
            "--threads" => {
                i += 1;
                config.threads = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--threads needs a number")?;
            }
            "--exact" => config.exact_lines = true,
            "--child-nash" => config.child_nash = true,
            "--beam" => {
                i += 1;
                config.reply_beam = Some(
                    args.get(i)
                        .and_then(|s| s.parse().ok())
                        .ok_or("--beam needs a number")?,
                );
            }
            "--outcomes" => {
                i += 1;
                config.outcome_cap = Some(
                    args.get(i)
                        .and_then(|s| s.parse().ok())
                        .ok_or("--outcomes needs a number")?,
                );
            }
            "--all-targets" => config.pruning = Pruning::All,
            other if scenario.is_none() => scenario = Some(other.to_owned()),
            other => return Err(format!("unexpected argument {other}")),
        }
        i += 1;
    }
    config.us = us;
    if pessimistic {
        config.rolls = RollMode::Pessimistic(us);
    }
    let scenario = scenario.ok_or(
        "usage: lab-plan <scenario.json> [--side p1|p2] [--depth n] [--rng expect|worst] \
         [--before report.json] [--top k] [--exact] [--all-targets] [--max-turns n] \
         [--rolls full|extremes|quartiles|median|pessimistic] [--eval material|heuristic]",
    )?;

    let loaded = load_scenario_file(&scenario).map_err(|e| e.to_string())?;
    let setup_options = lab_engine::turn::EnumerateOptions { rolls: setup_rolls };
    // Opponent model ②'s observations: (setup turn, what the opponent saw of our side then).
    let mut observations: Vec<(usize, Vec<(String, f32)>)> = Vec::new();
    for (turn, text) in &observed_turns {
        observations.push((*turn, parse_observation(text)?));
    }
    if let Some(text) = &observed {
        observations.push((loaded.setup_turns.len(), parse_observation(text)?));
    }
    observations.sort_by_key(|(turn, _)| *turn);
    if let Some(pair) = observations.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(format!(
            "two observations for setup turn {}; give one --observed-turn per turn",
            pair[0].0
        ));
    }
    if let Some((turn, _)) = observations
        .iter()
        .find(|(turn, _)| *turn == 0 || *turn > loaded.setup_turns.len())
    {
        return Err(format!(
            "--observed-turn {turn}: the scenario has {} setup turns (an observation belongs to one of them)",
            loaded.setup_turns.len()
        ));
    }
    let (mut positions, trace) = observed_positions(
        &loaded,
        us,
        &observations,
        observed_tolerance,
        setup_options,
        Replay::Real,
    )?;
    if !observations.is_empty() {
        for (turn, matched, total, share) in &trace {
            println!(
                "setup turn {turn}: {matched} of {total} positions match the observation ({:.1}% of the probability reaching the turn)",
                share * 100.0
            );
        }
        if positions.is_empty() {
            return Err(
                "no position after the setup turns matches the observations; check the names, percentages and tolerance (it must cover the roll spread of --setup-rolls)".into(),
            );
        }
        positions.sort_by(|a, b| b.probability.total_cmp(&a.probability));
        if positions.len() > 1 && position_index.is_none() {
            println!(
                "{} positions survive the observations; taking the most probable (p={:.4}); pass --position to choose",
                positions.len(),
                positions[0].probability
            );
            positions.truncate(1);
        }
    }
    let position = pick_position(&loaded, positions, before.as_deref(), position_index, us)?;
    let them = us.other();
    let mut state = position.state.clone();

    println!("{}", loaded.meta.description.trim());
    println!(
        "format {}, turn {}, {} choices for {} (them: {})",
        loaded.meta.format,
        state.turn,
        side_name(us),
        roster(&loaded, &position, us),
        roster(&loaded, &position, them)
    );

    let evaluator: Box<dyn Evaluator<2> + Sync> = if eval == "material" {
        Box::new(Material)
    } else if let Some(path) = eval.strip_prefix("file:") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let value: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        let mut weights = [0.0f32; FEATURE_COUNT];
        for (i, name) in FEATURE_NAMES.iter().enumerate() {
            weights[i] = value["weights"][*name]
                .as_f64()
                .ok_or_else(|| format!("{path}: weights.{name} missing"))?
                as f32;
        }
        Box::new(Weighted { weights })
    } else {
        Box::new(Heuristic)
    };
    let mut solver = Solver::new(config, evaluator.as_ref());
    if !believed_teams.is_empty() {
        // Opponent model ③ with a belief: for each believed team (the same scenario with our
        // side's team file replaced) the opponent's equilibrium strategy; the mixture by weight
        // is what it plays; our best response is valued on the real position.
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
        let mut mixture: Vec<(Choice<2>, f32)> = Vec::new();
        let mut believed_values = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        let mut reference: Option<(Vec<Choice<2>>, Position, Decision)> = None;
        // With observations, each believed team's weight is multiplied by the probability that
        // its own replay of the setup turns produced all of them (Bayes with the engine as the
        // likelihood); the believed position is then the most probable surviving one.
        let mut posterior = weights.clone();
        let mut believed_positions = Vec::with_capacity(believed_teams.len());
        for (k, team_path) in believed_teams.iter().enumerate() {
            let bl = believed_loaded(&scenario, us, team_path)?;
            let (mut bpositions, _) = observed_positions(
                &bl,
                us,
                &observations,
                observed_tolerance,
                setup_options,
                Replay::Believed,
            )?;
            if !observations.is_empty() {
                let likelihood: f64 = bpositions.iter().map(|p| p.probability).sum();
                posterior[k] *= likelihood as f32;
                if bpositions.is_empty() {
                    believed_positions.push(None);
                    continue;
                }
                bpositions.sort_by(|a, b| b.probability.total_cmp(&a.probability));
                bpositions.truncate(1);
            }
            let bp = pick_position(&bl, bpositions, before.as_deref(), position_index, us)?;
            believed_positions.push(Some(bp));
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
            let Some(believed) = believed_positions[k].clone() else {
                believed_values.push((team_path.clone(), w, f32::NAN));
                continue;
            };
            let mut believed_state = believed.state.clone();
            let mixed = solver
                .analyse_mixed(&mut believed_state, None)
                .map_err(|e| format!("believed position {team_path}: {e}"))?;
            believed_values.push((team_path.clone(), w, mixed.equilibrium.value));
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
        let response = solver
            .best_response(&mut state, None, &mixture)
            .map_err(|e| e.to_string())?;
        if state != position.state {
            return Err("the solver changed the position (bug)".into());
        }
        let real = solver
            .analyse_mixed(&mut state, None)
            .map_err(|e| e.to_string())?;
        println!(
            "opponent model 3: their equilibrium strategies on the believed teams, mixed by weight, answered on the real position; real equilibrium {:+.1}; chance {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            real.equilibrium.value,
            config.chance,
            config.rolls,
            response.nodes,
            response.turns,
            response.elapsed.as_secs_f64()
        );
        for (k, (team_path, w, value)) in believed_values.iter().enumerate() {
            let prior = weights[k];
            if !observations.is_empty() {
                println!(
                    "  belief {:>5.1}% -> {:>5.1}%  {team_path}: equilibrium there {value:+.1} (from our side)",
                    prior * 100.0,
                    (w * 100.0).max(0.0)
                );
            } else {
                println!(
                    "  belief {:>5.1}%  {team_path}: equilibrium there {value:+.1} (from our side)",
                    w * 100.0
                );
            }
        }
        println!("their mixed strategy (>= 1%):");
        let mut shown: Vec<(Choice<2>, f32)> = mixture
            .iter()
            .filter(|(_, p)| *p >= 0.01)
            .copied()
            .collect();
        shown.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (choice, p) in shown.iter().take(top) {
            println!(
                "  {:>5.1}%  {}",
                p * 100.0,
                describe(&believed, believed_decision, them, choice)
            );
        }
        println!(
            "our best responses on the real position (best value {:+.1}; the gap to the real equilibrium, {:+.1}, is what the hidden sets are worth this turn):",
            response.lines[0].1,
            response.lines[0].1 - real.equilibrium.value
        );
        for (rank, (choice, value)) in response.lines.iter().take(top).enumerate() {
            println!(
                "{:>3}  {:>9}  {}",
                rank + 1,
                format!("{value:+.1}"),
                describe(&position, response.decision, us, choice)
            );
        }
        if !response.unsupported.is_empty() || !dropped.is_empty() {
            println!("dropped pairs reaching effects the engine does not implement:");
            let mut all: Vec<&String> = response.unsupported.iter().chain(&dropped).collect();
            all.sort();
            all.dedup();
            for why in all {
                println!("  - {why}");
            }
        }
        return Ok(());
    }
    if let Some(path) = &dump_children {
        // Feature vectors of the positions one turn ahead, each with its next-turn equilibrium
        // value as the fitting target (WORKPLAN S12): our `--beam` best choices by the root
        // matrix, their `--beam` worst replies, the `--outcomes` most probable outcomes of each
        // pair (the deep analysis' children, cached).
        let beam = config.reply_beam.unwrap_or(6);
        let deep = solver
            .analyse_deep(&mut state, None, beam)
            .map_err(|e| e.to_string())?;
        let decision = deep.decision;
        let mut out = String::new();
        let mut rows = 0usize;
        // Only the beam's replies: their children were just valued by `analyse_deep`, so the
        // targets come from the solver's cache instead of hundreds of fresh matrix games.
        for line in &deep.lines {
            for &(b, _) in &line.replies {
                let pair = match us {
                    SideId::One => [line.ours, b],
                    SideId::Two => [b, line.ours],
                };
                let Ok(mut outcomes) = lab_search::transitions(
                    &mut state,
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
                    let target = solver.nash_value(&mut state, o.suspension.as_ref());
                    let f = features(&state);
                    let sign = if us == SideId::One { 1.0 } else { -1.0 };
                    state.reverse(&o.instructions);
                    let Ok(target) = target else { continue };
                    if target.is_nan() {
                        continue;
                    }
                    let f: Vec<f32> = f.iter().map(|x| x * sign).collect();
                    out.push_str(
                        &serde_json::to_string(&serde_json::json!({
                            "features": f,
                            "target": target,
                            "p": o.probability,
                            "ours": describe(&position, decision, us, &line.ours),
                            "theirs": describe(&position, decision, them, &b),
                        }))
                        .expect("serializable"),
                    );
                    out.push('\n');
                    rows += 1;
                }
            }
        }
        std::fs::write(path, out).map_err(|e| format!("{path}: {e}"))?;
        println!(
            "wrote {rows} child positions to {path} (features {:?}; targets: next-turn equilibrium from our side); {} nodes, {} enumerations",
            FEATURE_NAMES, deep.nodes, deep.turns
        );
        return Ok(());
    }
    if let Some(text) = &plan {
        let order = &position.order[us.index()];
        let mut choices = Vec::new();
        for (n, segment) in text.split('/').map(str::trim).enumerate() {
            let action = lab_scenario::parse_choice(&state, us, order, segment)
                .map_err(|e| format!("plan turn {}: {e}", n + 1))?;
            choices.push(Choice::Turn(action));
        }
        let report = solver
            .evaluate_plan(&mut state, None, &choices)
            .map_err(|e| e.to_string())?;
        if state != position.state {
            return Err("the solver changed the position (bug)".into());
        }
        println!(
            "plan of {} turn(s), then {} maximin turn(s); chance {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            choices.len(),
            config.depth.saturating_sub(1),
            config.chance,
            config.rolls,
            report.nodes,
            report.turns,
            report.elapsed.as_secs_f64()
        );
        for (n, choice) in choices.iter().enumerate() {
            println!(
                "  turn {}: {}",
                n + 1,
                describe(&position, Decision::Turn, us, choice)
            );
        }
        println!(
            "value {:+.1} against the worst replies; broken at {} position(s)",
            report.value, report.broken
        );
        if !report.unsupported.is_empty() {
            println!(
                "dropped {} pair(s) that reach effects the engine does not implement:",
                report.omitted_pairs
            );
            for why in &report.unsupported {
                println!("  - {why}");
            }
        }
        println!("their turn-1 replies, worst for us first:");
        for (reply, value) in report.replies.iter().take(top) {
            println!(
                "  {:>9}  {}",
                format!("{value:+.1}"),
                describe(&position, report.decision, them, reply)
            );
        }
        if let Some(child) = &report.child {
            println!(
                "with the positions after the plan valued by their next-turn equilibrium ({} worst replies, {} outcomes each): value {:+.1}",
                child.beam,
                child.outcome_cap.map_or("all".to_owned(), |k| k.to_string()),
                child.value
            );
            for (reply, value) in child.replies.iter().take(top) {
                println!(
                    "  {:>9}  {}",
                    format!("{value:+.1}"),
                    describe(&position, report.decision, them, reply)
                );
            }
        }
        return Ok(());
    }
    if solve == "deep" {
        let beam = config.reply_beam.unwrap_or(6);
        let deep = solver
            .analyse_deep(&mut state, None, beam)
            .map_err(|e| e.to_string())?;
        if state != position.state {
            return Err("the solver changed the position (bug)".into());
        }
        println!(
            "decision {:?}, deep: beam {} x {} (outcomes {}), chance {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            deep.decision,
            deep.beam,
            deep.beam,
            deep.outcome_cap.map_or("all".to_owned(), |k| k.to_string()),
            config.chance,
            config.rolls,
            deep.nodes,
            deep.turns,
            deep.elapsed.as_secs_f64()
        );
        println!(
            "{:>3}  {:>9}  {:>9}  {:<44}  worst reply (deep)",
            "#", "deep", "shallow", "our choice"
        );
        for (rank, line) in deep.lines.iter().enumerate() {
            let reply = line
                .replies
                .first()
                .map(|(r, _)| describe(&position, deep.decision, them, r))
                .unwrap_or_default();
            println!(
                "{:>3}  {:>9}  {:>9}  {:<44}  {}",
                rank + 1,
                format!("{:+.1}", line.deep),
                format!("{:+.1}", line.shallow),
                describe(&position, deep.decision, us, &line.ours),
                reply
            );
        }
        if !deep.shallow_rest.is_empty() {
            println!(
                "... {} more choices outside the beam (shallow values {:+.1} .. {:+.1})",
                deep.shallow_rest.len(),
                deep.shallow_rest.first().map_or(0.0, |x| x.1),
                deep.shallow_rest.last().map_or(0.0, |x| x.1)
            );
        }
        if !deep.unsupported.is_empty() {
            println!(
                "dropped {} pair(s) that reach effects the engine does not implement:",
                deep.omitted_pairs
            );
            for why in &deep.unsupported {
                println!("  - {why}");
            }
        }
        return Ok(());
    }
    if solve == "nash" {
        let mixed = solver
            .analyse_mixed(&mut state, None)
            .map_err(|e| e.to_string())?;
        if state != position.state {
            return Err("the solver changed the position (bug)".into());
        }
        println!(
            "decision {:?}, depth {}, chance {:?}, pruning {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            mixed.decision, mixed.depth, config.chance, config.pruning, config.rolls, mixed.nodes, mixed.turns,
            mixed.elapsed.as_secs_f64()
        );
        println!(
            "matrix {}x{}; equilibrium value {:+.1} (exploitability {:.3}, {} RM+ iterations); pure maximin {:+.1}",
            mixed.matrix.rows,
            mixed.matrix.cols,
            mixed.equilibrium.value,
            mixed.equilibrium.exploitability,
            mixed.equilibrium.iterations,
            mixed.maximin.1
        );
        if !mixed.unsupported.is_empty() {
            println!(
                "dropped {} of their replies and {} of our choices that reach effects the engine does not implement:",
                mixed.omitted_theirs, mixed.omitted_ours
            );
            for why in &mixed.unsupported {
                println!("  - {why}");
            }
        }
        println!("our mixed strategy (>= 1%):");
        for (choice, p) in mixed.our_support(0.01).iter().take(top) {
            println!(
                "  {:>5.1}%  {}",
                p * 100.0,
                describe(&position, mixed.decision, us, choice)
            );
        }
        println!("their mixed strategy (>= 1%):");
        for (choice, p) in mixed.their_support(0.01).iter().take(top) {
            println!(
                "  {:>5.1}%  {}",
                p * 100.0,
                describe(&position, mixed.decision, them, choice)
            );
        }
        return Ok(());
    }
    let analysis = solver
        .analyse(&mut state, None)
        .map_err(|e| e.to_string())?;
    if state != position.state {
        return Err("the solver changed the position (bug)".into());
    }
    println!(
        "decision {:?}, depth {}, chance {:?}, pruning {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
        analysis.decision,
        analysis.depth,
        config.chance,
        config.pruning,
        config.rolls,
        analysis.nodes,
        analysis.turns,
        analysis.elapsed.as_secs_f64()
    );
    if analysis.lines.is_empty() {
        println!("the battle is over: value {:+.1}", analysis.value);
        return Ok(());
    }
    println!(
        "{} choices for us, {} replies; best value {:+.1}",
        analysis.lines.len(),
        analysis
            .lines
            .iter()
            .filter_map(|l| l.reply)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        analysis.value
    );
    println!(
        "{:>3}  {:>9}  {:<44}  reply that holds it",
        "#", "value", "our choice"
    );
    for (rank, line) in analysis.lines.iter().take(top).enumerate() {
        let value = if line.exact {
            format!("{:+.1}", line.value)
        } else {
            format!("<={:+.1}", line.value)
        };
        let ours = describe(&position, analysis.decision, us, &line.ours);
        let reply = line
            .reply
            .map(|r| describe(&position, analysis.decision, them, &r))
            .unwrap_or_default();
        println!("{:>3}  {:>9}  {:<44}  {}", rank + 1, value, ours, reply);
    }
    if analysis.lines.len() > top {
        println!("... {} more", analysis.lines.len() - top);
    }
    if !analysis.unsupported.is_empty() {
        println!(
            "dropped {} pair(s) that reach effects the engine does not implement:",
            analysis.omitted_pairs
        );
        for why in &analysis.unsupported {
            println!("  - {why}");
        }
    }
    if let Some(turn) = &loaded.meta.turn {
        let own = match us {
            SideId::One => &turn.p1,
            SideId::Two => &turn.p2,
        };
        if analysis.decision == Decision::Turn {
            match lab_scenario::parse_choice(&state, us, &position.order[us.index()], own) {
                Ok(action) => {
                    let text = format_choice(&state, us, &position.order[us.index()], &action);
                    match analysis
                        .lines
                        .iter()
                        .position(|l| l.ours == Choice::Turn(action))
                    {
                        Some(rank) => println!(
                            "the scenario's own choice {text:?} ranks {} (value {:+.1}{})",
                            rank + 1,
                            analysis.lines[rank].value,
                            if analysis.lines[rank].exact {
                                ""
                            } else {
                                ", upper bound"
                            }
                        ),
                        None => println!(
                            "the scenario's own choice {text:?} is not among the considered choices"
                        ),
                    }
                }
                Err(e) => println!("the scenario's own choice {own:?} does not parse here: {e}"),
            }
        }
    }
    Ok(())
}

/// The scenario with our side's team file replaced by `team_path`, loaded (the callers replay
/// its setup turns under the observations and pick).
fn believed_loaded(
    scenario: &str,
    us: SideId,
    team_path: &str,
) -> Result<lab_scenario::LoadedScenario, String> {
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

/// Per observed setup turn: `(turn, matched, total, share)`, see [`observed_positions`].
type ObservationTrace = Vec<(usize, usize, usize, f64)>;

/// Whose teams a replay of the setup turns runs on: the real ones, where a choice that is not
/// legal is a scenario error, or a believed team's, where it contradicts the belief in that
/// position and the position is dropped (`scenario_positions_consistent`).
#[derive(Clone, Copy)]
enum Replay {
    Real,
    Believed,
}

/// The positions after the setup turns that survive every observation, each applied as soon as
/// its turn is played, with one `(turn, matched, total, share)` per observed turn: how many of
/// the positions reaching that turn matched and the share of their probability they hold. The
/// total probability of the result is the likelihood of the observations under this scenario's
/// teams.
fn observed_positions(
    loaded: &lab_scenario::LoadedScenario,
    us: SideId,
    observations: &[(usize, Vec<(String, f32)>)],
    tolerance: f32,
    setup_options: lab_engine::turn::EnumerateOptions,
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

/// `Name:pct,Name:pct`: what the opponent saw of our side's Pokémon after a setup turn.
fn parse_observation(text: &str) -> Result<Vec<(String, f32)>, String> {
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

/// The positions whose HP percentages of our named Pokémon match the observation within
/// `tolerance` points (a fainted Pokémon is 0%).
fn matching_positions(
    loaded: &lab_scenario::LoadedScenario,
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

fn pick_position(
    loaded: &lab_scenario::LoadedScenario,
    positions: Vec<Position>,
    before: Option<&str>,
    index: Option<usize>,
    us: SideId,
) -> Result<Position, String> {
    if let Some(i) = index {
        let n = positions.len();
        return positions
            .into_iter()
            .nth(i)
            .ok_or_else(|| format!("--position {i}: only {n} initial states"));
    }
    let wanted = match before {
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

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

/// `Gardevoir, Rillaboom | bench Sableye, Milotic` from the sidecar names.
fn roster(loaded: &lab_scenario::LoadedScenario, position: &Position, side: SideId) -> String {
    let s = position.state.side(side);
    let meta = &loaded.meta.sides[side.index()];
    let name = |party: u8| meta.name(party).unwrap_or("?").to_owned();
    let active: Vec<String> = s
        .slots
        .iter()
        .map(|slot| match slot.party_index {
            Some(p) if s.party[p as usize].hp > 0 => name(p),
            Some(p) => format!("{} (fainted)", name(p)),
            None => "-".to_owned(),
        })
        .collect();
    let bench: Vec<String> = (0..s.party.len() as u8)
        .filter(|&p| {
            !s.party[p as usize].species.is_none()
                && !s.slots.iter().any(|slot| slot.party_index == Some(p))
        })
        .map(|p| {
            if s.party[p as usize].hp > 0 {
                name(p)
            } else {
                format!("{} (fainted)", name(p))
            }
        })
        .collect();
    format!("{} | bench {}", active.join(", "), bench.join(", "))
}

fn describe(position: &Position, decision: Decision, side: SideId, choice: &Choice<2>) -> String {
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
