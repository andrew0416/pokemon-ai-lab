//! Policy mapping by information identity, action labels and semantic perfect recall.
//! This is an independent certificate gate, not a transfer of old regret statistics.
use super::*;
type Recall = Vec<(usize, String, String)>;
#[derive(Clone, Debug)]
struct Entry {
    actions: Vec<String>,
    policy: Vec<f64>,
    recall: Recall,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    entries: std::collections::HashMap<(usize, String), Entry>,
}
fn recall(tree: &Tree, info: &Information) -> Recall {
    info.own_sequence
        .iter()
        .map(|&(i, a)| {
            let previous = &tree.information[i];
            (
                previous.player,
                previous.key.clone(),
                previous.actions[a].clone(),
            )
        })
        .collect()
}
impl Snapshot {
    pub fn capture(tree: &Tree, policy: &Policy) -> Result<Self, Error> {
        tree.check_policy(policy)?;
        let entries = tree
            .information
            .iter()
            .zip(policy)
            .map(|(info, p)| {
                (
                    (info.player, info.key.clone()),
                    Entry {
                        actions: info.actions.clone(),
                        policy: p.clone(),
                        recall: recall(tree, info),
                    },
                )
            })
            .collect();
        Ok(Self { entries })
    }
    pub fn project(&self, tree: &Tree) -> Result<(Policy, usize), Error> {
        let mut policy = tree.uniform();
        let mut matched = 0;
        for (i, info) in tree.information.iter().enumerate() {
            let Some(old) = self.entries.get(&(info.player, info.key.clone())) else {
                continue;
            };
            if old.actions.len() != info.actions.len() || old.recall != recall(tree, info) {
                continue;
            }
            let indices: Option<Vec<_>> = info
                .actions
                .iter()
                .map(|a| old.actions.iter().position(|b| a == b))
                .collect();
            if let Some(indices) = indices {
                for (p, j) in policy[i].iter_mut().zip(indices) {
                    *p = old.policy[j];
                }
                matched += 1;
            }
        }
        tree.check_policy(&policy)?;
        Ok((policy, matched))
    }
}
