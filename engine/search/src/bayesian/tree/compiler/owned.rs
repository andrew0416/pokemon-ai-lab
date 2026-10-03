//! Consuming compiler, private to append-only growing builders. Old nonterminal
//! nodes cannot change. Only terminal leaves are replaced. No shared/unsafe storage.
//! Full reachability, depth and perfect-recall validation runs at every publication.
use super::*;

#[derive(Default, Debug)]
pub(crate) struct Delta {
    pub old_nodes: usize,
    pub replaced: Vec<usize>,
    pub old_to_new: Vec<usize>,
    pub rebuilt: bool,
}
#[derive(Default)]
pub(crate) struct Cache {
    terminals: Vec<usize>,
    ids: HashMap<String, usize>,
}
impl Cache {
    fn reset(&mut self, tree: &Tree) {
        self.terminals = tree
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| matches!(n, Compiled::Terminal(_)).then_some(i))
            .collect();
        self.ids = tree
            .information
            .iter()
            .enumerate()
            .map(|(i, v)| (v.key.clone(), i))
            .collect();
    }
    pub(crate) fn growing(
        &mut self,
        previous: Option<Tree>,
        raw: &[Node],
        root: usize,
    ) -> Result<(Tree, Delta), Error> {
        if let Some(previous) = previous {
            if let Ok(result) = self.update(previous, raw, root) {
                return Ok(result);
            }
        }
        // The original compiler owns error ordering. A successful fallback also
        // resets every cache field, so failed speculative parsing cannot leak.
        let tree = super::compile(raw, root)?;
        self.reset(&tree);
        Ok((
            tree,
            Delta {
                rebuilt: true,
                ..Default::default()
            },
        ))
    }
    fn update(
        &mut self,
        previous: Tree,
        raw: &[Node],
        root: usize,
    ) -> Result<(Tree, Delta), Error> {
        if raw.len() < previous.nodes.len() || root != previous.root || raw.is_empty() {
            return Err(Error("invalid owned growth".into()));
        }
        let Tree {
            mut nodes,
            mut information,
            ..
        } = previous;
        let old_info = information.len();
        let mut delta = Delta {
            old_nodes: nodes.len(),
            ..Default::default()
        };
        for n in std::mem::take(&mut self.terminals) {
            if matches!((&nodes[n],&raw[n]),(Compiled::Terminal(a),Node::Terminal(b)) if a.to_bits()==b.to_bits())
            {
                self.terminals.push(n);
            } else {
                delta.replaced.push(n);
                nodes[n] = parse(&raw[n], &mut information, &mut self.ids)?;
                if matches!(nodes[n], Compiled::Terminal(_)) {
                    self.terminals.push(n);
                }
            }
        }
        for (n, raw) in raw.iter().enumerate().skip(delta.old_nodes) {
            let node = parse(raw, &mut information, &mut self.ids)?;
            if matches!(node, Compiled::Terminal(_)) {
                self.terminals.push(n);
            }
            nodes.push(node);
        }
        // Move labels/menus, preserve all existing child-vector allocations, and
        // restore reference numeric information IDs (first raw-node occurrence).
        let mut map = vec![usize::MAX; information.len()];
        let mut source: Vec<_> = information.into_iter().map(Some).collect();
        let mut information = Vec::with_capacity(source.len());
        let mut scale = 0f64;
        for node in &mut nodes {
            match node {
                Compiled::Terminal(v) => scale = scale.max(v.abs()),
                Compiled::Chance(_) => {}
                Compiled::Decision { info, .. } => {
                    if map[*info] == usize::MAX {
                        map[*info] = information.len();
                        let mut item = source[*info].take().unwrap();
                        item.nodes.clear();
                        item.own_sequence.clear();
                        information.push(item);
                    }
                    *info = map[*info];
                }
            }
        }
        for id in self.ids.values_mut() {
            *id = map[*id];
        }
        delta.old_to_new = map[..old_info].to_vec();
        let tree = super::finish(nodes, root, information, scale)?;
        Ok((tree, delta))
    }
}
fn parse(
    node: &Node,
    information: &mut Vec<Information>,
    ids: &mut HashMap<String, usize>,
) -> Result<Compiled, Error> {
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
            let info = if let Some(&i) = ids.get(key) {
                if information[i].player != *player || information[i].actions != *actions {
                    return Err(Error(format!("inconsistent information set: {key}")));
                }
                i
            } else {
                let i = information.len();
                ids.insert(key.clone(), i);
                information.push(Information {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn d(key: &str, children: Vec<usize>) -> Node {
        Node::Decision {
            player: 0,
            information: key.into(),
            actions: (0..children.len()).map(|i| i.to_string()).collect(),
            children,
        }
    }
    #[test]
    fn owned_storage_survives_canonical_reindex_and_scale_shrink() {
        let mut raw = vec![
            Node::Chance(vec![(0.5, 1), (0.5, 2)]),
            Node::Terminal(100.),
            d("later", vec![3, 4]),
            Node::Terminal(1.),
            Node::Terminal(-1.),
        ];
        let mut c = Cache::default();
        let (mut tree, _) = c.growing(None, &raw, 0).unwrap();
        let Compiled::Chance(edges) = &tree.nodes[0] else {
            panic!()
        };
        let ptr = edges.as_ptr();
        raw[1] = d("earlier", vec![5, 6]);
        raw.extend([Node::Terminal(2.), Node::Terminal(-2.)]);
        let (next, delta) = c.growing(Some(tree), &raw, 0).unwrap();
        tree = next;
        assert_eq!(delta.old_to_new, vec![1]);
        assert_eq!(delta.replaced, vec![1]);
        assert!(!delta.rebuilt);
        assert_eq!(
            format!("{tree:?}"),
            format!("{:?}", super::super::compile(&raw, 0).unwrap())
        );
        assert_eq!(tree.scale, 2.);
        let Compiled::Chance(edges) = &tree.nodes[0] else {
            panic!()
        };
        assert_eq!(ptr, edges.as_ptr());
        raw[5] = d("child", vec![7, 8]);
        raw.extend([Node::Terminal(0.), Node::Terminal(-0.)]);
        let (tree, _) = c.growing(Some(tree), &raw, 0).unwrap();
        assert_eq!(
            format!("{tree:?}"),
            format!("{:?}", super::super::compile(&raw, 0).unwrap())
        );
    }
    #[test]
    fn ordered_errors_and_fallback_reset_match_original() {
        for mode in 0..6 {
            let mut raw = vec![
                d("root", vec![1, 2]),
                Node::Terminal(0.),
                Node::Terminal(0.),
            ];
            let mut c = Cache::default();
            let (tree, _) = c.growing(None, &raw, 0).unwrap();
            raw[1] = match mode {
                0 => d("forgot", vec![3]),
                1 => Node::Chance(vec![(1., 1)]),
                2 => Node::Chance(vec![(0.9, 3)]),
                3 => Node::Terminal(f64::NAN),
                4 => d("root", vec![3]),
                _ => Node::Terminal(1.),
            };
            if mode == 0 {
                raw[2] = d("forgot", vec![4]);
                raw.extend([Node::Terminal(1.), Node::Terminal(2.)]);
            }
            if mode == 2 || mode == 4 {
                raw.push(Node::Terminal(0.));
            }
            assert_eq!(
                format!("{:?}", c.growing(Some(tree), &raw, 0).map(|x| x.0)),
                format!("{:?}", super::super::compile(&raw, 0))
            );
        }
    }
}
