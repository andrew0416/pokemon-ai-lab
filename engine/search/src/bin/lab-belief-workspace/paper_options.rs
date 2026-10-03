//! Strict experimental options shared by correctness and speed CLIs.
use lab_search::bayesian::tree::{
    builder::growing::reuse::paper::PaperStats,
    paper::{Checks, Rule, Settings, Stats},
};
use serde_json::{json, Value};
pub fn parse(v: &Value) -> Result<Settings, String> {
    let object = v.as_object().ok_or("paper settings must be object")?;
    for key in object.keys() {
        if ![
            "rule",
            "checks",
            "compact",
            "sequence",
            "reuse_policy",
            "warm_iterations",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unknown paper field {key}"));
        }
    }
    let string = |key: &str, default: &str| -> Result<String, String> {
        v.get(key).map_or(Ok(default.into()), |s| {
            s.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("invalid {key}"))
        })
    };
    let flag = |key: &str| -> Result<bool, String> {
        v.get(key).map_or(Ok(false), |x| {
            x.as_bool().ok_or_else(|| format!("invalid {key}"))
        })
    };
    Ok(Settings {
        rule: Rule::parse(&string("rule", "lcfr")?).map_err(|e| e.to_string())?,
        checks: match string("checks", "periodic")?.as_str() {
            "periodic" => Checks::Periodic,
            "geometric" => Checks::Geometric,
            _ => return Err("invalid checks".into()),
        },
        compact: flag("compact")?,
        sequence: flag("sequence")?,
        reuse_policy: flag("reuse_policy")?,
        warm_iterations: v.get("warm_iterations").map_or(Ok(0), |x| {
            x.as_u64()
                .filter(|&x| x <= 1_000_000)
                .map(|x| x as usize)
                .ok_or_else(|| "invalid warm_iterations".to_string())
        })?,
    })
}
pub fn stats(s: &Stats) -> Value {
    json!({"assessments":s.assessments,"compact":s.compact,"matrix_entries":s.matrix_entries,"sequence":s.sequence,"sequence_entries":s.sequence_entries,"scalar_reach_fallbacks":s.scalar_reach_fallbacks,"mapped_information":s.mapped_information,"reuse_attempted":s.reuse_attempted,"reused":s.reused,"warm_attempted":s.warm_attempted,"warm_applied":s.warm_applied,"virtual_iterations":s.virtual_iterations,"substitute_root_sum":s.substitute_root_sum})
}
pub fn growth(s: &PaperStats) -> Value {
    json!({"requests":s.requests,"reused":s.reused,"compact":s.compact,"sequence":s.sequence,"assessments":s.assessments,"mapped_information":s.mapped_information,"scalar_reach_fallbacks":s.scalar_reach_fallbacks,"warm_attempted":s.warm_attempted,"warm_applied":s.warm_applied,"virtual_iterations":s.virtual_iterations})
}
