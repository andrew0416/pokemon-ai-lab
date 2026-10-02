//! Borrowed raw-tree compilation with interned perfect-recall sequences.
//!
//! Information IDs, DFS order, members and validation rules match `Tree::new`. A raw
//! information key/menu is cloned only when its information set is first encountered.
//! Own histories are persistent integer links while validating; chance siblings share
//! their link IDs. Full own sequences are materialized once per information set.
use super::*;

pub fn compile(raw: &[Node], root: usize) -> Result<Tree, Error> {
    if raw.is_empty() || root >= raw.len() {
        return Err(Error("invalid root".into()));
    }
    let mut nodes = Vec::with_capacity(raw.len());
    let mut information: Vec<Information> = Vec::new();
    let mut ids: HashMap<&str, usize> = HashMap::new();
    let mut scale = 0f64;
    for node in raw {
        nodes.push(match node {
            Node::Terminal(v) => {
                if !v.is_finite() || v.abs() > f64::MAX / 8. {
                    return Err(Error("invalid terminal value".into()));
                }
                scale = scale.max(v.abs());
                Compiled::Terminal(*v)
            }
            Node::Chance(edges) => {
                if edges.is_empty()
                    || edges
                        .iter()
                        .any(|(p, _)| !p.is_finite() || *p < 0. || *p > 1.)
                    || (edges.iter().map(|v| v.0).sum::<f64>() - 1.).abs() > 1e-10
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
                let info = if let Some(&i) = ids.get(key.as_str()) {
                    let existing = &information[i];
                    if existing.player != *player || existing.actions != *actions {
                        return Err(Error(format!("inconsistent information set: {key}")));
                    }
                    i
                } else {
                    let i = information.len();
                    ids.insert(key.as_str(), i);
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
        });
    }
    let mut seen = vec![false; nodes.len()];
    let mut order = Vec::new();
    // Link 0 represents the empty history. Interning (previous, info, action) means
    // equal link IDs iff complete sequences are equal, not merely their last action.
    let mut links = vec![(0usize, 0usize, 0usize)];
    let mut intern = HashMap::new();
    let mut recall = vec![0; information.len()];
    let mut stack = vec![(root, [0usize; 2], 0usize)];
    while let Some((id, sequences, depth)) = stack.pop() {
        if id >= nodes.len() || seen[id] || depth > 512 {
            return Err(Error(
                "tree has invalid/shared/cyclic nodes or exceeds depth 512".into(),
            ));
        }
        seen[id] = true;
        order.push(id);
        match &nodes[id] {
            Compiled::Terminal(_) => {}
            Compiled::Chance(edges) => {
                for &(_, child) in edges.iter().rev() {
                    stack.push((child, sequences, depth + 1));
                }
            }
            Compiled::Decision { info, children } => {
                let i = &mut information[*info];
                let sequence = sequences[i.player];
                if i.nodes.is_empty() {
                    recall[*info] = sequence;
                    let mut link = sequence;
                    while link != 0 {
                        let (previous, info, action) = links[link];
                        i.own_sequence.push((info, action));
                        link = previous;
                    }
                    i.own_sequence.reverse();
                } else if recall[*info] != sequence {
                    return Err(Error(format!("imperfect recall in {}", i.key)));
                }
                i.nodes.push(id);
                for (a, &child) in children.iter().enumerate().rev() {
                    let key = (sequence, *info, a);
                    let next_id = *intern.entry(key).or_insert_with(|| {
                        let id = links.len();
                        links.push(key);
                        id
                    });
                    let mut next = sequences;
                    next[i.player] = next_id;
                    stack.push((child, next, depth + 1));
                }
            }
        }
    }
    if seen.iter().any(|v| !*v) {
        return Err(Error("unreachable tree node".into()));
    }
    Ok(Tree {
        nodes,
        root,
        order,
        information,
        scale: scale.max(f64::MIN_POSITIVE),
        worlds: Vec::new(),
        public_keys: Vec::new(),
        private_keys: Vec::new(),
        boundaries: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compare(raw: Vec<Node>, root: usize) {
        assert_eq!(
            format!("{:?}", Tree::new(raw.clone(), root)),
            format!("{:?}", compile(&raw, root))
        );
    }
    fn decision(player: usize, key: &str, children: Vec<usize>) -> Node {
        Node::Decision {
            player,
            information: key.into(),
            actions: (0..children.len()).map(|a| format!("a{a}")).collect(),
            children,
        }
    }
    #[test]
    fn malformed_and_edge_trees_match_reference_errors() {
        compare(vec![], 0);
        compare(vec![Node::Terminal(0.)], 1);
        for value in [0., -0., 1e-300, f64::NAN, f64::INFINITY, f64::MAX] {
            compare(vec![Node::Terminal(value)], 0);
        }
        for probability in [0., 0.9, 1., -1., f64::NAN] {
            compare(
                vec![Node::Chance(vec![(probability, 1)]), Node::Terminal(0.)],
                0,
            );
        }
        compare(vec![Node::Chance(vec![])], 0);
        compare(vec![Node::Chance(vec![(1., 0)])], 0);
        compare(vec![Node::Chance(vec![(1., 9)])], 0);
        compare(
            vec![Node::Chance(vec![(0.5, 1), (0.5, 1)]), Node::Terminal(0.)],
            0,
        );
        compare(vec![Node::Terminal(0.), Node::Terminal(0.)], 0);
        for player in [0, 1, 2] {
            compare(vec![decision(player, "key", vec![])], 0);
        }
        compare(vec![decision(0, "", vec![1]), Node::Terminal(0.)], 0);
        compare(
            vec![
                decision(0, "root", vec![1, 2]),
                decision(0, "forgot", vec![3]),
                decision(0, "forgot", vec![4]),
                Node::Terminal(0.),
                Node::Terminal(1.),
            ],
            0,
        );
        compare(
            vec![
                Node::Chance(vec![(0.5, 1), (0.5, 2)]),
                decision(0, "same", vec![3]),
                decision(1, "same", vec![4]),
                Node::Terminal(0.),
                Node::Terminal(1.),
            ],
            0,
        );
    }
    #[test]
    fn repeated_histories_and_depth_limit_match_reference() {
        // Many chance histories with one shared root decision and perfect-recall second
        // decisions. Numeric IDs/members/order must match even when nodes are interleaved.
        for worlds in [1, 2, 8, 31] {
            let mut raw = vec![Node::Terminal(0.)];
            let mut edges = Vec::new();
            for w in 0..worlds {
                let n = raw.len();
                edges.push((1. / worlds as f64, n));
                raw.extend([
                    decision(0, "root", vec![n + 1, n + 2]),
                    decision(0, "after-a0", vec![n + 3, n + 4]),
                    decision(0, "after-a1", vec![n + 5, n + 6]),
                    Node::Terminal(w as f64),
                    Node::Terminal(-(w as f64)),
                    Node::Terminal(1.),
                    Node::Terminal(0.),
                ]);
            }
            raw[0] = Node::Chance(edges);
            compare(raw, 0);
        }
        for depth in [1, 32, 512, 513] {
            let mut raw: Vec<_> = (0..depth)
                .map(|n| decision(n % 2, &format!("step{n}"), vec![n + 1]))
                .collect();
            raw.push(Node::Terminal(1.));
            compare(raw, 0);
        }
    }
}
