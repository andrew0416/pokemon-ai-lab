//! Same-work paired benchmark: exact reference/candidate parity precedes timing.
//! Loading, JSON serialization, diagnostics and returned-tree destruction are not timed.
use lab_engine::{eval::Evaluator, rules::Ruleset, state::SideId, turn::EnumerateOptions};
use lab_search::{
    bayesian::{
        self,
        engine::{EngineWorld, Knowledge},
        tree::{
            self,
            builder::{growing, Built, Limits},
        },
    },
    budgeted::Position,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, hint::black_box, io::Write, path::Path, time::Instant};

struct Input {
    worlds: Vec<EngineWorld<2>>,
    knowledge: Knowledge,
    limits: Limits,
    solver: bayesian::Config,
}
struct Output {
    built: Built,
    solved: tree::Solution,
    growth: Option<(growing::Work, growing::Stop, usize)>,
    complete: bool,
}
enum Method {
    Exhaustive,
    Growing(growing::Config),
    Reusing(growing::Config, growing::reuse::Options),
    ExhaustiveWorkspace,
    ExhaustiveCached,
    ExhaustiveWriting,
}
fn uint(v: &Value, k: &str, d: usize) -> Result<usize, String> {
    match v.get(k) {
        None => Ok(d),
        Some(x) => x
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| format!("invalid {k}")),
    }
}
fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or_else(|| format!("invalid {k}"))
}
fn load(v: &Value, base: &Path) -> Result<Input, String> {
    let mut worlds = Vec::new();
    for w in v["worlds"].as_array().ok_or("worlds array")? {
        let loaded = lab_scenario::load_scenario_file(base.join(string(w, "scenario")?))
            .map_err(|e| e.to_string())?;
        let mut positions =
            lab_scenario::scenario_positions_with(&loaded, EnumerateOptions::default())
                .map_err(|e| e.to_string())?;
        if positions.len() != 1 {
            return Err("ambiguous root".into());
        }
        worlds.push(EngineWorld {
            id: string(w, "id")?.into(),
            weight: w["weight"].as_f64().ok_or("weight")?,
            position: Position {
                state: positions.remove(0).state,
                suspension: None,
            },
        });
    }
    let l = &v["limits"];
    let s = &v["solver"];
    Ok(Input {
        worlds,
        knowledge: Knowledge {
            hidden_stats: v["hidden_stats"].as_bool().unwrap_or(false),
            ..Knowledge::default()
        },
        limits: Limits {
            turns: uint(l, "turns", 2)?
                .try_into()
                .map_err(|_| "turn overflow")?,
            max_nodes: uint(l, "max_nodes", 50000)?,
            max_transitions: uint(l, "max_transitions", 20000)?,
            max_decisions: uint(l, "max_decisions", 32)?,
        },
        solver: bayesian::Config {
            iterations: uint(s, "iterations", 1000)?,
            check_every: uint(s, "check_every", 32)?,
            tolerance: s["tolerance"].as_f64().unwrap_or(0.1),
        },
    })
}
fn compute(i: &Input, e: &dyn Evaluator<2>, variant: &Method) -> Result<Output, String> {
    let i = black_box(i);
    if matches!(
        variant,
        Method::Exhaustive
            | Method::ExhaustiveWorkspace
            | Method::ExhaustiveCached
            | Method::ExhaustiveWriting
    ) {
        let build = if matches!(variant, Method::ExhaustiveWriting) {
            tree::engine::build_writing
        } else {
            tree::engine::build
        };
        let built = build(
            &i.worlds,
            SideId::One,
            Ruleset::CHAMPIONS_MC,
            e,
            &i.knowledge,
            i.limits,
        )
        .map_err(|e| e.to_string())?;
        let solved = if matches!(
            variant,
            Method::ExhaustiveCached | Method::ExhaustiveWriting
        ) {
            tree::workspace::cached::solve(&built.tree, i.solver)
        } else if matches!(variant, Method::ExhaustiveWorkspace) {
            tree::workspace::solve(&built.tree, i.solver)
        } else {
            tree::solve(&built.tree, i.solver)
        }
        .map_err(|e| e.to_string())?;
        Ok(Output {
            built,
            solved,
            growth: None,
            complete: true,
        })
    } else {
        let r = match variant {
            Method::Growing(g) => tree::engine::growing(
                &i.worlds,
                SideId::One,
                Ruleset::CHAMPIONS_MC,
                e,
                &i.knowledge,
                i.limits,
                *g,
            ),
            Method::Reusing(g, options) => tree::engine::growing_reusing(
                &i.worlds,
                SideId::One,
                Ruleset::CHAMPIONS_MC,
                e,
                &i.knowledge,
                i.limits,
                growing::reuse::Settings {
                    growth: *g,
                    storage: *options,
                },
            ),
            _ => unreachable!(),
        }
        .map_err(|e| e.to_string())?;
        Ok(Output {
            built: r.built,
            solved: r.solution,
            growth: Some((r.work, r.stop, r.frontier_public_groups)),
            complete: r.horizon_complete,
        })
    }
}
fn root_policy(o: &Output) -> BTreeMap<String, Vec<f64>> {
    o.built
        .tree
        .information()
        .iter()
        .enumerate()
        .filter(|(_, i)| i.player == 0 && i.own_sequence.is_empty())
        .map(|(id, i)| (i.key.clone(), o.solved.policy[id].clone()))
        .collect()
}
fn diagnostics(o: &Output) -> Value {
    let growth=o.growth.as_ref().map(|(w,stop,frontier)|json!({"attempted_transitions":w.attempted_transitions,
        "committed_expansions":w.committed_expansions,"walks":w.walks,"solves":w.solves,
        "total_cfr_iterations":w.cfr_iterations,"stop":format!("{stop:?}"),"frontier_groups":frontier}));
    json!({"nodes":o.built.tree.node_count(),"information_sets":o.built.tree.information().len(),
        "transitions":o.built.stats.transitions,"chance_outcomes":o.built.stats.chance_outcomes,
        "leaves":o.built.stats.leaves,"switch_decisions":o.built.stats.switch_decisions,
        "iterations":o.solved.iterations,"gap":o.solved.assessment.gap,"value":o.solved.assessment.value,
        "solver_converged":o.solved.converged,"horizon_complete":o.complete,"growth":growth,
        "root_policy":root_policy(o)})
}
// Full and growing builders assign different physical IDs. Canonical preorder removes
// only this numbering; information keys, menus, chance probabilities and leaf values stay.
fn canonical(t: &tree::Tree) -> String {
    fn visit(n: usize, raw: &[tree::Node], out: &mut Vec<tree::Node>) -> usize {
        let id = out.len();
        out.push(tree::Node::Terminal(0.));
        out[id] = match &raw[n] {
            tree::Node::Terminal(v) => tree::Node::Terminal(*v),
            tree::Node::Chance(edges) => tree::Node::Chance(
                edges
                    .iter()
                    .map(|(p, c)| (*p, visit(*c, raw, out)))
                    .collect(),
            ),
            tree::Node::Decision {
                player,
                information,
                actions,
                children,
            } => tree::Node::Decision {
                player: *player,
                information: information.clone(),
                actions: actions.clone(),
                children: children.iter().map(|c| visit(*c, raw, out)).collect(),
            },
        };
        id
    }
    let raw = t.export_nodes();
    let mut out = Vec::new();
    visit(t.root(), &raw, &mut out);
    format!("{:?}|{:?}", t.worlds(), out)
}
fn semantic_policy(o: &Output) -> BTreeMap<String, Vec<u64>> {
    o.built
        .tree
        .information()
        .iter()
        .enumerate()
        .map(|(id, i)| {
            (
                i.key.clone(),
                o.solved.policy[id].iter().map(|p| p.to_bits()).collect(),
            )
        })
        .collect()
}
fn validate(reference: &Output, o: &Output, variant: &Value) -> Result<(), String> {
    if !o.solved.converged {
        return Err("requested solver tolerance was not met".into());
    }
    if variant["comparison"] == "same-finite-game" {
        if !reference.complete || !o.complete {
            return Err("incomplete requested horizon".into());
        }
        if canonical(&reference.built.tree) != canonical(&o.built.tree)
            || semantic_policy(reference) != semantic_policy(o)
            || format!("{:?}", reference.solved.assessment) != format!("{:?}", o.solved.assessment)
            || reference.built.stats.transitions != o.built.stats.transitions
        {
            return Err("finite-game tree/policy/certificate parity failed".into());
        }
        return Ok(());
    }
    // Exact comparison of the entire policy, tree, observation metadata, best responses,
    // stopping state and charged/committed work. This also covers partial/limited trees.
    if format!("{:?}", reference.built.tree) != format!("{:?}", o.built.tree)
        || format!("{:?}", reference.solved) != format!("{:?}", o.solved)
        || format!("{:?}", reference.built.stats) != format!("{:?}", o.built.stats)
        || diagnostics(reference) != diagnostics(o)
    {
        return Err("exact same-work parity failed".into());
    }
    Ok(())
}
fn method(v: &Value, solver: bayesian::Config) -> Result<Method, String> {
    let config = growing::Config {
        max_expansions: uint(v, "max_expansions", 10000)?,
        max_walks: uint(v, "max_walks", 20000)?,
        exploration: 1.,
        seed: v["seed"].as_u64().unwrap_or(1),
        solver,
    };
    Ok(match string(v, "kind")? {
        "exhaustive" => Method::Exhaustive,
        "exhaustive-workspace" => Method::ExhaustiveWorkspace,
        "exhaustive-cached" => Method::ExhaustiveCached,
        "exhaustive-writing" => Method::ExhaustiveWriting,
        "growing" => Method::Growing(config),
        "in-place" | "workspace" | "combined" | "compiler" | "in-place-compiler" | "all"
        | "cached" | "all-cached" | "writer" | "all-writer" => Method::Reusing(
            config,
            growing::reuse::Options {
                in_place: matches!(
                    v["kind"].as_str(),
                    Some(
                        "in-place"
                            | "combined"
                            | "in-place-compiler"
                            | "all"
                            | "all-cached"
                            | "all-writer"
                    )
                ),
                workspace: matches!(
                    v["kind"].as_str(),
                    Some("workspace" | "combined" | "all" | "cached" | "all-cached" | "all-writer")
                ),
                compiler: matches!(
                    v["kind"].as_str(),
                    Some("compiler" | "in-place-compiler" | "all" | "all-cached" | "all-writer")
                ),
                static_values: matches!(
                    v["kind"].as_str(),
                    Some("cached" | "all-cached" | "all-writer")
                ),
                direct_write: matches!(v["kind"].as_str(), Some("writer" | "all-writer")),
            },
        ),
        _ => return Err("invalid method".into()),
    })
}
fn emit(v: Value) -> Result<(), String> {
    println!("{}", serde_json::to_string(&v).map_err(|e| e.to_string())?);
    std::io::stdout().flush().map_err(|e| e.to_string())
}
// Each timing covers build + CFR until a decision result is available. Diagnostics and
// the returned graph's destructor run after the timer, identically for both paths.
fn timed(i: &Input, e: &dyn Evaluator<2>, v: &Method, n: usize) -> Result<(f64, Output), String> {
    let mut ns = 0.;
    let mut last = None;
    for _ in 0..n {
        drop(last.take());
        let start = Instant::now();
        let output = compute(i, e, v)?;
        black_box(&output);
        ns += start.elapsed().as_nanos() as f64;
        last = Some(output);
    }
    Ok((ns / n as f64, last.unwrap()))
}
fn main() -> std::process::ExitCode {
    let work = || -> Result<(), String> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 2 || !["--check", "--measure"].contains(&args[0].as_str()) {
            return Err("usage: lab-belief-workspace-speed --check|--measure case.json".into());
        }
        let path = Path::new(&args[1]);
        let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let i = load(&v, path.parent().unwrap_or(Path::new(".")))?;
        let evaluator = lab_search::model::load_evaluator("heuristic")?;
        let variants = v["variants"].as_array().ok_or("variants")?;
        let methods = variants
            .iter()
            .map(|v| method(v, i.solver))
            .collect::<Result<Vec<_>, _>>()?;
        let base = &v["baseline"];
        let baseline_method = method(base, i.solver)?;
        let reference = compute(&i, evaluator.as_ref(), &baseline_method)?;
        validate(&reference, &reference, base)?;
        emit(json!({"type":"baseline","diagnostics":diagnostics(&reference)}))?;
        let mut batches = Vec::new();
        let mut expected = Vec::new();
        let target = uint(&v, "sample_ms", 80)? as f64 * 1e6;
        for (variant, method) in variants.iter().zip(&methods) {
            let o = compute(&i, evaluator.as_ref(), method)?;
            validate(&reference, &o, variant)?;
            let d = diagnostics(&o);
            expected.push(d.clone());
            emit(json!({"type":"validation","variant":variant,"diagnostics":d}))?;
            if args[0] == "--measure" {
                let (baseline_ns, _) = timed(&i, evaluator.as_ref(), &baseline_method, 1)?;
                let (variant_ns, _) = timed(&i, evaluator.as_ref(), method, 1)?;
                let counts = [baseline_ns, variant_ns]
                    .map(|ns| (target / ns).ceil().clamp(1., 64.) as usize);
                batches.push(counts);
                emit(
                    json!({"type":"calibration","variant":variant["id"],"baseline_ns":baseline_ns,"variant_ns":variant_ns,"counts":counts}),
                )?;
            }
        }
        if args[0] == "--check" {
            return emit(json!({"type":"checked","timing":false}));
        }
        let rounds = uint(&v, "rounds", 7)?;
        if !(3..=31).contains(&rounds) {
            return Err("rounds range 3..31".into());
        }
        let base_d = diagnostics(&reference);
        drop(reference);
        for block in 0..rounds {
            for offset in 0..variants.len() {
                // Rotate treatments between blocks; ABBA/BAAB balances within-block drift.
                let index = (offset + block) % variants.len();
                let variant = &variants[index];
                let order = if block % 2 == 0 {
                    [0, 1, 1, 0]
                } else {
                    [1, 0, 0, 1]
                };
                let mut samples = Vec::new();
                for (slot, which) in order.into_iter().enumerate() {
                    let treatment = if which == 0 {
                        &baseline_method
                    } else {
                        &methods[index]
                    };
                    let (ns, o) = timed(&i, evaluator.as_ref(), treatment, batches[index][which])?;
                    if diagnostics(&o)
                        != if which == 0 {
                            base_d.clone()
                        } else {
                            expected[index].clone()
                        }
                    {
                        return Err("nondeterministic result during timing".into());
                    }
                    samples.push(json!({"slot":slot,"treatment":if which==0 {"baseline"} else {"candidate"},"ns_per_solve":ns,"batch":batches[index][which]}));
                }
                emit(
                    json!({"type":"paired","block":block,"variant":variant["id"],"samples":samples}),
                )?;
            }
        }
        emit(json!({"type":"completed","rounds":rounds,"timing":true}))
    };
    match work() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-belief-workspace-speed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
