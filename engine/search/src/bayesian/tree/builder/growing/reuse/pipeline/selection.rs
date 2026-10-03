//! The index exists only while selecting from one solved frontier snapshot.
//! Group order, RNG calls, prior calls and visit updates match the reference.
use super::*;
fn sample_edges(random: &mut Random, edges: &[(f64, usize)]) -> usize {
    let mut x = random.unit();
    for (i, &(p, _)) in edges.iter().enumerate() {
        if x < p {
            return i;
        }
        x -= p;
    }
    edges
        .iter()
        .rposition(|&(p, _)| p > 0.)
        .expect("validated probabilities")
}
#[allow(clippy::too_many_arguments)]
pub(super) fn keys<D: ObservedDomain, P: Prior>(
    g: &Growing<'_, D>,
    tree: &Tree,
    policy: &Policy,
    q: &[Vec<f64>],
    prior: &P,
    visits: &mut Visits,
    random: &mut Random,
    work: &mut Work,
    cfg: Config,
    target: usize,
) -> Result<Vec<Vec<String>>, Error> {
    let mut node_group = vec![usize::MAX; tree.node_count()];
    let mut groups = Vec::with_capacity(g.frontier.len());
    for (group, (key, histories)) in g.frontier.iter().enumerate() {
        groups.push(key);
        for history in histories {
            node_group[history.node] = group;
        }
    }
    let mut selected_groups = Vec::with_capacity(target);
    while selected_groups.len() < target && work.walks < cfg.max_walks {
        work.walks += 1;
        let selected = select(tree, policy, q, prior, visits, random, cfg.exploration)?;
        let group = node_group[selected];
        if group != usize::MAX && !selected_groups.contains(&group) {
            selected_groups.push(group);
        }
    }
    Ok(selected_groups
        .into_iter()
        .map(|i| groups[i].clone())
        .collect())
}
fn select<P: Prior>(
    t: &Tree,
    policy: &Policy,
    q: &[Vec<f64>],
    prior: &P,
    visits: &mut Visits,
    random: &mut Random,
    exploration: f64,
) -> Result<usize, Error> {
    let mut n = t.root();
    loop {
        n = match &t.nodes[n] {
            Compiled::Terminal(_) => return Ok(n),
            Compiled::Chance(edges) => edges[sample_edges(random, edges)].1,
            Compiled::Decision { info, children } => {
                let information = &t.information[*info];
                let mut p = prior.weights(information);
                if p.len() != children.len() {
                    return Err(Error("prior menu width mismatch".into()));
                }
                normalize(&mut p)?;
                let counts = visits
                    .entry(information.key.clone())
                    .or_insert_with(|| vec![0; children.len()]);
                if counts.len() != children.len() {
                    return Err(Error("changed selection menu".into()));
                }
                let total = counts.iter().map(|v| *v as f64).sum::<f64>();
                let mut best = 0;
                let mut score = f64::NEG_INFINITY;
                for a in 0..children.len() {
                    let s = q[*info][a]
                        + exploration * p[a] * (total + 1.).sqrt() / (1. + counts[a] as f64);
                    if !s.is_finite() {
                        return Err(Error("nonfinite PUCT score".into()));
                    }
                    if s > score {
                        score = s;
                        best = a;
                    }
                }
                let a = if random.unit() < 0.5 {
                    best
                } else {
                    random.sample(&policy[*info])
                };
                counts[a] = counts[a]
                    .checked_add(1)
                    .ok_or_else(|| Error("visit count overflow".into()))?;
                children[a]
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampler_preserves_draws_zero_edges_and_tail_fallback() {
        for weights in [
            vec![0., 1., 0.],
            vec![0.1, 0.2, 0.7],
            vec![f64::from_bits(1), 1., 0.],
            vec![0.2, 0.3, 0.499999999999],
        ] {
            let edges: Vec<_> = weights.iter().enumerate().map(|(i, &p)| (p, i)).collect();
            let mut a = Random(19);
            let mut b = Random(19);
            for _ in 0..10000 {
                assert_eq!(a.sample(&weights), sample_edges(&mut b, &edges));
                assert_eq!(a.0, b.0);
            }
        }
    }
    #[test]
    fn selection_preserves_visits_and_stateful_prior_calls() {
        struct Stateful(std::cell::Cell<usize>);
        impl Prior for Stateful {
            fn weights(&self, info: &tree::Information) -> Vec<f64> {
                self.0.set(self.0.get() + 1);
                (0..info.actions.len())
                    .map(|a| 1. + ((a + self.0.get()) % 3) as f64)
                    .collect()
            }
        }
        let t = Tree::new(
            vec![
                Node::Chance(vec![(0., 1), (0.4, 2), (0.6, 3)]),
                Node::Terminal(0.),
                Node::Decision {
                    player: 0,
                    information: "same".into(),
                    actions: vec!["a".into(), "b".into()],
                    children: vec![4, 5],
                },
                Node::Decision {
                    player: 0,
                    information: "same".into(),
                    actions: vec!["a".into(), "b".into()],
                    children: vec![6, 7],
                },
                Node::Terminal(1.),
                Node::Terminal(-1.),
                Node::Terminal(-1.),
                Node::Terminal(1.),
            ],
            0,
        )
        .unwrap();
        let p = t.uniform();
        let q = scores(&t, &p).unwrap();
        let mut a = Random(7);
        let mut b = Random(7);
        let mut va = Visits::new();
        let mut vb = Visits::new();
        let pa = Stateful(std::cell::Cell::new(0));
        let pb = Stateful(std::cell::Cell::new(0));
        for _ in 0..1000 {
            assert_eq!(
                super::super::super::select(&t, &p, &q, &pa, &mut va, &mut a, 1.).unwrap(),
                select(&t, &p, &q, &pb, &mut vb, &mut b, 1.).unwrap()
            );
            assert_eq!(a.0, b.0);
            assert_eq!(va, vb);
            assert_eq!(pa.0.get(), pb.0.get());
        }
    }
}
