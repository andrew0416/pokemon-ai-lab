//! Strict opt-in flags. Unknown or incompatible requests fail before domain work.
use lab_search::bayesian::tree::pipeline::{Metrics, Settings};
use serde_json::{json, Value};
pub fn parse(v: &Value) -> Result<Settings, String> {
    let object = v.as_object().ok_or("pipeline must be object")?;
    for key in object.keys() {
        if ![
            "frontier_index",
            "owned_compiler",
            "incremental_sequence",
            "compressed_checks",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unknown pipeline field {key}"));
        }
    }
    let flag = |key: &str| {
        v.get(key).map_or(Ok(false), |v| {
            v.as_bool().ok_or_else(|| format!("invalid pipeline {key}"))
        })
    };
    Ok(Settings {
        frontier_index: flag("frontier_index")?,
        owned_compiler: flag("owned_compiler")?,
        incremental_sequence: flag("incremental_sequence")?,
        compressed_checks: flag("compressed_checks")?,
    })
}
pub fn metrics(m: &Metrics) -> Value {
    json!({"owned_compiles":m.owned_compiles,"sequence_full_builds":m.sequence_full_builds,"sequence_delta_updates":m.sequence_delta_updates,"sequence_leaf_visits":m.sequence_leaf_visits,"sequence_scale_refreshes":m.sequence_scale_refreshes,"sequence_fallbacks":m.sequence_fallbacks,"compressed_checks":m.compressed_checks,"compressed_rejections":m.compressed_rejections})
}
pub fn request(v: &Value) -> Result<Option<Settings>, String> {
    let Some(p) = v["solver"].get("pipeline") else {
        return Ok(None);
    };
    let p = parse(p)?;
    let solver = super::paper_options::parse(
        v["solver"]
            .get("paper")
            .ok_or("pipeline requires explicit paper settings")?,
    )?;
    lab_search::bayesian::tree::pipeline::validate(p, solver).map_err(|e| e.to_string())?;
    if (p.frontier_index || p.owned_compiler || p.incremental_sequence) && v.get("growth").is_none()
    {
        return Err("growth pipeline flags require a growing request".into());
    }
    Ok(Some(p))
}
