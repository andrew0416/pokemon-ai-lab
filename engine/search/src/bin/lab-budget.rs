//! Isolated experiment CLI. No wall-time/performance claim is made by this report.
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::EnumerateOptions;
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::budgeted::{search, Config, EngineDomain, Position, Uniform};
use lab_search::model::load_evaluator;
use lab_search::node::parse_rolls;
use lab_search::{format_choice, format_switches, Choice, Pruning};
use serde_json::json;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-budget: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("lab-budget <scenario.json> [--budget 1000] [--depth 3] [--seed 1] [--position 0] [--rolls full] [--eval heuristic] [--side p1|p2] [--turn-cost 10] [--switch-cost 1] [--max-nodes 20000] [--exploration 25] [--iterations 2000] [--tolerance 0.01] [--dense] [--all-targets]");
        return Ok(());
    }
    let mut config = Config::default();
    let mut rolls = "full".to_owned();
    let mut evaluation = "heuristic".to_owned();
    let mut us = SideId::One;
    let mut pruning = Pruning::Sensible;
    let mut position_index: Option<usize> = None;
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        if flag == "--dense" {
            config.double_oracle = false;
            i += 1;
            continue;
        }
        if flag == "--all-targets" {
            pruning = Pruning::All;
            i += 1;
            continue;
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{flag} needs a value"))?;
        macro_rules! number {
            () => {
                value
                    .parse()
                    .map_err(|_| format!("invalid {flag}: {value}"))?
            };
        }
        match flag {
            "--budget" => config.budget = number!(),
            "--depth" => config.max_turns = number!(),
            "--seed" => config.seed = number!(),
            "--max-nodes" => config.max_nodes = number!(),
            "--turn-cost" => config.turn_cost = number!(),
            "--switch-cost" => config.switch_cost = number!(),
            "--exploration" => config.exploration = number!(),
            "--iterations" => config.matrix_iterations = number!(),
            "--tolerance" => config.matrix_tolerance = number!(),
            "--position" => position_index = Some(number!()),
            "--rolls" => rolls = value.clone(),
            "--eval" => evaluation = value.clone(),
            "--side" => {
                us = match value.as_str() {
                    "p1" => SideId::One,
                    "p2" => SideId::Two,
                    _ => return Err("--side needs p1 or p2".into()),
                }
            }
            _ => return Err(format!("unknown option {flag}")),
        }
        i += 2;
    }
    let options = EnumerateOptions {
        rolls: parse_rolls(&rolls, Some(us)).map_err(|e| e.to_string())?,
    };
    let loaded = load_scenario_file(&args[0]).map_err(|e| e.to_string())?;
    let mut positions = scenario_positions_with(&loaded, options).map_err(|e| e.to_string())?;
    if positions.len() != 1 && position_index.is_none() {
        return Err(format!(
            "{} initial positions; explicitly select --position (no belief mixing)",
            positions.len()
        ));
    }
    let selected = position_index.unwrap_or(0);
    if selected >= positions.len() {
        return Err("--position out of range".into());
    }
    let initial_count = positions.len();
    let initial = positions.remove(selected);
    let evaluator = load_evaluator(&evaluation)?;
    let domain = EngineDomain {
        ruleset: Ruleset::CHAMPIONS_MC,
        options,
        pruning,
        us,
        evaluator: evaluator.as_ref(),
    };
    let report = search(
        &domain,
        &Uniform,
        Position {
            state: initial.state.clone(),
            suspension: None,
        },
        config.clone(),
    )
    .map_err(|e| e.to_string())?;
    let root_decision = lab_search::decision(&initial.state, None).map_err(|e| e.to_string())?;
    let format = |side: SideId, a: &Choice<2>| match a {
        Choice::Turn(actions) => {
            format_choice(&initial.state, side, &initial.order[side as usize], actions)
        }
        Choice::Switches(switches) => format_switches(
            &initial.order[side as usize],
            &lab_search::game::asked_slots(&initial.state, root_decision, side),
            switches,
        ),
    };
    let policy = report.policy.as_ref().map(|p| json!({
        "ours": p.actions[0].iter().zip(&p.equilibrium.rows).map(|(a,q)| json!({"choice":format(us,a),"probability":q})).collect::<Vec<_>>(),
        "theirs":p.actions[1].iter().zip(&p.equilibrium.cols).map(|(a,q)| json!({"choice":format(us.other(),a),"probability":q})).collect::<Vec<_>>(),
        "value":p.equilibrium.value,"local_leaf_matrix_gap":p.equilibrium.exploitability,
        "local_matrix_tolerance_met":p.local_tolerance_met,
        "known_root_cells":p.known_cells,"total_root_cells":p.total_cells,
    }));
    let s = &report.stats;
    println!("{}", serde_json::to_string_pretty(&json!({
        "schema":1,"mode":"experimental-perfect-information-selective",
        "parent_source":"c54becc9fb7bb9101cac363f389d4a84792016ab",
        "scenario":args[0],"position":selected,"initial_positions":initial_count,
        "selected_position_probability":initial.probability,"rolls":rolls,"evaluator":evaluation,
        "side":if us == SideId::One {"p1"} else {"p2"},"pruning":format!("{pruning:?}"),
        "stop":format!("{:?}",report.stop),"policy":policy,"terminal_value":report.terminal_value,
        "budget":config.budget,"turn_cost":config.turn_cost,"switch_cost":config.switch_cost,
        "seed":report.seed,"max_turns":report.max_turns,"max_nodes":config.max_nodes,
        "double_oracle":config.double_oracle,"matrix_iterations":config.matrix_iterations,
        "matrix_tolerance":config.matrix_tolerance,"exploration":config.exploration,
        "work":{"cost_used":s.cost_used,"transitions":s.transitions,"turn_transitions":s.turn_transitions,
            "switch_transitions":s.switch_transitions,"evaluations":s.evaluations,
            "stored_nodes":s.stored_nodes,"expanded_nodes":s.expanded_nodes,
            "matrix_solves":s.matrix_solves,"matrix_iterations":s.matrix_iterations,
            "committed_updates":s.committed_updates,"attempted_updates":s.attempted_updates,
            "walks":s.walks,"frontier_scans":s.frontier_scans,"max_turn_depth":s.max_turn_depth},
        "scope":{"belief_cfr":false,"full_game_exploitability_certificate":false,
            "wall_time_deadline":false,"performance_measurement":false,
            "policy_is_last_completed_backup":true,"work_includes_uncommitted_attempt":true,
            "setup_enumeration_in_budget":false}
    })).map_err(|e| e.to_string())?);
    if report.policy.is_none() && report.terminal_value.is_none() {
        return Err("no completed root policy; increase the work or node budget".into());
    }
    Ok(())
}
