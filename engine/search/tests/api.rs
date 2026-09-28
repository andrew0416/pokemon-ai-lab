//! The JSON reports of every search mode (board PY3b): each agrees with the analysis it
//! reports, writes its choices as strings the node parses back, and survives a JSON round trip.
//! On the oracle's `eject-button-uturn` (plus believed copies of its p1 team in `tests/data`).

use std::path::PathBuf;

use serde_json::Value;

use lab_engine::eval::Heuristic;
use lab_engine::rules::Ruleset;
use lab_engine::state::SideId;
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions_with};
use lab_search::api::{self, BeliefRequest};
use lab_search::node::{scenario_nodes, Node};
use lab_search::rollout::{Policy, RolloutSettings};
use lab_search::{Config, DeepLevel, Solver};

fn engine_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn scenario_path() -> String {
    engine_dir()
        .join("oracle/scenarios/eject-button-uturn.json")
        .to_string_lossy()
        .into_owned()
}

fn median() -> EnumerateOptions {
    EnumerateOptions {
        rolls: RollMode::Median,
    }
}

fn node() -> Node<2> {
    let loaded = load_scenario_file(scenario_path()).unwrap();
    scenario_nodes(&loaded, median(), false)
        .unwrap()
        .remove(0)
        .1
}

fn config() -> Config {
    let mut config = Config::new(Ruleset::CHAMPIONS_MC, SideId::One);
    config.rolls = RollMode::Median;
    config.threads = 2;
    config
}

/// The report survives serialisation, and every choice string in `field` parses at the node.
fn check(node: &Node<2>, report: &Value, fields: &[(&str, SideId)]) {
    let text = serde_json::to_string(report).unwrap();
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(back["mode"], report["mode"]);
    for (field, side) in fields {
        for choice in report[*field].as_array().unwrap() {
            let text = choice
                .as_str()
                .or_else(|| choice[0].as_str())
                .or_else(|| choice["ours"].as_str())
                .unwrap();
            node.parse(*side, text).unwrap();
        }
    }
}

#[test]
fn search_reports_match_the_solver() {
    let node = node();
    let evaluator = Heuristic;

    let maximin = api::maximin(&node, config(), &evaluator).unwrap();
    let mut state = node.state.clone();
    let direct = Solver::new(config(), &evaluator)
        .analyse(&mut state, None)
        .unwrap();
    assert_eq!(maximin["mode"], "maximin");
    assert_eq!(maximin["value"].as_f64().unwrap() as f32, direct.value);
    assert_eq!(
        maximin["lines"].as_array().unwrap().len(),
        direct.lines.len()
    );
    check(&node, &maximin, &[("lines", SideId::One)]);

    let nash = api::nash(&node, config(), &evaluator, false).unwrap();
    let direct = Solver::new(config(), &evaluator)
        .analyse_mixed(&mut state, None)
        .unwrap();
    assert_eq!(
        nash["value"].as_f64().unwrap() as f32,
        direct.equilibrium.value
    );
    assert_eq!(nash["matrix"].as_array().unwrap().len(), direct.matrix.rows);
    check(
        &node,
        &nash,
        &[("ours", SideId::One), ("theirs", SideId::Two)],
    );
    let lazy = api::nash(&node, config(), &evaluator, true).unwrap();
    assert_eq!(lazy["mode"], "nash-lazy");
    assert!((lazy["value"].as_f64().unwrap() - nash["value"].as_f64().unwrap()).abs() < 0.2);

    let deep = api::deep(&node, config(), &evaluator, 3).unwrap();
    assert_eq!(deep["beam"], 3);
    check(&node, &deep, &[("lines", SideId::One)]);

    let level = DeepLevel {
        beam: 2,
        outcomes: Some(2),
    };
    let deep2 = api::deep_nash(&node, config(), &evaluator, &[level]).unwrap();
    assert_eq!(deep2["depth"], 2);
    let deep3 = api::deep_nash(&node, config(), &evaluator, &[level, level]).unwrap();
    assert_eq!(deep3["depth"], 3);
    let direct = Solver::new(config(), &evaluator)
        .analyse_deep_mixed_levels(&mut state, None, &[level, level])
        .unwrap();
    assert_eq!(
        deep3["value"].as_f64().unwrap() as f32,
        direct.equilibrium.value
    );
    check(
        &node,
        &deep3,
        &[("ours", SideId::One), ("theirs", SideId::Two)],
    );
    assert!(deep3["stats"]["deep_tt_misses"].as_u64().unwrap() > 0);

    let ours = node
        .legal_choices(SideId::One, lab_search::Pruning::Sensible)
        .unwrap();
    let mut child = config();
    child.child_nash = true;
    child.reply_beam = Some(2);
    let plan = api::plan(&node, child, &evaluator, &ours[..1]).unwrap();
    assert_eq!(plan["plan"][0], ours[0].as_str());
    assert!(plan["child"]["value"].is_number());
    check(&node, &plan, &[("replies", SideId::Two)]);
    assert!(api::plan(&node, config(), &evaluator, &["move bogus".to_owned()]).is_err());
}

/// Model ③: believing our real team gives their real equilibrium strategy, so our best
/// response is worth the real equilibrium value (within the solver's tolerance); two beliefs
/// get their weights normalised. Model ② without beliefs is the plain matrix game here (no
/// setup turns).
#[test]
fn believed_and_observed_reports() {
    let data = engine_dir().join("search/tests/data");
    let same = data
        .join("eject-button-uturn.p1-team.json")
        .to_string_lossy()
        .into_owned();
    let bulky = data
        .join("eject-button-uturn.p1-team-bulky.json")
        .to_string_lossy()
        .into_owned();
    let evaluator = Heuristic;
    let request = BeliefRequest {
        scenario: scenario_path(),
        believed_teams: vec![same.clone()],
        tolerance: 1.0,
        setup: median(),
        ..BeliefRequest::default()
    };
    let report = api::believed(&request, config(), &evaluator).unwrap();
    assert_eq!(report["mode"], "believed");
    let real = report["real_equilibrium"].as_f64().unwrap();
    let best = report["best_response"].as_f64().unwrap();
    assert!(
        (best - real).abs() < 0.1,
        "best response {best} vs equilibrium {real}"
    );
    let two = BeliefRequest {
        believed_teams: vec![same, bulky],
        believed_weights: vec![3.0, 1.0],
        ..request.clone()
    };
    let report = api::believed(&two, config(), &evaluator).unwrap();
    let teams = report["teams"].as_array().unwrap();
    assert!((teams[0]["prior"].as_f64().unwrap() - 0.75).abs() < 1e-6);
    serde_json::to_string(&report).unwrap();
    let observed = BeliefRequest {
        believed_teams: Vec::new(),
        ..request
    };
    let report = api::believed(&observed, config(), &evaluator).unwrap();
    assert_eq!(report["mode"], "observed");
    assert!((report["value"].as_f64().unwrap() - real).abs() < 1e-3);
}

/// Rollout reports: the tally adds up to the games, and depth-3 deep-nash runs.
#[test]
fn rollout_report() {
    let loaded = load_scenario_file(scenario_path()).unwrap();
    let positions = scenario_positions_with(&loaded, median()).unwrap();
    let evaluator = Heuristic;
    let mut config = config();
    config.threads = 1;
    let settings = RolloutSettings {
        config,
        max_turns: 3,
        policy: Policy::Nash,
        beam: 2,
        deep_rest: None,
        master_seed: 5,
        lazy: false,
    };
    let path = scenario_path();
    let report = api::rollout(
        &loaded,
        Some(&path),
        &positions,
        &settings,
        &evaluator,
        3,
        2,
    )
    .unwrap();
    let tally = &report["tally"];
    let total: u64 = ["p1", "p2", "tie", "cutoff", "aborted"]
        .iter()
        .map(|k| tally[*k].as_u64().unwrap())
        .sum();
    assert_eq!(total, 3);
    assert_eq!(report["game_records"].as_array().unwrap().len(), 3);
    let deep = RolloutSettings {
        policy: Policy::DeepNash,
        deep_rest: Some(DeepLevel {
            beam: 2,
            outcomes: Some(1),
        }),
        max_turns: 2,
        ..settings
    };
    let report = api::rollout(&loaded, None, &positions, &deep, &evaluator, 1, 1).unwrap();
    assert_eq!(report["policy"]["depth"], 3);
    assert_eq!(report["game_records"].as_array().unwrap().len(), 1);
}
