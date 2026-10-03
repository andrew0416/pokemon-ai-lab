//! Explicit observed transition tables for integration/oracle fixtures. State IDs are
//! simulator references only; only supplied observations enter the information key.
use super::*;
use lab_search::budgeted::{Domain, Phase};
use tree::builder::{self, growing, Built, Observation, ObservedDomain, Seed};

struct State {
    phase: Phase,
    value: f32,
    observed: Observation,
    actions: [Vec<String>; 2],
    transitions: Vec<Vec<Vec<(f64, usize)>>>,
}
struct Table(Vec<State>);
impl Domain for Table {
    type Position = usize;
    type Action = usize;
    fn phase(&self, p: &usize) -> Result<Phase, String> {
        Ok(self.0[*p].phase)
    }
    fn value(&self, p: &usize) -> f32 {
        self.0[*p].value
    }
    fn actions(&self, p: &usize, player: usize) -> Result<Vec<usize>, String> {
        Ok((0..self.0[*p].actions[player].len()).collect())
    }
    fn transitions(&self, p: &usize, a: [&usize; 2]) -> Result<Vec<(f64, usize)>, String> {
        Ok(self.0[*p].transitions[*a[0]][*a[1]].clone())
    }
}
impl ObservedDomain for Table {
    fn observation(&self, p: &usize) -> Result<Observation, String> {
        Ok(self.0[*p].observed.clone())
    }
    fn action_id(&self, p: &usize, player: usize, a: &usize) -> String {
        self.0[*p].actions[player][*a].clone()
    }
}

pub fn growth_config(v: &Value, solver: Config) -> Result<Option<growing::Config>, String> {
    let Some(c) = v.get("growth") else {
        return Ok(None);
    };
    #[cfg(not(feature = "experiment-growth-cadence"))]
    keys(c, &["max_expansions", "max_walks", "exploration", "seed"])?;
    #[cfg(feature = "experiment-growth-cadence")]
    keys(
        c,
        &[
            "max_expansions",
            "max_walks",
            "exploration",
            "seed",
            "cadence",
        ],
    )?;
    let mut g = growing::Config {
        solver,
        ..growing::Config::default()
    };
    g.max_expansions = integer(c, "max_expansions", g.max_expansions)?;
    g.max_walks = integer(c, "max_walks", g.max_walks)?;
    if let Some(x) = c.get("seed") {
        g.seed = x.as_u64().ok_or("seed must be u64")?;
    }
    if c.get("exploration").is_some() {
        g.exploration = number(c, "exploration")?;
    }
    Ok(Some(g))
}
pub fn metadata(r: &growing::ResultTree) -> Value {
    json!({"stop":format!("{:?}",r.stop),"horizon_complete":r.horizon_complete,
        "frontier_histories":r.frontier_histories,"frontier_public_groups":r.frontier_public_groups,
        "attempted_transitions":r.work.attempted_transitions,"committed_transitions":r.built.stats.transitions,
        "attempted_expansions":r.work.attempted_expansions,"committed_expansions":r.work.committed_expansions,
        "walks":r.work.walks,"solves":r.work.solves,"total_cfr_iterations":r.work.cfr_iterations,
        "selection":"half-puct-half-average-policy","prior":"uniform-information-menu",
        "regrets":"reset-after-growth","switch_closure":true,"performance_measured":false})
}
pub fn request(
    v: &Value,
    config: Config,
) -> Result<(Built, Option<tree::Solution>, Value), String> {
    keys(
        v,
        &[
            "mode",
            "states",
            "worlds",
            "limits",
            "growth",
            "solver",
            "include_keys",
        ],
    )?;
    let mut limits = Limits::default();
    if let Some(l) = v.get("limits") {
        keys(
            l,
            &["turns", "max_nodes", "max_transitions", "max_decisions"],
        )?;
        limits.turns = u32::try_from(integer(l, "turns", limits.turns as usize)?)
            .map_err(|_| "turns overflow")?;
        limits.max_nodes = integer(l, "max_nodes", limits.max_nodes)?;
        limits.max_transitions = integer(l, "max_transitions", limits.max_transitions)?;
        limits.max_decisions = integer(l, "max_decisions", limits.max_decisions)?;
    }
    let raw = array(&v["states"])?;
    let mut states = Vec::new();
    for s in raw {
        keys(
            s,
            &[
                "phase",
                "value",
                "public",
                "private",
                "actions",
                "transitions",
            ],
        )?;
        let phase = match string(s, "phase")?.as_str() {
            "turn" => Phase::Turn,
            "switch" => Phase::Switch,
            "terminal" => Phase::Terminal,
            _ => return Err("invalid phase".into()),
        };
        let value = number(s, "value")? as f32;
        if !value.is_finite() {
            return Err("invalid table utility".into());
        }
        let private = strings(&s["private"])?;
        if private.len() != 2 {
            return Err("private needs two owner observations".into());
        }
        let observed = Observation {
            public: string(s, "public")?,
            private: [private[0].clone(), private[1].clone()],
        };
        let mut actions = [Vec::new(), Vec::new()];
        let mut transitions = Vec::new();
        if phase != Phase::Terminal {
            let a = array(&s["actions"])?;
            if a.len() != 2 {
                return Err("actions needs two menus".into());
            }
            actions = [strings(&a[0])?, strings(&a[1])?];
            for row in array(&s["transitions"])? {
                let mut columns = Vec::new();
                for cell in array(row)? {
                    let mut edges = Vec::new();
                    for edge in array(cell)? {
                        keys(edge, &["probability", "to"])?;
                        let to = index(&edge["to"])?;
                        if to >= raw.len() {
                            return Err("transition outside table".into());
                        }
                        edges.push((number(edge, "probability")?, to));
                    }
                    columns.push(edges);
                }
                if columns.len() != actions[1].len() {
                    return Err("column count mismatch".into());
                }
                transitions.push(columns);
            }
            if transitions.len() != actions[0].len() {
                return Err("row count mismatch".into());
            }
        }
        states.push(State {
            phase,
            value,
            observed,
            actions,
            transitions,
        });
    }
    let mut seeds = Vec::new();
    for w in array(&v["worlds"])? {
        keys(w, &["id", "weight", "position"])?;
        let position = index(&w["position"])?;
        if position >= states.len() {
            return Err("world outside table".into());
        }
        seeds.push(Seed {
            id: string(w, "id")?,
            weight: number(w, "weight")?,
            position,
        });
    }
    let domain = Table(states);
    let (built, solution, growth) = if let Some(c) = growth_config(v, config)? {
        #[cfg(feature = "experiment-growth-cadence")]
        let r = growing::reuse::cadence::search(
            &domain,
            &seeds,
            limits,
            c,
            &growing::Uniform,
            growing::reuse::Options {
                in_place: true,
                workspace: true,
                compiler: true,
                static_values: true,
                direct_write: false,
            },
            cadence(v)?,
        )
        .map_err(|e| e.to_string())?;
        #[cfg(not(feature = "experiment-growth-cadence"))]
        let r = search_backend(
            &domain,
            &seeds,
            limits,
            c,
            &growing::Uniform,
            growing::reuse::Options {
                in_place: true,
                workspace: true,
                compiler: true,
                static_values: true,
                direct_write: false,
            },
        )
        .map_err(|e| e.to_string())?;
        #[allow(unused_mut)]
        let mut m = metadata(&r);
        #[cfg(feature = "experiment-growth-cadence")]
        cadence_metadata(&mut m, cadence(v)?);
        (r.built, Some(r.solution), m)
    } else {
        (
            builder::build(&domain, &seeds, limits).map_err(|e| e.to_string())?,
            None,
            Value::Null,
        )
    };
    let meta = json!({"scope":"caller-supplied-observed-transition-game","growth":growth,
        "observation_source":"explicit-table-fields","engine_battle_log_implemented":false,
        "transitions":built.stats.transitions,"switch_decisions":built.stats.switch_decisions});
    Ok((built, solution, meta))
}
pub fn export(t: &Tree) -> Value {
    let nodes:Vec<_>=t.export_nodes().into_iter().map(|n|match n {
        Node::Terminal(v)=>json!({"type":"terminal","value":v}),
        Node::Chance(e)=>json!({"type":"chance","edges":e.into_iter().map(|(p,c)|json!({"probability":p,"child":c})).collect::<Vec<_>>()}),
        Node::Decision {player,information,actions,children}=>json!({"type":"decision","player":player,"information":information,"actions":actions,"children":children}),
    }).collect();
    json!({"mode":"tree","root":t.root(),"nodes":nodes})
}

#[cfg(all(
    not(feature = "experiment-shared-final-passes"),
    not(feature = "experiment-incremental-compilation")
))]
#[cfg(not(feature = "experiment-growth-cadence"))]
use growing::reuse::search as search_backend;
#[cfg(feature = "experiment-incremental-compilation")]
#[cfg(not(feature = "experiment-growth-cadence"))]
use growing::reuse::search_incremental as search_backend;
#[cfg(all(
    feature = "experiment-shared-final-passes",
    not(feature = "experiment-incremental-compilation")
))]
#[cfg(not(feature = "experiment-growth-cadence"))]
use growing::reuse::search_shared as search_backend;

#[cfg(feature = "experiment-growth-cadence")]
pub fn cadence(v: &Value) -> Result<usize, String> {
    let n = integer(&v["growth"], "cadence", 1)?;
    if ![1, 2, 4].contains(&n) {
        return Err("cadence must be 1, 2 or 4".into());
    }
    Ok(n)
}

#[cfg(feature = "experiment-growth-cadence")]
pub fn cadence_metadata(m: &mut Value, n: usize) {
    m["solve_cadence"] = json!(n);
    m["admission_commit"] = json!("selected-batch-atomic");
    m["selection_snapshot"] = json!("one-solved-snapshot-per-batch");
    m["regrets"] = json!("reset-after-admission-batch");
}
