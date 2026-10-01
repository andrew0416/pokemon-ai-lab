//! The factored enumeration of a turn (WORKPLAN P1b; DESIGN.md "Full 모드의 HP 인수분해").
//!
//! [`super::enumerate_stages`] keeps one state per distinct position after every stage. With
//! the 16 damage rolls independent per target, a turn with two spread moves reaches millions of
//! positions that differ only in HP. Here a position is instead a *product component*: the state
//! without the HP of the living party members (the key), and for each of them an independent HP
//! distribution. A stage runs once per component, with the members that have several possible HP
//! values as lazy units (`lazy.rs`): the run is valid for every combination of their values as
//! long as it reads their HP only through thresholds on which all values agree and changes it
//! only by moving all values together; otherwise it asks to split (or expand) a unit and the
//! component runs again in parts. After the stage the results with the same key are compacted:
//! components that differ in one unit only merge into one with that unit's mixture.
//!
//! The result is exact up to floating-point rounding: compaction takes two HP distributions for
//! equal when every probability agrees to [`CLOSE`] (relative), far below the parity tests'
//! tolerance.

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::OnceLock;

use crate::instruction::{Instruction, Outcome};
use crate::state::{LazyTag, PokemonRef, SideId, State, PARTY_SIZE};

use super::battle::{Battle, RunBuffers, RunStart};
use super::branch::Chooser;
use super::lazy::{self, Request, MAX_UNITS};
use super::merge::Merger;
use super::{EnumerateOptions, StageEnd, Suspension, TurnError};

/// Relative tolerance under which compaction takes two probabilities for equal.
const CLOSE: f64 = 1e-12;

/// The HP a key state holds for a living party member (its real HP is in the component).
const MASK: i16 = i16::MAX;

// ---- switching the flat entry points to this enumeration ---------------------------------------

thread_local! {
    static OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether [`super::enumerate_stages`] (every exact entry point: `enumerate_turn_with`,
/// `resume_turn_with`, `enumerate_start`, `enumerate_replacements`) runs the factored enumeration
/// and expands its result: the thread's [`FactoredScope`] if one is open, else the environment
/// variable `LAB_ENGINE_FACTORED` (set and not `0`). The result is the same distribution either
/// way; this is how the parity fixtures check the factored path.
pub(crate) fn factored_active() -> bool {
    OVERRIDE.with(Cell::get).unwrap_or_else(|| {
        static ENV: OnceLock<bool> = OnceLock::new();
        *ENV.get_or_init(|| std::env::var_os("LAB_ENGINE_FACTORED").is_some_and(|v| v != "0"))
    })
}

/// Makes this thread's exact enumerations factored (`true`) or flat (`false`) until dropped
/// ([`factored_active`]).
pub struct FactoredScope {
    previous: Option<bool>,
}

impl FactoredScope {
    pub fn new(factored: bool) -> FactoredScope {
        FactoredScope {
            previous: OVERRIDE.with(|o| o.replace(Some(factored))),
        }
    }
}

impl Drop for FactoredScope {
    fn drop(&mut self) {
        OVERRIDE.with(|o| o.set(self.previous));
    }
}

// ---- distributions ----------------------------------------------------------------------------

/// One party member's HP distribution relative to its smallest value: `(offset, probability)`
/// with offsets ascending from 0 and probabilities summing to 1.
#[derive(Clone, Debug)]
struct Dist {
    points: Vec<(i16, f64)>,
}

impl Dist {
    fn span(&self) -> i16 {
        self.points.last().expect("non-empty").0
    }

    /// The distribution of `(value, weight)` pairs (any order, repeats add up), as the smallest
    /// value and the normalized distribution above it; `None` for no weight.
    fn from_weights(mut weights: Vec<(i16, f64)>) -> (i16, Dist) {
        weights.sort_by_key(|&(v, _)| v);
        let mut points: Vec<(i16, f64)> = Vec::with_capacity(weights.len());
        for (v, w) in weights {
            match points.last_mut() {
                Some(last) if last.0 == v => last.1 += w,
                _ => points.push((v, w)),
            }
        }
        let total: f64 = points.iter().map(|&(_, w)| w).sum();
        let base = points[0].0;
        for point in &mut points {
            point.0 -= base;
            point.1 /= total;
        }
        (base, Dist { points })
    }
}

thread_local! {
    static POINT: Rc<Dist> = Rc::new(Dist { points: vec![(0, 1.0)] });
}

fn point() -> Rc<Dist> {
    POINT.with(Rc::clone)
}

/// One party member's HP in a component: `base + offset` for the points of `dist`.
#[derive(Clone, Debug)]
struct UnitHp {
    base: i16,
    dist: Rc<Dist>,
}

impl UnitHp {
    /// The same distribution (probabilities within [`CLOSE`]).
    fn close(&self, other: &UnitHp) -> bool {
        self.base == other.base
            && (Rc::ptr_eq(&self.dist, &other.dist)
                || (self.dist.points.len() == other.dist.points.len()
                    && self
                        .dist
                        .points
                        .iter()
                        .zip(&other.dist.points)
                        .all(|(&(a, p), &(b, q))| a == b && (p - q).abs() <= CLOSE * p.max(q))))
    }

    fn hash_support(&self, h: &mut KeyHasher) {
        h.write_i16(self.base);
        h.write_usize(self.dist.points.len());
        for &(offset, _) in &self.dist.points {
            h.write_i16(offset);
        }
    }
}

/// A product component: `weight` times the product of the units' HP distributions (in the order
/// of the entry's `units`).
#[derive(Clone, Debug)]
struct Component {
    weight: f64,
    hps: Vec<UnitHp>,
}

fn unit_ref(unit: u8) -> PokemonRef {
    let unit = usize::from(unit);
    PokemonRef {
        side: if unit < PARTY_SIZE {
            SideId::One
        } else {
            SideId::Two
        },
        party: (unit % PARTY_SIZE) as u8,
    }
}

fn unit_index(pokemon: PokemonRef) -> u8 {
    (pokemon.side.index() * PARTY_SIZE + usize::from(pokemon.party)) as u8
}

/// The living party members of `state` in unit order.
fn living<const N: usize>(state: &State<N>) -> impl Iterator<Item = PokemonRef> + '_ {
    (0..MAX_UNITS as u8)
        .map(unit_ref)
        .filter(|&p| state.pokemon(p).hp > 0)
}

// ---- positions keyed without HP ---------------------------------------------------------------

/// FxHash-style mixing (as `merge.rs`), finished with the MurmurHash3 64-bit mixer.
#[derive(Default)]
struct KeyHasher(u64);

impl KeyHasher {
    const SEED: u64 = 0x517c_c1b7_2722_0a95;

    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(Self::SEED);
    }
}

impl Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    fn finish(&self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^ (h >> 33)
    }
}

/// The positions reached by a stage: key state (every living member's HP masked) and remaining
/// work, with the product components reached there.
struct Entry<const N: usize, Q> {
    key: State<N>,
    rest: Q,
    /// The living party members (unit indices), the order of each component's `hps`.
    units: Vec<u8>,
    components: Vec<Component>,
}

struct Positions<const N: usize, Q> {
    entries: Vec<Entry<N, Q>>,
    index: HashMap<u64, Vec<u32>>,
}

impl<const N: usize, Q: Hash + Eq + Clone> Positions<N, Q> {
    fn new() -> Self {
        Positions {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// The entry of `key` (a state with the HP of its living members masked) and `rest`.
    fn entry(&mut self, key: &State<N>, rest: &Q, units: &[u8]) -> u32 {
        let mut hasher = KeyHasher::default();
        key.hash(&mut hasher);
        rest.hash(&mut hasher);
        let ids = self.index.entry(hasher.finish()).or_default();
        for &id in ids.iter() {
            let entry = &self.entries[id as usize];
            if entry.key == *key && entry.rest == *rest {
                return id;
            }
        }
        let id = u32::try_from(self.entries.len()).expect("fewer than 2^32 positions");
        ids.push(id);
        let mut key = key.clone();
        for side in &mut key.sides {
            for mon in &mut side.party {
                mon.lazy = LazyTag::default();
            }
        }
        self.entries.push(Entry {
            key,
            rest: rest.clone(),
            units: units.to_vec(),
            components: Vec::new(),
        });
        id
    }

    fn components(&self) -> usize {
        self.entries.iter().map(|e| e.components.len()).sum()
    }
}

// ---- compaction -------------------------------------------------------------------------------

/// Merges components that differ in one unit only (that unit's distribution becomes their
/// weighted mixture), for every unit, until nothing merges.
fn compact(mut components: Vec<Component>) -> Vec<Component> {
    let units = components.first().map_or(0, |c| c.hps.len());
    loop {
        let before = components.len();
        for unit in 0..units {
            if components.len() < 2 {
                return components;
            }
            components = merge_along(components, unit);
        }
        if components.len() == before {
            return components;
        }
    }
}

fn merge_along(components: Vec<Component>, unit: usize) -> Vec<Component> {
    // Clusters of components whose other units agree, in first-reached order.
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut index: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, c) in components.iter().enumerate() {
        let mut h = KeyHasher::default();
        for (u, hp) in c.hps.iter().enumerate() {
            if u != unit {
                hp.hash_support(&mut h);
            }
        }
        let candidates = index.entry(h.finish()).or_default();
        let found = candidates.iter().copied().find(|&k| {
            let head = &components[clusters[k][0]];
            head.hps
                .iter()
                .zip(&c.hps)
                .enumerate()
                .all(|(u, (a, b))| u == unit || a.close(b))
        });
        match found {
            Some(k) => clusters[k].push(i),
            None => {
                candidates.push(clusters.len());
                clusters.push(vec![i]);
            }
        }
    }
    if clusters.len() == components.len() {
        return components;
    }
    let mut out = Vec::with_capacity(clusters.len());
    for cluster in clusters {
        let head = &components[cluster[0]];
        if cluster.len() == 1 {
            out.push(head.clone());
            continue;
        }
        let mut weight = 0.0;
        let mut weights = Vec::new();
        for &i in &cluster {
            let c = &components[i];
            weight += c.weight;
            let hp = &c.hps[unit];
            weights.extend(
                hp.dist
                    .points
                    .iter()
                    .map(|&(offset, p)| (hp.base + offset, c.weight * p)),
            );
        }
        let (base, dist) = Dist::from_weights(weights);
        let mut hps = head.hps.clone();
        hps[unit] = UnitHp {
            base,
            dist: if dist.points.len() == 1 {
                point()
            } else {
                Rc::new(dist)
            },
        };
        out.push(Component { weight, hps });
    }
    out
}

// ---- the enumeration --------------------------------------------------------------------------

/// A component to run a stage from: `state` holds the smallest value of each lazy unit.
struct Group<const N: usize, P> {
    state: State<N>,
    pending: P,
    weight: f64,
    lazy: Vec<(u8, Rc<Dist>)>,
}

/// A final position of the factored enumeration: `state` (each listed member at its smallest HP
/// value), the remaining work if the turn suspended there, the probability, and the members with
/// several HP values: `(member, [(hp, probability)])`, independent of each other.
pub(crate) struct FactoredEnding<const N: usize, P> {
    pub state: State<N>,
    pub pending: Option<P>,
    pub probability: f64,
    pub hp: Vec<(PokemonRef, Vec<(i16, f64)>)>,
}

/// Counters of one enumeration (`LAB_ENGINE_STATS`).
#[derive(Default)]
struct Stats {
    runs: usize,
    groups: usize,
    splits: usize,
    expansions: usize,
}

/// Clears the lazy tags and the thread-local run table however a group ends.
struct GroupGuard;

impl Drop for GroupGuard {
    fn drop(&mut self) {
        lazy::end();
    }
}

fn clear_tags<const N: usize>(state: &mut State<N>, lazy: &[(u8, Rc<Dist>)]) {
    for &(unit, _) in lazy {
        state.pokemon_mut(unit_ref(unit)).lazy = LazyTag::default();
    }
}

/// The approximation of [`FactoredOptions::max_support`] (WORKPLAN P1c): a member's HP
/// distribution with more values than `max_support` keeps its `max_support` most probable values,
/// and each other value's probability moves to the nearest kept value (the lower one on a tie).
/// The moved probability, times the component's weight, bounds the total variation distance this
/// adds to the turn's outcome distribution (for a product, the distance is at most the sum over
/// its factors; later stages cannot increase it), so the sum over every step is a bound for the
/// whole enumeration.
struct Approx {
    max_support: Option<usize>,
    tv_bound: f64,
}

impl Approx {
    fn cap(&mut self, component: &mut Component) {
        let Some(k) = self.max_support else {
            return;
        };
        let k = k.max(1);
        for hp in &mut component.hps {
            if hp.dist.points.len() <= k {
                continue;
            }
            let mut order: Vec<usize> = (0..hp.dist.points.len()).collect();
            // Most probable first; ties to the lower value (a stable sort keeps index order).
            order.sort_by(|&a, &b| hp.dist.points[b].1.total_cmp(&hp.dist.points[a].1));
            let mut kept: Vec<usize> = order[..k].to_vec();
            kept.sort_unstable();
            let mut moved = 0.0;
            let mut weights: Vec<(i16, f64)> = Vec::with_capacity(hp.dist.points.len());
            for (i, &(offset, p)) in hp.dist.points.iter().enumerate() {
                let nearest = kept
                    .iter()
                    .map(|&j| hp.dist.points[j].0)
                    .min_by_key(|&o| ((o - offset).abs(), o))
                    .expect("k >= 1");
                if !kept.contains(&i) {
                    moved += p;
                }
                weights.push((hp.base + nearest, p));
            }
            let (base, dist) = Dist::from_weights(weights);
            *hp = UnitHp {
                base,
                dist: Rc::new(dist),
            };
            self.tv_bound += component.weight * moved;
        }
    }
}

/// Options of the factored enumeration ([`super::enumerate_turn_factored_with`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactoredOptions {
    /// Which damage rolls to branch on ([`EnumerateOptions::rolls`]).
    pub rolls: super::RollMode,
    /// At most this many HP values per member and component, the rest merged into the nearest
    /// kept value after every stage (P1c: approximate, with [`Factored::tv_bound`]); `None` is
    /// exact.
    pub max_support: Option<usize>,
}

/// The factored outcomes of a turn and the bound on their total variation distance from the
/// exact distribution (0 without [`FactoredOptions::max_support`], and for [`super::RollMode`]s
/// other than `Full` only relative to that mode's distribution).
#[derive(Clone, Debug)]
pub struct Factored {
    pub outcomes: Vec<FactoredOutcome>,
    pub tv_bound: f64,
}

/// [`super::enumerate_stages`] with factored positions; also returns the approximation's TV
/// bound.
pub(crate) fn enumerate_factored<const N: usize, P: Clone + Eq + Hash>(
    state: &State<N>,
    start: P,
    options: FactoredOptions,
    mut stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
) -> Result<(Vec<FactoredEnding<N, P>>, f64), TurnError> {
    #[cfg(feature = "experiment-factored-first-hit")]
    let first_hit_policy = super::first_hit::Policy::for_options(options);
    let mut approx = Approx {
        max_support: options.max_support,
        tv_bound: 0.0,
    };
    let options = EnumerateOptions {
        rolls: options.rolls,
    };
    let stats_on = std::env::var_os("LAB_ENGINE_STATS").is_some();
    let mut frontier = vec![Group {
        state: state.clone(),
        pending: start,
        weight: 1.0,
        lazy: Vec::new(),
    }];
    let mut finished: Positions<N, Option<P>> = Positions::new();
    let mut buffers = RunBuffers::default();
    while !frontier.is_empty() {
        let started = std::time::Instant::now();
        let mut stats = Stats::default();
        let mut next: Positions<N, P> = Positions::new();
        let mut stack: Vec<Group<N, P>> = frontier.into_iter().rev().collect();
        while let Some(group) = stack.pop() {
            stats.groups += 1;
            let parts = run_group(
                group,
                &mut next,
                &mut finished,
                options,
                &mut stage,
                &mut buffers,
                &mut stats,
                #[cfg(feature = "experiment-factored-first-hit")]
                first_hit_policy,
            )?;
            stack.extend(parts.into_iter().rev());
        }
        let reached = next.components();
        frontier = groups(next, &mut approx);
        if stats_on {
            eprintln!(
                "lab-engine: factored stage {} groups ({} splits, {} expansions), {} runs -> {} \
                 components -> {} compacted, {} finished; {:.1} ms",
                stats.groups,
                stats.splits,
                stats.expansions,
                stats.runs,
                reached,
                frontier.len(),
                finished.components(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    let mut out = Vec::new();
    for entry in finished.entries {
        for mut component in compact(entry.components) {
            approx.cap(&mut component);
            let mut end = entry.key.clone();
            let mut hp = Vec::new();
            for (&unit, unit_hp) in entry.units.iter().zip(&component.hps) {
                let pokemon = unit_ref(unit);
                end.pokemon_mut(pokemon).hp = unit_hp.base;
                if unit_hp.dist.points.len() > 1 {
                    hp.push((
                        pokemon,
                        unit_hp
                            .dist
                            .points
                            .iter()
                            .map(|&(offset, p)| (unit_hp.base + offset, p))
                            .collect(),
                    ));
                }
            }
            out.push(FactoredEnding {
                state: end,
                pending: entry.rest.clone(),
                probability: component.weight,
                hp,
            });
        }
    }
    Ok((out, approx.tv_bound))
}

/// The compacted (and capped) components of `positions` as the next stage's groups.
fn groups<const N: usize, P: Clone>(
    positions: Positions<N, P>,
    approx: &mut Approx,
) -> Vec<Group<N, P>> {
    let mut out = Vec::new();
    for entry in positions.entries {
        for mut component in compact(entry.components) {
            approx.cap(&mut component);
            let mut state = entry.key.clone();
            let mut lazy = Vec::new();
            for (&unit, hp) in entry.units.iter().zip(&component.hps) {
                state.pokemon_mut(unit_ref(unit)).hp = hp.base;
                if hp.dist.points.len() > 1 {
                    lazy.push((unit, hp.dist.clone()));
                }
            }
            out.push(Group {
                state,
                pending: entry.rest.clone(),
                weight: component.weight,
                lazy,
            });
        }
    }
    out
}

/// Runs one stage from `group`. Every run's end goes to `next` (the turn goes on) or `finished`;
/// if a run asks to split or expand a lazy unit, nothing is kept and the group's parts are
/// returned instead.
#[allow(clippy::too_many_arguments)]
fn run_group<const N: usize, P: Clone + Eq + Hash>(
    group: Group<N, P>,
    next: &mut Positions<N, P>,
    finished: &mut Positions<N, Option<P>>,
    options: EnumerateOptions,
    stage: &mut impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
    buffers: &mut RunBuffers,
    stats: &mut Stats,
    #[cfg(feature = "experiment-factored-first-hit")] first_hit_policy: super::first_hit::Policy,
) -> Result<Vec<Group<N, P>>, TurnError> {
    let Group {
        state: mut work,
        pending,
        weight,
        lazy,
    } = group;
    let spans: Vec<(usize, i16)> = lazy
        .iter()
        .map(|(unit, dist)| (usize::from(*unit), dist.span()))
        .collect();
    for &(unit, _) in &lazy {
        work.pokemon_mut(unit_ref(unit)).lazy = lazy::tag(usize::from(unit));
    }
    lazy::begin(&spans);
    let _guard = GroupGuard;
    let mut chooser = Chooser::with_rolls(options.rolls);
    let mut start: Option<RunStart> = None;
    let mut after = pending.clone();
    // (finished?, entry, component) of every run, kept once the whole group ran.
    let mut reached: Vec<(bool, u32, Component)> = Vec::new();
    let mut masked: Vec<(PokemonRef, i16, LazyTag)> = Vec::new();
    let mut units: Vec<u8> = Vec::new();
    loop {
        stats.runs += 1;
        chooser.begin_run();
        after.clone_from(&pending);
        let result = {
            let mut owned = std::mem::take(buffers);
            let mut b = match &start {
                Some(start) => Battle::replay(&mut work, &mut chooser, start, owned),
                None => {
                    owned.log.clear();
                    Battle::recycle(&mut work, &mut chooser, owned)
                }
            };
            if start.is_none() {
                start = Some(b.run_start());
            }
            #[cfg(feature = "experiment-factored-first-hit")]
            {
                b.first_hit_policy = first_hit_policy;
            }
            let result = stage(&mut b, &mut after);
            *buffers = b.into_buffers();
            result
        };
        let end = result?;
        if let Some((unit, request)) = lazy::take_request() {
            work.reverse(&buffers.log);
            clear_tags(&mut work, &lazy);
            match request {
                Request::Split { .. } => stats.splits += 1,
                Request::Expand => stats.expansions += 1,
            }
            return Ok(split(work, pending, weight, lazy, unit as u8, request));
        }
        let p = weight * chooser.probability();
        // The key: every living member's HP masked (and remembered for the component).
        masked.clear();
        units.clear();
        let alive: Vec<PokemonRef> = living(&work).collect();
        for pokemon in alive {
            let mon = work.pokemon_mut(pokemon);
            masked.push((pokemon, mon.hp, mon.lazy));
            units.push(unit_index(pokemon));
            mon.hp = MASK;
        }
        let (done, id) = match end {
            StageEnd::Continue => (false, next.entry(&work, &after, &units)),
            StageEnd::Finished | StageEnd::Suspended => {
                let kept = (end == StageEnd::Suspended).then(|| after.clone());
                (true, finished.entry(&work, &kept, &units))
            }
        };
        let mut hps = Vec::with_capacity(masked.len());
        for &(pokemon, hp, tag) in &masked {
            work.pokemon_mut(pokemon).hp = hp;
            let dist = if tag.0 != 0 {
                let unit = unit_index(pokemon);
                lazy.iter()
                    .find(|(u, _)| *u == unit)
                    .map(|(_, d)| d.clone())
                    .expect("a lazy unit of the group")
            } else {
                point()
            };
            hps.push(UnitHp { base: hp, dist });
        }
        reached.push((done, id, Component { weight: p, hps }));
        work.reverse(&buffers.log);
        #[cfg(feature = "experiment-lazy-ko-damage")]
        for &(unit, _) in &lazy {
            // A universally fainted unit shed its live tag. Instructions restore its HP;
            // the group restores its original tag before the next chooser replay.
            work.pokemon_mut(unit_ref(unit)).lazy = lazy::tag(usize::from(unit));
        }
        if !chooser.advance() {
            break;
        }
    }
    for (done, id, component) in reached {
        if done {
            finished.entries[id as usize].components.push(component);
        } else {
            next.entries[id as usize].components.push(component);
        }
    }
    Ok(Vec::new())
}

/// The parts of a group whose run asked `request` for lazy unit `unit`, in ascending HP order.
fn split<const N: usize, P: Clone>(
    state: State<N>,
    pending: P,
    weight: f64,
    lazy: Vec<(u8, Rc<Dist>)>,
    unit: u8,
    request: Request,
) -> Vec<Group<N, P>> {
    let position = lazy
        .iter()
        .position(|(u, _)| *u == unit)
        .expect("a lazy unit of the group");
    let dist = &lazy[position].1;
    let base = state.pokemon(unit_ref(unit)).hp;
    let parts: Vec<Vec<(i16, f64)>> = match request {
        Request::Split { at } => {
            let (low, high): (Vec<_>, Vec<_>) = dist.points.iter().partition(|&&(d, _)| d <= at);
            vec![low, high]
        }
        Request::Expand => dist.points.iter().map(|&point| vec![point]).collect(),
    };
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        if part.is_empty() {
            continue;
        }
        let mass: f64 = part.iter().map(|&(_, p)| p).sum();
        let (value, dist) = Dist::from_weights(part.iter().map(|&(d, p)| (base + d, p)).collect());
        let mut state = state.clone();
        state.pokemon_mut(unit_ref(unit)).hp = value;
        let mut lazy = lazy.clone();
        if dist.points.len() > 1 {
            lazy[position].1 = Rc::new(dist);
        } else {
            lazy.remove(position);
        }
        out.push(Group {
            state,
            pending: pending.clone(),
            weight: weight * mass,
            lazy,
        });
    }
    out
}

/// The factored enumeration expanded into flat endings (equal end states merged in first-reached
/// order), for the flat entry points under [`factored_active`].
pub(crate) fn enumerate_expanded<const N: usize, P: Clone + Eq + Hash>(
    state: &State<N>,
    start: P,
    options: EnumerateOptions,
    stage: impl FnMut(&mut Battle<'_, N>, &mut P) -> Result<StageEnd, TurnError>,
) -> Result<super::Endings<N, P>, TurnError> {
    let options = FactoredOptions {
        rolls: options.rolls,
        max_support: None,
    };
    let (factored, _) = enumerate_factored(state, start, options, stage)?;
    let mut merged: Merger<N, Option<P>> = Merger::new();
    for ending in factored {
        let mut end = ending.state;
        let mut counters = vec![0usize; ending.hp.len()];
        loop {
            let mut p = ending.probability;
            for (&(pokemon, ref values), &k) in ending.hp.iter().zip(&counters) {
                end.pokemon_mut(pokemon).hp = values[k].0;
                p *= values[k].1;
            }
            let hash = end.position_hash();
            merged.add(&end, hash, &ending.pending, p);
            // Odometer over the members' values.
            let mut i = 0;
            loop {
                if i == counters.len() {
                    break;
                }
                counters[i] += 1;
                if counters[i] < ending.hp[i].1.len() {
                    break;
                }
                counters[i] = 0;
                i += 1;
            }
            if i == counters.len() {
                break;
            }
        }
    }
    Ok(merged.into_chunks())
}

/// One factored outcome of a turn ([`super::enumerate_turn_factored`]): with probability
/// `probability`, the turn ends in the state `instructions` lead to from the starting state,
/// except that each member listed in `hp` has one of the listed HP values (the instructions set
/// the smallest), independently of the others. `suspension` as in [`Outcome`].
#[derive(Clone, Debug)]
pub struct FactoredOutcome {
    pub probability: f64,
    pub instructions: Vec<Instruction>,
    pub hp: Vec<(PokemonRef, Vec<(i16, f64)>)>,
    pub suspension: Option<Suspension>,
}

impl FactoredOutcome {
    /// How many flat outcomes this one stands for (the product of the listed members' value
    /// counts).
    pub fn flat_count(&self) -> f64 {
        self.hp
            .iter()
            .map(|(_, values)| values.len() as f64)
            .product()
    }

    /// The flat outcomes this one stands for: the instructions, then a heal from the smallest
    /// value up to each member's value.
    pub fn expand(&self) -> Vec<Outcome> {
        let mut out = Vec::new();
        let mut counters = vec![0usize; self.hp.len()];
        loop {
            let mut instructions = self.instructions.clone();
            let mut p = self.probability;
            for (&(pokemon, ref values), &k) in self.hp.iter().zip(&counters) {
                let above = values[k].0 - values[0].0;
                if above > 0 {
                    instructions.push(Instruction::Heal {
                        target: pokemon,
                        amount: above,
                    });
                }
                p *= values[k].1;
            }
            out.push(Outcome {
                probability: p,
                instructions,
                suspension: self.suspension.clone(),
            });
            let mut i = 0;
            while i < counters.len() {
                counters[i] += 1;
                if counters[i] < self.hp[i].1.len() {
                    break;
                }
                counters[i] = 0;
                i += 1;
            }
            if i == counters.len() {
                return out;
            }
        }
    }
}

/// The factored endings as outcomes from `start`.
pub(crate) fn factored_outcomes<const N: usize, P>(
    start: &State<N>,
    endings: Vec<FactoredEnding<N, P>>,
    suspend: impl Fn(P) -> Suspension,
) -> Vec<FactoredOutcome> {
    endings
        .into_iter()
        .map(|ending| FactoredOutcome {
            probability: ending.probability,
            instructions: super::diff::instructions(start, &ending.state),
            hp: ending.hp,
            suspension: ending.pending.map(&suspend),
        })
        .collect()
}

#[cfg(test)]
#[path = "frontier/lazy_ko_tests.rs"]
mod p1e_lazy_ko_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(base: i16, points: &[(i16, f64)]) -> UnitHp {
        UnitHp {
            base,
            dist: Rc::new(Dist {
                points: points.to_vec(),
            }),
        }
    }

    /// Independent rolls on two units compact to one product component; a coupled pair stays.
    #[test]
    fn compaction_finds_products() {
        let a = [(10i16, 0.25), (12, 0.75)];
        let b = [(40i16, 0.5), (41, 0.3), (45, 0.2)];
        let mut components = Vec::new();
        for &(x, p) in &a {
            for &(y, q) in &b {
                components.push(Component {
                    weight: 0.8 * p * q,
                    hps: vec![unit(x, &[(0, 1.0)]), unit(y, &[(0, 1.0)])],
                });
            }
        }
        let compacted = compact(components);
        assert_eq!(compacted.len(), 1);
        let c = &compacted[0];
        assert!((c.weight - 0.8).abs() < 1e-15);
        assert_eq!(c.hps[0].base, 10);
        assert_eq!(c.hps[0].dist.points.len(), 2);
        assert!((c.hps[0].dist.points[1].1 - 0.75).abs() < 1e-15);
        assert_eq!(c.hps[1].base, 40);
        assert_eq!(
            c.hps[1]
                .dist
                .points
                .iter()
                .map(|&(d, _)| d)
                .collect::<Vec<_>>(),
            vec![0, 1, 5]
        );
        // Coupled: (10, 40) and (12, 41) only.
        let coupled = vec![
            Component {
                weight: 0.5,
                hps: vec![unit(10, &[(0, 1.0)]), unit(40, &[(0, 1.0)])],
            },
            Component {
                weight: 0.5,
                hps: vec![unit(12, &[(0, 1.0)]), unit(41, &[(0, 1.0)])],
            },
        ];
        assert_eq!(compact(coupled).len(), 2);
    }

    #[test]
    fn expansion_lists_every_combination() {
        let outcome = FactoredOutcome {
            probability: 0.5,
            instructions: Vec::new(),
            hp: vec![
                (
                    PokemonRef {
                        side: SideId::One,
                        party: 0,
                    },
                    vec![(10, 0.5), (12, 0.5)],
                ),
                (
                    PokemonRef {
                        side: SideId::Two,
                        party: 1,
                    },
                    vec![(3, 0.25), (4, 0.25), (9, 0.5)],
                ),
            ],
            suspension: None,
        };
        assert_eq!(outcome.flat_count(), 6.0);
        let flat = outcome.expand();
        assert_eq!(flat.len(), 6);
        let total: f64 = flat.iter().map(|o| o.probability).sum();
        assert!((total - 0.5).abs() < 1e-15);
    }
}
