//! S25b protocol binary. One pinned executable runs both arms and the full-tree reference.
//! JSONL snapshots are completed policies, not promises of a preemptible search.
use std::io::{self, Write};
use std::time::Instant;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::EnumerateOptions;
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::budgeted::{self, Domain, EngineDomain, Phase, Position, Uniform};
use lab_search::nash;
use lab_search::node::parse_rolls;
use lab_search::{Choice, Config, Decision, Equilibrium, Matrix, Pruning, Solver};
use serde_json::{json, Value};

fn emit(value: Value) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, &value).expect("stdout");
    writeln!(out).expect("stdout");
    out.flush().expect("stdout");
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            emit(json!({"kind":"error","error":error}));
            std::process::ExitCode::FAILURE
        }
    }
}

fn lift(actions: &[Choice<2>], probabilities: &[f32], full: &[Choice<2>]) -> Vec<f32> {
    let mut result = vec![0.; full.len()];
    assert_eq!(actions.len(), probabilities.len());
    for (action, probability) in actions.iter().zip(probabilities) {
        result[full.iter().position(|a| a == action).expect("legal root action")] += probability;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn snapshot(
    started: Instant,
    stage: &str,
    actions: [&[Choice<2>]; 2],
    full: &[Vec<Choice<2>>; 2],
    eq: &Equilibrium,
    transitions: u64,
    work: Value,
) {
    let rows = lift(actions[0], &eq.rows, &full[0]);
    let cols = lift(actions[1], &eq.cols, &full[1]);
    emit(json!({"kind":"policy","elapsed_s":started.elapsed().as_secs_f64(),
        "stage":stage,"rows":rows,"cols":cols,"estimated_value":eq.value,
        "local_gap":eq.exploitability,"transitions":transitions,"work":work}));
}

#[derive(Clone, Copy)]
struct Estimate { value: f32, lower: f64, upper: f64 }

#[derive(Default)]
struct Reference { calls: u64, matrices: u64, max_local_gap: f32, switch_nodes: u64 }

impl Reference {
    // Independent exhaustive recursion: no budgeted tree, PUCT, DO or beam code.
    // RM+ is shared. Propagate best-response intervals so convergence is not assumed.
    fn solve<D: Domain>(&mut self, domain: &D, p: &D::Position, remaining: u32,
                        switch_chain: u32) -> Result<(Estimate, Option<Value>), String> {
        let phase = domain.phase(p)?;
        if phase == Phase::Terminal || (phase == Phase::Turn && remaining == 0) {
            let value = domain.value(p);
            if !value.is_finite() { return Err("nonfinite reference leaf".into()); }
            return Ok((Estimate { value, lower: value as f64, upper: value as f64 }, None));
        }
        if switch_chain > 64 { return Err("reference switch chain".into()); }
        self.switch_nodes += u64::from(phase == Phase::Switch);
        let rows = domain.actions(p, 0)?;
        let cols = domain.actions(p, 1)?;
        if rows.is_empty() || cols.is_empty() { return Err("empty reference menu".into()); }
        let mut values = Vec::new();
        let mut lowers = Vec::new();
        let mut uppers = Vec::new();
        for a in &rows {
            for b in &cols {
                self.calls += 1;
                if self.calls > 2_000_000 { return Err("reference transition limit".into()); }
                let outcomes = domain.transitions(p, [a, b])?;
                let mass: f64 = outcomes.iter().map(|(q, _)| q).sum();
                if outcomes.is_empty() || outcomes.iter().any(|(q,_)| !q.is_finite() || *q < 0.)
                    || (mass - 1.).abs() > 1e-9 { return Err("reference probability mass".into()); }
                let (mut value, mut lower, mut upper) = (0., 0., 0.);
                for (q, next) in outcomes {
                    if q == 0. { continue; }
                    let (e, _) = self.solve(domain, &next, remaining - u32::from(phase == Phase::Turn),
                        if phase == Phase::Turn { 0 } else { switch_chain + 1 })?;
                    value += q * e.value as f64;
                    lower += q * e.lower;
                    upper += q * e.upper;
                }
                values.push(value as f32);
                lowers.push(lower);
                uppers.push(upper);
            }
        }
        self.matrices += 1;
        let matrix = Matrix::new(rows.len(), cols.len(), values);
        let eq = nash::solve(&matrix, 200_000, 0.0001);
        self.max_local_gap = self.max_local_gap.max(eq.exploitability);
        let pnorm: f64 = eq.rows.iter().map(|v| *v as f64).sum();
        let qnorm: f64 = eq.cols.iter().map(|v| *v as f64).sum();
        let lower = (0..cols.len()).map(|c| (0..rows.len()).map(|r|
            eq.rows[r] as f64 / pnorm * lowers[r * cols.len() + c]).sum::<f64>())
            .fold(f64::INFINITY, f64::min);
        let upper = (0..rows.len()).map(|r| (0..cols.len()).map(|c|
            eq.cols[c] as f64 / qnorm * uppers[r * cols.len() + c]).sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max);
        // Numeric bounds, not directed-rounding interval arithmetic. Record all cell bounds.
        let detail = json!({"rows":rows.len(),"cols":cols.len(),"matrix":matrix.values,
            "cell_lower":lowers,"cell_upper":uppers,"row_policy":eq.rows,"col_policy":eq.cols,
            "value":eq.value,"lower":lower,"upper":upper,"root_solver_gap":eq.exploitability});
        Ok((Estimate { value:eq.value, lower, upper }, Some(detail)))
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--fingerprint"] {
        emit(json!({"kind":"fingerprint","quality_feature":cfg!(feature="experiment-search-quality"),
            "budgeted_feature":cfg!(feature="experiment-budgeted-search"),
            "prepared":cfg!(feature="experiment-prepared-turn"),
            "leaf_endings":cfg!(feature="experiment-leaf-ending-states"),
            "borrowed_keys":cfg!(feature="experiment-borrowed-child-keys"),
            "observer_protocol":"completed-backup-v1","terminal_depth_bonus_default":true,
            "timing_terminal_depth_bonus":false,"hardware_intrinsic_requirement":false}));
        return Ok(());
    }
    if args.len() != 6 {
        return Err("usage: lab-search-quality <scenario> <baseline|selective|reference> <full|median> <position> <seed> <limit-ms>".into());
    }
    let mode = args[1].as_str();
    let index: usize = args[3].parse().map_err(|_| "position")?;
    let seed: u64 = args[4].parse().map_err(|_| "seed")?;
    let limit: f64 = args[5].parse::<f64>().map_err(|_| "limit")? / 1000.;
    if !limit.is_finite() || limit <= 0. { return Err("positive finite limit".into()); }
    let options = EnumerateOptions { rolls: parse_rolls(&args[2], Some(SideId::One)).map_err(|e|e.to_string())? };
    let setup = Instant::now();
    let loaded = load_scenario_file(&args[0]).map_err(|e|e.to_string())?;
    let mut positions = scenario_positions_with(&loaded, options).map_err(|e|e.to_string())?;
    let count = positions.len();
    if index >= count { return Err("position out of range".into()); }
    let initial = positions.remove(index);
    let position = Position { state:initial.state.clone(), suspension:None };
    if lab_search::decision(&position.state, None).map_err(|e|e.to_string())? != Decision::Turn {
        return Err("S25b requires normal-turn roots (old deep root switch horizon differs)".into());
    }
    let domain = EngineDomain { ruleset:Ruleset::CHAMPIONS_MC,options,pruning:Pruning::Sensible,
        us:SideId::One,evaluator:&Heuristic };
    let full = [domain.actions(&position, 0)?, domain.actions(&position, 1)?];
    emit(json!({"kind":"ready","mode":mode,"scenario":args[0],"rolls":args[2],"position":index,
        "seed":seed,"depth":2,"evaluator":"Heuristic","pruning":"Sensible","outcome_cap":null,
        "initial_positions":count,"selected_probability":initial.probability,
        "setup_seconds":setup.elapsed().as_secs_f64(),
        "actions":[full[0].iter().map(|a|format!("{a:?}")).collect::<Vec<_>>(),
                     full[1].iter().map(|a|format!("{a:?}")).collect::<Vec<_>>()]}));
    let started = Instant::now();
    match mode {
        "selective" => {
            let config = budgeted::Config { budget:2_000_000,turn_cost:1,switch_cost:1,
                max_turns:2,max_nodes:200_000,matrix_iterations:20_000,seed,..Default::default() };
            let report = budgeted::search_with_observer(&domain, &Uniform, position, config, |p,s| {
                snapshot(started,"selective",[&p.actions[0],&p.actions[1]],&full,&p.equilibrium,
                    s.transitions,json!({"cost":s.cost_used,"turns":s.turn_transitions,
                    "switches":s.switch_transitions,"stored_nodes":s.stored_nodes,
                    "evaluations":s.evaluations,"matrix_solves":s.matrix_solves,
                    "matrix_iterations":s.matrix_iterations,"updates":s.committed_updates}));
                started.elapsed().as_secs_f64() >= limit
            }).map_err(|e|e.to_string())?;
            emit(json!({"kind":"done","stop":format!("{:?}",report.stop),
                "elapsed_s":started.elapsed().as_secs_f64(),"transitions":report.stats.transitions,
                "stored_nodes":report.stats.stored_nodes}));
        }
        "baseline" => {
            let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
            config.threads = 1;
            config.rolls = options.rolls;
            config.pruning = Pruning::Sensible;
            config.outcome_cap = None;
            config.max_turns = None; // Existing child budgets are not a hard global cap.
            config.terminal_depth_bonus = false;
            let mut solver = Solver::new(config, &Heuristic);
            let mut state = initial.state.clone();
            let mut total = 0;
            let max_beam = full[0].len().max(full[1].len());
            let mut stages = vec![0]; // One completed depth-1 incumbent, then existing deep-nash.
            for beam in [1, 2, 4, 8, max_beam] {
                let beam = beam.min(max_beam);
                if !stages.contains(&beam) { stages.push(beam); }
            }
            for beam in stages {
                let (ours,theirs,eq,turns,nodes,unsupported,omitted) = if beam == 0 {
                    let a = solver.analyse_mixed(&mut state,None).map_err(|e|e.to_string())?;
                    (a.ours,a.theirs,a.equilibrium,a.turns,a.nodes,a.unsupported,a.omitted_ours+a.omitted_theirs)
                } else {
                    let a = solver.analyse_deep_mixed(&mut state,None,beam).map_err(|e|e.to_string())?;
                    (a.ours,a.theirs,a.equilibrium,a.turns,a.nodes,a.unsupported,a.omitted_ours+a.omitted_theirs)
                };
                if !unsupported.is_empty() || omitted != 0 { return Err(format!("baseline omitted cells: {unsupported:?}, {omitted}")); }
                if state != initial.state { return Err("baseline failed restoration".into()); }
                total += turns;
                let stats = solver.stats();
                snapshot(started,&format!("beam-{beam}"),[&ours,&theirs],&full,&eq,total,
                    json!({"stage_transitions":turns,"nodes":nodes,"tt_hits":stats.tt_hits,
                    "matrix_solves":stats.nash_solves,"matrix_iterations":stats.nash_iterations,
                    "matrix_seconds":stats.nash_seconds,"enumerate_seconds":stats.enumerate_seconds,
                    "stage_depth":if beam==0 {1}else{2},"beam":beam}));
                if started.elapsed().as_secs_f64() >= limit { break; }
            }
            emit(json!({"kind":"done","stop":"schedule-finished-or-soft-limit",
                "elapsed_s":started.elapsed().as_secs_f64(),"transitions":total}));
        }
        "reference" => {
            let mut reference = Reference::default();
            let (_, matrix) = reference.solve(&domain,&position,2,0)?;
            emit(json!({"kind":"reference","elapsed_s":started.elapsed().as_secs_f64(),
                "reference":matrix,"transitions":reference.calls,"matrices":reference.matrices,
                "max_local_gap":reference.max_local_gap,"switch_nodes":reference.switch_nodes,
                "scope":"all-action recursion under specified roll mode; shared RM+ with propagated numeric bounds"}));
        }
        _ => return Err("unknown mode".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_interval_contains_tiny_engine_equilibrium() {
        // Existing non-damaging real engine scenario: full random mode, two normal turns.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../oracle/scenarios/psych-up-speed-swap.json");
        let options = EnumerateOptions { rolls: lab_engine::turn::RollMode::Full };
        let initial = scenario_positions_with(&load_scenario_file(path).unwrap(),options).unwrap().remove(0);
        let p = Position {state:initial.state,suspension:None};
        let domain = EngineDomain {ruleset:Ruleset::CHAMPIONS_MC,options,pruning:Pruning::Sensible,
            us:SideId::One,evaluator:&Heuristic};
        let mut reference=Reference::default();
        let (value,_) = reference.solve(&domain,&p,2,0).unwrap();
        assert!(value.upper - value.lower < 0.02);
        let report=budgeted::search(&domain,&Uniform,p,budgeted::Config {
            budget:100_000,turn_cost:1,switch_cost:1,max_turns:2,max_nodes:100_000,
            matrix_iterations:20_000,..Default::default()
        }).unwrap();
        assert_eq!(report.stop,budgeted::Stop::FrontierExhausted);
        let actual=report.policy.unwrap().equilibrium.value as f64;
        assert!(actual >= value.lower-0.05 && actual <= value.upper+0.05,
            "{actual} outside [{},{}]",value.lower,value.upper);
    }
}
