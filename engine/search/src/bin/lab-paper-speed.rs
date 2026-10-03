//! Equal-gap benchmark. Fixed-tree and full-growing costs are separate experiments.
use lab_engine::{eval::Evaluator, rules::Ruleset, state::SideId, turn::EnumerateOptions};
use lab_search::{
    bayesian::{
        self,
        engine::{EngineWorld, Knowledge},
        tree::{
            self,
            builder::{growing, Built, Limits},
            paper,
        },
    },
    budgeted::Position,
};
use serde_json::{json, Value};
use std::{hint::black_box, io::Write, path::Path, time::Instant};
#[path = "lab-belief-workspace/paper_options.rs"]
mod paper_options;
struct Input {
    worlds: Vec<EngineWorld<2>>,
    knowledge: Knowledge,
    limits: Limits,
    solver: bayesian::Config,
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

// Keep the common timing wrapper on the stack rather than adding a timed heap allocation.
#[allow(clippy::large_enum_variant)]
enum ResultRun {
    Fixed(paper::Run),
    Grown(growing::reuse::paper::PaperResult),
}
impl ResultRun {
    fn solution(&self) -> &tree::Solution {
        match self {
            Self::Fixed(r) => &r.solution,
            Self::Grown(r) => &r.search.solution,
        }
    }
    fn tree<'a>(&'a self, fixed: &'a Option<Built>) -> &'a tree::Tree {
        match self {
            Self::Fixed(_) => &fixed.as_ref().unwrap().tree,
            Self::Grown(r) => &r.search.built.tree,
        }
    }
    fn complete(&self) -> bool {
        match self {
            Self::Fixed(_) => true,
            Self::Grown(r) => r.search.horizon_complete,
        }
    }
    fn diagnostics(&self, fixed: &Option<Built>) -> Value {
        let s = self.solution();
        let t = self.tree(fixed);
        let mut v = json!({"nodes":t.node_count(),"information":t.information().len(),"iterations":s.iterations,"gap":s.assessment.gap,"value":s.assessment.value,"lower":s.assessment.lower,"upper":s.assessment.upper,"converged":s.converged,"complete":self.complete()});
        match self {
            Self::Fixed(r) => v["paper"] = paper_options::stats(&r.stats),
            Self::Grown(r) => {
                let w = &r.search.work;
                v["paper"] = paper_options::growth(&r.stats);
                v["growth"] = json!({"solves":w.solves,"cfr_iterations":w.cfr_iterations,"attempted_transitions":w.attempted_transitions,"committed_expansions":w.committed_expansions,"walks":w.walks});
            }
        }
        v
    }
}
fn compute(
    i: &Input,
    e: &dyn Evaluator<2>,
    fixed: &Option<Built>,
    pool: Option<&rayon::ThreadPool>,
    variant: &Value,
) -> Result<ResultRun, String> {
    let settings = paper_options::parse(&variant["paper"])?;
    if let Some(built) = fixed {
        return paper::solve(&built.tree, i.solver, settings)
            .map(ResultRun::Fixed)
            .map_err(|e| e.to_string());
    }
    let search = growing::reuse::Settings {
        growth: growing::Config {
            solver: i.solver,
            max_expansions: 10000,
            max_walks: 100000,
            exploration: 1.,
            seed: 1,
        },
        storage: growing::reuse::Options {
            in_place: true,
            workspace: true,
            compiler: true,
            static_values: true,
            direct_write: true,
        },
    };
    if variant["reference"].as_bool().unwrap_or(false) {
        let search = tree::engine::cadence::growing(
            &i.worlds,
            SideId::One,
            Ruleset::CHAMPIONS_MC,
            e,
            &i.knowledge,
            i.limits,
            tree::engine::cadence::Settings {
                search,
                cadence: 4,
                pool,
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(ResultRun::Grown(growing::reuse::paper::PaperResult {
            search,
            stats: Default::default(),
        }))
    } else {
        tree::engine::paper::growing(
            &i.worlds,
            SideId::One,
            Ruleset::CHAMPIONS_MC,
            e,
            &i.knowledge,
            i.limits,
            tree::engine::paper::Settings {
                search,
                cadence: 4,
                pool,
                solver: settings,
            },
        )
        .map(ResultRun::Grown)
        .map_err(|e| e.to_string())
    }
}
fn emit(v: Value) -> Result<(), String> {
    println!("{}", serde_json::to_string(&v).map_err(|e| e.to_string())?);
    std::io::stdout().flush().map_err(|e| e.to_string())
}
#[allow(clippy::too_many_arguments)]
fn timed(
    i: &Input,
    e: &dyn Evaluator<2>,
    fixed: &Option<Built>,
    pool: Option<&rayon::ThreadPool>,
    variant: &Value,
    count: usize,
) -> Result<(f64, ResultRun), String> {
    let mut ns = 0.;
    let mut last = None;
    for _ in 0..count {
        drop(last.take());
        let started = Instant::now();
        let r = compute(black_box(i), e, fixed, pool, variant)?;
        black_box(&r);
        ns += started.elapsed().as_nanos() as f64;
        last = Some(r);
    }
    Ok((ns / count as f64, last.unwrap()))
}
fn main() -> std::process::ExitCode {
    let work = || -> Result<(), String> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 2 || !["--check", "--measure"].contains(&args[0].as_str()) {
            return Err("usage: lab-paper-speed --check|--measure case.json".into());
        }
        let measure = args[0] == "--measure";
        let path = Path::new(&args[1]);
        let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let i = load(&v, path.parent().unwrap_or(Path::new(".")))?;
        let e = lab_search::model::load_evaluator("heuristic")?;
        let fixed = match string(&v, "phase")? {
            "fixed" => Some(
                tree::engine::build_owned(
                    &i.worlds,
                    SideId::One,
                    Ruleset::CHAMPIONS_MC,
                    e.as_ref(),
                    &i.knowledge,
                    i.limits,
                )
                .map_err(|e| e.to_string())?,
            ),
            "growing" => None,
            _ => return Err("phase must be fixed or growing".into()),
        };
        let threads = uint(&v, "threads", 1)?;
        if ![1, 2, 4].contains(&threads) {
            return Err("invalid threads".into());
        }
        let pool = if threads > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .stack_size(16 * 1024 * 1024)
                    .build()
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let baseline = &v["baseline"];
        let variants = v["variants"].as_array().ok_or("variants")?;
        let reference = compute(&i, e.as_ref(), &fixed, pool.as_ref(), baseline)?;
        let canonical_reference = canonical(reference.tree(&fixed));
        let baseline_diag = reference.diagnostics(&fixed);
        emit(json!({"type":"baseline","diagnostics":baseline_diag}))?;
        let mut expected = Vec::new();
        let mut eligible = Vec::new();
        for variant in variants {
            let r = compute(&i, e.as_ref(), &fixed, pool.as_ref(), variant)?;
            if canonical(r.tree(&fixed)) != canonical_reference
                || !r.complete()
                || !reference.complete()
            {
                return Err("not the same completed finite game".into());
            }
            let a = r
                .tree(&fixed)
                .assess(&r.solution().policy)
                .map_err(|e| e.to_string())?;
            if format!("{a:?}") != format!("{:?}", r.solution().assessment) {
                return Err("original-tree certificate mismatch".into());
            }
            let s = r.solution();
            let rs = reference.solution();
            if a.lower > rs.assessment.upper + 1e-8 || a.upper < rs.assessment.lower - 1e-8 {
                return Err("disjoint certified value intervals".into());
            }
            if variant["exact_control"].as_bool().unwrap_or(false)
                && (s.policy != rs.policy || format!("{s:?}") != format!("{rs:?}"))
            {
                return Err("LCFR original control mismatch".into());
            }
            let ok = s.converged && rs.converged;
            let diag = r.diagnostics(&fixed);
            expected.push(diag.clone());
            eligible.push(ok);
            emit(
                json!({"type":"validation","variant":variant,"diagnostics":diag,"equal_gap_eligible":ok}),
            )?;
        }
        if !measure {
            return emit(json!({"type":"checked","timing":false}));
        }
        drop(reference);
        let rounds = uint(&v, "rounds", 9)?;
        if !(3..=31).contains(&rounds) {
            return Err("invalid rounds".into());
        }
        let target = uint(&v, "sample_ms", 80)? as f64 * 1e6;
        let mut counts = Vec::new();
        for (index, variant) in variants.iter().enumerate() {
            if !eligible[index] {
                counts.push([0, 0]);
                continue;
            }
            let (a, _) = timed(&i, e.as_ref(), &fixed, pool.as_ref(), baseline, 1)?;
            let (b, _) = timed(&i, e.as_ref(), &fixed, pool.as_ref(), variant, 1)?;
            let n = [a, b].map(|ns| (target / ns).ceil().clamp(1., 64.) as usize);
            counts.push(n);
            emit(
                json!({"type":"calibration","variant":variant["id"],"baseline_ns":a,"variant_ns":b,"counts":n}),
            )?;
        }
        for block in 0..rounds {
            for offset in 0..variants.len() {
                let index = (block + offset) % variants.len();
                if !eligible[index] {
                    continue;
                }
                let order = if block % 2 == 0 {
                    [0, 1, 1, 0]
                } else {
                    [1, 0, 0, 1]
                };
                let mut samples = Vec::new();
                for (slot, which) in order.into_iter().enumerate() {
                    let variant = if which == 0 {
                        baseline
                    } else {
                        &variants[index]
                    };
                    let (ns, r) = timed(
                        &i,
                        e.as_ref(),
                        &fixed,
                        pool.as_ref(),
                        variant,
                        counts[index][which],
                    )?;
                    if r.diagnostics(&fixed)
                        != if which == 0 {
                            baseline_diag.clone()
                        } else {
                            expected[index].clone()
                        }
                    {
                        return Err("nondeterministic timing result".into());
                    }
                    samples.push(json!({"slot":slot,"treatment":if which==0 {"baseline"} else {"candidate"},"ns_per_solve":ns,"batch":counts[index][which]}));
                }
                emit(
                    json!({"type":"paired","block":block,"variant":variants[index]["id"],"samples":samples}),
                )?;
            }
        }
        emit(json!({"type":"completed","timing":true,"rounds":rounds,"eligible":eligible}))
    };
    match work() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lab-paper-speed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
