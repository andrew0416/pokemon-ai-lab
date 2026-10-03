//! Reuse payoff structure across a private consuming-compiler lineage.
//! Stable own-sequence IDs survive canonical information-ID renumbering.
//! Contributions are stored before scaling; no subtraction of rounded aggregates.
use super::*;
use crate::bayesian::tree::{compiler::owned::Delta, pipeline::Metrics};

#[derive(Clone, Copy)]
struct Leaf {
    sequence: [usize; 2],
    chance: f64,
    value: f64,
    bucket: usize,
    slot: usize,
}
struct Bucket {
    sequence: [usize; 2],
    members: Vec<usize>,
    coefficient: f64,
    dirty: bool,
}
#[derive(Default)]
pub(in crate::bayesian::tree) struct Cache {
    pub(in crate::bayesian::tree) kernel: Option<Kernel>,
    leaves: Vec<Option<Leaf>>,
    buckets: Vec<Bucket>,
    lookup: HashMap<[usize; 2], usize>,
    scale: f64,
}
impl Cache {
    pub(in crate::bayesian::tree) fn update(
        &mut self,
        tree: &Tree,
        delta: Option<&Delta>,
        metrics: &mut Metrics,
    ) {
        if let Some(d) = delta.filter(|d| !d.rebuilt) {
            if self.kernel.is_some() && self.advance(tree, d, metrics).is_some() {
                metrics.sequence_delta_updates += 1;
                return;
            }
        }
        *self = Self::default();
        metrics.sequence_full_builds += 1;
        if self.build(tree, metrics).is_none() {
            *self = Self::default();
            metrics.sequence_fallbacks += 1;
        }
    }
    fn layout(tree: &Tree, previous: Option<(&Kernel, &Delta)>) -> Option<Kernel> {
        let mut offsets = vec![usize::MAX; tree.information.len()];
        let mut counts = [1usize; 2];
        if let Some((old, d)) = previous {
            if old.offsets.len() != d.old_to_new.len() {
                return None;
            }
            counts = old.counts;
            for (old_id, &new_id) in d.old_to_new.iter().enumerate() {
                *offsets.get_mut(new_id)? = old.offsets[old_id];
            }
        }
        for (i, info) in tree.information.iter().enumerate() {
            if offsets[i] == usize::MAX {
                offsets[i] = counts[info.player];
                counts[info.player] = counts[info.player].checked_add(info.actions.len())?;
            }
        }
        if counts.iter().sum::<usize>() > 1_048_576 {
            return None;
        }
        let parent = tree
            .information
            .iter()
            .map(|i| i.own_sequence.last().map_or(0, |&(j, a)| offsets[j] + a))
            .collect();
        let mut order: Vec<_> = (0..tree.information.len()).collect();
        order.sort_by_key(|&i| tree.information[i].own_sequence.len());
        Some(Kernel {
            offsets,
            parent,
            counts,
            order,
            entries: Vec::new(),
            min_chance: 1.,
            realization: Vec::new(),
            gradient: Vec::new(),
            safe: false,
        })
    }
    fn build(&mut self, tree: &Tree, metrics: &mut Metrics) -> Option<()> {
        self.kernel = Some(Self::layout(tree, None)?);
        self.leaves.resize(tree.nodes.len(), None);
        self.visit(tree, tree.root, [0, 0], 1., metrics)?;
        self.finish(tree, true)
    }
    fn advance(&mut self, tree: &Tree, d: &Delta, metrics: &mut Metrics) -> Option<()> {
        if d.old_nodes != self.leaves.len() || tree.nodes.len() < d.old_nodes {
            return None;
        }
        let old = self.kernel.as_ref()?;
        let mut layout = Self::layout(tree, Some((old, d)))?;
        let old = self.kernel.take()?;
        layout.entries = old.entries;
        layout.realization = old.realization;
        layout.gradient = old.gradient;
        // A conservative historical minimum is safe. It can only trigger extra
        // original reach checks after a tiny-probability terminal disappears.
        layout.min_chance = old.min_chance;
        self.kernel = Some(layout);
        self.leaves.resize(tree.nodes.len(), None);
        for &n in &d.replaced {
            let old = self.leaves.get_mut(n)?.take()?;
            let bucket = &mut self.buckets[old.bucket];
            bucket.members.swap_remove(old.slot);
            bucket.dirty = true;
            if let Some(&moved) = bucket.members.get(old.slot) {
                self.leaves[moved].as_mut()?.slot = old.slot;
            }
            self.visit(tree, n, old.sequence, old.chance, metrics)?;
        }
        let changed = self.scale.to_bits() != tree.scale.to_bits();
        metrics.sequence_scale_refreshes += usize::from(changed);
        self.finish(tree, changed)
    }
    fn visit(
        &mut self,
        tree: &Tree,
        root: usize,
        sequence: [usize; 2],
        chance: f64,
        metrics: &mut Metrics,
    ) -> Option<()> {
        let kernel = self.kernel.as_mut()?;
        let mut stack = vec![(root, sequence, chance)];
        while let Some((n, sequence, chance)) = stack.pop() {
            match &tree.nodes[n] {
                Compiled::Terminal(value) => {
                    metrics.sequence_leaf_visits += 1;
                    if self.leaves[n].is_some() {
                        return None;
                    }
                    let index = if let Some(&i) = self.lookup.get(&sequence) {
                        i
                    } else {
                        if self.buckets.len() >= 1_048_576 {
                            return None;
                        }
                        let i = self.buckets.len();
                        self.lookup.insert(sequence, i);
                        self.buckets.push(Bucket {
                            sequence,
                            members: Vec::new(),
                            coefficient: 0.,
                            dirty: true,
                        });
                        i
                    };
                    let bucket = &mut self.buckets[index];
                    self.leaves[n] = Some(Leaf {
                        sequence,
                        chance,
                        value: *value,
                        bucket: index,
                        slot: bucket.members.len(),
                    });
                    bucket.members.push(n);
                    bucket.dirty = true;
                    if chance > 0. {
                        kernel.min_chance = kernel.min_chance.min(chance);
                    }
                }
                Compiled::Chance(edges) => {
                    for &(p, c) in edges.iter().rev() {
                        stack.push((c, sequence, multiply(chance, p).ok()?));
                    }
                }
                Compiled::Decision { info, children } => {
                    let player = tree.information[*info].player;
                    if sequence[player] != kernel.parent[*info] {
                        return None;
                    }
                    for (a, &c) in children.iter().enumerate().rev() {
                        let mut next = sequence;
                        next[player] = kernel.offsets[*info] + a;
                        stack.push((c, next, chance));
                    }
                }
            }
        }
        Some(())
    }
    fn finish(&mut self, tree: &Tree, rescale: bool) -> Option<()> {
        let mut rank = vec![0; tree.nodes.len()];
        for (i, &n) in tree.order.iter().enumerate() {
            rank[n] = i;
        }
        let kernel = self.kernel.as_mut()?;
        kernel.entries.clear();
        for bucket in &mut self.buckets {
            if bucket.members.is_empty() {
                bucket.dirty = false;
                continue;
            }
            if bucket.dirty || rescale {
                // Re-sum active members in original DFS order; cancellation cannot
                // retain a removed heuristic value, even after large scale changes.
                bucket.members.sort_by_key(|&n| rank[n]);
                let mut value = 0.;
                for (slot, &n) in bucket.members.iter().enumerate() {
                    let leaf = self.leaves[n].as_mut()?;
                    leaf.slot = slot;
                    value += leaf.chance * (leaf.value / tree.scale);
                }
                if !value.is_finite() {
                    return None;
                }
                bucket.coefficient = value;
                bucket.dirty = false;
            }
            kernel
                .entries
                .push((bucket.sequence[0], bucket.sequence[1], bucket.coefficient));
        }
        kernel.entries.sort_by_key(|&(a, b, _)| (a, b));
        let width = kernel.counts[0].max(kernel.counts[1]);
        kernel.realization.resize(width, 0.);
        kernel.gradient.resize(width, 0.);
        self.scale = tree.scale;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bayesian::tree::{compiler, Node};
    fn d(p: usize, key: &str, children: Vec<usize>) -> Node {
        Node::Decision {
            player: p,
            information: key.into(),
            actions: (0..children.len()).map(|i| i.to_string()).collect(),
            children,
        }
    }
    fn check(tree: &Tree, c: &mut Cache) {
        let mut reference = Kernel::new(tree).unwrap();
        let candidate = c.kernel.as_mut().unwrap();
        let mut p = tree.uniform();
        for (i, v) in p.iter_mut().enumerate() {
            if v.len() == 2 {
                v[0] = (i % 5 + 1) as f64 / 7.;
                v[1] = 1. - v[0];
            }
        }
        for player in 0..2 {
            let mut a: Policy = p.iter().map(|v| vec![0.; v.len()]).collect();
            let mut b = a.clone();
            assert!(reference.prepare(tree, &p, player));
            assert!(candidate.prepare(tree, &p, player));
            reference.accumulate(tree, &p, player, &mut a, 1.);
            candidate.accumulate(tree, &p, player, &mut b, 1.);
            for (a, b) in a.iter().flatten().zip(b.iter().flatten()) {
                assert!((a - b).abs() < 1e-12, "{a} != {b}");
            }
        }
    }
    #[test]
    fn replaced_heuristics_scale_changes_and_reindexed_recall_match_rebuild() {
        let mut raw = vec![
            Node::Chance(vec![(0.5, 1), (0.5, 2)]),
            Node::Terminal(1e100),
            d(0, "later", vec![3, 4]),
            Node::Terminal(1.),
            Node::Terminal(-1.),
        ];
        let mut compiler = compiler::owned::Cache::default();
        let (mut tree, _) = compiler.growing(None, &raw, 0).unwrap();
        let mut cache = Cache::default();
        let mut metrics = Metrics::default();
        cache.update(&tree, None, &mut metrics);
        check(&tree, &mut cache);
        raw[1] = d(1, "earlier", vec![5, 6]);
        raw.extend([Node::Terminal(2.), Node::Terminal(-2.)]);
        let (next, delta) = compiler.growing(Some(tree), &raw, 0).unwrap();
        tree = next;
        cache.update(&tree, Some(&delta), &mut metrics);
        check(&tree, &mut cache);
        raw[5] = d(0, "child", vec![7, 8]);
        raw.extend([Node::Terminal(1e110), Node::Terminal(-1e110)]);
        let (next, delta) = compiler.growing(Some(tree), &raw, 0).unwrap();
        tree = next;
        cache.update(&tree, Some(&delta), &mut metrics);
        check(&tree, &mut cache);
        raw[7] = Node::Terminal(0.);
        raw[8] = Node::Terminal(-0.);
        let (next, delta) = compiler.growing(Some(tree), &raw, 0).unwrap();
        tree = next;
        cache.update(&tree, Some(&delta), &mut metrics);
        check(&tree, &mut cache);
        assert_eq!(metrics.sequence_full_builds, 1);
        assert_eq!(metrics.sequence_delta_updates, 3);
        assert_eq!(metrics.sequence_scale_refreshes, 3);
    }
    #[test]
    fn removal_cancels_whole_old_bucket_and_reuses_terminal_slots() {
        let mut raw = vec![
            Node::Chance(vec![(0., 1), (1., 2)]),
            Node::Terminal(1e200),
            Node::Terminal(1.),
        ];
        let mut compiler = compiler::owned::Cache::default();
        let (mut tree, _) = compiler.growing(None, &raw, 0).unwrap();
        let mut c = Cache::default();
        let mut m = Metrics::default();
        c.update(&tree, None, &mut m);
        for round in 0..8 {
            raw[2] = Node::Terminal(round as f64 - 4.);
            let (next, d) = compiler.growing(Some(tree), &raw, 0).unwrap();
            tree = next;
            c.update(&tree, Some(&d), &mut m);
            check(&tree, &mut c);
        }
        assert_eq!(c.buckets.iter().map(|b| b.members.len()).sum::<usize>(), 2);
        assert_eq!(m.sequence_full_builds, 1);
    }
    #[test]
    fn positive_chance_underflow_discards_cache_and_uses_scalar_path() {
        let t = Tree::new(
            vec![
                Node::Chance(vec![(1e-200, 1), (1., 2)]),
                Node::Chance(vec![(1e-200, 3), (1., 4)]),
                Node::Terminal(0.),
                Node::Terminal(1.),
                Node::Terminal(-1.),
            ],
            0,
        )
        .unwrap();
        let mut c = Cache::default();
        let mut m = Metrics::default();
        c.update(&t, None, &mut m);
        assert!(c.kernel.is_none());
        assert_eq!(m.sequence_fallbacks, 1);
    }
}
