//! Same growing-tree algorithm with optional owned-builder reuse and CFR scratch reuse.
//! A failed admission terminates this call. Its mutable builder is never published;
//! the last compiled tree, policy and committed counters are returned atomically.
use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub in_place: bool,
    pub workspace: bool,
    pub compiler: bool,
    pub static_values: bool,
    /// Storage-only observation formatter; consumed by the engine adapter.
    pub direct_write: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub growth: Config,
    pub storage: Options,
}
fn compile<D: ObservedDomain>(g: &Growing<'_, D>, borrowed: bool) -> Result<Tree, Error> {
    if !borrowed {
        return g.compile();
    }
    let mut t = tree::compiler::compile(&g.b.nodes, g.root)?;
    t.worlds = g.worlds.clone();
    t.public_keys = g.b.public_keys.clone();
    t.private_keys = g.b.private_keys.clone();
    t.boundaries = g.b.boundaries.clone();
    Ok(t)
}
fn summary<D: ObservedDomain>(g: &Growing<'_, D>) -> (Stats, usize, usize) {
    (
        g.b.stats.clone(),
        g.frontier.values().map(Vec::len).sum(),
        g.frontier.len(),
    )
}
fn solve(t: &Tree, config: CfrConfig, options: Options) -> Result<Solution, Error> {
    if options.static_values {
        tree::workspace::cached::solve(t, config)
    } else if options.workspace {
        tree::workspace::solve(t, config)
    } else {
        tree::solve(t, config)
    }
}

pub fn search<D: ObservedDomain, P: Prior>(
    domain: &D,
    seeds: &[Seed<D::Position>],
    limits: Limits,
    cfg: Config,
    prior: &P,
    options: Options,
) -> Result<ResultTree, Error> {
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
                memory,
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
    let mut t = compile(&growing, options.compiler)?;
    let mut solved = solve(&t, cfg.solver, options)?;
    work.solves += 1;
    work.cfr_iterations += solved.iterations;
    // Only this compact committed summary is externally observable if admission fails.
    // The working builder is private to this call and is discarded on any failed attempt.
    let mut committed = summary(&growing);
    let mut q = scores(&t, &solved.policy)?;
    let mut random = Random(cfg.seed);
    let mut visits = Visits::new();
    let stop = loop {
        if growing.frontier.is_empty() {
            break Stop::HorizonComplete;
        }
        if work.attempted_expansions >= cfg.max_expansions {
            break Stop::ExpansionLimit;
        }
        if work.walks >= cfg.max_walks {
            break Stop::WalkLimit;
        }
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
        let key = growing
            .frontier
            .iter()
            .find(|(_, g)| g.iter().any(|h| h.node == selected))
            .map(|(k, _)| k.clone());
        let Some(key) = key else { continue };
        let mut fork = (!options.in_place).then(|| growing.fork());
        let candidate = fork.as_mut().unwrap_or(&mut growing);
        let before = work.attempted_expansions;
        match candidate.admit(&key, &mut work, cfg) {
            Err(Failure::Limit(s)) => break s,
            Err(Failure::Invalid(e)) => return Err(e),
            Ok(()) => {}
        }
        let next = compile(candidate, options.compiler)?;
        let solution = solve(&next, cfg.solver, options)?;
        q = scores(&next, &solution.policy)?;
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
    Ok(ResultTree {
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
    })
}
