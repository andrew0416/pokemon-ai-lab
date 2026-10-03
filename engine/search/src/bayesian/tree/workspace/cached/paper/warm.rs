//! Strategy-based warm starting (Brown/Sandholm AAAI 2016, corrected appendix).
//! Eq. 14 substitute values; Eq. 16 local squared-regret bounds and the root-sum
//! condition are checked in the CURRENT scaled game. Bisection moves lambda
//! toward the root-sum boundary; virtual T is explicit (not empirically fitted).
use super::*;
pub(super) struct Initial {
    pub regrets: Policy,
    pub sums: Policy,
    pub total: Vec<f64>,
    pub root_sum: f64,
}

pub(super) fn average(
    tree: &Tree,
    policy: &Policy,
    sums: &mut Policy,
    total: &mut [f64],
    weight: f64,
) -> Result<(), Error> {
    for (i, info) in tree.information.iter().enumerate() {
        let mut reach = 1.;
        for &(j, a) in &info.own_sequence {
            reach = multiply(reach, policy[j][a])?;
        }
        let w = multiply(weight, reach)?;
        total[i] += w;
        for (s, p) in sums[i].iter_mut().zip(&policy[i]) {
            *s += w * p;
        }
    }
    Ok(())
}

struct Work<'a> {
    tree: &'a Tree,
    policy: &'a Policy,
    player: usize,
    cf: &'a [f64],
    values: Vec<Option<f64>>,
    information: Vec<Option<f64>>,
    ranges: &'a [(f64, f64)],
    regrets: &'a mut Policy,
    t: f64,
    lambda: f64,
    valid: bool,
}
impl Work<'_> {
    fn value(&mut self, node: usize) -> f64 {
        if let Some(v) = self.values[node] {
            return v;
        }
        let v = match &self.tree.nodes[node] {
            Compiled::Terminal(v) => (if self.player == 0 { *v } else { -*v }) / self.tree.scale,
            Compiled::Chance(edges) => edges.iter().map(|&(p, n)| p * self.value(n)).sum(),
            Compiled::Decision { info, children } => {
                if self.tree.information[*info].player == self.player {
                    self.information_value(*info)
                } else {
                    children
                        .iter()
                        .enumerate()
                        .map(|(a, &n)| self.policy[*info][a] * self.value(n))
                        .sum()
                }
            }
        };
        self.valid &= v.is_finite();
        self.values[node] = Some(v);
        v
    }
    fn information_value(&mut self, i: usize) -> f64 {
        if let Some(v) = self.information[i] {
            return v;
        }
        let info = &self.tree.information[i];
        let mass: f64 = info.nodes.iter().map(|&n| self.cf[n]).sum();
        let mut actions = vec![0.; info.actions.len()];
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &n in &info.nodes {
            lo = lo.min(self.ranges[n].0);
            hi = hi.max(self.ranges[n].1);
            let Compiled::Decision { children, .. } = &self.tree.nodes[n] else {
                unreachable!()
            };
            for (a, &child) in children.iter().enumerate() {
                actions[a] += self.cf[n] * self.value(child);
            }
        }
        if mass == 0. {
            self.information[i] = Some(0.);
            return 0.;
        }
        let bound = self.lambda * mass * (hi - lo).powi(2) * actions.len() as f64 / self.t;
        let maximum = actions.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut left = maximum - bound.sqrt();
        let mut right = maximum;
        // Right endpoint always satisfies Eq. 16. Keep that endpoint on rounding.
        for _ in 0..80 {
            let mid = left + (right - left) / 2.;
            let square: f64 = actions.iter().map(|a| (a - mid).max(0.).powi(2)).sum();
            if square <= bound {
                right = mid;
            } else {
                left = mid;
            }
        }
        let square: f64 = actions.iter().map(|a| (a - right).max(0.).powi(2)).sum();
        self.valid &= bound.is_finite() && square <= bound;
        for (r, a) in self.regrets[i].iter_mut().zip(actions) {
            *r = self.t * (a - right);
            self.valid &= r.is_finite();
        }
        let conditional = right / mass;
        self.valid &= conditional.is_finite();
        self.information[i] = Some(conditional);
        conditional
    }
}

pub(super) fn initialize(tree: &Tree, seed: &Policy, t: usize) -> Result<Option<Initial>, Error> {
    tree.check_policy(seed)?;
    let zero: Policy = seed.iter().map(|p| vec![0.; p.len()]).collect();
    let mut ranges = vec![(0f64, 0f64); tree.nodes.len()];
    for &n in tree.order.iter().rev() {
        ranges[n] = match &tree.nodes[n] {
            Compiled::Terminal(v) => (v / tree.scale, v / tree.scale),
            Compiled::Chance(edges) => edges
                .iter()
                .map(|&(_, c)| ranges[c])
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
                    (lo.min(a), hi.max(b))
                }),
            Compiled::Decision { children, .. } => children
                .iter()
                .map(|&c| ranges[c])
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| {
                    (lo.min(a), hi.max(b))
                }),
        };
    }
    let reaches = [tree.reach(seed, Some(0))?, tree.reach(seed, Some(1))?];
    let attempt = |lambda| -> Option<(Policy, f64)> {
        let mut regrets = zero.clone();
        let mut root_sum = 0.;
        for (player, cf) in reaches.iter().enumerate() {
            let mut work = Work {
                tree,
                policy: seed,
                player,
                cf,
                values: vec![None; tree.nodes.len()],
                information: vec![None; seed.len()],
                ranges: &ranges,
                regrets: &mut regrets,
                t: t as f64,
                lambda,
                valid: true,
            };
            root_sum += work.value(tree.root);
            if !work.valid {
                return None;
            }
        }
        // No epsilon allowance for the theorem's joint root condition.
        (root_sum.is_finite() && root_sum <= 0.).then_some((regrets, root_sum))
    };
    let Some(mut accepted) = attempt(1.) else {
        return Ok(None);
    };
    let mut low = 0.;
    let mut high = 1.;
    for _ in 0..16 {
        let mid = (low + high) / 2.;
        if let Some(next) = attempt(mid) {
            high = mid;
            accepted = next;
        } else {
            low = mid;
        }
    }
    let (regrets, root_sum) = accepted;
    let mut sums = zero;
    let mut total = vec![0.; seed.len()];
    average(tree, seed, &mut sums, &mut total, t as f64)?;
    Ok(Some(Initial {
        regrets,
        sums,
        total,
        root_sum,
    }))
}
