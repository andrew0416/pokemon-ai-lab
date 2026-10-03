//! Private growing-builder cache: old nonterminal nodes are immutable; only old
//! terminal leaves may be replaced, and all other nodes are appended. It is NOT a
//! general mutable-tree compiler. External IDs are rebuilt in raw-index order;
//! full DFS/recall/reachability validation still runs on every snapshot.
use super::*;

#[derive(Default)]
pub(crate) struct Cache {
    nodes: Vec<Compiled>,
    terminals: Vec<usize>,
    information: Vec<Information>,
    ids: HashMap<String, usize>,
    #[cfg(test)]
    pub parsed: usize,
}
impl Cache {
    pub(crate) fn growing(&mut self, raw: &[Node], root: usize) -> Result<Tree, Error> {
        // On a validation error the reference determines the exact original error
        // order. The caller discards this cache when compilation fails.
        self.update(raw, root)
            .or_else(|_| super::compile(raw, root))
    }
    fn parse(&mut self, node: &Node) -> Result<Compiled, Error> {
        #[cfg(test)]
        {
            self.parsed += 1;
        }
        Ok(match node {
            Node::Terminal(v) => {
                if !v.is_finite() || v.abs() > f64::MAX / 8. {
                    return Err(Error("invalid terminal value".into()));
                }
                Compiled::Terminal(*v)
            }
            Node::Chance(edges) => {
                if edges.is_empty()
                    || edges
                        .iter()
                        .any(|(p, _)| !p.is_finite() || *p < 0. || *p > 1.)
                    || (edges.iter().map(|e| e.0).sum::<f64>() - 1.).abs() > 1e-10
                {
                    return Err(Error("chance probabilities must sum to one".into()));
                }
                Compiled::Chance(edges.clone())
            }
            Node::Decision {
                player,
                information: key,
                actions,
                children,
            } => {
                if *player > 1 || key.is_empty() || actions.len() != children.len() {
                    return Err(Error("invalid information/menu".into()));
                }
                labels(actions.iter().map(String::as_str), "action ID")?;
                let info = if let Some(&i) = self.ids.get(key) {
                    let old = &self.information[i];
                    if old.player != *player || old.actions != *actions {
                        return Err(Error(format!("inconsistent information set: {key}")));
                    }
                    i
                } else {
                    let i = self.information.len();
                    self.ids.insert(key.clone(), i);
                    self.information.push(Information {
                        player: *player,
                        key: key.clone(),
                        actions: actions.clone(),
                        nodes: Vec::new(),
                        own_sequence: Vec::new(),
                    });
                    i
                };
                Compiled::Decision {
                    info,
                    children: children.clone(),
                }
            }
        })
    }
    fn update(&mut self, raw: &[Node], root: usize) -> Result<Tree, Error> {
        if raw.is_empty() || root >= raw.len() || raw.len() < self.nodes.len() {
            return Err(Error("invalid incremental growth".into()));
        }
        let old_len = self.nodes.len();
        let old_terminals = std::mem::take(&mut self.terminals);
        for n in old_terminals {
            match (&self.nodes[n], &raw[n]) {
                (Compiled::Terminal(a), Node::Terminal(b)) if a.to_bits() == b.to_bits() => {
                    self.terminals.push(n)
                }
                _ => {
                    self.nodes[n] = self.parse(&raw[n])?;
                    if matches!(self.nodes[n], Compiled::Terminal(_)) {
                        self.terminals.push(n);
                    }
                }
            }
        }
        for (n, node) in raw.iter().enumerate().skip(old_len) {
            let compiled = self.parse(node)?;
            if matches!(compiled, Compiled::Terminal(_)) {
                self.terminals.push(n);
            }
            self.nodes.push(compiled);
        }
        // Scale may DECREASE when an extreme heuristic leaf is expanded away.
        let mut scale = 0f64;
        let mut ids = vec![usize::MAX; self.information.len()];
        let mut information = Vec::new();
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            nodes.push(match node {
                Compiled::Terminal(v) => {
                    scale = scale.max(v.abs());
                    Compiled::Terminal(*v)
                }
                Compiled::Chance(e) => Compiled::Chance(e.clone()),
                Compiled::Decision { info, children } => {
                    if ids[*info] == usize::MAX {
                        ids[*info] = information.len();
                        information.push(self.information[*info].clone());
                    }
                    Compiled::Decision {
                        info: ids[*info],
                        children: children.clone(),
                    }
                }
            });
        }
        super::finish(nodes, root, information, scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decision(key: &str, children: Vec<usize>) -> Node {
        Node::Decision {
            player: 0,
            information: key.into(),
            actions: (0..children.len()).map(|i| i.to_string()).collect(),
            children,
        }
    }
    fn compare(c: &mut Cache, raw: &[Node]) {
        assert_eq!(
            format!("{:?}", super::super::compile(raw, 0)),
            format!("{:?}", c.growing(raw, 0))
        );
    }
    #[test]
    fn old_leaf_inserts_canonical_ids_and_scale_can_shrink() {
        let mut raw = vec![
            Node::Chance(vec![(0.5, 1), (0.5, 2)]),
            Node::Terminal(100.),
            decision("later", vec![3, 4]),
            Node::Terminal(1.),
            Node::Terminal(-1.),
        ];
        let mut c = Cache::default();
        compare(&mut c, &raw);
        assert_eq!(c.parsed, 5);
        raw[1] = decision("earlier", vec![5, 6]);
        raw.extend([Node::Terminal(2.), Node::Terminal(-2.)]);
        compare(&mut c, &raw);
        assert_eq!(c.parsed, 8);
        let t = c.growing(&raw, 0).unwrap();
        assert_eq!(t.scale, 2.);
        assert_eq!(t.information[0].key, "earlier");
        compare(&mut c, &raw);
        assert_eq!(c.parsed, 8);
        raw[5] = decision("child", vec![7, 8]);
        raw.extend([Node::Terminal(0.), Node::Terminal(-0.)]);
        compare(&mut c, &raw);
        assert_eq!(c.parsed, 11);
    }
    #[test]
    fn full_validation_still_rejects_new_recall_cycles_and_ordered_errors() {
        for mode in 0..5 {
            let mut c = Cache::default();
            let mut raw = vec![
                decision("root", vec![1, 2]),
                Node::Terminal(0.),
                Node::Terminal(0.),
            ];
            compare(&mut c, &raw);
            raw[1] = decision(if mode == 0 { "root" } else { "new" }, vec![3, 4]);
            raw.extend([Node::Terminal(1.), Node::Terminal(2.)]);
            if mode == 1 {
                raw[2] = decision("new", vec![5, 6]);
                raw.extend([Node::Terminal(1.), Node::Terminal(2.)]);
            }
            if mode == 2 {
                raw[1] = Node::Chance(vec![(1., 1)]);
            }
            if mode == 3 {
                raw[3] = Node::Terminal(f64::NAN);
            }
            if mode == 4 {
                raw[1] = decision("root", vec![3]);
                raw[2] = Node::Chance(vec![]);
            }
            assert!(super::super::compile(&raw, 0).is_err());
            compare(&mut c, &raw);
        }
    }
}
