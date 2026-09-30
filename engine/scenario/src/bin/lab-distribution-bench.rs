//! Table-2-style distribution diagnostics on this engine, not a paper-schema reproduction.
//! SCENARIO --joint-seed U64 --describe
//! SCENARIO --joint-seed U64 --plan PLAN.json --sample-seeds A,B,C,D,E
//! Exact Full factorization is uncapped; global outcome pruning is not implemented here.
use lab_engine::{
    action::{JointAction, SlotAction},
    instruction::Outcome,
    rules::Ruleset,
    state::{PokemonRef, SideId, State},
    turn::{
        enumerate_turn_factored_with, legal_joint_actions, sample_turn, FactoredOptions,
        FactoredOutcome, RollMode, Suspension,
    },
};
use lab_scenario::{load_scenario_file, scenario_positions, LoadedScenario, Position};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

const EPS: f64 = 1e-9;
const SAMPLE_COUNTS: [usize; 3] = [16, 64, 256];
type Key = (State<2>, Option<Suspension>);
type Distribution = HashMap<Key, f64>;

#[derive(Clone)]
struct SplitMix64(u64);
impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn index(&mut self, n: usize) -> Result<usize, String> {
        if n == 0 {
            return Err("empty action support".into());
        }
        let n = u64::try_from(n).map_err(|_| "index bound exceeds u64")?;
        let threshold = n.wrapping_neg() % n;
        loop {
            let x = self.next();
            if x >= threshold {
                return Ok((x % n) as usize);
            }
        }
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
}

fn positive(v: f64, name: &str) -> Result<(), String> {
    if !v.is_finite() || v <= 0.0 {
        Err(format!("{name} is not finite positive: {v}"))
    } else {
        Ok(())
    }
}
fn unit_mass(v: f64, name: &str) -> Result<(), String> {
    positive(v, name)?;
    if (v - 1.0).abs() > EPS {
        Err(format!("{name} does not sum to one: {v}"))
    } else {
        Ok(())
    }
}
fn bounded(v: f64, name: &str) -> Result<f64, String> {
    if !v.is_finite() || v < -EPS || v > 1.0 + EPS {
        Err(format!("{name} outside probability bounds: {v}"))
    } else {
        Ok(v.clamp(0.0, 1.0))
    }
}
fn project(key: &Key) -> Key {
    let mut out = key.clone();
    for side in &mut out.0.sides {
        for mon in &mut side.party {
            mon.hp = 0;
        }
    }
    out
}
fn restore_checked(state: &State<2>, original: &State<2>, stage: &str) -> Result<(), String> {
    if state != original {
        Err(format!("State was not restored after {stage}"))
    } else {
        Ok(())
    }
}

struct Selection {
    state: State<2>,
    choices: [JointAction<2>; 2],
    description: Value,
}
fn describe(loaded: &LoadedScenario, seed: u64) -> Result<Selection, String> {
    if !loaded.setup_turns.is_empty()
        || loaded.patch.is_some()
        || loaded.start_state.is_some()
        || !loaded.setup_states.is_empty()
        || loaded.setup_rolls.is_some()
        || loaded.mid_turn.iter().any(|v| !v.is_empty())
    {
        return Err(
            "opening input may not contain setup, patch, filtered starts or midTurn replay".into(),
        );
    }
    let positions = scenario_positions(loaded).map_err(|e| format!("opening setup: {e}"))?;
    select_position(positions, seed)
}
fn select_position(positions: Vec<Position>, seed: u64) -> Result<Selection, String> {
    if positions.is_empty() {
        return Err("opening support is empty".into());
    }
    let mut ordered: Vec<_> = positions
        .into_iter()
        .map(|p| {
            let sort = (
                format!("{:?}", p.state),
                format!("{:?}", p.order),
                p.probability.to_bits(),
            );
            (sort, p)
        })
        .collect();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total = 0.0;
    for (_, p) in &ordered {
        positive(p.probability, "opening probability")?;
        total += p.probability;
    }
    unit_mass(total, "opening probabilities")?;
    let mut rng = SplitMix64(seed);
    let target = rng.unit() * total;
    let mut cumulative = 0.0;
    let mut chosen = ordered.len() - 1;
    for (i, (_, p)) in ordered.iter().enumerate() {
        cumulative += p.probability;
        if target < cumulative {
            chosen = i;
            break;
        }
    }
    let position = &ordered[chosen].1;
    let mut choices = [[SlotAction::Pass; 2]; 2];
    let mut counts = [0usize; 2];
    let mut indices = [0usize; 2];
    for side in [SideId::One, SideId::Two] {
        let actions: Vec<_> = legal_joint_actions(&position.state, Ruleset::CHAMPIONS_MC, side)
            .into_iter()
            .filter(|joint| joint.iter().all(|a| matches!(a, SlotAction::Move { .. })))
            .collect();
        let i = side.index();
        counts[i] = actions.len();
        indices[i] = rng.index(actions.len())?;
        choices[i] = actions[indices[i]];
    }
    let action_json: Vec<Vec<Value>> = choices
        .iter()
        .map(|side| {
            side.iter()
                .map(|action| match action {
                    SlotAction::Move {
                        index,
                        target,
                        gimmick,
                    } => {
                        json!({"move_index":index,"target":target,"gimmick":format!("{gimmick:?}")})
                    }
                    _ => unreachable!("move-only selection"),
                })
                .collect()
        })
        .collect();
    let description = json!({"schema":1,"kind":"frozen-opening-selection","joint_seed":seed,
        "rng":"splitmix64-v1; weighted opening uses high53-bit uniform; joint index uses rejection",
        "position_order":"State Debug, party-order Debug, probability bits; ascending",
        "joint_policy":"uniform legal move-only joint actions per side; target and legal Mega variants count separately; no switch/pass",
        "ruleset":"CHAMPIONS_MC","position_count":ordered.len(),"position_index":chosen,
        "position_probability":position.probability,"position_probability_bits":position.probability.to_bits(),
        "position_total_mass":total,"eligible_joint_counts":counts,"joint_indices":indices,
        "full_state_debug":format!("{:?}",position.state),"party_order_debug":format!("{:?}",position.order),
        "choices_debug":format!("{choices:?}"),"choices":action_json});
    Ok(Selection {
        state: position.state.clone(),
        choices,
        description,
    })
}
fn description_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
fn check_plan(description: &Value, path: &Path) -> Result<(), String> {
    let supplied = std::fs::read(path).map_err(|e| format!("plan read: {e}"))?;
    check_plan_bytes(description, &supplied)
}
fn check_plan_bytes(description: &Value, supplied: &[u8]) -> Result<(), String> {
    if supplied != description_bytes(description)? {
        return Err("frozen description bytes do not match reproduced state/actions".into());
    }
    Ok(())
}

struct Component {
    base: Key,
    weight: f64,
    hp: Vec<(PokemonRef, Vec<(i16, f64)>)>,
}
struct Reference {
    components: Vec<Component>,
    // Projection key -> component indices. All exact-overlap components contribute.
    buckets: HashMap<Key, Vec<usize>>,
    projection: Distribution,
    mass: f64,
    flat_count_upper_bound: f64,
}
impl Reference {
    fn prepare(original: &State<2>, outcomes: &[FactoredOutcome]) -> Result<Self, String> {
        if outcomes.is_empty() {
            return Err("exact reference is empty".into());
        }
        let mut state = original.clone();
        let mut out = Self {
            components: Vec::new(),
            buckets: HashMap::new(),
            projection: HashMap::new(),
            mass: 0.0,
            flat_count_upper_bound: 0.0,
        };
        for outcome in outcomes {
            positive(outcome.probability, "reference component probability")?;
            state.apply(&outcome.instructions);
            let base = (state.clone(), outcome.suspension.clone());
            state.reverse(&outcome.instructions);
            restore_checked(&state, original, "reference instruction roundtrip")?;
            let mut mass = outcome.probability;
            let mut seen = HashSet::new();
            for (pokemon, points) in &outcome.hp {
                if !seen.insert(*pokemon)
                    || points.is_empty()
                    || usize::from(pokemon.party) >= base.0.sides[pokemon.side.index()].party.len()
                {
                    return Err(
                        "reference HP factor has duplicate/invalid member or empty support".into(),
                    );
                }
                let mut hp_sum = 0.0;
                for (i, &(hp, p)) in points.iter().enumerate() {
                    if hp < 0 || (i > 0 && points[i - 1].0 >= hp) {
                        return Err("reference HP support must be distinct and ascending".into());
                    }
                    positive(p, "reference HP probability")?;
                    hp_sum += p;
                }
                unit_mass(hp_sum, "reference HP probabilities")?;
                if base.0.pokemon(*pokemon).hp != points[0].0 {
                    return Err("factored base does not carry minimum HP".into());
                }
                mass *= hp_sum;
            }
            positive(mass, "reference component mass")?;
            let projection = project(&base);
            *out.projection.entry(projection.clone()).or_insert(0.0) += mass;
            out.buckets
                .entry(projection)
                .or_default()
                .push(out.components.len());
            out.components.push(Component {
                base,
                weight: outcome.probability,
                hp: outcome.hp.clone(),
            });
            out.mass += mass;
            out.flat_count_upper_bound += outcome.flat_count();
        }
        unit_mass(out.mass, "exact reference total mass")?;
        positive(
            out.flat_count_upper_bound,
            "factored flat-count upper bound",
        )?;
        for p in out.projection.values_mut() {
            *p /= out.mass;
        }
        Ok(out)
    }
    fn probability(&self, target: &Key) -> Result<f64, String> {
        let projection = project(target);
        let mut p = 0.0;
        if let Some(indices) = self.buckets.get(&projection) {
            for &i in indices {
                let component = &self.components[i];
                if target.1 != component.base.1 {
                    continue;
                }
                let mut normalized = target.0.clone();
                let mut weight = component.weight;
                for (pokemon, points) in &component.hp {
                    let hp = target.0.pokemon(*pokemon).hp;
                    if let Ok(at) = points.binary_search_by_key(&hp, |&(hp, _)| hp) {
                        weight *= points[at].1;
                    } else {
                        weight = 0.0;
                        break;
                    }
                    normalized.pokemon_mut(*pokemon).hp = component.base.0.pokemon(*pokemon).hp;
                }
                if weight > 0.0 && normalized == component.base.0 {
                    p += weight;
                }
            }
        }
        bounded(p / self.mass, "reference P(sample state)")
    }
}

fn sample_distribution(
    original: &State<2>,
    outcomes: &[Outcome],
    count: usize,
) -> Result<(Distribution, f64), String> {
    if outcomes.is_empty() || count == 0 {
        return Err("sample support/count is empty".into());
    }
    let mut state = original.clone();
    let mut distribution = HashMap::new();
    let mut mass = 0.0;
    let mut frequencies = 0usize;
    for outcome in outcomes {
        positive(outcome.probability, "sample weight")?;
        let frequency = outcome.probability * count as f64;
        if !frequency.is_finite()
            || (frequency - frequency.round()).abs() > EPS
            || frequency.round() < 1.0
        {
            return Err("sample weight is not a positive empirical count/N".into());
        }
        frequencies += frequency.round() as usize;
        state.apply(&outcome.instructions);
        let key = (state.clone(), outcome.suspension.clone());
        state.reverse(&outcome.instructions);
        restore_checked(&state, original, "sample instruction roundtrip")?;
        *distribution.entry(key).or_insert(0.0) += outcome.probability;
        mass += outcome.probability;
    }
    unit_mass(mass, "sample total mass")?;
    if frequencies != count {
        return Err("sample frequencies do not sum to requested count".into());
    }
    for q in distribution.values_mut() {
        *q /= mass;
    }
    Ok((distribution, mass))
}
fn projected_distribution(distribution: &Distribution) -> Distribution {
    let mut out = HashMap::new();
    for (key, &q) in distribution {
        *out.entry(project(key)).or_insert(0.0) += q;
    }
    out
}
fn metrics(
    sample: &Distribution,
    mut p: impl FnMut(&Key) -> Result<f64, String>,
) -> Result<Value, String> {
    if sample.is_empty() {
        return Err("metric sample support is empty".into());
    }
    // Stable summation order, independent of randomized HashMap bucket placement.
    let mut points: Vec<_> = sample
        .iter()
        .map(|(key, &q)| Ok((p(key)?, q)))
        .collect::<Result<_, String>>()?;
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut coverage = 0.0;
    let mut overlap = 0.0;
    let mut outside = 0.0;
    let mut qsum = 0.0;
    for (p, q) in points {
        positive(q, "empirical probability")?;
        bounded(p, "reference point probability")?;
        coverage += p;
        overlap += p.min(q);
        qsum += q;
        if p == 0.0 {
            outside += q;
        }
    }
    unit_mass(qsum, "metric empirical mass")?;
    Ok(
        json!({"coverage":bounded(coverage,"coverage")?,"tv":bounded(1.0-overlap,"TV")?,
        "outside_reference_mass":bounded(outside,"outside reference mass")?,"unique_states":sample.len()}),
    )
}
fn posthoc_top32(reference: &Distribution) -> Result<Value, String> {
    let mut p: Vec<_> = reference.values().copied().collect();
    p.sort_by(|a, b| b.total_cmp(a));
    let mass: f64 = p.iter().take(32).sum();
    let mass = bounded(mass, "posthoc retained mass")?;
    Ok(
        json!({"projection":"non_hp_state","support_size":p.len(),"retained_states":p.len().min(32),
        "retained_mass":mass,"omitted_mass":1.0-mass,"renormalized_tv":1.0-mass,
        "method":"posthoc probability ranking after exhaustive reference; not an engine branch cap",
        "timing_speedup_claim":false}),
    )
}
fn nanos(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
fn measure(selection: Selection, seeds: &[u64]) -> Result<Value, String> {
    let original = selection.state;
    let mut state = original.clone();
    let started = Instant::now();
    let generated = enumerate_turn_factored_with(
        &mut state,
        Ruleset::CHAMPIONS_MC,
        selection.choices,
        FactoredOptions {
            rolls: RollMode::Full,
            max_support: None,
        },
    );
    let reference_ns = nanos(started);
    restore_checked(&state, &original, "exact factored API")?;
    let generated = generated.map_err(|e| format!("exact reference: {e}"))?;
    if generated.tv_bound != 0.0 {
        return Err(format!(
            "uncapped reference TV bound is not zero: {}",
            generated.tv_bound
        ));
    }
    let started = Instant::now();
    let reference = Reference::prepare(&original, &generated.outcomes)?;
    let top32 = posthoc_top32(&reference.projection)?;
    let prepare_ns = nanos(started);
    let mut results = Vec::new();
    for count in SAMPLE_COUNTS {
        for &seed in seeds {
            let started = Instant::now();
            let sampled = sample_turn(
                &mut state,
                Ruleset::CHAMPIONS_MC,
                selection.choices,
                count,
                seed,
            );
            let kernel_ns = nanos(started);
            restore_checked(&state, &original, "sampling API")?;
            let sampled = sampled.map_err(|e| format!("sample N={count},seed={seed}: {e}"))?;
            let started = Instant::now();
            let (distribution, mass) = sample_distribution(&original, &sampled, count)?;
            let full = metrics(&distribution, |key| reference.probability(key))?;
            let projection = projected_distribution(&distribution);
            let projected = metrics(&projection, |key| {
                Ok(reference.projection.get(key).copied().unwrap_or(0.0))
            })?;
            let metric_ns = nanos(started);
            results.push(json!({"count":count,"seed":seed,"kernel_ns":kernel_ns,"metric_ns":metric_ns,
            "raw_outcomes":sampled.len(),"sample_total_mass":mass,"unique_full_states":distribution.len(),
            "full_state":full,"non_hp_state":projected}));
        }
    }
    let supported = results
        .iter()
        .all(|r| r["full_state"]["outside_reference_mass"].as_f64() == Some(0.0));
    Ok(
        json!({"schema":1,"status":"ok","description":selection.description,
        "metric_schema":"engine full State plus Suspension; non_hp_state zeros only current Pokemon.hp; not the paper outcome schema",
        "reference":{"method":"factored-full-exact","kernel_ns":reference_ns,"metric_prepare_ns":prepare_ns,
            "components":reference.components.len(),"total_mass":reference.mass,"tv_bound":generated.tv_bound,
            "flat_count_upper_bound":reference.flat_count_upper_bound,"full_support_materialized":false,
            "suspended_components":generated.outcomes.iter().filter(|o|o.suspension.is_some()).count()},
        "non_hp_state_posthoc_top32":top32,"samples":results,"all_state_restored":true,
        "all_sample_outcomes_in_reference":supported,
        "timing_scope":"kernel_ns surrounds one engine API call only; loading, selection, metrics and reference indexing are excluded; allocation inside the API is included",
        "suspension_scope":"both APIs stop at the first mid-turn switch; no automatic resume",
        "sample_policy":"same seed reused across N gives nested deterministic prefixes; five distinct seeds; uniform empirical weights",
        "probability_policy":"raw reference and sample mass must be finite positive and within 1e-9 of one; each distribution is then normalized by its measured total mass; factored overlaps are summed; probability bounds permit only 1e-9 floating roundoff before clamping",
        "execution_policy":"driver launches a fresh process per case; no warmup; one exact reference first, then N=16,64,256 in supplied seed order; startup and first-call page/cache effects remain; not the paper's identical protocol"}),
    )
}

struct Args {
    scenario: PathBuf,
    joint_seed: u64,
    describe: bool,
    plan: Option<PathBuf>,
    seeds: Vec<u64>,
}
fn args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let scenario = PathBuf::from(args.next().ok_or("scenario path required")?);
    let mut joint_seed = None;
    let mut describe = false;
    let mut plan = None;
    let mut seeds = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--joint-seed" => {
                if joint_seed.is_some() {
                    return Err("duplicate joint seed".into());
                }
                joint_seed = Some(
                    args.next()
                        .ok_or("joint seed missing")?
                        .parse()
                        .map_err(|_| "bad u64 joint seed")?,
                );
            }
            "--describe" => {
                if describe {
                    return Err("duplicate describe".into());
                }
                describe = true;
            }
            "--plan" => {
                if plan.is_some() {
                    return Err("duplicate plan".into());
                }
                plan = Some(PathBuf::from(args.next().ok_or("plan path missing")?));
            }
            "--sample-seeds" => {
                if seeds.is_some() {
                    return Err("duplicate sample seeds".into());
                }
                seeds = Some(
                    args.next()
                        .ok_or("sample seeds missing")?
                        .split(',')
                        .map(|x| {
                            x.parse::<u64>()
                                .map_err(|_| "bad u64 sample seed".to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            _ => return Err(format!("unknown argument: {flag}")),
        }
    }
    let seeds = seeds.unwrap_or_default();
    if describe {
        if plan.is_some() || !seeds.is_empty() {
            return Err("describe does not accept plan/sample seeds".into());
        }
    } else if plan.is_none() || seeds.len() != 5 || seeds.iter().collect::<HashSet<_>>().len() != 5
    {
        return Err("measurement requires plan and exactly five distinct sample seeds".into());
    }
    Ok(Args {
        scenario,
        joint_seed: joint_seed.ok_or("joint seed required")?,
        describe,
        plan,
        seeds,
    })
}
fn run() -> Result<Value, String> {
    let args = args()?;
    let loaded = load_scenario_file(&args.scenario).map_err(|e| format!("scenario load: {e}"))?;
    let selection = describe(&loaded, args.joint_seed)?;
    if args.describe {
        return Ok(selection.description);
    }
    check_plan(&selection.description, args.plan.as_ref().unwrap())?;
    measure(selection, &args.seeds)
}
fn main() -> std::process::ExitCode {
    let (value, status) = match run() {
        Ok(v) => (v, std::process::ExitCode::SUCCESS),
        Err(e) => (
            json!({"schema":1,"status":"error","error":e}),
            std::process::ExitCode::FAILURE,
        ),
    };
    let mut out = std::io::stdout().lock();
    if serde_json::to_writer(&mut out, &value).is_err() || writeln!(out).is_err() {
        return std::process::ExitCode::FAILURE;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }
    fn fixture(name: &str) -> LoadedScenario {
        load_scenario_file(engine_dir().join(format!("oracle/scenarios/{name}.json"))).unwrap()
    }
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} != {b}");
    }
    fn expanded(original: &State<2>, factored: &[FactoredOutcome]) -> Distribution {
        let mut state = original.clone();
        let mut out = HashMap::new();
        // Only bounded synthetic/fixture tests expand, never the production benchmark path.
        assert!(factored.iter().map(|f| f.flat_count()).sum::<f64>() < 100_000.0);
        for f in factored {
            for outcome in f.expand() {
                state.apply(&outcome.instructions);
                *out.entry((state.clone(), outcome.suspension.clone()))
                    .or_insert(0.0) += outcome.probability;
                state.reverse(&outcome.instructions);
                assert_eq!(&state, original);
            }
        }
        out
    }

    #[test]
    fn seeded_selection_is_order_independent_and_preserves_mega_move_variants() {
        let mut rng = SplitMix64(0);
        assert_eq!(rng.next(), 0xe220a8397b1dcdaf);
        assert_eq!(rng.next(), 0x6e789e6aa1b965f4);
        assert!(rng.index(0).is_err());
        for n in [1, 2, 3, 7, 257] {
            for _ in 0..50 {
                assert!(rng.index(n).unwrap() < n);
            }
        }
        let mon = |species, item| json!({"species":species,"item":item,"ability":"Honey Gather","nature":"Serious","evs":{},"moves":["Harden"],"level":50});
        let scenario = json!({"format":"gen9championsvgc2026regmc",
            "p1":{"team":[mon("Gardevoir","Gardevoirite"),mon("Snorlax","")],"order":"12"},
            "p2":{"team":[mon("Swampert",""),mon("Excadrill","")],"order":"12"}});
        let loaded = lab_scenario::load_scenario_str(&scenario.to_string(), &engine_dir()).unwrap();
        let mut positions = scenario_positions(&loaded).unwrap();
        assert_eq!(positions.len(), 1);
        let mut second = positions[0].clone();
        positions[0].probability = 0.25;
        second.probability = 0.75;
        second.state.sides[0].party[0].hp -= 1;
        positions.push(second);
        let mut reversed = positions.clone();
        reversed.reverse();
        let mut saw_mega = false;
        for seed in 0..16 {
            let a = select_position(positions.clone(), seed).unwrap();
            let b = select_position(reversed.clone(), seed).unwrap();
            assert_eq!(
                description_bytes(&a.description).unwrap(),
                description_bytes(&b.description).unwrap()
            );
            assert_eq!(a.description["eligible_joint_counts"], json!([2, 1]));
            for side in a.choices {
                for action in side {
                    assert!(matches!(action, SlotAction::Move { .. }));
                    saw_mega |= action.gimmick() != lab_engine::gimmick::Gimmick::None;
                }
            }
        }
        assert!(saw_mega);
        let a = describe(&loaded, 123).unwrap();
        let mut bytes = description_bytes(&a.description).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert!(!bytes.contains(&b'\r'));
        bytes.pop();
        assert_ne!(bytes, description_bytes(&a.description).unwrap());
    }

    #[test]
    fn non_hp_projection_keeps_all_other_fields_and_hidden_party_order() {
        let mut state = State::<2>::default();
        state.sides[0].party[0].hp = 10;
        state.sides[0].party[0].max_hp = 20;
        state.sides[0].slots[0].substitute_hp = 3;
        let a = (state.clone(), None);
        let mut changed = a.clone();
        changed.0.sides[0].party[0].hp = 7;
        assert_ne!(a, changed);
        assert_eq!(project(&a), project(&changed));
        changed.0.sides[0].party[0].max_hp = 21;
        assert_ne!(project(&a), project(&changed));
        changed = a.clone();
        changed.0.sides[0].slots[0].substitute_hp = 4;
        assert_ne!(project(&a), project(&changed));
        changed = a.clone();
        changed.0.sides[0].party_order.swap(0, 1);
        assert_ne!(project(&a), project(&changed));
        assert_eq!(project(&a).0.sides[0].slots[0].substitute_hp, 3);
    }

    #[test]
    fn frozen_plan_rejects_changed_state_actions_seed_and_noncanonical_bytes() {
        let expected = json!({"schema":1,"joint_seed":7,"full_state_debug":"complete state",
            "choices_debug":"complete actions","choices":[[1],[2]]});
        let bytes = description_bytes(&expected).unwrap();
        check_plan_bytes(&expected, &bytes).unwrap();
        for field in ["joint_seed", "full_state_debug", "choices_debug", "choices"] {
            let mut changed = expected.clone();
            changed[field] = json!("tampered");
            assert!(check_plan_bytes(&changed, &bytes).is_err());
        }
        assert!(check_plan_bytes(&expected, &bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes.clone();
        extra.push(b'\n');
        assert!(check_plan_bytes(&expected, &extra).is_err());
        assert!(check_plan_bytes(
            &expected,
            serde_json::to_string_pretty(&expected).unwrap().as_bytes()
        )
        .is_err());
    }

    #[test]
    fn overlapping_factored_components_match_expansion_and_exact_tv() {
        let first = PokemonRef {
            side: SideId::One,
            party: 0,
        };
        let second = PokemonRef {
            side: SideId::Two,
            party: 0,
        };
        let mut state = State::<2>::default();
        state.pokemon_mut(first).hp = 10;
        state.pokemon_mut(second).hp = 4;
        let components = vec![
            FactoredOutcome {
                probability: 0.6,
                instructions: vec![],
                hp: vec![
                    (first, vec![(10, 0.5), (12, 0.5)]),
                    (second, vec![(4, 0.25), (8, 0.75)]),
                ],
                suspension: None,
            },
            FactoredOutcome {
                probability: 0.4,
                instructions: vec![],
                hp: vec![
                    (first, vec![(10, 0.5), (12, 0.5)]),
                    (second, vec![(4, 0.75), (8, 0.25)]),
                ],
                suspension: None,
            },
        ];
        let reference = Reference::prepare(&state, &components).unwrap();
        let flat = expanded(&state, &components);
        assert_eq!(flat.len(), 4);
        close(reference.flat_count_upper_bound, 8.0);
        for (key, &p) in &flat {
            close(reference.probability(key).unwrap(), p);
        }
        let target = (state.clone(), None);
        let p = flat[&target];
        close(p, 0.225);
        let sample = HashMap::from([(target.clone(), 1.0)]);
        let result = metrics(&sample, |k| reference.probability(k)).unwrap();
        close(result["coverage"].as_f64().unwrap(), p);
        close(result["tv"].as_f64().unwrap(), 1.0 - p);
        let direct_tv = 0.5
            * flat
                .iter()
                .map(|(k, p)| (p - sample.get(k).copied().unwrap_or(0.0)).abs())
                .sum::<f64>();
        close(result["tv"].as_f64().unwrap(), direct_tv);
        let projected = projected_distribution(&sample);
        let metric = metrics(&projected, |k| {
            Ok(reference.projection.get(k).copied().unwrap_or(0.0))
        })
        .unwrap();
        close(metric["coverage"].as_f64().unwrap(), 1.0);
        close(metric["tv"].as_f64().unwrap(), 0.0);
        let mut unknown = target;
        unknown.0.turn = 3;
        close(reference.probability(&unknown).unwrap(), 0.0);
        let outside = metrics(&HashMap::from([(unknown, 1.0)]), |k| {
            reference.probability(k)
        })
        .unwrap();
        close(outside["outside_reference_mass"].as_f64().unwrap(), 1.0);
        close(outside["tv"].as_f64().unwrap(), 1.0);
    }

    #[test]
    fn invalid_probability_supports_fail_and_top32_is_projection_posthoc() {
        let state = State::<2>::default();
        assert!(Reference::prepare(&state, &[]).is_err());
        for probability in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.5] {
            assert!(Reference::prepare(
                &state,
                &[FactoredOutcome {
                    probability,
                    instructions: vec![],
                    hp: vec![],
                    suspension: None
                }]
            )
            .is_err());
        }
        let member = PokemonRef {
            side: SideId::One,
            party: 0,
        };
        for points in [
            vec![],
            vec![(0, 0.5), (0, 0.5)],
            vec![(0, f64::NAN)],
            vec![(1, 1.0)],
            vec![(0, 0.5)],
        ] {
            assert!(Reference::prepare(
                &state,
                &[FactoredOutcome {
                    probability: 1.0,
                    instructions: vec![],
                    hp: vec![(member, points)],
                    suspension: None
                }]
            )
            .is_err());
        }
        let mut distribution = HashMap::new();
        for turn in 0..40 {
            let mut s = state.clone();
            s.turn = turn;
            distribution.insert((s, None), 1.0 / 40.0);
        }
        let top = posthoc_top32(&distribution).unwrap();
        assert_eq!(top["retained_states"], 32);
        close(top["retained_mass"].as_f64().unwrap(), 0.8);
        close(top["renormalized_tv"].as_f64().unwrap(), 0.2);
        assert_eq!(top["timing_speedup_claim"], false);
        for probability in [0.0, -0.25, f64::INFINITY, 0.1, 0.5] {
            assert!(sample_distribution(
                &state,
                &[Outcome {
                    probability,
                    instructions: vec![],
                    suspension: None
                }],
                16
            )
            .is_err());
        }
    }

    #[test]
    fn correlated_components_and_fixed_hp_singletons_do_not_gain_cross_support() {
        use lab_engine::instruction::Instruction;
        let a = PokemonRef {
            side: SideId::One,
            party: 0,
        };
        let b = PokemonRef {
            side: SideId::Two,
            party: 0,
        };
        let mut state = State::<2>::default();
        state.pokemon_mut(a).hp = 10;
        state.pokemon_mut(b).hp = 4;
        let components = vec![
            FactoredOutcome {
                probability: 0.5,
                instructions: vec![],
                hp: vec![(a, vec![(10, 1.0)]), (b, vec![(4, 1.0)])],
                suspension: None,
            },
            FactoredOutcome {
                probability: 0.5,
                instructions: vec![
                    Instruction::Heal {
                        target: a,
                        amount: 2,
                    },
                    Instruction::Heal {
                        target: b,
                        amount: 4,
                    },
                ],
                hp: vec![(a, vec![(12, 1.0)]), (b, vec![(8, 1.0)])],
                suspension: None,
            },
        ];
        let reference = Reference::prepare(&state, &components).unwrap();
        close(reference.probability(&(state.clone(), None)).unwrap(), 0.5);
        let mut alternate = state.clone();
        alternate.pokemon_mut(a).hp = 12;
        alternate.pokemon_mut(b).hp = 8;
        close(
            reference.probability(&(alternate.clone(), None)).unwrap(),
            0.5,
        );
        alternate.pokemon_mut(a).hp = 10;
        close(
            reference.probability(&(alternate.clone(), None)).unwrap(),
            0.0,
        );
        alternate.pokemon_mut(a).hp = 12;
        alternate.pokemon_mut(b).hp = 4;
        close(
            reference.probability(&(alternate.clone(), None)).unwrap(),
            0.0,
        );
        assert_eq!(expanded(&state, &components).len(), 2);
        let singleton = Reference::prepare(
            &state,
            &[FactoredOutcome {
                probability: 1.0,
                instructions: vec![],
                hp: vec![],
                suspension: None,
            }],
        )
        .unwrap();
        close(singleton.probability(&(state.clone(), None)).unwrap(), 1.0);
        assert_eq!(project(&(state, None)), project(&(alternate.clone(), None)));
        close(singleton.probability(&(alternate, None)).unwrap(), 0.0);
    }

    #[test]
    fn bounded_real_full_factored_and_sample_paths_preserve_state_and_suspension() {
        for name in ["single-hit", "uturn-pause"] {
            let loaded = fixture(name);
            let positions = scenario_positions(&loaded).unwrap();
            assert!(!positions.is_empty() && positions.len() <= 4);
            for position in positions {
                let lab_scenario::Decision::Turn(choices) =
                    lab_scenario::scenario_decision(&loaded, &position).unwrap()
                else {
                    panic!("turn fixture")
                };
                let original = position.state;
                let mut state = original.clone();
                let factored = enumerate_turn_factored_with(
                    &mut state,
                    Ruleset::CHAMPIONS_MC,
                    choices,
                    FactoredOptions {
                        rolls: RollMode::Full,
                        max_support: None,
                    },
                )
                .unwrap();
                assert_eq!(state, original);
                assert_eq!(factored.tv_bound, 0.0);
                let reference = Reference::prepare(&original, &factored.outcomes).unwrap();
                let expanded = expanded(&original, &factored.outcomes);
                let flat = lab_engine::turn::enumerate_turn_with(
                    &mut state,
                    Ruleset::CHAMPIONS_MC,
                    choices,
                    lab_engine::turn::EnumerateOptions {
                        rolls: RollMode::Full,
                    },
                )
                .unwrap();
                assert_eq!(state, original);
                let mut actual = HashMap::new();
                for outcome in flat {
                    state.apply(&outcome.instructions);
                    *actual
                        .entry((state.clone(), outcome.suspension))
                        .or_insert(0.0) += outcome.probability;
                    state.reverse(&outcome.instructions);
                }
                assert_eq!(state, original);
                assert_eq!(actual.len(), expanded.len());
                for (key, &p) in &actual {
                    close(p, expanded[key]);
                    close(reference.probability(key).unwrap(), p);
                }
                for seed in [0, 17] {
                    for count in [16, 64] {
                        let sampled =
                            sample_turn(&mut state, Ruleset::CHAMPIONS_MC, choices, count, seed)
                                .unwrap();
                        assert_eq!(state, original);
                        let (sample, _) = sample_distribution(&original, &sampled, count).unwrap();
                        let result = metrics(&sample, |k| reference.probability(k)).unwrap();
                        close(result["outside_reference_mass"].as_f64().unwrap(), 0.0);
                        let direct = 0.5
                            * actual
                                .iter()
                                .map(|(k, p)| (p - sample.get(k).copied().unwrap_or(0.0)).abs())
                                .sum::<f64>();
                        close(result["tv"].as_f64().unwrap(), direct);
                        if name == "uturn-pause" {
                            assert!(sample.keys().all(|k| k.1.is_some()));
                            let key = sample.keys().next().unwrap();
                            let mut without = key.clone();
                            without.1 = None;
                            close(reference.probability(&without).unwrap(), 0.0);
                            assert_ne!(project(key), project(&without));
                        }
                    }
                }
            }
        }
    }
}
