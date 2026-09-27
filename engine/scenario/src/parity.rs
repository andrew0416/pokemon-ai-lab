//! Parity with the Showdown oracle: an engine decision's outcome distribution over canonical
//! states, an `enumerate.cjs` report's, and their comparison. The fixture tests
//! (`tests/common/mod.rs`) and `lab-check` (the parity sweep, FF-parity-harness) share this
//! comparator.
//!
//! Canonical states are compared as JSON values, keyed by `serde_json`'s serialization of the
//! parsed state (object keys sorted), so the oracle's and the engine's key order never matter.

use std::collections::HashMap;

use serde_json::Value;

use lab_engine::instruction::Outcome;
use lab_engine::Doubles;

use crate::{canonical_value, ScenarioMeta};

/// Canonical state key → probability.
pub type Distribution = HashMap<String, f64>;

/// A canonical state as an order-independent key (serde_json sorts object keys).
pub fn value_key(state: &Value) -> String {
    serde_json::to_string(state).expect("serializable")
}

/// The engine's outcomes (instructions from `state`, which is left unchanged) as canonical
/// state key → probability; engine states that differ only in what the canonical form leaves
/// out merge.
pub fn engine_distribution(
    meta: &ScenarioMeta,
    state: &mut Doubles,
    outcomes: &[Outcome],
) -> Result<Distribution, String> {
    let mut out = Distribution::new();
    for outcome in outcomes {
        state.apply(&outcome.instructions);
        let value = canonical_value(state, meta);
        state.reverse(&outcome.instructions);
        let value = value.map_err(|e| e.to_string())?;
        *out.entry(value_key(&value)).or_insert(0.0) += outcome.probability;
    }
    Ok(out)
}

/// An oracle report's (or fixture's) `outcomes` as canonical state key → probability.
pub fn report_distribution(report: &Value) -> Result<Distribution, String> {
    let outcomes = report["outcomes"]
        .as_array()
        .ok_or("the report has no outcomes array")?;
    let mut out = Distribution::new();
    for o in outcomes {
        let p = o["p"].as_f64().ok_or("an outcome without p")?;
        *out.entry(value_key(&o["state"])).or_insert(0.0) += p;
    }
    Ok(out)
}

/// How two distributions over canonical states differ.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comparison {
    pub engine_outcomes: usize,
    pub oracle_outcomes: usize,
    /// Keys only the engine produced, most probable first.
    pub only_engine: Vec<String>,
    /// Keys only the oracle produced, most probable first.
    pub only_oracle: Vec<String>,
    /// The largest probability difference over shared keys.
    pub max_shared_diff: f64,
    /// Total variation distance.
    pub tv: f64,
}

impl Comparison {
    /// Same outcomes, probabilities within `tolerance`.
    pub fn exact(&self, tolerance: f64) -> bool {
        self.only_engine.is_empty()
            && self.only_oracle.is_empty()
            && self.max_shared_diff <= tolerance
    }
}

/// Compares the engine's distribution with the oracle's.
pub fn compare(engine: &Distribution, oracle: &Distribution) -> Comparison {
    let mut only_engine: Vec<(&String, f64)> = Vec::new();
    let mut only_oracle: Vec<(&String, f64)> = Vec::new();
    let mut max_shared_diff = 0.0f64;
    let mut tv = 0.0;
    for (k, &p) in engine {
        match oracle.get(k) {
            Some(&q) => {
                max_shared_diff = max_shared_diff.max((p - q).abs());
                tv += (p - q).abs() / 2.0;
            }
            None => {
                only_engine.push((k, p));
                tv += p / 2.0;
            }
        }
    }
    for (k, &q) in oracle {
        if !engine.contains_key(k) {
            only_oracle.push((k, q));
            tv += q / 2.0;
        }
    }
    let by_probability = |v: &mut Vec<(&String, f64)>| {
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    };
    by_probability(&mut only_engine);
    by_probability(&mut only_oracle);
    Comparison {
        engine_outcomes: engine.len(),
        oracle_outcomes: oracle.len(),
        only_engine: only_engine.into_iter().map(|(k, _)| k.clone()).collect(),
        only_oracle: only_oracle.into_iter().map(|(k, _)| k.clone()).collect(),
        max_shared_diff,
        tv,
    }
}

/// Up to `limit` paths at which `a` and `b` differ, as `path: a vs b` (a missing value is
/// `-`). Arrays of objects with a `name` (a side's Pokémon) are matched by name.
pub fn json_diff(a: &Value, b: &Value, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    diff_at(String::new(), a, b, limit, &mut out);
    out
}

fn short(v: Option<&Value>) -> String {
    match v {
        None => "-".to_owned(),
        Some(v) => {
            let text = v.to_string();
            if text.len() > 80 {
                format!("{}…", &text[..text.floor_char_boundary(80)])
            } else {
                text
            }
        }
    }
}

fn diff_at(path: String, a: &Value, b: &Value, limit: usize, out: &mut Vec<String>) {
    if out.len() >= limit || a == b {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) => diff_at(p, u, v, limit, out),
                    (u, v) => {
                        if out.len() < limit {
                            out.push(format!("{p}: {} vs {}", short(u), short(v)));
                        }
                    }
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            let named = |v: &Vec<Value>| {
                v.iter()
                    .all(|e| e.get("name").is_some_and(Value::is_string))
            };
            if named(x) && named(y) && !x.is_empty() {
                let mut names: Vec<&str> = x
                    .iter()
                    .chain(y.iter())
                    .map(|e| e["name"].as_str().unwrap())
                    .collect();
                names.sort();
                names.dedup();
                for name in names {
                    let u = x.iter().find(|e| e["name"] == name);
                    let v = y.iter().find(|e| e["name"] == name);
                    let p = format!("{path}[{name}]");
                    match (u, v) {
                        (Some(u), Some(v)) => diff_at(p, u, v, limit, out),
                        (u, v) => {
                            if out.len() < limit {
                                out.push(format!("{p}: {} vs {}", short(u), short(v)));
                            }
                        }
                    }
                }
            } else {
                for i in 0..x.len().max(y.len()) {
                    let p = format!("{path}[{i}]");
                    match (x.get(i), y.get(i)) {
                        (Some(u), Some(v)) => diff_at(p, u, v, limit, out),
                        (u, v) => {
                            if out.len() < limit {
                                out.push(format!("{p}: {} vs {}", short(u), short(v)));
                            }
                        }
                    }
                }
            }
        }
        _ => out.push(format!("{path}: {} vs {}", short(Some(a)), short(Some(b)))),
    }
}

/// The canonical fields in which the most probable outcome only the oracle produced differs
/// from the closest outcome of the engine (and the other way round when the engine has an
/// extra outcome but the oracle none), as `path: oracle vs engine`. Empty when both have the
/// same outcomes.
pub fn first_differences(
    comparison: &Comparison,
    engine: &Distribution,
    oracle: &Distribution,
    limit: usize,
) -> Vec<String> {
    let parse = |k: &str| -> Value { serde_json::from_str(k).expect("keys are JSON") };
    let closest = |target: &Value, pool: &Distribution| -> Vec<String> {
        let mut best: Option<Vec<String>> = None;
        for k in pool.keys() {
            let d = json_diff(target, &parse(k), 64);
            if best.as_ref().is_none_or(|b| d.len() < b.len()) {
                best = Some(d);
            }
        }
        let mut best = best.unwrap_or_default();
        best.truncate(limit);
        best
    };
    if let Some(k) = comparison.only_oracle.first() {
        let mut out = closest(&parse(k), engine);
        for d in &mut out {
            d.insert_str(0, "oracle→engine ");
        }
        return out;
    }
    if let Some(k) = comparison.only_engine.first() {
        let mut out = closest(&parse(k), oracle);
        for d in &mut out {
            d.insert_str(0, "engine→oracle ");
        }
        return out;
    }
    Vec::new()
}
