//! Experimental additive CLI: explicit matrices, or conservative one-turn engine worlds.
use lab_engine::{rules::Ruleset, state::SideId, turn::EnumerateOptions};
use lab_search::bayesian::{
    self,
    engine::{self, EngineWorld, Knowledge},
    Config, Game, World,
};
use lab_search::budgeted::Position;
use lab_search::model::load_evaluator;
use lab_search::{format_choice, Choice};
use serde_json::{json, Value};
use std::path::Path;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("lab-bayes <request.json>\nmode: matrix | engine-one-turn; see benchmarks/s26_bayesian/CONTRACT.md");
        return std::process::ExitCode::SUCCESS;
    }
    let result = if args.len() == 1 {
        run(Path::new(&args[0]))
    } else {
        Err("usage: lab-bayes <request.json>".into())
    };
    match result {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lab-bayes: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn string(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} must be a string"))
}
fn number(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_f64()
        .ok_or_else(|| format!("{key} must be a number"))
}
fn strings(v: &Value) -> Result<Vec<String>, String> {
    v.as_array()
        .ok_or("expected string array")?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or("expected string".into())
        })
        .collect()
}
fn integer(v: &Value, key: &str, default: usize) -> Result<usize, String> {
    if v.get(key).is_none() {
        return Ok(default);
    }
    v[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("{key} must be a nonnegative integer"))
}
fn boolean(v: &Value, key: &str, default: bool) -> Result<bool, String> {
    match v.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("{key} must be a boolean")),
    }
}
fn keys(v: &Value, allowed: &[&str]) -> Result<(), String> {
    let object = v.as_object().ok_or("expected object")?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown field: {key}"));
        }
    }
    Ok(())
}

fn run(path: &Path) -> Result<Value, String> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    request(&v, path.parent().unwrap_or(Path::new(".")))
}

fn request(v: &Value, base: &Path) -> Result<Value, String> {
    let mode = string(v, "mode")?;
    let mut config = Config::default();
    if let Some(c) = v.get("solver") {
        keys(c, &["iterations", "tolerance", "check_every"])?;
        config.iterations = integer(c, "iterations", config.iterations)?;
        config.check_every = integer(c, "check_every", config.check_every)?;
        if c.get("tolerance").is_some() {
            config.tolerance = number(c, "tolerance")?;
        }
    }
    let worlds = v["worlds"].as_array().ok_or("worlds must be an array")?;
    let (mut game, metadata) = match mode.as_str() {
        "matrix" => {
            keys(v, &["mode", "rows", "worlds", "solver", "likelihoods"])?;
            let mut matrices = Vec::new();
            for w in worlds {
                keys(w, &["id", "weight", "columns", "payoffs"])?;
                matrices.push(World {
                    id: string(w, "id")?,
                    weight: number(w, "weight")?,
                    columns: strings(&w["columns"])?,
                    payoffs: w["payoffs"]
                        .as_array()
                        .ok_or("payoffs must be a flat row-major array")?
                        .iter()
                        .map(|x| x.as_f64().ok_or("payoff must be a number"))
                        .collect::<Result<_, _>>()?,
                });
            }
            (
                Game::new(strings(&v["rows"])?, matrices).map_err(|e| e.to_string())?,
                json!({"scope":"supplied-fixed-matrices"}),
            )
        }
        "engine-one-turn" => {
            keys(
                v,
                &[
                    "mode",
                    "worlds",
                    "solver",
                    "likelihoods",
                    "side",
                    "evaluator",
                    "knowledge",
                    "max_cells",
                ],
            )?;
            let us = match v.get("side").and_then(Value::as_str).unwrap_or("p1") {
                "p1" => SideId::One,
                "p2" => SideId::Two,
                _ => return Err("side must be p1 or p2".into()),
            };
            if v.get("side").is_some() {
                string(v, "side")?;
            }
            let mut knowledge = Knowledge::default();
            if let Some(k) = v.get("knowledge") {
                keys(k, &["hidden_stats", "unrevealed_reserves"])?;
                knowledge.hidden_stats = boolean(k, "hidden_stats", false)?;
                if let Some(indices) = k.get("unrevealed_reserves") {
                    knowledge.unrevealed_reserves = indices
                        .as_array()
                        .ok_or("unrevealed_reserves must be an array")?
                        .iter()
                        .map(|x| {
                            x.as_u64()
                                .and_then(|n| usize::try_from(n).ok())
                                .ok_or("invalid reserve index")
                        })
                        .collect::<Result<_, _>>()?;
                }
            }
            let mut states = Vec::new();
            let mut orders = Vec::new();
            for w in worlds {
                keys(w, &["id", "weight", "scenario"])?;
                let loaded = lab_scenario::load_scenario_file(base.join(string(w, "scenario")?))
                    .map_err(|e| e.to_string())?;
                if !knowledge.unrevealed_reserves.is_empty() && !loaded.setup_turns.is_empty() {
                    return Err(
                        "hidden-reserve mode requires opening scenarios without setup turns".into(),
                    );
                }
                let mut positions =
                    lab_scenario::scenario_positions_with(&loaded, EnumerateOptions::default())
                        .map_err(|e| e.to_string())?;
                if positions.len() != 1 {
                    return Err("each scenario must produce exactly one initial position; do not silently pick a hidden outcome".into());
                }
                let p = positions.remove(0);
                orders.push(p.order);
                states.push(EngineWorld {
                    id: string(w, "id")?,
                    weight: number(w, "weight")?,
                    position: Position {
                        state: p.state,
                        suspension: None,
                    },
                });
            }
            let evaluation = match v.get("evaluator") {
                Some(_) => string(v, "evaluator")?,
                None => "heuristic".into(),
            };
            let evaluator = load_evaluator(&evaluation)?;
            let built = engine::one_turn(
                &states,
                us,
                Ruleset::CHAMPIONS_MC,
                evaluator.as_ref(),
                &knowledge,
                integer(v, "max_cells", 100_000)?,
            )
            .map_err(|e| e.to_string())?;
            let names = |index: usize, side: SideId, actions: &[Choice<2>]| {
                actions
                    .iter()
                    .map(|a| match a {
                        Choice::Turn(a) => format_choice(
                            &states[index].position.state,
                            side,
                            &orders[index][side.index()],
                            a,
                        ),
                        Choice::Switches(_) => unreachable!("only normal turn roots are accepted"),
                    })
                    .collect::<Vec<_>>()
            };
            let metadata = json!({"scope":"one-turn-leaf-evaluator", "rolls":"full", "pruning":"all", "evaluator":evaluation,
                "side":if us == SideId::One {"p1"} else {"p2"}, "transitions":built.transitions, "outcomes":built.outcomes,
                "row_choices":names(0, us, &built.ours), "world_column_choices":built.theirs.iter().enumerate().map(|(i,a)| names(i, us.other(), a)).collect::<Vec<_>>() });
            (built.game, metadata)
        }
        _ => return Err("mode must be matrix or engine-one-turn".into()),
    };
    if let Some(likelihoods) = v.get("likelihoods") {
        let likelihoods = likelihoods
            .as_array()
            .ok_or("likelihoods must be an array")?
            .iter()
            .map(|v| {
                keys(v, &["world", "probability"])?;
                Ok((string(v, "world")?, number(v, "probability")?))
            })
            .collect::<Result<Vec<_>, String>>()?;
        game = game.conditioned(&likelihoods).map_err(|e| e.to_string())?;
    }
    let result = bayesian::solve(&game, config).map_err(|e| e.to_string())?;
    Ok(
        json!({"schema":1,"mode":mode,"algorithm":"alternating-linear-cfr", "metadata":metadata,
        "iterations":result.iterations,"converged":result.converged,"requested_tolerance":config.tolerance,
        "value":result.assessment.value,"lower":result.assessment.lower,"upper":result.assessment.upper,
        "fixed_matrix_gap":result.assessment.gap,
        "ours":game.rows().iter().zip(result.ours).map(|(id,p)| json!({"action":id,"probability":p})).collect::<Vec<_>>(),
        "worlds":game.worlds().iter().zip(result.theirs).zip(result.assessment.world_values).map(|((w,q),value)|
            json!({"id":w.id,"posterior":w.weight,"value":value,"theirs":w.columns.iter().zip(q).map(|(id,p)|
                json!({"action":id,"probability":p})).collect::<Vec<_>>() })).collect::<Vec<_>>() }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_validates_and_reports_shared_strategy() {
        let mut v = json!({"mode":"matrix","rows":["a","b"],"worlds":[
            {"id":"x","weight":1,"columns":["x"],"payoffs":[1,-1]},
            {"id":"y","weight":1,"columns":["y"],"payoffs":[-1,1]}]});
        let r = request(&v, Path::new(".")).unwrap();
        assert_eq!(r["value"], 0.0);
        assert_eq!(r["ours"].as_array().unwrap().len(), 2);
        v["solver"] = json!({"iterations":0});
        assert!(request(&v, Path::new(".")).is_err());
        v["solver"] = json!({"iterations":1,"tolerence":0.1});
        assert!(request(&v, Path::new(".")).is_err());
    }
}
