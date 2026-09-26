//! Values every choice of one side at an oracle scenario's decision (DESIGN.md "탐색의 용도와
//! 정보 모델": opponent model ①, chance averaged or at its worst).
//!
//! Usage: lab-plan <scenario.json> [--side p1|p2] [--depth n] [--rng expect|worst]
//!                 [--before <oracle-report.json>] [--top k] [--exact] [--all-targets]
//!                 [--max-turns n] [--rolls full|extremes|quartiles|median|pessimistic]
//!                 [--eval material|heuristic] [--position i] [--solve maximin|nash|deep]
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

use lab_engine::eval::{Evaluator, Heuristic, Material};
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::RollMode;
use lab_scenario::{canonical_json, load_scenario_file, scenario_positions, Position};
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
                    _ => return Err("--eval needs material or heuristic".into()),
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
    let positions = scenario_positions(&loaded)?;
    let position = pick_position(&loaded, positions, before.as_deref(), position_index)?;
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
    } else {
        Box::new(Heuristic)
    };
    let mut solver = Solver::new(config, evaluator.as_ref());
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

fn pick_position(
    loaded: &lab_scenario::LoadedScenario,
    positions: Vec<Position>,
    before: Option<&str>,
    index: Option<usize>,
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
                text.push_str(&format!(
                    "  {i}: p={:.4} {}
",
                    p.probability,
                    actives.join(", ")
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
