//! Parity positions from public Showdown replays (board V13-replay-parity).
//!
//! Usage: lab-replay <game.json>... --out-dir <dir> [--checks-dir <dir>] [--rolls full|extremes]
//!                   [--max-combos n]
//!
//! Each `<game.json>` is one replay as `engine/scripts/replay_parse.py` writes it: both teams
//! rebuilt from the log (open team sheet or the revealed sets, Stat Points assumed), the leads,
//! and every decision of the game with the actions the log shows (a move with its printed
//! target, Mega Evolution, a switch, `unknown` for a Pokémon that did not act) and the
//! observation after it (HP percentages as shown, statuses, faints, slots, formes, boosts,
//! known items, weather, terrain, pseudo-weathers, side conditions).
//!
//! The game is replayed on the engine: at each decision every legal choice pair consistent
//! with the logged actions is enumerated (`--rolls`, default full; mid-turn switches follow
//! the log's order), and the outcome closest to the log's observation is kept: fewest
//! structural differences (faint, status, slot, Mega, boosts, item, field), then the smallest
//! sum of HP-percentage differences, then the most probable. A move the log prints without a
//! target takes a foe. If no choice pair consistent with the printed targets reaches a
//! structurally equal outcome, a move may take any target on the printed target's side
//! (redirection, retargeting) and the search is repeated ("tier 2").
//!
//! Output per game, like `lab-parity`: `<dir>/<id>.s<step>.json`, one pinned scenario per
//! decision (`startState` the chosen lead outcome, `setupTurns` the earlier decisions pinned by
//! `setupStates`, `setupRolls` the roll mode, teams inlined), and `<checks-dir>/<id>.replay.json`
//! (default: the out dir):
//! the per-decision observation check (model ② consistency: structural differences and HP
//! distance of the closest outcome, number of outcomes, tier) and why the replay stopped.
//!
//! What the observation check can and cannot say: the teams' Stat Points (and, without an open
//! team sheet, unrevealed items and abilities) are assumed, and every pick is the closest
//! outcome rather than the logged one, so HP differences accumulate; a structural difference is
//! a candidate for an engine bug only after the assumptions are ruled out by hand.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use serde_json::{json, Map, Value};

use lab_engine::action::SlotAction;
use lab_engine::gimmick::Gimmick;
use lab_engine::instruction::Instruction;
use lab_engine::rules::Ruleset;
use lab_engine::state::{SideId, SlotRef, State};
use lab_engine::turn::{EnumerateOptions, RollMode, Suspension, RECHARGE_INDEX, STRUGGLE_INDEX};
use lab_scenario::{
    advance_order, canonical_value, load_scenario_str, scenario_positions, LoadedScenario,
    PartyOrder,
};
use lab_search::game::asked_slots;
use lab_search::{
    decision, format_choice, format_switches, legal_choices, transitions, Choice, Decision, Pruning,
};

const SIDES: [SideId; 2] = [SideId::One, SideId::Two];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-replay: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    rolls: RollMode,
    rolls_name: &'static str,
    max_combos: usize,
    out_dir: PathBuf,
    checks_dir: PathBuf,
    fit_budget: usize,
    fit_seconds: f64,
    fit_rolls: RollMode,
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut games: Vec<String> = Vec::new();
    let mut out_dir: Option<String> = None;
    let mut checks_dir: Option<String> = None;
    let mut rolls = (RollMode::Full, "full");
    let mut max_combos = 64usize;
    let mut fit_budget = 0usize;
    let mut fit_seconds = 60.0f64;
    let mut fit_rolls = RollMode::Extremes;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                i += 1;
                out_dir = args.get(i).cloned();
            }
            "--checks-dir" => {
                i += 1;
                checks_dir = args.get(i).cloned();
            }
            "--rolls" => {
                i += 1;
                rolls = match args.get(i).map(String::as_str) {
                    Some("full") => (RollMode::Full, "full"),
                    Some("extremes") => (RollMode::Extremes, "extremes"),
                    _ => return Err("--rolls needs full or extremes".into()),
                };
            }
            "--fit-sp" => {
                i += 1;
                fit_budget = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--fit-sp needs a number")?;
            }
            "--fit-seconds" => {
                i += 1;
                fit_seconds = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--fit-seconds needs a number")?;
            }
            "--fit-rolls" => {
                i += 1;
                fit_rolls = match args.get(i).map(String::as_str) {
                    Some("full") => RollMode::Full,
                    Some("extremes") => RollMode::Extremes,
                    _ => return Err("--fit-rolls needs full or extremes".into()),
                };
            }
            "--max-combos" => {
                i += 1;
                max_combos = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or("--max-combos needs a number")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => games.push(other.to_owned()),
        }
        i += 1;
    }
    if games.is_empty() {
        return Err("usage: lab-replay <game.json>... --out-dir <dir> [--rolls full|extremes] [--max-combos n]".into());
    }
    let out_dir = PathBuf::from(out_dir.ok_or("--out-dir is required")?);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let checks_dir = checks_dir.map_or_else(|| out_dir.clone(), PathBuf::from);
    std::fs::create_dir_all(&checks_dir).map_err(|e| format!("{}: {e}", checks_dir.display()))?;
    let options = Options {
        rolls: rolls.0,
        rolls_name: rolls.1,
        max_combos,
        out_dir,
        checks_dir,
        fit_budget,
        fit_seconds,
        fit_rolls,
    };
    for game in &games {
        let started = Instant::now();
        match replay_game(game, &options) {
            Ok(summary) => println!(
                "{game}: {summary} ({:.1} s)",
                started.elapsed().as_secs_f64()
            ),
            Err(e) => println!("{game}: error: {e}"),
        }
    }
    Ok(())
}

/// One decision as the replay played it.
struct Step {
    turn: u64,
    kind: &'static str,
    p1: String,
    p2: String,
    mid_turn: [Vec<String>; 2],
    after: Value,
}

/// One outcome path of a choice pair: the instruction lists of its stages (the turn, then each
/// mid-turn continuation) and the mid-turn choice strings.
#[derive(Clone)]
struct Path_ {
    stages: Vec<Vec<Instruction>>,
    mid_turn: [Vec<String>; 2],
    probability: f64,
}

#[derive(Clone)]
struct Best {
    structural: Vec<String>,
    hp: i64,
    probability: f64,
    path: Path_,
    choices: [String; 2],
}

impl Best {
    fn better_than(&self, other: &Best) -> bool {
        (self.structural.len(), self.hp)
            .cmp(&(other.structural.len(), other.hp))
            .then_with(|| other.probability.total_cmp(&self.probability))
            .is_lt()
    }
}

/// One replay of a game's decisions on given teams.
struct Run {
    start_state: Value,
    steps: Vec<Step>,
    checks: Vec<Value>,
    stop: Option<String>,
    diverged_at: Option<usize>,
    /// Sum of the HP distances of the decisions before the first divergence.
    hp_before: i64,
}

impl Run {
    /// Fitting objective: more decisions replayed without a structural difference, then a
    /// smaller HP distance over them (larger is better).
    fn fit_key(&self) -> (usize, i64) {
        let consistent = self.diverged_at.unwrap_or(self.steps.len());
        (consistent, -self.hp_before)
    }
}

fn run_replay(
    game: &Value,
    teams: &[Value; 2],
    rolls: RollMode,
    max_combos: usize,
    stop_on_divergence: bool,
) -> Result<Run, String> {
    let scenario = json!({
        "description": "V13 replay",
        "format": game["format"],
        "p1": {"team": teams[0], "order": game["p1"]["order"]},
        "p2": {"team": teams[1], "order": game["p2"]["order"]},
    });
    let loaded = load_scenario_str(&scenario.to_string(), Path::new("."))
        .map_err(|e| format!("load: {e}"))?;
    let positions = scenario_positions(&loaded).map_err(|e| format!("start: {e}"))?;
    // The lead outcome closest to the observation at turn 1.
    let start_obs = &game["startObs"];
    let mut start = 0;
    let mut start_score = (usize::MAX, i64::MAX);
    for (k, p) in positions.iter().enumerate() {
        let c = canonical_value(&p.state, &loaded.meta).map_err(|e| format!("canonical: {e}"))?;
        let (s, hp) = score(&c, start_obs);
        if (s.len(), hp) < start_score {
            start_score = (s.len(), hp);
            start = k;
        }
    }
    let mut state = positions[start].state.clone();
    let mut order = positions[start].order.clone();
    let start_state = canonical_value(&state, &loaded.meta).map_err(|e| e.to_string())?;
    let mut steps: Vec<Step> = Vec::new();
    let mut checks: Vec<Value> = Vec::new();
    let mut stop: Option<String> = None;
    let mut diverged_at: Option<usize> = None;
    let empty = Vec::new();
    let decisions = game["decisions"].as_array().unwrap_or(&empty);
    for (k, d) in decisions.iter().enumerate() {
        let started = Instant::now();
        let turn = d["turn"].as_u64().unwrap_or(0);
        let kind = d["kind"].as_str().unwrap_or("");
        let dec = match decision(&state, None) {
            Ok(x) => x,
            Err(e) => {
                stop = Some(format!("decision {k} (turn {turn} {kind}): {e}"));
                break;
            }
        };
        let expected = matches!(
            (kind, dec),
            ("turn", Decision::Turn) | ("replacement", Decision::Replacement)
        );
        if !expected {
            stop = Some(format!(
                "decision {k} (turn {turn} {kind}): the engine's position asks for {dec:?}"
            ));
            break;
        }
        let mut best: Option<Best> = None;
        let mut tier = 1;
        let mut outcomes_seen = 0usize;
        let mut combos_tried = 0usize;
        let mut truncated = false;
        let mut error: Option<String> = None;
        let mut tier1: Option<Value> = None;
        for t in [1, 2] {
            let mut cands: [Vec<Choice<2>>; 2] = [Vec::new(), Vec::new()];
            for side in SIDES {
                let wanted = &d[side_key(side)];
                let all = legal_choices(&state, Ruleset::CHAMPIONS_MC, dec, side, Pruning::All);
                cands[side.index()] = all
                    .into_iter()
                    .filter(|c| matches(&state, &loaded, &order, dec, side, c, wanted, t == 2))
                    .collect();
            }
            if cands.iter().any(Vec::is_empty) {
                if t == 1 {
                    continue;
                }
                let side = if cands[0].is_empty() { "p1" } else { "p2" };
                error = Some(format!(
                    "no legal {side} choice fits the log's actions {}",
                    d[side]
                ));
                break;
            }
            let mut pairs = Vec::new();
            'outer: for a in &cands[0] {
                for b in &cands[1] {
                    if pairs.len() >= max_combos {
                        truncated = true;
                        break 'outer;
                    }
                    pairs.push([*a, *b]);
                }
            }
            for pair in pairs {
                combos_tried += 1;
                let strings = [
                    describe(&state, dec, SideId::One, &order, &pair[0]),
                    describe(&state, dec, SideId::Two, &order, &pair[1]),
                ];
                let paths = match expand(&mut state, &loaded, &order, dec, None, pair, d, rolls) {
                    Ok(p) => p,
                    Err(e) => {
                        error = Some(format!("{strings:?}: {e}"));
                        continue;
                    }
                };
                for path in paths {
                    outcomes_seen += 1;
                    for stage in &path.stages {
                        state.apply(stage);
                    }
                    let canonical = canonical_value(&state, &loaded.meta);
                    for stage in path.stages.iter().rev() {
                        state.reverse(stage);
                    }
                    let canonical = match canonical {
                        Ok(c) => c,
                        Err(e) => {
                            error = Some(format!("canonical: {e}"));
                            continue;
                        }
                    };
                    let (structural, hp) = score(&canonical, &d["obs"]);
                    if std::env::var("LAB_REPLAY_DEBUG").ok().as_deref() == Some(&k.to_string()) {
                        eprintln!(
                            "{strings:?} p={:.4} hp={hp} {structural:?}",
                            path.probability
                        );
                    }
                    let candidate = Best {
                        structural,
                        hp,
                        probability: path.probability,
                        path,
                        choices: strings.clone(),
                    };
                    if best.as_ref().is_none_or(|b| candidate.better_than(b)) {
                        best = Some(candidate);
                    }
                }
            }
            if best.as_ref().is_some_and(|b| b.structural.is_empty()) {
                tier = t;
                break;
            }
            if t == 1 {
                tier1 = best.as_ref().map(|b| {
                    json!({"choices": b.choices, "structural": b.structural, "hpDistance": b.hp})
                });
            }
            tier = t;
        }
        let Some(best) = best else {
            stop = Some(format!(
                "decision {k} (turn {turn} {kind}): {}",
                error.unwrap_or_else(|| "no outcome".into())
            ));
            break;
        };
        let mut instructions_all = Vec::new();
        for stage in &best.path.stages {
            state.apply(stage);
            advance_order(&mut order, stage);
            instructions_all.extend(stage.iter().cloned());
        }
        let after = canonical_value(&state, &loaded.meta).map_err(|e| e.to_string())?;
        if !best.structural.is_empty() && diverged_at.is_none() {
            diverged_at = Some(k);
        }
        checks.push(json!({
            "step": k,
            "turn": turn,
            "kind": kind,
            "choices": best.choices,
            "midTurn": best.path.mid_turn,
            "tier": tier,
            "tier1": tier1,
            "structural": best.structural,
            "hpDistance": best.hp,
            "probability": best.probability,
            "outcomes": outcomes_seen,
            "combos": combos_tried,
            "truncated": truncated,
            "error": error,
            "afterDivergence": diverged_at.is_some_and(|x| x < k),
            "ms": started.elapsed().as_millis() as u64,
        }));
        steps.push(Step {
            turn,
            kind: if kind == "turn" {
                "Turn"
            } else {
                "Replacement"
            },
            p1: best.choices[0].clone(),
            p2: best.choices[1].clone(),
            mid_turn: best.path.mid_turn.clone(),
            after,
        });
        if state.result.is_over() || (stop_on_divergence && diverged_at.is_some()) {
            break;
        }
    }
    let hp_before = checks
        .iter()
        .take(diverged_at.unwrap_or(checks.len()))
        .map(|c| c["hpDistance"].as_i64().unwrap_or(0))
        .sum();
    Ok(Run {
        start_state,
        steps,
        checks,
        stop,
        diverged_at,
        hp_before,
    })
}
/// Stat Point presets a Pokémon's assumed spread is fitted over: `off` is its attacking stat.
const PRESETS: [&[(&str, u8)]; 8] = [
    &[("hp", 32), ("off", 32), ("spe", 2)],
    &[("hp", 2), ("off", 32), ("spe", 32)],
    &[("hp", 32), ("def", 32), ("spd", 2)],
    &[("hp", 32), ("spd", 32), ("def", 2)],
    &[("hp", 32), ("def", 17), ("spd", 17)],
    &[("hp", 32), ("spe", 32), ("off", 2)],
    &[("hp", 2), ("off", 32), ("def", 32)],
    &[("hp", 2), ("off", 32), ("spd", 32)],
];

/// A preset as an `evs` object; the attacking stat is the one the parser's default spread
/// raised (`replay_parse.py`: the category its moves use most).
fn preset_evs(set: &Value, off: &str, preset: &[(&str, u8)]) -> Value {
    let _ = set;
    let mut out = json!({"hp": 0, "atk": 0, "def": 0, "spa": 0, "spd": 0, "spe": 0});
    for (stat, v) in preset {
        let stat = if *stat == "off" { off } else { stat };
        out[stat] = json!(v);
    }
    out
}

fn attacking_stat(set: &Value) -> &'static str {
    let evs = &set["evs"];
    if evs["spa"].as_u64().unwrap_or(0) > evs["atk"].as_u64().unwrap_or(0) {
        "spa"
    } else {
        "atk"
    }
}

/// The Pokémon (side, team index) the first divergence of `run` involves: the ones its
/// differences name, and every Pokémon active in the log's observation before it.
fn involved(game: &Value, teams: &[Value; 2], run: &Run) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let Some(k) = run.diverged_at else {
        return out;
    };
    let text = run.checks[k]["structural"].to_string();
    let before = if k == 0 {
        &game["startObs"]
    } else {
        &game["decisions"][k - 1]["obs"]
    };
    for (side, team) in teams.iter().enumerate() {
        let key = if side == 0 { "p1" } else { "p2" };
        for (i, set) in team.as_array().into_iter().flatten().enumerate() {
            let name = set["name"].as_str().unwrap_or("");
            let named = text.contains(&format!("{key}:{name} "));
            let active = before["mons"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|m| m["side"] == key && m["name"] == name && !m["slot"].is_null());
            if named || active {
                out.push((side, i));
            }
        }
    }
    out
}

/// One assumed-spread change the fit tries.
#[derive(Clone)]
struct Change {
    side: usize,
    i: usize,
    evs: Value,
    nature: Value,
}

/// The spreads the fit tries for a set: every preset with a neutral nature (or the open team
/// sheet's), plus the bulk and offense presets with a nature raising that stat;
/// `pair`: only the bulk presets and the frail-offense ones (for two-Pokémon changes).
fn candidates(set: &Value, off: &str, ots: bool, pair: bool) -> Vec<Change> {
    let base_nature = if ots {
        set["nature"].clone()
    } else {
        json!("Serious")
    };
    let physical = off == "atk";
    let mut out = Vec::new();
    let mut push = |p: usize, nature: Value| {
        out.push(Change {
            side: 0,
            i: 0,
            evs: preset_evs(set, off, PRESETS[p]),
            nature,
        });
    };
    let presets: &[usize] = if pair {
        &[2, 3, 4, 0, 1]
    } else {
        &[0, 1, 2, 3, 4, 5, 6, 7]
    };
    for &p in presets {
        push(p, base_nature.clone());
    }
    if !ots {
        push(2, json!(if physical { "Impish" } else { "Bold" }));
        push(3, json!(if physical { "Careful" } else { "Calm" }));
        push(0, json!(if physical { "Adamant" } else { "Modest" }));
        if !pair {
            push(1, json!(if physical { "Jolly" } else { "Timid" }));
        }
    }
    out
}

/// The Pokémon the first divergence's differences name.
fn named_in_divergence(teams: &[Value; 2], run: &Run) -> Vec<(usize, usize)> {
    let Some(k) = run.diverged_at else {
        return Vec::new();
    };
    let text = run.checks[k]["structural"].to_string();
    let mut out = Vec::new();
    for (side, team) in teams.iter().enumerate() {
        let key = if side == 0 { "p1" } else { "p2" };
        for (i, set) in team.as_array().into_iter().flatten().enumerate() {
            let name = set["name"].as_str().unwrap_or("");
            if text.contains(&format!("{key}:{name} ")) {
                out.push((side, i));
            }
        }
    }
    out
}

fn replay_game(file: &str, options: &Options) -> Result<String, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let mut game: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
    let id = game["id"].as_str().ok_or("game without id")?.to_owned();
    let mut teams = [game["p1"]["team"].clone(), game["p2"]["team"].clone()];
    let offense: Vec<Vec<&'static str>> = teams
        .iter()
        .map(|t| {
            t.as_array()
                .into_iter()
                .flatten()
                .map(attacking_stat)
                .collect()
        })
        .collect();
    // Stat Point fit (`--fit-sp n`): coordinate descent over the presets of the Pokémon each
    // divergence involves, on quick replays (`--fit-rolls`) that stop at the first divergence.
    let mut fit_log: Vec<Value> = Vec::new();
    let mut fit_runs = 0usize;
    let mut initial_consistent: Option<usize> = None;
    if options.fit_budget > 0 {
        let mut current = run_replay(&game, &teams, options.fit_rolls, options.max_combos, true)?;
        fit_runs += 1;
        initial_consistent = Some(current.fit_key().0);
        let fit_start = Instant::now();
        let ots = game["ots"].as_bool() == Some(true);
        let out_of_budget = |runs: usize| {
            runs >= options.fit_budget || fit_start.elapsed().as_secs_f64() > options.fit_seconds
        };
        'fit: while current.diverged_at.is_some() && !out_of_budget(fit_runs) {
            let involved = involved(&game, &teams, &current);
            let named = named_in_divergence(&teams, &current);
            // Single changes first; pairs (a Pokémon the difference names, made bulkier or
            // frailer, with another involved one) only when no single change helps.
            let mut trials: Vec<Vec<Change>> = Vec::new();
            for &(side, i) in &involved {
                for c in candidates(&teams[side][i], offense[side][i], ots, false) {
                    trials.push(vec![Change { side, i, ..c }]);
                }
            }
            let singles = trials.len();
            for &(sa, ia) in &named {
                for &(sb, ib) in &involved {
                    if (sa, ia) == (sb, ib) {
                        continue;
                    }
                    for ca in candidates(&teams[sa][ia], offense[sa][ia], ots, true) {
                        for cb in candidates(&teams[sb][ib], offense[sb][ib], ots, true) {
                            trials.push(vec![
                                Change {
                                    side: sa,
                                    i: ia,
                                    ..ca.clone()
                                },
                                Change {
                                    side: sb,
                                    i: ib,
                                    ..cb
                                },
                            ]);
                        }
                    }
                }
            }
            let mut best: Option<(Run, Vec<Change>)> = None;
            for (t, trial) in trials.into_iter().enumerate() {
                if t == singles && best.is_some() {
                    break;
                }
                if out_of_budget(fit_runs) {
                    break;
                }
                let mut team_trial = teams.clone();
                let mut changed = false;
                for c in &trial {
                    let set = &mut team_trial[c.side][c.i];
                    if set["evs"] != c.evs || set["nature"] != c.nature {
                        changed = true;
                    }
                    set["evs"] = c.evs.clone();
                    set["nature"] = c.nature.clone();
                }
                if !changed {
                    continue;
                }
                fit_runs += 1;
                let Ok(run) = run_replay(
                    &game,
                    &team_trial,
                    options.fit_rolls,
                    options.max_combos,
                    true,
                ) else {
                    continue;
                };
                let reference = best.as_ref().map_or(current.fit_key(), |b| b.0.fit_key());
                if run.fit_key() > reference {
                    best = Some((run, trial));
                }
            }
            let Some((run, trial)) = best else {
                break 'fit;
            };
            for c in &trial {
                teams[c.side][c.i]["evs"] = c.evs.clone();
                teams[c.side][c.i]["nature"] = c.nature.clone();
                fit_log.push(json!({
                    "side": if c.side == 0 { "p1" } else { "p2" },
                    "name": teams[c.side][c.i]["name"],
                    "evs": c.evs,
                    "nature": c.nature,
                    "consistentDecisions": run.fit_key().0,
                }));
            }
            current = run;
        }
    }
    game["p1"]["team"] = teams[0].clone();
    game["p2"]["team"] = teams[1].clone();
    let Run {
        start_state,
        steps,
        checks,
        stop,
        diverged_at,
        ..
    } = run_replay(&game, &teams, options.rolls, options.max_combos, false)?;
    let empty = Vec::new();
    let decisions = game["decisions"].as_array().unwrap_or(&empty);
    // Pinned scenarios, one per decision.
    let mut files = Vec::new();
    for s in 0..steps.len() {
        let file = format!("{id}.s{s:02}.json");
        let setup: Vec<Value> = steps[..s]
            .iter()
            .map(|t| {
                if t.mid_turn.iter().all(Vec::is_empty) {
                    json!([t.p1, t.p2])
                } else {
                    json!([t.p1, t.p2, {"p1": t.mid_turn[0], "p2": t.mid_turn[1]}])
                }
            })
            .collect();
        let states: Vec<Value> = steps[..s].iter().map(|t| t.after.clone()).collect();
        let step = &steps[s];
        let mut out = json!({
            "description": format!(
                "V13 replay position (V13-replay-parity): {} ({}), decision {s} = turn {} {}. Teams rebuilt from the log{} with assumed Stat Points; the earlier decisions are the logged choices, pinned by setupStates to the engine outcome closest to the log. Not a recommendation.",
                id,
                game["url"].as_str().unwrap_or(""),
                step.turn,
                step.kind,
                if game["ots"].as_bool() == Some(true) { " and open team sheets" } else { "" }
            ),
            "format": game["format"],
            "p1": game["p1"],
            "p2": game["p2"],
            "startState": start_state,
            "setupRolls": options.rolls_name,
            "setupTurns": setup,
            "setupStates": states,
            "turn": {"p1": step.p1, "p2": step.p2},
        });
        if step.mid_turn.iter().any(|m| !m.is_empty()) {
            out["midTurn"] = json!({"p1": step.mid_turn[0], "p2": step.mid_turn[1]});
        }
        let text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
        std::fs::write(options.out_dir.join(&file), text).map_err(|e| format!("{file}: {e}"))?;
        files.push(file);
    }
    let consistent = checks
        .iter()
        .filter(|c| c["structural"].as_array().is_some_and(Vec::is_empty))
        .count();
    let summary = json!({
        "id": id,
        "url": game["url"],
        "ots": game["ots"],
        "rolls": options.rolls_name,
        "decisions": decisions.len(),
        "replayed": steps.len(),
        "consistent": consistent,
        "divergedAt": diverged_at,
        "stop": stop,
        "parseStop": game["stop"],
        "spFit": {"budget": options.fit_budget, "runs": fit_runs, "initialConsistent": initial_consistent, "changes": fit_log},
        "positions": files,
        "checks": checks,
    });
    let path = options.checks_dir.join(format!("{id}.replay.json"));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!(
        "{}/{} decisions replayed, {} structurally consistent, diverged at {:?}, stop {:?}",
        steps.len(),
        decisions.len(),
        consistent,
        diverged_at,
        stop
    ))
}

fn side_key(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        _ => "p2",
    }
}

fn describe(
    state: &State<2>,
    decision: Decision,
    side: SideId,
    order: &[PartyOrder; 2],
    choice: &Choice<2>,
) -> String {
    match choice {
        Choice::Turn(action) => format_choice(state, side, &order[side.index()], action),
        Choice::Switches(switches) => format_switches(
            &order[side.index()],
            &asked_slots(state, decision, side),
            switches,
        ),
    }
}

fn move_id(state: &State<2>, slot: SlotRef, index: u8) -> String {
    if index == RECHARGE_INDEX {
        return "recharge".into();
    }
    if index == STRUGGLE_INDEX {
        return "struggle".into();
    }
    state
        .active(slot)
        .and_then(|mon| mon.moves.get(index as usize))
        .map(|m| m.id.id().to_owned())
        .unwrap_or_default()
}

/// Whether a legal choice fits the log's actions for this side (`relaxed`: any target).
#[allow(clippy::too_many_arguments)]
fn matches(
    state: &State<2>,
    loaded: &LoadedScenario,
    _order: &[PartyOrder; 2],
    dec: Decision,
    side: SideId,
    choice: &Choice<2>,
    wanted: &Value,
    relaxed: bool,
) -> bool {
    let meta = &loaded.meta.sides[side.index()];
    let Some(wanted) = wanted.as_array() else {
        return false;
    };
    match choice {
        Choice::Turn(action) => {
            for (i, a) in action.iter().enumerate() {
                let w = &wanted[i.min(wanted.len().saturating_sub(1))];
                let kind = w["kind"].as_str().unwrap_or("unknown");
                let slot = SlotRef {
                    side,
                    slot: i as u8,
                };
                let mega = w["mega"].as_bool().unwrap_or(false);
                let ok = match (kind, a) {
                    ("none", SlotAction::Pass) => true,
                    ("none", _) => false,
                    (_, SlotAction::Pass) => state.active(slot).is_none_or(|m| m.hp <= 0),
                    ("switch", SlotAction::Switch { party_index }) => {
                        meta.party_index(w["name"].as_str().unwrap_or("")) == Some(*party_index)
                    }
                    ("switch", _) => false,
                    ("unknown", SlotAction::Move { gimmick, .. }) => {
                        (*gimmick == Gimmick::Mega) == mega
                    }
                    ("unknown", _) => false,
                    (
                        "move",
                        SlotAction::Move {
                            index,
                            target,
                            gimmick,
                        },
                    ) => {
                        let id = move_id(state, slot, *index);
                        let wanted_id = w["move"].as_str().unwrap_or("");
                        let id_ok = id == wanted_id
                            || *index == RECHARGE_INDEX
                            || (*index == STRUGGLE_INDEX && wanted_id == "struggle");
                        // Tier 2 keeps the printed target's side (redirection and
                        // retargeting move a hit between foes, not onto an ally).
                        let wanted_loc = wanted_target(side, &w["target"]);
                        let target_ok = *target == 0
                            || match wanted_loc {
                                None => *target > 0,
                                Some(loc) if relaxed => loc.signum() == target.signum(),
                                Some(loc) => loc == *target,
                            };
                        id_ok && target_ok && ((*gimmick == Gimmick::Mega) == mega)
                    }
                    ("move", _) => false,
                    _ => false,
                };
                if !ok {
                    return false;
                }
            }
            true
        }
        Choice::Switches(switches) => {
            let asked = asked_slots(state, dec, side);
            for i in 0..2 {
                let w = &wanted[i.min(wanted.len().saturating_sub(1))];
                let want = if w["kind"] == "switch" {
                    meta.party_index(w["name"].as_str().unwrap_or(""))
                } else {
                    None
                };
                if asked.contains(&i) {
                    if switches[i] != want {
                        return false;
                    }
                } else if want.is_some() {
                    return false;
                }
            }
            true
        }
    }
}

/// The log's target (`{"side": "p2", "slot": 1}`) as Showdown's relative target location.
fn wanted_target(user: SideId, target: &Value) -> Option<i8> {
    let side = target["side"].as_str()?;
    let slot = target["slot"].as_i64()? as i8;
    if side == side_key(user) {
        Some(-(slot + 1))
    } else {
        Some(slot + 1)
    }
}

/// Every outcome path of `pair` at `dec`, mid-turn switches taken from the log in order.
#[allow(clippy::too_many_arguments)]
fn expand(
    state: &mut State<2>,
    loaded: &LoadedScenario,
    order: &[PartyOrder; 2],
    dec: Decision,
    suspension: Option<&Suspension>,
    pair: [Choice<2>; 2],
    d: &Value,
    rolls: RollMode,
) -> Result<Vec<Path_>, String> {
    let outcomes = transitions(
        state,
        Ruleset::CHAMPIONS_MC,
        EnumerateOptions { rolls },
        dec,
        suspension,
        pair,
    )
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for o in outcomes {
        let base = Path_ {
            stages: vec![o.instructions.clone()],
            mid_turn: [Vec::new(), Vec::new()],
            probability: o.probability,
        };
        match o.suspension {
            None => out.push(base),
            Some(s) => {
                let mut order2 = order.clone();
                state.apply(&o.instructions);
                advance_order(&mut order2, &o.instructions);
                let rest = continue_mid(state, loaded, &order2, &s, d, rolls, [0, 0]);
                state.reverse(&o.instructions);
                for r in rest? {
                    let mut p = base.clone();
                    p.stages.extend(r.stages);
                    p.mid_turn = r.mid_turn;
                    p.probability *= r.probability;
                    out.push(p);
                }
            }
        }
    }
    Ok(out)
}

/// The paths after a suspension: each asked side switches to the next Pokémon the log's
/// mid-turn list names (`used`: how many of each side's list this branch has consumed).
fn continue_mid(
    state: &mut State<2>,
    loaded: &LoadedScenario,
    order: &[PartyOrder; 2],
    s: &Suspension,
    d: &Value,
    rolls: RollMode,
    used: [usize; 2],
) -> Result<Vec<Path_>, String> {
    let mid = decision(state, Some(s)).map_err(|e| e.to_string())?;
    let mut pair = [Choice::WAIT; 2];
    let mut strings: [Option<String>; 2] = [None, None];
    let mut used2 = used;
    for side in SIDES {
        let asked = asked_slots(state, mid, side);
        if asked.is_empty() {
            continue;
        }
        let list = d["mid"][side_key(side)].as_array();
        let name = list
            .and_then(|l| l.get(used[side.index()]))
            .and_then(Value::as_str);
        let Some(name) = name else {
            // The log has no switch left for this side: this branch is not the logged one.
            return Ok(Vec::new());
        };
        let meta = &loaded.meta.sides[side.index()];
        let Some(party) = meta.party_index(name) else {
            return Ok(Vec::new());
        };
        let legal = legal_choices(state, Ruleset::CHAMPIONS_MC, mid, side, Pruning::All);
        let Some(choice) = legal.into_iter().find(|c| match c {
            Choice::Switches(sw) => sw.contains(&Some(party)),
            _ => false,
        }) else {
            return Ok(Vec::new());
        };
        strings[side.index()] = Some(describe(state, mid, side, order, &choice));
        pair[side.index()] = choice;
        used2[side.index()] += 1;
    }
    let outcomes = transitions(
        state,
        Ruleset::CHAMPIONS_MC,
        EnumerateOptions { rolls },
        mid,
        Some(s),
        pair,
    )
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for o in outcomes {
        let mut base = Path_ {
            stages: vec![o.instructions.clone()],
            mid_turn: [Vec::new(), Vec::new()],
            probability: o.probability,
        };
        for side in SIDES {
            if let Some(t) = &strings[side.index()] {
                base.mid_turn[side.index()].push(t.clone());
            }
        }
        match o.suspension {
            None => out.push(base),
            Some(s2) => {
                let mut order2 = order.clone();
                state.apply(&o.instructions);
                advance_order(&mut order2, &o.instructions);
                let rest = continue_mid(state, loaded, &order2, &s2, d, rolls, used2);
                state.reverse(&o.instructions);
                for r in rest? {
                    let mut p = base.clone();
                    p.stages.extend(r.stages);
                    for side in 0..2 {
                        p.mid_turn[side].extend(r.mid_turn[side].iter().cloned());
                    }
                    p.probability *= r.probability;
                    out.push(p);
                }
            }
        }
    }
    Ok(out)
}

/// Structural differences between a canonical state and the log's observation, and the sum
/// of HP-percentage differences of the Pokémon alive in both.
fn score(canonical: &Value, obs: &Value) -> (Vec<String>, i64) {
    let mut diffs = Vec::new();
    let mut hp = 0i64;
    let empty = Vec::new();
    for o in obs["mons"].as_array().unwrap_or(&empty) {
        let side = if o["side"] == "p1" { 0 } else { 1 };
        let name = o["name"].as_str().unwrap_or("");
        let Some(c) = canonical["sides"][side]["pokemon"]
            .as_array()
            .and_then(|l| l.iter().find(|p| p["name"] == name))
        else {
            diffs.push(format!("{name}: missing"));
            continue;
        };
        let c_hp = c["hp"].as_i64().unwrap_or(0);
        let c_max = c["maxhp"].as_i64().unwrap_or(1).max(1);
        let c_fnt = c_hp <= 0 || c["status"] == "fnt";
        let o_fnt = o["fainted"].as_bool().unwrap_or(false);
        let label = format!("{}:{name}", if side == 0 { "p1" } else { "p2" });
        if c_fnt != o_fnt {
            diffs.push(format!(
                "{label} faint engine {c_fnt} log {o_fnt} (engine hp {c_hp}/{c_max}, log {}%)",
                o["pct"]
            ));
            continue;
        }
        if c_fnt {
            continue;
        }
        let pct = ((100 * c_hp) / c_max).max(1);
        hp += (pct - o["pct"].as_i64().unwrap_or(0)).abs();
        let c_status = c["status"].as_str().unwrap_or("");
        let o_status = o["status"].as_str().unwrap_or("");
        if c_status != o_status {
            diffs.push(format!(
                "{label} status engine {c_status:?} log {o_status:?}"
            ));
        }
        let c_slot = c["slot"].as_i64();
        let o_slot = o["slot"].as_i64();
        if c_slot != o_slot {
            diffs.push(format!("{label} slot engine {c_slot:?} log {o_slot:?}"));
        }
        let c_mega = c["species"].as_str().unwrap_or("").contains("-Mega");
        let o_mega = o["species"].as_str().unwrap_or("").contains("-Mega");
        if c_mega != o_mega {
            diffs.push(format!(
                "{label} forme engine {} log {}",
                c["species"], o["species"]
            ));
        }
        if o_slot.is_some() {
            let norm = |v: &Value| -> Map<String, Value> {
                v.as_object()
                    .map(|m| {
                        m.iter()
                            .filter(|(_, x)| x.as_i64() != Some(0))
                            .map(|(k, x)| (k.clone(), x.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let cb = norm(&c["boosts"]);
            let ob = norm(&o["boosts"]);
            if cb != ob {
                diffs.push(format!(
                    "{label} boosts engine {} log {}",
                    Value::Object(cb),
                    Value::Object(ob)
                ));
            }
        }
        if let Some(item) = o.get("item").and_then(Value::as_str) {
            let c_item = c["item"].as_str().unwrap_or("");
            if c_item != item {
                diffs.push(format!("{label} item engine {c_item:?} log {item:?}"));
            }
        }
    }
    let field = &canonical["field"];
    let c_weather = field["weather"].as_str().unwrap_or("");
    let o_weather = obs["weather"].as_str().unwrap_or("");
    if c_weather != o_weather {
        diffs.push(format!("weather engine {c_weather:?} log {o_weather:?}"));
    }
    let c_terrain = field["terrain"].as_str().unwrap_or("");
    let o_terrain = obs["terrain"].as_str().unwrap_or("");
    if c_terrain != o_terrain {
        diffs.push(format!("terrain engine {c_terrain:?} log {o_terrain:?}"));
    }
    let keys = |v: &Value| -> Vec<String> {
        let mut k: Vec<String> = v
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        k.sort();
        k
    };
    let strings = |v: &Value| -> Vec<String> {
        let mut k: Vec<String> = v
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        k.sort();
        k
    };
    let c_pseudo = keys(&field["pseudoWeather"]);
    let o_pseudo = strings(&obs["pseudoWeather"]);
    if c_pseudo != o_pseudo {
        diffs.push(format!(
            "pseudoWeather engine {c_pseudo:?} log {o_pseudo:?}"
        ));
    }
    for (i, side) in ["p1", "p2"].into_iter().enumerate() {
        let c_cond = keys(&canonical["sides"][i]["conditions"]);
        let o_cond = strings(&obs["sideConditions"][side]);
        if c_cond != o_cond {
            diffs.push(format!(
                "{side} conditions engine {c_cond:?} log {o_cond:?}"
            ));
        }
    }
    (diffs, hp)
}
