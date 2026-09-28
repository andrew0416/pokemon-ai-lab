//! Self-play rollouts under the one-turn equilibrium policy: at every decision both sides draw
//! their choice from the matrix-game equilibrium `lab-plan --solve nash` would print (opponent
//! model ①, the same evaluator on both sides), the turn is then played once with exact chance,
//! and the game runs to its end. The tally of wins compares two leads or two teams on game
//! results rather than on the evaluator's scale — the evaluator still shapes every choice, so
//! this is "how the two do under the same policy", not their true strength (DESIGN.md "탐색의
//! 용도와 정보 모델", AGENTS.md "대전과 비교 방법").
//!
//! Usage: lab-rollout <scenario.json> [--games n] [--seed s] [--threads k] [--max-turns t]
//!                    [--policy nash|deep-nash [--beam b[,b2]] [--outcomes k[,k2]]]
//!                    [--rolls median|extremes|quartiles|full|pessimistic]
//!                    [--eval material|heuristic|file:<weights.json>]
//!                    [--setup-rolls full|median|extremes|quartiles] [--position i]
//!                    [--out results.json] [--quiet] [--lazy]
//!
//! The start is the scenario's position (after switch-ins, setup turns and patch); with several
//! initial states one is drawn per game by its probability (`--position` fixes one). `--rolls`
//! is the damage-roll mode the policy's matrix game is solved with (default median); the game
//! itself always runs with exact chance (`sample_turn`). `--policy deep-nash` draws from the
//! depth-2 mixed equilibrium instead (`Solver::analyse_deep_mixed`: both sides' `--beam` best
//! choices plus their shallow supports, children worth their next-turn equilibrium over the
//! `--outcomes` most probable outcomes) — much slower per decision, less bound to the
//! evaluator's one-turn view. Two entries (`--beam 3,2 --outcomes 2,1`) make it depth 3 (S24c):
//! the children are depth-2 analyses with the second level's beam and cap. Game `g` of seed `s` is deterministic
//! given the engine, the evaluator and the policy. A game the solver cannot value (an effect the
//! engine does not implement) is aborted, a game still going after `--max-turns` turns is a
//! cutoff; both are reported outside the win tally (AGENTS.md: 중단 경기는 승패 집계에서 제외).
//! With `--threads k` (default: the machine's cores) `k` games run at once, each solving its
//! matrix games on one thread; `--threads 1` runs the games in turn, each solving on all cores.
//! The policy's strategies are cached across games by position (state and suspension hash), so
//! the opening decision is solved once per initial state and repeated positions are reused;
//! the policy is deterministic, so the cache changes nothing but the time.

use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::RollMode;
use lab_scenario::{load_scenario_file, scenario_positions_with, Position};
use lab_search::model::{load_evaluator, side_name};
use lab_search::rollout::{
    run_games, team_files, wilson, winner_label, Ending, Policy, RolloutSettings,
};
use lab_search::{Config, DeepLevel};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-rollout: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut scenario: Option<String> = None;
    let mut games: usize = 20;
    let mut seed: u64 = 1;
    let mut threads: usize = 0;
    let mut max_turns: u16 = 30;
    let mut rolls = RollMode::Median;
    let mut setup_rolls = RollMode::Full;
    let mut policy = Policy::Nash;
    let mut beam: usize = 3;
    let mut outcomes: Option<usize> = Some(2);
    let mut eval = "heuristic".to_owned();
    let mut position_index: Option<usize> = None;
    let mut out: Option<String> = None;
    let mut quiet = false;
    let mut lazy = false;
    let mut rest_beam: Option<usize> = None;
    let mut rest_outcomes: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--games" => {
                i += 1;
                games = parse(&args, i, "--games")?;
            }
            "--seed" => {
                i += 1;
                seed = parse(&args, i, "--seed")?;
            }
            "--threads" => {
                i += 1;
                threads = parse(&args, i, "--threads")?;
            }
            "--max-turns" => {
                i += 1;
                max_turns = parse(&args, i, "--max-turns")?;
            }
            "--position" => {
                i += 1;
                position_index = Some(parse(&args, i, "--position")?);
            }
            "--rolls" => {
                i += 1;
                rolls = roll_mode(args.get(i), "--rolls")?;
            }
            "--policy" => {
                i += 1;
                policy = match args.get(i).map(String::as_str) {
                    Some("nash") => Policy::Nash,
                    Some("deep-nash") => Policy::DeepNash,
                    _ => return Err("--policy needs nash or deep-nash".into()),
                };
            }
            "--beam" => {
                i += 1;
                let list = parse_list(&args, i, "--beam")?;
                beam = list[0];
                rest_beam = list.get(1).copied();
                if list.len() > 2 {
                    return Err("--beam takes at most two levels (depth 3)".into());
                }
            }
            "--outcomes" => {
                i += 1;
                let list = parse_list(&args, i, "--outcomes")?;
                outcomes = Some(list[0]);
                rest_outcomes = list.get(1).copied();
                if list.len() > 2 {
                    return Err("--outcomes takes at most two levels (depth 3)".into());
                }
            }
            "--setup-rolls" => {
                i += 1;
                setup_rolls = roll_mode(args.get(i), "--setup-rolls")?;
            }
            "--eval" => {
                i += 1;
                eval = args
                    .get(i)
                    .cloned()
                    .ok_or("--eval needs material, heuristic or file:<weights.json>")?;
            }
            "--out" => {
                i += 1;
                out = Some(args.get(i).cloned().ok_or("--out needs a file")?);
            }
            "--quiet" => quiet = true,
            "--lazy" => lazy = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => scenario = Some(other.to_owned()),
        }
        i += 1;
    }
    let scenario = scenario.ok_or(
        "usage: lab-rollout <scenario.json> [--games n] [--seed s] [--threads k] [--max-turns t] \
         [--rolls median|extremes|quartiles|full|pessimistic] [--eval material|heuristic|file:<w.json>] \
         [--setup-rolls ...] [--position i] [--out results.json] [--quiet]",
    )?;
    if games == 0 {
        return Err("--games must be at least 1".into());
    }
    let game_threads = if threads == 0 {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        threads
    }
    .min(games);

    let loaded = load_scenario_file(&scenario).map_err(|e| e.to_string())?;
    let setup_options = lab_engine::turn::EnumerateOptions { rolls: setup_rolls };
    let mut positions = scenario_positions_with(&loaded, setup_options)?;
    if let Some(index) = position_index {
        let n = positions.len();
        let chosen = positions
            .into_iter()
            .nth(index)
            .ok_or_else(|| format!("--position {index}: only {n} initial states"))?;
        positions = vec![Position {
            probability: 1.0,
            ..chosen
        }];
    }
    let start_weights: Vec<f64> = positions.iter().map(|p| p.probability).collect();

    let evaluator = load_evaluator(&eval)?;

    // The policy: opponent model ① at depth 1, mixed. Each game's solver works on one thread
    // when games run in parallel, on all cores when they run in turn.
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = rolls;
    config.threads = if game_threads == 1 { 0 } else { 1 };
    config.outcome_cap = outcomes;
    // `--beam a,b` / `--outcomes a,b`: depth 3 (S24c), the second entry the children's level
    // (a missing one repeats the first).
    let deep_rest = (rest_beam.is_some() || rest_outcomes.is_some()).then(|| DeepLevel {
        beam: rest_beam.unwrap_or(beam),
        outcomes: rest_outcomes.or(outcomes),
    });
    let deep_depth = if deep_rest.is_some() { 3 } else { 2 };
    let policy_text = match policy {
        Policy::Nash if lazy => "nash depth 1 (double oracle)".to_owned(),
        Policy::Nash => "nash depth 1".to_owned(),
        Policy::DeepNash => match deep_rest {
            Some(rest) => format!(
                "deep-nash depth 3 beams {beam},{} outcomes {outcomes:?},{:?}",
                rest.beam, rest.outcomes
            ),
            None => format!("deep-nash beam {beam} outcomes {outcomes:?}"),
        },
    };

    let teams = team_files(&scenario)?;
    println!(
        "{}\nformat {}, {} initial state(s); {} games, seed {seed}, policy {policy_text} rolls {:?} eval {eval}, exact chance in play, max {max_turns} turns, {game_threads} game thread(s)",
        loaded.meta.description,
        loaded.meta.format,
        positions.len(),
        games,
        rolls
    );
    for (side, path, hash) in &teams {
        println!("  {side} team {path} (fnv1a64 {hash:016x})");
    }

    let started = Instant::now();
    let settings = RolloutSettings {
        config,
        max_turns,
        policy,
        beam,
        deep_rest,
        master_seed: seed,
        lazy,
    };
    let stdout = Mutex::new(std::io::stdout());
    let (records, cache) = run_games(
        &loaded,
        &positions,
        &start_weights,
        &settings,
        evaluator.as_ref(),
        games,
        game_threads,
        &|record| {
            if !quiet {
                let mut o = stdout.lock().unwrap();
                let _ = writeln!(
                    o,
                    "game {:>3} seed {:016x} start {}: {} after {} turn(s) ({:.1} s){}",
                    record.index,
                    record.seed,
                    record.start,
                    match &record.ending {
                        Ending::Win(side) => format!("{} wins", side_name(*side)),
                        Ending::Tie => "tie".to_owned(),
                        Ending::Cutoff => "cutoff".to_owned(),
                        Ending::Aborted(_) => "aborted".to_owned(),
                    },
                    record.turns,
                    record.elapsed,
                    match &record.ending {
                        Ending::Aborted(why) => format!(": {why}"),
                        _ => String::new(),
                    }
                );
            }
        },
    );
    let elapsed = started.elapsed().as_secs_f64();

    let count = |f: &dyn Fn(&Ending) -> bool| records.iter().filter(|r| f(&r.ending)).count();
    let p1 = count(&|e| *e == Ending::Win(SideId::One));
    let p2 = count(&|e| *e == Ending::Win(SideId::Two));
    let ties = count(&|e| *e == Ending::Tie);
    let cutoffs = count(&|e| *e == Ending::Cutoff);
    let aborted = count(&|e| matches!(e, Ending::Aborted(_)));
    let decided = p1 + p2 + ties;
    let (rate, lo, hi) = wilson(p1 as f64 + 0.5 * ties as f64, decided);
    let mean_turns = {
        let finished: Vec<f64> = records
            .iter()
            .filter(|r| matches!(r.ending, Ending::Win(_) | Ending::Tie))
            .map(|r| f64::from(r.turns))
            .collect();
        if finished.is_empty() {
            f64::NAN
        } else {
            finished.iter().sum::<f64>() / finished.len() as f64
        }
    };
    let (cache_hits, cache_misses) = (
        cache.hits.load(std::sync::atomic::Ordering::Relaxed),
        cache.misses.load(std::sync::atomic::Ordering::Relaxed),
    );
    println!(
        "\ntally: p1 {p1}, p2 {p2}, tie {ties} of {decided} decided; cutoff {cutoffs}, aborted {aborted}; p1 score {:.1}% (ties half; Wilson 95% {:.1}–{:.1}%), mean {:.1} turns, {:.1} s; strategy cache {cache_hits} hits / {cache_misses} solves",
        rate * 100.0,
        lo * 100.0,
        hi * 100.0,
        mean_turns,
        elapsed
    );
    let mut reasons: Vec<(&String, usize)> = Vec::new();
    for r in &records {
        if let Ending::Aborted(why) = &r.ending {
            match reasons.iter_mut().find(|(w, _)| *w == why) {
                Some(entry) => entry.1 += 1,
                None => reasons.push((why, 1)),
            }
        }
    }
    if !reasons.is_empty() {
        println!("aborted games:");
        for (why, n) in &reasons {
            println!("  {n} x {why}");
        }
    }

    if let Some(path) = out {
        let mut report = json!({
            "scenario": scenario,
            "description": loaded.meta.description,
            "format": loaded.meta.format,
            "teams": teams.iter().map(|(side, path, hash)| json!({"side": side, "file": path, "fnv1a64": format!("{hash:016x}")})).collect::<Vec<_>>(),
            "games": games,
            "seed": seed,
            "policy": {"solve": match policy { Policy::Nash => "nash", Policy::DeepNash => "deep-nash" }, "depth": match policy { Policy::Nash => 1, Policy::DeepNash => deep_depth }, "beam": match policy { Policy::Nash => Value::Null, Policy::DeepNash => json!(beam) }, "outcomes": outcomes, "children_level": match (policy, deep_rest) { (Policy::DeepNash, Some(rest)) => json!({"beam": rest.beam, "outcomes": rest.outcomes}), _ => Value::Null }, "rolls": format!("{rolls:?}"), "eval": eval, "chance_in_play": "exact (sample_turn)"},
            "max_turns": max_turns,
            "initial_states": positions.len(),
            "tally": {"p1": p1, "p2": p2, "tie": ties, "cutoff": cutoffs, "aborted": aborted, "decided": decided},
            "p1_score": rate,
            "wilson95": [lo, hi],
            "mean_turns": if mean_turns.is_nan() { Value::Null } else { json!(mean_turns) },
            "elapsed_s": elapsed,
            "strategy_cache": {"hits": cache_hits, "solves": cache_misses},
            "lab_search_version": env!("CARGO_PKG_VERSION"),
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
        });
        if lazy && policy == Policy::Nash {
            report["policy"]["double_oracle"] = json!(true);
        }
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
        println!("written {path}");
    }
    Ok(())
}

/// A number or two, comma-separated (`3` or `3,2`).
fn parse_list(args: &[String], i: usize, flag: &str) -> Result<Vec<usize>, String> {
    args.get(i)
        .and_then(|s| {
            s.split(',')
                .map(|x| x.trim().parse().ok())
                .collect::<Option<Vec<usize>>>()
        })
        .filter(|l| !l.is_empty())
        .ok_or_else(|| format!("{flag} needs a number or a list a,b"))
}

fn parse<T: std::str::FromStr>(args: &[String], i: usize, flag: &str) -> Result<T, String> {
    args.get(i)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("{flag} needs a number"))
}

fn roll_mode(arg: Option<&String>, flag: &str) -> Result<RollMode, String> {
    match arg.map(String::as_str) {
        Some("full") => Ok(RollMode::Full),
        Some("extremes") => Ok(RollMode::Extremes),
        Some("quartiles") => Ok(RollMode::Quartiles),
        Some("median") => Ok(RollMode::Median),
        Some("pessimistic") => Ok(RollMode::Pessimistic(SideId::One)),
        _ => Err(format!(
            "{flag} needs full, extremes, quartiles, median or pessimistic"
        )),
    }
}
