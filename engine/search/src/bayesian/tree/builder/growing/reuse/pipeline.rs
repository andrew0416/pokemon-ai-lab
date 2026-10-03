//! Isolated paper solvers inside the original atomic cadence admission loop.
//! Admissions in one selected batch publish atomically after a complete CFR solve.
//! Budget failure discards the whole pending batch and returns the last solved snapshot.
pub use super::paper::PaperStats;
pub struct PaperResult {
    pub search: ResultTree,
    pub stats: PaperStats,
    pub metrics: tree::pipeline::Metrics,
}
impl From<super::paper::PaperResult> for PaperResult {
    fn from(r: super::paper::PaperResult) -> Self {
        Self {
            search: r.search,
            stats: r.stats,
            metrics: Default::default(),
        }
    }
}
use super::*;
mod selection;
#[allow(clippy::too_many_arguments)]
pub fn search<D: ObservedDomain, P: Prior>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
    cfg: Config,
    prior: &P,
    options: Options,
    cadence: usize,
    solver_settings: tree::paper::Settings,
    pipeline_settings: tree::pipeline::Settings,
) -> Result<PaperResult, Error> {
    with_batch(
        domain,
        seeds,
        limits,
        cfg,
        prior,
        options,
        cadence,
        solver_settings,
        pipeline_settings,
        None,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn with_batch<D: ObservedDomain, P: Prior>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
    cfg: Config,
    prior: &P,
    options: Options,
    cadence: usize,
    solver_settings: tree::paper::Settings,
    pipeline_settings: tree::pipeline::Settings,
    batch: Option<&dyn Batch<D>>,
) -> Result<PaperResult, Error> {
    tree::pipeline::validate(pipeline_settings, solver_settings)?;
    // A one-turn admission closes the horizon before its first solve. There can
    // be no second compiled snapshot to reuse, so avoid constructing its caches.
    let pipeline_settings = tree::pipeline::Settings {
        owned_compiler: pipeline_settings.owned_compiler && limits.turns > 1,
        incremental_sequence: pipeline_settings.incremental_sequence && limits.turns > 1,
        ..pipeline_settings
    };
    macro_rules! run {
        ($n:literal) => {
            if limits.turns > 1 {
                search_impl::<D, P, true, $n>(
                    domain,
                    seeds,
                    limits,
                    cfg,
                    prior,
                    options,
                    solver_settings,
                    pipeline_settings,
                    batch,
                )
            } else {
                search_impl::<D, P, false, $n>(
                    domain,
                    seeds,
                    limits,
                    cfg,
                    prior,
                    options,
                    solver_settings,
                    pipeline_settings,
                    batch,
                )
            }
        };
    }
    match cadence {
        1 => run!(1),
        2 => run!(2),
        4 => run!(4),
        _ => Err(Error("cadence must be 1, 2 or 4".into())),
    }
}

#[allow(clippy::too_many_arguments)]
fn search_impl<D: ObservedDomain, P: Prior, const INCREMENTAL: bool, const CADENCE: usize>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
    cfg: Config,
    prior: &P,
    options: Options,
    solver_settings: tree::paper::Settings,
    pipeline_settings: tree::pipeline::Settings,
    #[cfg(feature = "experiment-parallel-transitions")] batch: Option<&dyn Batch<D>>,
) -> Result<PaperResult, Error> {
    if cfg.max_expansions == 0
        || cfg.max_walks == 0
        || !cfg.exploration.is_finite()
        || cfg.exploration < 0.
        || limits.turns == 0
        || limits.turns > 32
        || limits.max_decisions == 0
        || limits.max_decisions > 128
        || limits.max_nodes == 0
        || limits.max_transitions == 0
    {
        return Err(Error("invalid growing-tree configuration".into()));
    }
    // Validate solver configuration before any domain calls or transition work.
    if cfg.solver.iterations == 0
        || cfg.solver.iterations > 10_000_000
        || cfg.solver.check_every == 0
        || !cfg.solver.tolerance.is_finite()
        || cfg.solver.tolerance < 0.
    {
        return Err(Error("invalid CFR configuration".into()));
    }
    labels(seeds.iter().map(|s| s.id.as_str()), "world ID")?;
    let mut weights: Vec<_> = seeds.iter().map(|s| s.weight).collect();
    normalize(&mut weights)?;
    let first = domain.observation(&seeds[0].position)?;
    let mut growing = Growing {
        #[cfg(feature = "experiment-parallel-transitions")]
        batch,
        b: Builder {
            domain,
            limits,
            nodes: Vec::new(),
            stats: Stats::default(),
            menus: HashMap::new(),
            public: HashMap::new(),
            public_keys: Vec::new(),
            private: HashMap::new(),
            private_keys: Vec::new(),
            boundaries: Vec::new(),
        },
        root: 0,
        worlds: seeds.iter().map(|s| s.id.clone()).collect(),
        frontier: BTreeMap::new(),
    };
    let initial = (|| -> Attempt<()> {
        growing.root = growing.push(Node::Terminal(0.))?;
        let mut edges = Vec::new();
        for (world, (s, weight)) in seeds.iter().zip(weights).enumerate() {
            let obs = domain.observation(&s.position)?;
            if obs.public != first.public
                || obs.private[0] != first.private[0]
                || domain.phase(&s.position)? != Phase::Turn
            {
                return Err(Error(
                    "root worlds must share our initial information at a normal turn".into(),
                )
                .into());
            }
            let memory = [
                Cursor::new(0, None, &obs)?.memory,
                Cursor::new(1, Some(&s.id), &obs)?.memory,
            ];
            let n = growing.leaf(Frontier {
                node: 0,
                position: s.position.clone(),
                world,
                memory: History::new(memory, pipeline_settings.encoded_history),
                public: vec![obs.public],
                turns: limits.turns,
                decisions: 0,
                phase: Phase::Turn,
            })?;
            edges.push((weight, n));
        }
        growing.b.nodes[growing.root] = Node::Chance(edges);
        Ok(())
    })();
    let fatal = |e: Failure| match e {
        Failure::Invalid(e) => e,
        Failure::Limit(s) => Error(format!("root incomplete: {s:?}; no strategy returned")),
    };
    initial.map_err(fatal)?;
    let mut work = Work::default();
    growing
        .admit(&[first.public], &mut work, cfg)
        .map_err(fatal)?;
    work.committed_expansions = work.attempted_expansions;
    #[cfg(feature = "experiment-incremental-compilation")]
    let mut incremental = tree::compiler::incremental::Cache::default();
    #[allow(unused_mut)]
    let mut compile = |g: &Growing<'_, D>| -> Result<Tree, Error> {
        #[cfg(feature = "experiment-phase-cost")]
        let _compile = crate::bayesian::tree::phase_cost::Span::new(
            crate::bayesian::tree::phase_cost::Phase::Compile,
        );
        #[cfg(feature = "experiment-incremental-compilation")]
        if INCREMENTAL {
            let mut t = incremental.growing(&g.b.nodes, g.root)?;
            t.worlds = g.worlds.clone();
            t.public_keys = g.b.public_keys.clone();
            t.private_keys = g.b.private_keys.clone();
            t.boundaries = g.b.boundaries.clone();
            return Ok(t);
        }
        compile(g, options.compiler)
    };
    let mut owned = tree::compiler::owned::Cache::default();
    let mut previous = None;
    let mut context = tree::pipeline::Context::default();
    let mut metrics = tree::pipeline::Metrics::default();
    let mut paper_stats = PaperStats::default();
    let mut solve_paper = |tree: &Tree,
                           delta: Option<&tree::compiler::owned::Delta>|
     -> Result<(Solution, Vec<Vec<f64>>), Error> {
        let run = if pipeline_settings.incremental_sequence || pipeline_settings.compressed_checks {
            tree::pipeline::solve_from(
                tree,
                cfg.solver,
                solver_settings,
                previous.as_ref(),
                pipeline_settings,
                delta,
                &mut context,
                &mut metrics,
            )?
        } else {
            tree::paper::solve_from(tree, cfg.solver, solver_settings, previous.as_ref())?
        };
        let q = scores(tree, &run.solution.policy)?;
        if solver_settings.reuse_policy || solver_settings.warm_iterations > 0 {
            previous = Some(tree::paper::reuse::Snapshot::capture(
                tree,
                &run.solution.policy,
            )?);
        }
        paper_stats.requests += 1;
        paper_stats.reused += usize::from(run.stats.reused);
        paper_stats.compact += usize::from(run.stats.compact);
        paper_stats.sequence += usize::from(run.stats.sequence);
        paper_stats.assessments += run.stats.assessments;
        paper_stats.mapped_information += run.stats.mapped_information;
        paper_stats.scalar_reach_fallbacks += run.stats.scalar_reach_fallbacks;
        paper_stats.warm_attempted += usize::from(run.stats.warm_attempted);
        paper_stats.warm_applied += usize::from(run.stats.warm_applied);
        paper_stats.virtual_iterations += run.stats.virtual_iterations;
        Ok((run.solution, q))
    };
    let mut t = if pipeline_settings.owned_compiler {
        #[cfg(feature = "experiment-phase-cost")]
        let _compile = crate::bayesian::tree::phase_cost::Span::new(
            crate::bayesian::tree::phase_cost::Phase::Compile,
        );
        let (mut t, _) = owned.growing(None, &growing.b.nodes, growing.root)?;
        t.worlds = growing.worlds.clone();
        t.public_keys = growing.b.public_keys.clone();
        t.private_keys = growing.b.private_keys.clone();
        t.boundaries = growing.b.boundaries.clone();
        t
    } else {
        compile(&growing)?
    };
    let (mut solved, mut q) = solve_paper(&t, None)?;
    work.solves += 1;
    work.cfr_iterations += solved.iterations;
    // Only this compact committed summary is externally observable if admission fails.
    // The working builder is private to this call and is discarded on any failed attempt.
    let mut committed = summary(&growing);
    let mut random = Random(cfg.seed);
    let mut visits = Visits::new();
    let stop = 'growth: loop {
        if growing.frontier.is_empty() {
            break Stop::HorizonComplete;
        }
        if work.attempted_expansions >= cfg.max_expansions {
            break Stop::ExpansionLimit;
        }
        if work.walks >= cfg.max_walks {
            break Stop::WalkLimit;
        }
        // Select distinct existing public groups against ONE fully solved snapshot.
        // New continuations cannot be selected until the next full solve.
        #[cfg(feature = "experiment-phase-cost")]
        let selection_phase = crate::bayesian::tree::phase_cost::Span::new(
            crate::bayesian::tree::phase_cost::Phase::Selection,
        );
        let target = CADENCE
            .min(cfg.max_expansions - work.attempted_expansions)
            .min(growing.frontier.len());
        let keys = if pipeline_settings.frontier_index {
            selection::keys(
                &growing,
                &t,
                &solved.policy,
                &q,
                prior,
                &mut visits,
                &mut random,
                &mut work,
                cfg,
                target,
            )?
        } else {
            let mut keys = Vec::with_capacity(target);
            while keys.len() < target && work.walks < cfg.max_walks {
                work.walks += 1;
                let selected = select(
                    &t,
                    &solved.policy,
                    &q,
                    prior,
                    &mut visits,
                    &mut random,
                    cfg.exploration,
                )?;
                if let Some(key) = growing
                    .frontier
                    .iter()
                    .find(|(_, g)| g.iter().any(|h| h.node == selected))
                    .map(|(k, _)| k.clone())
                {
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
            keys
        };
        #[cfg(feature = "experiment-phase-cost")]
        drop(selection_phase);
        if keys.is_empty() {
            continue;
        }
        let mut fork = (!options.in_place).then(|| growing.fork());
        let candidate = fork.as_mut().unwrap_or(&mut growing);
        let before = work.attempted_expansions;
        for key in keys {
            match candidate.admit(&key, &mut work, cfg) {
                Err(Failure::Limit(s)) => break 'growth s,
                Err(Failure::Invalid(e)) => return Err(e),
                Ok(()) => {}
            }
        }
        // Admission limits were handled above, while `t` was still intact.
        // Compilation/solver errors return Err, so no old snapshot is promised there.
        let (next, delta) = if pipeline_settings.owned_compiler {
            #[cfg(feature = "experiment-phase-cost")]
            let _compile = crate::bayesian::tree::phase_cost::Span::new(
                crate::bayesian::tree::phase_cost::Phase::Compile,
            );
            let (mut next, delta) = owned.growing(Some(t), &candidate.b.nodes, candidate.root)?;
            next.worlds = candidate.worlds.clone();
            next.public_keys = candidate.b.public_keys.clone();
            next.private_keys = candidate.b.private_keys.clone();
            next.boundaries = candidate.b.boundaries.clone();
            (next, Some(delta))
        } else {
            (compile(candidate)?, None)
        };
        let (solution, next_q) = solve_paper(&next, delta.as_ref())?;
        q = next_q;
        work.committed_expansions += work.attempted_expansions - before;
        work.solves += 1;
        work.cfr_iterations += solution.iterations;
        committed = summary(candidate);
        if let Some(candidate) = fork {
            growing = candidate;
        }
        t = next;
        solved = solution;
    };
    metrics.owned_compiles = if pipeline_settings.owned_compiler {
        work.solves
    } else {
        0
    };
    Ok(PaperResult {
        metrics,
        search: ResultTree {
            built: Built {
                tree: t,
                stats: committed.0,
            },
            solution: solved,
            stop,
            frontier_histories: committed.1,
            frontier_public_groups: committed.2,
            horizon_complete: committed.2 == 0,
            work,
        },
        stats: paper_stats,
    })
}
