//! Experimental finite information-tree CLI. See benchmarks/s26_tree/CONTRACT.md.
use lab_engine::{rules::Ruleset, state::SideId, turn::EnumerateOptions};
#[cfg(not(feature = "experiment-owned-transitions"))]
use lab_search::bayesian::tree::engine::{
    build as build_backend, growing_reusing as growing_backend,
};
#[cfg(feature = "experiment-interned-history")]
use lab_search::bayesian::tree::engine::{
    build_interned as build_backend, growing_interned as growing_backend,
};
#[cfg(all(
    feature = "experiment-owned-transitions",
    not(feature = "experiment-interned-history")
))]
use lab_search::bayesian::tree::engine::{
    build_owned as build_backend, growing_owned as growing_backend,
};
use lab_search::bayesian::{
    engine::{EngineWorld, Knowledge},
    tree::{self, builder::Limits, Node, Tree},
    Config,
};
use lab_search::budgeted::Position;
use serde_json::{json, Value};
use std::path::Path;

#[cfg(feature = "experiment-growing-belief")]
#[path = "lab-belief-workspace/observed.rs"]
mod observed;

fn string(v: &Value, k: &str) -> Result<String, String> {
    v[k].as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{k}: expected string"))
}
fn number(v: &Value, k: &str) -> Result<f64, String> {
    v[k].as_f64().ok_or_else(|| format!("{k}: expected number"))
}
fn index(v: &Value) -> Result<usize, String> {
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("expected nonnegative integer".into())
}
fn integer(v: &Value, k: &str, d: usize) -> Result<usize, String> {
    v.get(k).map_or(Ok(d), index)
}
fn boolean(v: &Value, k: &str, d: bool) -> Result<bool, String> {
    v.get(k).map_or(Ok(d), |x| {
        x.as_bool().ok_or_else(|| format!("{k}: expected boolean"))
    })
}
fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array().ok_or("expected array".into())
}
fn strings(v: &Value) -> Result<Vec<String>, String> {
    array(v)?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or("expected string".into())
        })
        .collect()
}
fn keys(v: &Value, allowed: &[&str]) -> Result<(), String> {
    for k in v.as_object().ok_or("expected object")?.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("unknown field {k}"));
        }
    }
    Ok(())
}

fn request(v: &Value, base: &Path) -> Result<Value, String> {
    let mut config = Config::default();
    if let Some(c) = v.get("solver") {
        keys(c, &["iterations", "tolerance", "check_every"])?;
        config.iterations = integer(c, "iterations", config.iterations)?;
        config.check_every = integer(c, "check_every", config.check_every)?;
        if c.get("tolerance").is_some() {
            config.tolerance = number(c, "tolerance")?;
        }
    }
    if config.iterations == 0
        || config.iterations > 10_000_000
        || config.check_every == 0
        || !config.tolerance.is_finite()
        || config.tolerance < 0.
    {
        return Err("invalid solver config".into());
    }
    let mode = string(v, "mode")?;
    let include_keys = boolean(v, "include_keys", false)?;
    let mut growth_solution = None;
    let (tree, metadata) = match mode.as_str() {
        #[cfg(feature = "experiment-growing-belief")]
        "observed" => {
            let (built, solution, metadata) = observed::request(v, config)?;
            growth_solution = solution;
            (built.tree, metadata)
        }
        "tree" => {
            keys(v, &["mode", "nodes", "root", "solver", "include_keys"])?;
            let mut nodes = Vec::new();
            for n in array(&v["nodes"])? {
                nodes.push(match string(n, "type")?.as_str() {
                    "terminal" => {
                        keys(n, &["type", "value"])?;
                        Node::Terminal(number(n, "value")?)
                    }
                    "chance" => {
                        keys(n, &["type", "edges"])?;
                        Node::Chance(
                            array(&n["edges"])?
                                .iter()
                                .map(|e| {
                                    keys(e, &["probability", "child"])?;
                                    Ok((number(e, "probability")?, index(&e["child"])?))
                                })
                                .collect::<Result<_, String>>()?,
                        )
                    }
                    "decision" => {
                        keys(n, &["type", "player", "information", "actions", "children"])?;
                        Node::Decision {
                            player: index(&n["player"])?,
                            information: string(n, "information")?,
                            actions: strings(&n["actions"])?,
                            children: array(&n["children"])?
                                .iter()
                                .map(index)
                                .collect::<Result<_, _>>()?,
                        }
                    }
                    _ => return Err("unknown node type".into()),
                });
            }
            (
                Tree::new(nodes, integer(v, "root", 0)?).map_err(|e| e.to_string())?,
                json!({"scope":"supplied-finite-information-tree"}),
            )
        }
        "engine" => {
            keys(
                v,
                &[
                    "mode",
                    "worlds",
                    "side",
                    "evaluator",
                    "knowledge",
                    "limits",
                    "solver",
                    "include_keys",
                    "observation_model",
                    "growth",
                ],
            )?;
            if string(v, "observation_model")? != "snapshot-information-v1" {
                return Err("explicit observation_model snapshot-information-v1 required; complete battle-log observations are not implemented".into());
            }
            let side = match v.get("side") {
                None => SideId::One,
                Some(_) => match string(v, "side")?.as_str() {
                    "p1" => SideId::One,
                    "p2" => SideId::Two,
                    _ => return Err("side must be p1 or p2".into()),
                },
            };
            let mut knowledge = Knowledge::default();
            if let Some(k) = v.get("knowledge") {
                keys(k, &["hidden_stats", "unrevealed_reserves"])?;
                knowledge.hidden_stats = boolean(k, "hidden_stats", false)?;
                if let Some(indices) = k.get("unrevealed_reserves") {
                    knowledge.unrevealed_reserves = array(indices)?
                        .iter()
                        .map(index)
                        .collect::<Result<_, _>>()?;
                }
            }
            let mut limits = Limits::default();
            if let Some(l) = v.get("limits") {
                keys(
                    l,
                    &["turns", "max_nodes", "max_transitions", "max_decisions"],
                )?;
                limits.turns = u32::try_from(integer(l, "turns", limits.turns as usize)?)
                    .map_err(|_| "turn count overflow")?;
                limits.max_nodes = integer(l, "max_nodes", limits.max_nodes)?;
                limits.max_transitions = integer(l, "max_transitions", limits.max_transitions)?;
                limits.max_decisions = integer(l, "max_decisions", limits.max_decisions)?;
            }
            let mut worlds = Vec::new();
            for w in array(&v["worlds"])? {
                keys(w, &["id", "weight", "scenario"])?;
                let loaded = lab_scenario::load_scenario_file(base.join(string(w, "scenario")?))
                    .map_err(|e| e.to_string())?;
                if !knowledge.unrevealed_reserves.is_empty() && !loaded.setup_turns.is_empty() {
                    return Err(
                        "hidden-reserve roots require opening scenarios without setup".into(),
                    );
                }
                let mut positions =
                    lab_scenario::scenario_positions_with(&loaded, EnumerateOptions::default())
                        .map_err(|e| e.to_string())?;
                if positions.len() != 1 {
                    return Err("each world must have one root position; ambiguous setup cannot be silently selected".into());
                }
                worlds.push(EngineWorld {
                    id: string(w, "id")?,
                    weight: number(w, "weight")?,
                    position: Position {
                        state: positions.remove(0).state,
                        suspension: None,
                    },
                });
            }
            let evaluation = if v.get("evaluator").is_some() {
                string(v, "evaluator")?
            } else {
                "heuristic".into()
            };
            let evaluator = lab_search::model::load_evaluator(&evaluation)?;
            #[cfg(not(feature = "experiment-growing-belief"))]
            if v.get("growth").is_some() {
                return Err("growth requires experiment-growing-belief".into());
            }
            #[cfg(feature = "experiment-growing-belief")]
            let growing_config = observed::growth_config(v, config)?;
            #[cfg(feature = "experiment-growing-belief")]
            let grown = if let Some(c) = growing_config {
                Some(
                    growing_backend(
                        &worlds,
                        side,
                        Ruleset::CHAMPIONS_MC,
                        evaluator.as_ref(),
                        &knowledge,
                        limits,
                        tree::builder::growing::reuse::Settings {
                            growth: c,
                            storage: tree::builder::growing::reuse::Options {
                                in_place: true,
                                workspace: true,
                                compiler: true,
                                static_values: true,
                                direct_write: true,
                            },
                        },
                    )
                    .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            #[cfg(not(feature = "experiment-growing-belief"))]
            let grown: Option<()> = None;
            let (built, growth_metadata, solution) = match grown {
                None => (
                    build_backend(
                        &worlds,
                        side,
                        Ruleset::CHAMPIONS_MC,
                        evaluator.as_ref(),
                        &knowledge,
                        limits,
                    )
                    .map_err(|e| e.to_string())?,
                    Value::Null,
                    None,
                ),
                Some(g) => {
                    #[cfg(feature = "experiment-growing-belief")]
                    {
                        let meta = observed::metadata(&g);
                        (g.built, meta, Some(g.solution))
                    }
                    #[cfg(not(feature = "experiment-growing-belief"))]
                    {
                        let () = g;
                        unreachable!()
                    }
                }
            };
            growth_solution = solution;
            let stats = built.stats;
            let root_ours: Vec<_> = built
                .tree
                .boundaries()
                .iter()
                .filter(|b| b.public == 0)
                .map(|b| built.tree.information_at(b.node).unwrap())
                .collect();
            (
                built.tree,
                json!({"scope":"finite-depth-snapshot-information-game","observation_model":"snapshot-information-v1",
                "full_battle_log_information":false,"rolls":"full","pruning":"all","evaluator":evaluation,"turns":limits.turns,
                "root_our_information":root_ours,"transitions":stats.transitions,"chance_outcomes":stats.chance_outcomes,
                "turn_decisions":stats.turn_decisions,"switch_decisions":stats.switch_decisions,"leaves":stats.leaves,"growth":growth_metadata}),
            )
        }
        _ => return Err("mode must be tree or engine".into()),
    };
    let solved = match growth_solution {
        Some(s) => s,
        None => tree::workspace::cached::solve(&tree, config).map_err(|e| e.to_string())?,
    };
    let beliefs = tree
        .public_beliefs(&solved.policy)
        .map_err(|e| e.to_string())?;
    let policies:Vec<_>=tree.information().iter().enumerate().map(|(id,i)|{
        let mut v=json!({"id":id,"player":i.player,"actions":i.actions,"probabilities":solved.policy[id],"history_count":i.nodes.len(),"own_sequence":i.own_sequence});
        if include_keys{v["key"]=json!(i.key);}v
    }).collect();
    let beliefs:Vec<_>=beliefs.into_iter().map(|b|{
        let mut v=json!({"id":b.id,"reach":b.reach,"posterior":b.posterior,"world_conditional_values":b.world_values,"history_count":b.histories.len()});
        if include_keys{v["key"]=json!(b.key);v["history_weights"]=json!(b.histories);}v
    }).collect();
    let mut result = json!({"schema":1,"mode":mode,"algorithm":"extensive-form-alternating-linear-cfr","metadata":metadata,
        "nodes":tree.node_count(),"information_sets":policies.len(),"iterations":solved.iterations,"converged":solved.converged,
        "tolerance":config.tolerance,"value":solved.assessment.value,"lower":solved.assessment.lower,"upper":solved.assessment.upper,
        "finite_game_gap":solved.assessment.gap,"policies":policies,"worlds":tree.worlds(),"public_beliefs":beliefs});
    if !result["metadata"]["growth"].is_null() {
        let complete = result["metadata"]["growth"]["horizon_complete"]
            .as_bool()
            .unwrap();
        result["algorithm"] = json!("public-history-closed-growing-tree-cfr");
        result["surrogate_converged"] = json!(solved.converged);
        result["converged"] = json!(complete && solved.converged);
        result["full_horizon_gap"] = if complete {
            json!(solved.assessment.gap)
        } else {
            Value::Null
        };
        result["certificate_scope"] = json!(if complete {
            "requested-finite-horizon"
        } else {
            "current-fixed-leaf-surrogate-only"
        });
    }
    #[cfg(feature = "experiment-growing-belief")]
    if mode == "observed" {
        result["oracle_tree"] = observed::export(&tree);
    }
    Ok(result)
}

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("lab-belief <request.json>\nmode: tree | engine; engine requires explicit snapshot-information-v1 observation model");
        return std::process::ExitCode::SUCCESS;
    }
    let run = || -> Result<Value, String> {
        if args.len() != 1 {
            return Err("usage: lab-belief <request.json>".into());
        }
        let path = Path::new(&args[0]);
        let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        request(&v, path.parent().unwrap_or(Path::new(".")))
    };
    match run() {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lab-belief: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_cli_validates_and_reports_scope() {
        let mut v =
            json!({"mode":"tree","nodes":[{"type":"terminal","value":3}],"include_keys":true});
        let r = request(&v, Path::new(".")).unwrap();
        assert_eq!(r["value"], 3.);
        assert_eq!(r["finite_game_gap"], 0.);
        v["solver"] = json!({"iterations":0});
        assert!(request(&v, Path::new(".")).is_err());
        let v = json!({"mode":"engine","worlds":[]});
        assert!(request(&v, Path::new("."))
            .err()
            .unwrap()
            .contains("observation_model"));
    }
}
