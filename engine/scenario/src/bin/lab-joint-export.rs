//! P1e2 validation-only exact joint-HP export. No engine runtime changes.
//! Same frozen opening selection as lab-distribution-bench; Full RNG remains untouched.
//! Output retains the complete non-HP State and Suspension plus all-party joint HP.
//! The external runner supplies wall/CPU/RSS limits; incomplete export is not evidence.
use lab_engine::{
    action::{JointAction, SlotAction},
    rules::Ruleset,
    state::{LazyTag, SideId, State, PARTY_SIZE},
    turn::{enumerate_turn_factored_with, legal_joint_actions, FactoredOptions,
           FactoredOutcome, RollMode, Suspension},
};
use lab_scenario::{load_scenario_file, scenario_positions, LoadedScenario, Position};
use serde_json::{json, Value};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    fs::{File, OpenOptions},
    hash::{Hash, Hasher},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};
const EPS: f64 = 1e-9;
const UNITS: usize = 2 * PARTY_SIZE;
const ROW_BYTES: u64 = 4 + 2 * UNITS as u64 + 8;
const DEFAULT_MAX_ROWS: u64 = 10_000_000;
const DEFAULT_MAX_BYTES: u64 = 536_870_912;
type Key = (State<2>, Option<Suspension>);
type HpTuple = [i16; UNITS];

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


// All code below is validation/export code, not an alternate turn implementation.
#[derive(Clone, Copy, Default)]
struct Sum { value: f64, correction: f64 }
impl Sum {
    fn add(&mut self, x: f64) {
        let t = self.value + x;
        self.correction += if self.value.abs() >= x.abs() {
            (self.value - t) + x
        } else {
            (x - t) + self.value
        };
        self.value = t;
    }
    fn get(self) -> f64 { self.value + self.correction }
}
fn key_hash(value: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}
fn plain_hp(state: &State<2>) -> Result<HpTuple, String> {
    let mut tuple = [0; UNITS];
    for (s, side) in state.sides.iter().enumerate() {
        if side.party.len() != PARTY_SIZE { return Err("party shape mismatch".into()); }
        for (i, mon) in side.party.iter().enumerate() {
            // LazyTag deliberately ignores its value in Eq/Hash.
            if format!("{:?}", mon.lazy) != format!("{:?}", LazyTag::default()) {
                return Err(format!("unresolved LazyTag at {s}:{i}"));
            }
            if mon.hp < 0 || mon.max_hp < 0 || mon.hp > mon.max_hp {
                return Err(format!("invalid HP at {s}:{i}: {}/{}", mon.hp, mon.max_hp));
            }
            tuple[s * PARTY_SIZE + i] = mon.hp;
        }
    }
    Ok(tuple)
}
fn restored(state: &State<2>, original: &State<2>, stage: &str) -> Result<(), String> {
    restore_checked(state, original, stage)?;
    if key_hash(state) != key_hash(original) || format!("{state:?}") != format!("{original:?}") {
        return Err(format!("State Hash/Debug restoration failed after {stage}"));
    }
    plain_hp(state)?;
    Ok(())
}
fn full_debug(key: &Key) -> String { format!("{:?}\n{:?}", key.0, key.1) }

struct Product {
    weight: f64,
    values: [Vec<(i16, f64)>; UNITS],
    indices: [usize; UNITS],
}
impl Product {
    fn tuple(&self) -> HpTuple {
        std::array::from_fn(|i| self.values[i][self.indices[i]].0)
    }
    fn probability(&self) -> Result<f64, String> {
        let p = self.values.iter().zip(self.indices).fold(
            self.weight, |p, (v, i)| p * v[i].1);
        positive(p, "joint contribution")?;
        Ok(p)
    }
    // Last coordinate advances fastest, so iteration order is lexicographic HP order.
    fn advance(&mut self) -> bool {
        for i in (0..UNITS).rev() {
            self.indices[i] += 1;
            if self.indices[i] < self.values[i].len() { return true; }
            self.indices[i] = 0;
        }
        false
    }
}
struct Bucket { text: String, products: Vec<Product> }
struct Prepared {
    buckets: Vec<Bucket>,
    components: usize,
    suspended_components: usize,
    component_mass: f64,
    flat_count_upper_bound: u64,
}
fn prepare(original: &State<2>, outcomes: Vec<FactoredOutcome>, max_bytes: u64)
    -> Result<Prepared, String>
{
    plain_hp(original)?;
    if outcomes.is_empty() { return Err("empty exact outcome support".into()); }
    let components = outcomes.len();
    let mut suspended_components = 0;
    let mut state = original.clone();
    let mut keys: HashMap<Key, usize> = HashMap::new();
    let mut buckets: Vec<Bucket> = Vec::new();
    let mut dictionary_text_bytes = 0u64;
    let mut mass = Sum::default();
    let mut flat_count_upper_bound = 0u64;
    for outcome in outcomes {
        positive(outcome.probability, "component probability")?;
        state.apply(&outcome.instructions);
        let base_hp = plain_hp(&state)?;
        let mut values: [Vec<(i16, f64)>; UNITS] =
            std::array::from_fn(|i| vec![(base_hp[i], 1.0)]);
        let mut units = HashSet::new();
        let mut count = 1u64;
        for (mon, points) in outcome.hp {
            let party = usize::from(mon.party);
            if party >= PARTY_SIZE { return Err("factor party index out of range".into()); }
            let unit = mon.side.index() * PARTY_SIZE + party;
            if !units.insert(unit) { return Err("duplicate factor unit".into()); }
            if points.is_empty() { return Err("empty HP factor".into()); }
            if points[0].0 != base_hp[unit] {
                return Err("base instructions do not set minimum factor HP".into());
            }
            let max_hp = state.sides[mon.side.index()].party[party].max_hp;
            let mut previous = None;
            let mut factor_mass = Sum::default();
            for &(hp, p) in &points {
                if hp < 0 || hp > max_hp || previous.is_some_and(|last| hp <= last) {
                    return Err("HP factor support not strict, sorted and in range".into());
                }
                positive(p, "HP factor probability")?;
                factor_mass.add(p);
                previous = Some(hp);
            }
            unit_mass(factor_mass.get(), "HP factor")?;
            count = count.checked_mul(points.len() as u64).ok_or("flat count overflow")?;
            values[unit] = points;
        }
        flat_count_upper_bound = flat_count_upper_bound.checked_add(count)
            .ok_or("flat count sum overflow")?;
        suspended_components += usize::from(outcome.suspension.is_some());
        let key = project(&(state.clone(), outcome.suspension));
        // The real Eq/Hash key is consulted before Debug serialization. Hashing a pending
        // symbolic damage carrier also fails closed in the candidate implementation.
        let product = Product { weight: outcome.probability, values, indices: [0; UNITS] };
        if let Some(&index) = keys.get(&key) {
            if buckets[index].text != full_debug(&key) {
                return Err("equal Eq/Hash keys have different Debug serialization".into());
            }
            buckets[index].products.push(product);
        } else {
            if buckets.len() >= u32::MAX as usize { return Err("dictionary id overflow".into()); }
            let text = full_debug(&key);
            dictionary_text_bytes = dictionary_text_bytes.checked_add(text.len() as u64)
                .ok_or("dictionary size overflow")?;
            if dictionary_text_bytes > max_bytes { return Err("dictionary exceeds byte limit".into()); }
            let index = buckets.len();
            keys.insert(key, index);
            buckets.push(Bucket { text, products: vec![product] });
        }
        mass.add(outcome.probability);
        state.reverse(&outcome.instructions);
        restored(&state, original, "component instruction apply/reverse")?;
    }
    unit_mass(mass.get(), "component probabilities")?;
    // Actual keys above establish equality; this separate check proves Debug is injective
    // over the observed keys before it becomes the cross-process identity.
    buckets.sort_by(|a, b| a.text.cmp(&b.text));
    if buckets.windows(2).any(|p| p[0].text == p[1].text) {
        return Err("distinct Eq/Hash keys collide in Debug serialization".into());
    }
    Ok(Prepared { buckets, components, suspended_components,
                  component_mass: mass.get(), flat_count_upper_bound })
}
fn merge_products(products: &mut [Product], mut emit: impl FnMut(HpTuple, f64) -> Result<(), String>)
    -> Result<(u64, f64), String>
{
    if products.is_empty() { return Err("empty projection bucket".into()); }
    let mut heap: BinaryHeap<Reverse<(HpTuple, usize)>> = BinaryHeap::new();
    for (i, p) in products.iter().enumerate() { heap.push(Reverse((p.tuple(), i))); }
    let mut current = None;
    let mut group = Sum::default();
    let mut total = Sum::default();
    let mut rows = 0u64;
    while let Some(Reverse((tuple, index))) = heap.pop() {
        if current.is_some_and(|old| old != tuple) {
            let p = group.get();
            positive(p, "summed joint probability")?;
            emit(current.unwrap(), p)?;
            total.add(p);
            rows = rows.checked_add(1).ok_or("row count overflow")?;
            group = Sum::default();
        }
        current = Some(tuple);
        group.add(products[index].probability()?);
        if products[index].advance() {
            heap.push(Reverse((products[index].tuple(), index)));
        }
    }
    if let Some(tuple) = current {
        let p = group.get();
        positive(p, "summed joint probability")?;
        emit(tuple, p)?;
        total.add(p);
        rows = rows.checked_add(1).ok_or("row count overflow")?;
    }
    Ok((rows, total.get()))
}
#[derive(Clone, Copy)]
struct Limits { rows: u64, bytes: u64 }
fn add_bytes(current: u64, extra: u64, limits: Limits) -> Result<u64, String> {
    let next = current.checked_add(extra).ok_or("export byte count overflow")?;
    if next > limits.bytes { return Err("export exceeds max-bytes; result inconclusive".into()); }
    Ok(next)
}
fn checked_row_count(current: u64, limits: Limits) -> Result<u64, String> {
    let next = current.checked_add(1).ok_or("export row count overflow")?;
    if next > limits.rows { return Err("export exceeds max-rows; result inconclusive".into()); }
    Ok(next)
}
fn create(path: &Path) -> Result<File, String> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| format!("create {}: {e}", path.display()))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = create(path)?;
    file.write_all(bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    file.sync_all().map_err(|e| format!("sync {}: {e}", path.display()))
}
fn row_bytes(id: u32, tuple: HpTuple, p: f64) -> [u8; ROW_BYTES as usize] {
    let mut bytes = [0; ROW_BYTES as usize];
    bytes[..4].copy_from_slice(&id.to_le_bytes());
    for (i, hp) in tuple.into_iter().enumerate() {
        bytes[4 + 2*i..6 + 2*i].copy_from_slice(&hp.to_le_bytes());
    }
    bytes[4 + 2*UNITS..].copy_from_slice(&p.to_le_bytes());
    bytes
}
fn export(mut prepared: Prepared, description: &Value, output: &Path, limits: Limits,
          kernel_ns: u128, prepare_ns: u128) -> Result<Value, String>
{
    // Existing output is never overwritten, even when a previous run was incomplete.
    std::fs::create_dir(output).map_err(|e| format!("create output directory: {e}"))?;
    let started = Instant::now();
    let entries: Vec<_> = prepared.buckets.iter().map(|b| &b.text).collect();
    let dictionary = description_bytes(&json!({
        "schema":1,"kind":"full-non-hp-state-suspension-dictionary",
        "party_lengths":[PARTY_SIZE,PARTY_SIZE],"entries":entries
    }))?;
    let plan = description_bytes(description)?;
    let mut output_bytes = add_bytes(0, dictionary.len() as u64, limits)?;
    output_bytes = add_bytes(output_bytes, plan.len() as u64, limits)?;
    write_new(&output.join("dictionary.json"), &dictionary)?;
    write_new(&output.join("selection.json"), &plan)?;
    let mut file = BufWriter::new(create(&output.join("joint.bin"))?);
    let mut rows = 0u64;
    let mut total = Sum::default();
    for (id, bucket) in prepared.buckets.iter_mut().enumerate() {
        let (count, mass) = merge_products(&mut bucket.products, |tuple, p| {
            rows = checked_row_count(rows, limits)?;
            output_bytes = add_bytes(output_bytes, ROW_BYTES, limits)?;
            file.write_all(&row_bytes(id as u32, tuple, p)).map_err(|e| e.to_string())
        })?;
        if count == 0 { return Err("empty dictionary support".into()); }
        total.add(mass);
    }
    unit_mass(total.get(), "emitted joint probabilities")?;
    if (total.get() - prepared.component_mass).abs() > EPS {
        return Err("emitted mass differs from component mass".into());
    }
    if rows > prepared.flat_count_upper_bound { return Err("unique support exceeds flat upper bound".into()); }
    file.flush().map_err(|e| e.to_string())?;
    file.get_ref().sync_all().map_err(|e| e.to_string())?;
    let manifest = json!({
        "schema":1,"kind":"exact-joint-hp-export","status":"complete",
        "method":"unmodified public enumerate_turn_factored_with Full max_support=None",
        "metric_schema":"full State plus full Suspension; only all current Pokemon.hp values projected and restored by joint tuple",
        "dictionary":"dictionary.json","joint":"joint.bin","selection":"selection.json",
        "party_lengths":[PARTY_SIZE,PARTY_SIZE],"hp_unit_count":UNITS,
        "hp_unit_order":"SideId::One party index ascending, then SideId::Two party index ascending; all members including inactive, fainted and empty",
        "record_bytes":ROW_BYTES,"record_layout":"little-endian u32 dictionary_id; hp_unit_count signed i16 HP; IEEE754 binary64 probability; no header or padding",
        "record_order":"dictionary_id ascending then signed HP tuple lexicographic; duplicate tuples summed",
        "probability_policy":"raw probabilities, no renormalization, pruning, clamping, or zero dropping; positive finite; factor/component/joint mass within 1e-9 of one; Neumaier sums",
        "dictionary_entries":prepared.buckets.len(),"components":prepared.components,
        "suspended_components":prepared.suspended_components,
        "unique_joint_rows":rows,"flat_count_upper_bound":prepared.flat_count_upper_bound,
        "component_mass":prepared.component_mass,"joint_mass":total.get(),"tv_bound":0.0,
        "joint_bytes":rows*ROW_BYTES,"dictionary_bytes":dictionary.len(),"selection_bytes":plan.len(),
        "payload_bytes_excluding_manifest":output_bytes,
        "limits":{"max_rows":limits.rows,"max_bytes_including_manifest":limits.bytes},
        "all_state_restored":true,"all_lazy_tags_clear":true,"actual_eq_hash_dictionary":true,
        "debug_injective_on_observed_keys":true,
        "suspension_scope":"first mid-turn pause, no automatic resume; complete pending state included in key",
        "kernel_ns":kernel_ns,"prepare_ns":prepare_ns,"export_ns_before_manifest":started.elapsed().as_nanos(),
        "timing_scope":"kernel_ns covers only the one enumeration API call; exporter is accuracy validation, not a paired performance measurement"
    });
    let bytes = description_bytes(&manifest)?;
    add_bytes(output_bytes, bytes.len() as u64, limits)?;
    // A valid manifest is emitted last. The coordinator additionally requires child exit 0.
    write_new(&output.join("manifest.json"), &bytes)?;
    Ok(manifest)
}

struct Args {
    scenario: PathBuf, joint_seed: u64, describe_only: bool,
    plan: Option<PathBuf>, output: Option<PathBuf>, limits: Limits,
}
fn args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let scenario = PathBuf::from(args.next().ok_or("scenario path required")?);
    let mut seed = None;
    let mut describe_only = false;
    let mut plan = None;
    let mut output = None;
    let mut max_rows = None;
    let mut max_bytes = None;
    let mut seen = HashSet::new();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.clone()) { return Err(format!("duplicate flag {flag}")); }
        match flag.as_str() {
            "--describe" => describe_only = true,
            "--joint-seed" => seed = Some(args.next().ok_or("seed missing")?
                .parse().map_err(|_| "bad u64 joint seed")?),
            "--plan" => plan = Some(PathBuf::from(args.next().ok_or("plan missing")?)),
            "--output-dir" => output = Some(PathBuf::from(args.next().ok_or("output dir missing")?)),
            "--max-rows" => max_rows = Some(args.next().ok_or("row limit missing")?
                .parse::<u64>().map_err(|_| "bad u64 row limit")?),
            "--max-bytes" => max_bytes = Some(args.next().ok_or("byte limit missing")?
                .parse::<u64>().map_err(|_| "bad u64 byte limit")?),
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    if describe_only {
        if plan.is_some() || output.is_some() || max_rows.is_some() || max_bytes.is_some() {
            return Err("describe does not accept export arguments".into());
        }
    } else if plan.is_none() || output.is_none() {
        return Err("export requires --plan and --output-dir".into());
    }
    let limits = Limits { rows: max_rows.unwrap_or(DEFAULT_MAX_ROWS),
                          bytes: max_bytes.unwrap_or(DEFAULT_MAX_BYTES) };
    if limits.rows == 0 || limits.bytes == 0 { return Err("limits must be positive".into()); }
    Ok(Args { scenario, joint_seed: seed.ok_or("joint seed required")?,
              describe_only, plan, output, limits })
}
fn run() -> Result<Value, String> {
    let args = args()?;
    let loaded = load_scenario_file(&args.scenario).map_err(|e| format!("scenario load: {e}"))?;
    let selection = describe(&loaded, args.joint_seed)?;
    if args.describe_only { return Ok(selection.description); }
    check_plan(&selection.description, args.plan.as_ref().unwrap())?;
    if args.output.as_ref().unwrap().exists() { return Err("output directory already exists".into()); }
    plain_hp(&selection.state)?;
    let mut work = selection.state.clone();
    let started = Instant::now();
    let generated = enumerate_turn_factored_with(&mut work, Ruleset::CHAMPIONS_MC,
        selection.choices, FactoredOptions { rolls: RollMode::Full, max_support: None })
        .map_err(|e| format!("exact enumeration: {e}"))?;
    let kernel_ns = started.elapsed().as_nanos();
    restored(&work, &selection.state, "exact enumeration")?;
    if generated.tv_bound != 0.0 { return Err("nonzero/nonfinite TV bound".into()); }
    let started = Instant::now();
    let prepared = prepare(&selection.state, generated.outcomes, args.limits.bytes)?;
    let prepare_ns = started.elapsed().as_nanos();
    export(prepared, &selection.description, args.output.as_ref().unwrap(),
           args.limits, kernel_ns, prepare_ns)
}
fn main() -> std::process::ExitCode {
    let (value, status) = match run() {
        Ok(value) => (value, std::process::ExitCode::SUCCESS),
        Err(error) => (json!({"schema":1,"status":"error","error":error,"result":"inconclusive"}),
                       std::process::ExitCode::FAILURE),
    };
    let mut out = std::io::stdout().lock();
    match description_bytes(&value).and_then(|bytes| out.write_all(&bytes).map_err(|e| e.to_string())) {
        Ok(()) => status,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lab_engine::{instruction::Instruction, state::PokemonRef};

    fn state() -> State<2> {
        let mut state = State::default();
        for side in &mut state.sides { for mon in &mut side.party {
            mon.hp = 10;
            mon.max_hp = 100;
        }}
        state
    }
    fn mon(side: SideId, party: u8) -> PokemonRef { PokemonRef { side, party } }
    fn outcome(weight: f64, hp: Vec<(PokemonRef, Vec<(i16, f64)>)>) -> FactoredOutcome {
        FactoredOutcome { probability: weight, hp, instructions: vec![], suspension: None }
    }
    fn collect(mut prepared: Prepared) -> HashMap<String, f64> {
        let mut out = HashMap::new();
        for bucket in &mut prepared.buckets {
            let text = bucket.text.clone();
            let mut previous = None;
            merge_products(&mut bucket.products, |tuple, p| {
                assert!(previous.is_none_or(|last| last < tuple));
                previous = Some(tuple);
                assert!(out.insert(format!("{text}\n{tuple:?}"), p).is_none());
                Ok(())
            }).unwrap();
        }
        out
    }
    #[test]
    fn joint_export_overlap_matches_independent_flat_expansion() {
        let original = state();
        let outcomes = vec![
            outcome(0.4, vec![(mon(SideId::One, 0), vec![(10,0.25),(20,0.75)]),
                              (mon(SideId::Two, 5), vec![(10,0.5),(30,0.5)])]),
            outcome(0.6, vec![(mon(SideId::One, 0), vec![(10,0.5),(20,0.5)])]),
        ];
        // Independent public expand() path is used only for this tiny synthetic selftest.
        let mut flat: HashMap<Key,f64> = HashMap::new();
        for component in &outcomes { for branch in component.expand() {
            let mut work = original.clone();
            work.apply(&branch.instructions);
            *flat.entry((work.clone(), branch.suspension)).or_default() += branch.probability;
            work.reverse(&branch.instructions);
            restored(&work, &original, "selftest flat instructions").unwrap();
        }}
        let expected: HashMap<String,f64> = flat.into_iter().map(|(key,p)| {
            (format!("{}\n{:?}",full_debug(&project(&key)),plain_hp(&key.0).unwrap()),p)
        }).collect();
        let actual = collect(prepare(&original,outcomes,DEFAULT_MAX_BYTES).unwrap());
        assert_eq!(actual.len(),4);
        assert_eq!(actual.len(),expected.len());
        for (key,p) in expected { assert!((actual[&key]-p).abs()<1e-14); }
    }
    #[test]
    fn joint_export_preserves_hp_correlation_and_last_reserve() {
        let original = state();
        let mut second = outcome(0.5,vec![]);
        second.instructions = vec![
            Instruction::Heal { target:mon(SideId::One,0),amount:10 },
            Instruction::Heal { target:mon(SideId::Two,5),amount:10 },
        ];
        let actual = collect(prepare(&original,vec![outcome(0.5,vec![]),second],
                                     DEFAULT_MAX_BYTES).unwrap());
        assert_eq!(actual.len(),2);
        let mut mixed = [10;UNITS];
        mixed[0]=20;
        let projected = full_debug(&project(&(original,None)));
        assert!(!actual.contains_key(&format!("{projected}\n{mixed:?}")));
        mixed[UNITS-1]=20;
        assert_eq!(actual[&format!("{projected}\n{mixed:?}")],0.5);
    }
    #[test]
    fn joint_export_retains_hidden_non_hp_fields_and_dictionary_order() {
        let a = state();
        let mut b = a.clone();
        b.last_move = lab_engine::dex::moves::COPYCAT;
        assert_ne!(full_debug(&project(&(a.clone(),None))),full_debug(&project(&(b,None))));
        let mut changed = outcome(0.5,vec![]);
        // Turn increment is enough to create a second full-state dictionary bucket.
        changed.instructions.push(Instruction::SetTurn { old:a.turn,new:a.turn+1 });
        let prepared = prepare(&a,vec![changed,outcome(0.5,vec![])],DEFAULT_MAX_BYTES).unwrap();
        assert_eq!(prepared.buckets.len(),2);
        assert!(prepared.buckets[0].text < prepared.buckets[1].text);
    }
    #[test]
    fn joint_export_rejects_bad_factors_and_mass() {
        let s = state();
        let r = mon(SideId::One,0);
        let cases = vec![
            vec![outcome(1.0,vec![(r,vec![])])],
            vec![outcome(1.0,vec![(r,vec![(10,0.5),(10,0.5)])])],
            vec![outcome(1.0,vec![(r,vec![(11,1.0)])])],
            vec![outcome(1.0,vec![(r,vec![(10,0.5)])])],
            vec![outcome(1.0,vec![(r,vec![(10,0.5),(101,0.5)])])],
            vec![outcome(1.0,vec![(r,vec![(10,1.0)]),(r,vec![(10,1.0)])])],
            vec![outcome(1.0,vec![(mon(SideId::Two,6),vec![(10,1.0)])])],
            vec![outcome(1.0,vec![(r,vec![(10,f64::NAN)])])],
            vec![outcome(1.0,vec![(r,vec![(10,0.0),(20,1.0)])])],
            vec![outcome(0.5,vec![])],
            vec![outcome(f64::INFINITY,vec![])],
        ];
        for outcomes in cases { assert!(prepare(&s,outcomes,DEFAULT_MAX_BYTES).is_err()); }
        assert!(prepare(&s,vec![],DEFAULT_MAX_BYTES).is_err());
    }
    #[test]
    fn joint_export_row_encoding_and_limits_are_explicit() {
        assert_eq!(UNITS,12);
        assert_eq!(ROW_BYTES,36);
        let mut tuple = [0;UNITS]; tuple[0]=1; tuple[UNITS-1]=300;
        let bytes = row_bytes(0x01020304,tuple,0.25);
        assert_eq!(&bytes[0..4],&[4,3,2,1]);
        assert_eq!(i16::from_le_bytes(bytes[26..28].try_into().unwrap()),300);
        assert_eq!(f64::from_le_bytes(bytes[28..36].try_into().unwrap()),0.25);
        let limits = Limits { rows:1,bytes:36 };
        assert_eq!(checked_row_count(0,limits).unwrap(),1);
        assert!(checked_row_count(1,limits).is_err());
        assert_eq!(add_bytes(0,36,limits).unwrap(),36);
        assert!(add_bytes(36,1,limits).is_err());
    }
    #[test]
    fn joint_export_plan_comparison_requires_exact_bytes() {
        let plan = json!({"example":1});
        let exact = description_bytes(&plan).unwrap();
        check_plan_bytes(&plan,&exact).unwrap();
        assert!(check_plan_bytes(&plan,&exact[..exact.len()-1]).is_err());
    }
}

