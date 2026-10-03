//! Reuse only the FINAL AVERAGE policy's assessment passes. Working CFR policies
//! are never substituted. BR still chooses one action per information set.
use super::*;
pub(crate) struct Passes {
    pub values: Vec<f64>,
    pub reach: [Vec<f64>; 2],
}
pub(crate) fn assess(tree: &Tree, policy: &Policy) -> Result<(Assessment, Passes), Error> {
    tree.check_policy(policy)?;
    let values = tree.values(policy);
    let value = values[tree.root] * tree.scale;
    let (upper, _, first) = tree.response_with_reach(policy, 0)?;
    let (lower, _, second) = tree.response_with_reach(policy, 1)?;
    Ok((
        Assessment {
            value,
            lower,
            upper,
            gap: (upper - lower).max(0.),
        },
        Passes {
            values,
            reach: [first, second],
        },
    ))
}
pub(crate) fn solve(tree: &Tree, config: Config) -> Result<(Solution, Passes), Error> {
    workspace::cached::solve_with(tree, config, |p| assess(tree, p))
}
