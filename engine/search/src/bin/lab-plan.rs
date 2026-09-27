//! Values every choice of one side at an oracle scenario's decision (DESIGN.md "탐색의 용도와
//! 정보 모델": opponent model ①, chance averaged or at its worst).
//!
//! Usage: lab-plan <scenario.json> [--side p1|p2] [--depth n] [--rng expect|worst]
//!                 [--before <oracle-report.json>] [--top k] [--exact] [--all-targets]
//!                 [--max-turns n] [--rolls full|extremes|quartiles|median|pessimistic]
//!                 [--eval material|heuristic|file:<weights.json>] [--position i|max] [--setup-lenient]
//!                 [--solve maximin|nash|deep|deep-nash] [--dump-children <out.jsonl> [--beam b] [--outcomes k]]
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
//! which the choices actually made were not legal). The positions the observations cannot tell
//! apart (sleep turns, hidden rolls: chance neither player sees) form one matrix game, the
//! probability-weighted average of their payoff matrices, for `--solve nash` and the believed
//! analysis, as long as their choice lists coincide; `--position` picks one instead.
//! DESIGN.md "모델 ③·② 구현".
//!                 [--threads n] [--plan "<turn 1> / <turn 2> / ..."]
//!                 [--child-nash [--beam b] [--outcomes k]]
//!                 [--stats] [--no-transposition] [--no-dominance] [--full-children] [--lazy]
//!
//! `--stats` adds a line with the search's work (transposition-table hits, matrix games and
//! their RM+ time, enumeration time summed over threads). `--no-transposition` and
//! `--no-dominance` turn off the child-equilibrium table (S24a) and the dominance reduction of
//! child matrix games (S24d), `--full-children` values every cell of every child game instead
//! of double oracle over lazily valued cells (S24d), to check that they change nothing but the
//! time (within the equilibrium solver's tolerance). `--lazy` solves `--solve nash`'s root the
//! same way (double oracle): a fraction of the pairs, the equilibrium within tolerance, but no
//! full matrix, so the pure maximin is only over the rows valued in full.
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

use lab_engine::eval::FEATURE_NAMES;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::RollMode;
use lab_scenario::{load_scenario_file, Position};
use lab_search::model::{
    analyse_positions, believed_analysis, check_observations, describe, load_evaluator,
    observed_start, parse_observation, pick_position, side_name, BeliefSetup, BelievedReport,
    Observation, PositionPick,
};
use lab_search::{format_choice, Chance, Choice, Config, Decision, Pruning, Solver};

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
    // `--position max`: the most probable initial state (batch runs over replayed setup turns).
    let mut position_max = false;
    // `--setup-lenient`: drop the replayed positions in which a setup turn's choices are not
    // legal (turns copied from a played game: the other branches may have a faint the game did
    // not) instead of failing the replay.
    let mut setup_lenient = false;
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
    let mut show_stats = false;
    let mut lazy_root = false;
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
            "--setup-lenient" => setup_lenient = true,
            "--position" => {
                i += 1;
                match args.get(i).map(String::as_str) {
                    Some("max") => position_max = true,
                    Some(s) => {
                        position_index = Some(
                            s.parse::<usize>()
                                .map_err(|_| "--position needs an index or max")?,
                        )
                    }
                    None => return Err("--position needs an index or max".into()),
                }
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
                    Some(s @ ("maximin" | "nash" | "deep" | "deep-nash")) => s.to_owned(),
                    _ => return Err("--solve needs maximin, nash, deep or deep-nash".into()),
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
            "--stats" => show_stats = true,
            "--child-nash" => config.child_nash = true,
            "--no-transposition" => config.transposition = false,
            "--no-dominance" => config.dominance = false,
            "--full-children" => config.double_oracle = false,
            "--lazy" => lazy_root = true,
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
    let mut observations: Vec<(usize, Observation)> = Vec::new();
    for (turn, text) in &observed_turns {
        observations.push((*turn, parse_observation(text)?));
    }
    if let Some(text) = &observed {
        observations.push((loaded.setup_turns.len(), parse_observation(text)?));
    }
    check_observations(&mut observations, loaded.setup_turns.len())?;
    let pick = PositionPick {
        index: position_index,
        most_probable: position_max,
        before: before.clone(),
    };
    let (positions, trace, survivors) = observed_start(
        &loaded,
        us,
        &observations,
        observed_tolerance,
        setup_options,
        setup_lenient,
        pick.is_open(),
    )?;
    if !observations.is_empty() {
        for (turn, matched, total, share) in &trace {
            println!(
                "setup turn {turn}: {matched} of {total} positions match the observation ({:.1}% of the probability reaching the turn)",
                share * 100.0
            );
        }
    }
    // The positions the observations cannot tell apart (empty when there is one position or
    // `--position`).
    if !survivors.is_empty() {
        println!(
                "{} positions survive the observations (most probable p={:.4}); the matrix game is played over their mixture where the choices coincide; pass --position to pick one",
                survivors.len(),
                survivors[0].probability
            );
    }
    let position = pick_position(&loaded, positions, &pick, us)?;
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

    let evaluator = load_evaluator(&eval)?;
    let mut solver = Solver::new(config, evaluator.as_ref());
    if !believed_teams.is_empty() {
        // Opponent model ③ with a belief (② with observations), `lab_search::model`.
        let setup = BeliefSetup {
            observations: observations.clone(),
            tolerance: observed_tolerance,
            setup_options,
            pick: pick.clone(),
        };
        let report = believed_analysis(
            &mut solver,
            &scenario,
            us,
            &position,
            &survivors,
            &believed_teams,
            &believed_weights,
            &setup,
        )?;
        for note in &report.notes {
            println!("{note}");
        }
        let BelievedReport {
            teams,
            mixture,
            believed,
            believed_decision,
            real,
            response: response_lines,
            response_elapsed,
            dropped,
            ..
        } = report;
        println!(
            "opponent model 3: their equilibrium strategies on the believed teams, mixed by weight, answered on the real position; real equilibrium {:+.1}; chance {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            real.equilibrium.value,
            config.chance,
            config.rolls,
            real.nodes,
            real.turns,
            response_elapsed.as_secs_f64()
        );
        if show_stats {
            println!("search stats: {}", solver.stats());
        }
        for team in &teams {
            let (team_path, w, value, prior) = (&team.team, team.posterior, team.value, team.prior);
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
            response_lines[0].1,
            response_lines[0].1 - real.equilibrium.value
        );
        for (rank, (choice, value)) in response_lines.iter().take(top).enumerate() {
            println!(
                "{:>3}  {:>9}  {}",
                rank + 1,
                format!("{value:+.1}"),
                describe(&position, real.decision, us, choice)
            );
        }
        if !real.unsupported.is_empty() || !dropped.is_empty() {
            println!("dropped pairs reaching effects the engine does not implement:");
            let mut all: Vec<&String> = real.unsupported.iter().chain(&dropped).collect();
            all.sort();
            all.dedup();
            for why in all {
                println!("  - {why}");
            }
        }
        return Ok(());
    }
    if let Some(path) = &dump_children {
        // Feature vectors of the positions one turn ahead with their next-turn equilibrium
        // (WORKPLAN S12), `lab_search::model::dump_children`.
        let beam = config.reply_beam.unwrap_or(6);
        let (deep, children) = lab_search::model::dump_children(&mut solver, &mut state, beam)?;
        let decision = deep.decision;
        let mut out = String::new();
        let rows = children.len();
        for row in &children {
            out.push_str(
                &serde_json::to_string(&serde_json::json!({
                    "features": row.features,
                    "target": row.target,
                    "p": row.probability,
                    "ours": describe(&position, decision, us, &row.ours),
                    "theirs": describe(&position, decision, them, &row.theirs),
                }))
                .expect("serializable"),
            );
            out.push('\n');
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
        if show_stats {
            println!("search stats: {}", solver.stats());
        }
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
    if solve == "deep-nash" {
        // Depth-2 mixed equilibrium (S21): both sides' beams, cells worth the children's
        // next-turn equilibrium, solved as a matrix game.
        let beam = config.reply_beam.unwrap_or(4);
        let deep = solver
            .analyse_deep_mixed(&mut state, None, beam)
            .map_err(|e| e.to_string())?;
        if state != position.state {
            return Err("the solver changed the position (bug)".into());
        }
        println!(
            "decision {:?}, deep-nash: beams {} x {} (+ shallow support >= {:.0}%), outcomes {:?}, chance {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            deep.decision,
            deep.beam,
            deep.beam,
            lab_search::MIXED_SUPPORT * 100.0,
            deep.outcome_cap,
            config.chance,
            config.rolls,
            deep.nodes,
            deep.turns,
            deep.elapsed.as_secs_f64()
        );
        if show_stats {
            println!("search stats: {}", solver.stats());
        }
        println!(
            "shallow matrix {}x{}, equilibrium {:+.1}; deep matrix {}x{}, equilibrium {:+.1} (exploitability {:.3}, {} RM+ iterations); pure deep maximin {:+.1}",
            deep.shallow.matrix.rows,
            deep.shallow.matrix.cols,
            deep.shallow.equilibrium.value,
            deep.matrix.rows,
            deep.matrix.cols,
            deep.equilibrium.value,
            deep.equilibrium.exploitability,
            deep.equilibrium.iterations,
            deep.maximin.1
        );
        if deep.omitted_ours + deep.omitted_theirs > 0 {
            println!(
                "dropped {} of their beam replies and {} of our beam choices whose children reach effects the engine does not implement",
                deep.omitted_theirs, deep.omitted_ours
            );
        }
        println!("our deep mixed strategy (>= 1%; shallow probability in brackets):");
        for (choice, p) in deep.our_support(0.01).iter().take(top) {
            let shallow_p = deep
                .shallow
                .ours
                .iter()
                .position(|c| c == choice)
                .map_or(0.0, |i| deep.shallow.equilibrium.rows[i]);
            println!(
                "  {:>5.1}% [{:>5.1}%]  {}",
                p * 100.0,
                shallow_p * 100.0,
                describe(&position, deep.decision, us, choice)
            );
        }
        println!("their deep mixed strategy (>= 1%; shallow probability in brackets):");
        for (choice, p) in deep.their_support(0.01).iter().take(top) {
            let shallow_p = deep
                .shallow
                .theirs
                .iter()
                .position(|c| c == choice)
                .map_or(0.0, |i| deep.shallow.equilibrium.cols[i]);
            println!(
                "  {:>5.1}% [{:>5.1}%]  {}",
                p * 100.0,
                shallow_p * 100.0,
                describe(&position, deep.decision, them, choice)
            );
        }
        println!("deep matrix (rows: our beam, columns: their beam; values from our side):");
        for (r, a) in deep.ours.iter().enumerate() {
            let cells: Vec<String> = (0..deep.theirs.len())
                .map(|c| format!("{:>7.1}", deep.matrix.at(r, c)))
                .collect();
            println!(
                "  {}  | {}",
                cells.join(" "),
                describe(&position, deep.decision, us, a)
            );
        }
        for (c, b) in deep.theirs.iter().enumerate() {
            println!("  col {c}: {}", describe(&position, deep.decision, them, b));
        }
        if !deep.unsupported.is_empty() {
            println!("effects the engine does not implement met in the children (those pairs were dropped):");
            for why in &deep.unsupported {
                println!("  - {why}");
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
        if show_stats {
            println!("search stats: {}", solver.stats());
        }
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
        // `--lazy`: the root by double oracle (not over a mixture of surviving positions,
        // which averages full matrices).
        let lazy = lazy_root && survivors.len() <= 1;
        let (mixed, note) = if lazy {
            let mut state = position.state.clone();
            let mixed = solver
                .analyse_mixed_lazy(&mut state, None)
                .map_err(|e| e.to_string())?;
            (mixed, None)
        } else {
            analyse_positions(&mut solver, &position, &survivors, "position")?
        };
        if let Some(note) = note {
            println!("{note}");
        }
        println!(
            "decision {:?}, depth {}, chance {:?}, pruning {:?}, rolls {:?}, eval {eval}: {} nodes, {} enumerations, {:.2} s",
            mixed.decision, mixed.depth, config.chance, config.pruning, config.rolls, mixed.nodes, mixed.turns,
            mixed.elapsed.as_secs_f64()
        );
        if show_stats {
            println!("search stats: {}", solver.stats());
        }
        let valued = mixed.matrix.values.iter().filter(|v| !v.is_nan()).count();
        if lazy && valued < mixed.matrix.values.len() {
            println!(
                "matrix {}x{} ({} of {} pairs valued, double oracle); equilibrium value {:+.1} (exploitability in the full game {:.3}, {} RM+ iterations); pure maximin over the rows valued in full {:+.1} (a lower bound)",
                mixed.matrix.rows,
                mixed.matrix.cols,
                valued,
                mixed.matrix.values.len(),
                mixed.equilibrium.value,
                mixed.equilibrium.exploitability,
                mixed.equilibrium.iterations,
                mixed.maximin.1
            );
        } else {
            println!(
                "matrix {}x{}; equilibrium value {:+.1} (exploitability {:.3}, {} RM+ iterations); pure maximin {:+.1}",
                mixed.matrix.rows,
                mixed.matrix.cols,
                mixed.equilibrium.value,
                mixed.equilibrium.exploitability,
                mixed.equilibrium.iterations,
                mixed.maximin.1
            );
        }
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
    if show_stats {
        println!("search stats: {}", solver.stats());
    }
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
