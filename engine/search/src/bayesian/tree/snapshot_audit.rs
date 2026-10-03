//! Expensive, opt-in shadow checks; never enabled in performance binaries.
use super::*;
use std::cell::Cell;
#[derive(Clone, Copy, Debug, Default)]
pub struct Counters {
    pub compiled_snapshots: usize,
    pub sequence_snapshots: usize,
    pub contractions: usize,
    pub gap_checks: usize,
}
thread_local! { static COUNTERS: Cell<Counters> = Cell::new(Counters::default()); }
pub fn counters() -> Counters {
    COUNTERS.get()
}
pub fn reset() {
    COUNTERS.set(Counters::default());
}
pub(crate) fn sequence(contractions: usize, gap_checks: usize) {
    let mut c = COUNTERS.get();
    c.sequence_snapshots += 1;
    c.contractions += contractions;
    c.gap_checks += gap_checks;
    COUNTERS.set(c);
}
pub(crate) fn append_only(old: &[Node], raw: &[Node]) -> Result<(), Error> {
    if raw.len() < old.len() {
        return Err(Error("audit: append-only node count shrank".into()));
    }
    for (i, (a, b)) in old.iter().zip(raw).enumerate() {
        let same = match (a, b) {
            (Node::Terminal(_), _) => true,
            (Node::Chance(x), Node::Chance(y)) => {
                x.len() == y.len()
                    && x.iter()
                        .zip(y)
                        .all(|((p, a), (q, b))| p.to_bits() == q.to_bits() && a == b)
            }
            (
                Node::Decision {
                    player: a,
                    information: b,
                    actions: c,
                    children: d,
                },
                Node::Decision {
                    player: e,
                    information: f,
                    actions: g,
                    children: h,
                },
            ) => a == e && b == f && c == g && d == h,
            _ => false,
        };
        if !same {
            return Err(Error(format!("audit: old nonterminal changed at {i}")));
        }
    }
    Ok(())
}
pub(crate) fn compiled(raw: &[Node], root: usize, actual: &Tree) -> Result<(), Error> {
    let reference = compiler::compile(raw, root)?;
    if format!("{actual:?}") != format!("{reference:?}") {
        return Err(Error(
            "audit: incremental/full compiled snapshot mismatch".into(),
        ));
    }
    let mut c = COUNTERS.get();
    c.compiled_snapshots += 1;
    COUNTERS.set(c);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract_rejects_mutated_old_decisions_and_chance_but_allows_leaf_replacement() {
        let old = vec![Node::Chance(vec![(1., 1)]), Node::Terminal(0.)];
        let mut changed = old.clone();
        changed[0] = Node::Chance(vec![(0.5, 1), (0.5, 2)]);
        changed.push(Node::Terminal(1.));
        assert!(append_only(&old, &changed).is_err());
        changed = old.clone();
        changed[1] = Node::Decision {
            player: 0,
            information: "new mechanic".into(),
            actions: vec!["A".into()],
            children: vec![2],
        };
        changed.push(Node::Terminal(1.));
        assert!(append_only(&old, &changed).is_ok());
        let older = changed.clone();
        if let Node::Decision { actions, .. } = &mut changed[1] {
            actions[0] = "changed action".into();
        }
        assert!(append_only(&older, &changed).is_err());
    }
    #[test]
    fn generated_intermediate_compilers_match_for_many_seeds_and_scale_changes() {
        reset();
        for seed in 1..=64u64 {
            let mut random = seed;
            let mut next = || {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                random
            };
            let mut raw = vec![
                Node::Chance(vec![(0.5, 1), (0.5, 2)]),
                Node::Terminal(1e50),
                Node::Terminal(-1.),
            ];
            let mut owned = compiler::owned::Cache::default();
            let mut legacy = compiler::incremental::Cache::default();
            let (mut current, _) = owned.growing(None, &raw, 0).unwrap();
            legacy.growing(&raw, 0).unwrap();
            for step in 0..24 {
                let leaves: Vec<_> = raw
                    .iter()
                    .enumerate()
                    .filter_map(|(i, n)| matches!(n, Node::Terminal(_)).then_some(i))
                    .collect();
                let leaf = leaves[(next() as usize) % leaves.len()];
                let start = raw.len();
                let value = (next() % 1000) as f64 / 13. - 30.;
                raw[leaf] = if step % 3 == 0 {
                    let p = match step % 4 {
                        0 => 0.,
                        1 => 1e-150,
                        2 => 0.25,
                        _ => 1.,
                    };
                    Node::Chance(vec![(p, start), (1. - p, start + 1)])
                } else {
                    Node::Decision {
                        player: (next() % 2) as usize,
                        information: format!("mechanic-{seed}-{step}"),
                        actions: vec!["same spec A".into(), "same spec B".into()],
                        children: vec![start, start + 1],
                    }
                };
                raw.extend([Node::Terminal(value), Node::Terminal(-value)]);
                let expected = compiler::compile(&raw, 0);
                let candidate = owned.growing(Some(current), &raw, 0);
                let old = legacy.growing(&raw, 0);
                match expected {
                    Ok(t) => {
                        current = candidate.unwrap().0;
                        assert_eq!(format!("{current:?}"), format!("{t:?}"));
                        assert_eq!(format!("{:?}", old.unwrap()), format!("{t:?}"));
                    }
                    Err(e) => {
                        assert_eq!(candidate.err().unwrap().0, e.0);
                        assert_eq!(old.err().unwrap().0, e.0);
                        break;
                    }
                }
            }
        }
        assert!(counters().compiled_snapshots > 2000);
    }
}
