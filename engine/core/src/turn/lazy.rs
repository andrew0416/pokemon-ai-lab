//! Lazy HP units of the factored Full enumeration (WORKPLAN P1b; DESIGN.md "Full 모드의 HP
//! 인수분해").
//!
//! [`super::frontier`] runs a stage once for a whole group of positions that differ only in the HP
//! of some party members (the *lazy units*): the state holds each lazy unit's smallest value, the
//! [`Pokemon::lazy`] tag marks it, and a thread-local table holds the spread of its other values
//! (`span`: they are `hp + d` for offsets `0 <= d <= span`). The turn code reads HP only through
//! the accessors below:
//! - a threshold read ([`Pokemon::hp_le`] and the ones written with it) whose answer is the same
//!   for every value goes on; otherwise it asks to *split* the unit at the threshold;
//! - a read of the number itself ([`Pokemon::hp_value`]) asks to *expand* the unit (every value
//!   separately), unless it has one value;
//! - every HP change is an `Instruction::Damage` / `Heal`, which moves all values by the same
//!   amount; [`check_shift`] asks to expand when the move would take a value out of `1..=max_hp`.
//!
//! A run that asked is thrown away and the group is run again split as asked. A group whose runs
//! all finish without asking behaves the same for every combination of its units' values, so its
//! outcomes are exactly the product of the (shifted) unit distributions. Outside a factored
//! enumeration no tag is set and the accessors are plain reads.

use std::cell::RefCell;

use crate::state::{LazyTag, Pokemon, PARTY_SIZE};

/// Unit index of a party member: `side * PARTY_SIZE + party`.
pub(crate) const MAX_UNITS: usize = 2 * PARTY_SIZE;

#[cfg(feature = "experiment-lazy-ko-damage")]
const _: () = assert!(
    MAX_UNITS <= u16::BITS as usize,
    "damage dependency mask must cover every HP unit"
);

/// What a run could not treat alike for every value of a lazy unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// A threshold read: split the values into offsets `<= at` and `> at`.
    Split { at: i16 },
    /// The HP read as a number, or an HP change that does not fit every value: every value on
    /// its own.
    Expand,
}

#[derive(Default)]
struct Run {
    active: bool,
    /// Per unit index: the offset of the largest value (0 for a unit that is not lazy).
    spans: [i16; MAX_UNITS],
    /// The first request of the current run.
    request: Option<(usize, Request)>,
}

thread_local! {
    static RUN: RefCell<Run> = RefCell::new(Run::default());
}

/// Starts a group's runs: `spans` lists the lazy units (unit index, largest offset).
pub(crate) fn begin(spans: &[(usize, i16)]) {
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        r.active = true;
        r.spans = [0; MAX_UNITS];
        for &(unit, span) in spans {
            r.spans[unit] = span;
        }
        r.request = None;
    });
}

/// The request the current run made, if any (and clears it for the next run).
pub(crate) fn take_request() -> Option<(usize, Request)> {
    RUN.with(|r| r.borrow_mut().request.take())
}

/// Ends a group's runs.
pub(crate) fn end() {
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        r.active = false;
        r.request = None;
    });
}

/// The tag of lazy unit `unit`.
pub(crate) fn tag(unit: usize) -> LazyTag {
    LazyTag(u8::try_from(unit + 1).expect("few units"))
}

fn with_span(tag: LazyTag, f: impl FnOnce(i32) -> Option<Request>) {
    let unit = usize::from(tag.0 - 1);
    RUN.with(|r| {
        let mut r = r.borrow_mut();
        if !r.active {
            return;
        }
        let span = i32::from(r.spans[unit]);
        if span == 0 || r.request.is_some() {
            return;
        }
        if let Some(request) = f(span) {
            r.request = Some((unit, request));
        }
    });
}

/// `hp <= t` for a value `hp` of the unit tagged `tag` (the smallest; the others are up to the
/// unit's span above it).
#[inline]
fn le(hp: i16, tag: LazyTag, t: i32) -> bool {
    let hp = i32::from(hp);
    if tag.0 != 0 {
        with_span(tag, |span| {
            (hp <= t && t < hp + span).then(|| Request::Split {
                at: i16::try_from(t - hp).expect("offset within the span"),
            })
        });
    }
    hp <= t
}

#[inline]
fn value(hp: i16, tag: LazyTag) -> i16 {
    if tag.0 != 0 {
        with_span(tag, |_| Some(Request::Expand));
    }
    hp
}

/// Registered exact consumers of move-hit damage. New arithmetic/state sinks must choose a
/// consumer here and call `DealtDamage::exact`: the wrapper deliberately has no integer cast,
/// subtraction, ordering, or floating-point conversion. Positive/zero tests and nonnegative
/// sums alone do not observe a KO's varying previous HP.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ExactDamageConsumer {
    LegacyDamageReturn,
    Drain,
    Recoil,
    ShellBell,
    InnardsOut,
    Counter,
    LastDamagedBy,
    Berserk,
    EmergencyExit,
    PendingState,
}

impl ExactDamageConsumer {
    /// Exhaustive registration: adding a numeric consumer requires documenting what observes
    /// the value. This is metadata, not an optimization allowlist; exact always expands.
    pub const fn contract(self) -> &'static str {
        match self {
            Self::LegacyDamageReturn => "Public damage/direct_damage returns exact HP removed",
            Self::Drain => "Drain ratio turns exact dealt HP into healing or Liquid Ooze damage",
            Self::Recoil => "Dex recoil ratio turns exact move total into user damage",
            Self::ShellBell => "Effective Shell Bell heals from exact move total",
            Self::InnardsOut => "Fainted holder damages attacker using hit plus earlier total",
            Self::Counter => "Active Counter/Mirror Coat stores twice exact hit damage in state",
            Self::LastDamagedBy => "Registered history reader stores exact damage in state",
            Self::Berserk => "Living Berserk/Anger Shell holder compares pre-hit HP threshold",
            Self::EmergencyExit => "Living exit holder compares hurt history plus move damage",
            Self::PendingState => "Stage key and public suspension preserve exact numeric fields",
        }
    }
}

/// HP removed by a move, possibly dependent on the previous HP of fainted lazy units.
/// All values are nonnegative, and a dependent term is strictly positive. Dependencies are
/// retained until the run ends, even after an exact read has requested a discarded replay.
/// The default build carries only the exact integer; the experimental build adds a unit mask.
#[derive(Clone, Copy, Default)]
pub(crate) struct DealtDamage {
    representative: i32,
    #[cfg(feature = "experiment-lazy-ko-damage")]
    dependencies: u16,
}

impl std::fmt::Debug for DealtDamage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "experiment-lazy-ko-damage")]
        if self.dependencies != 0 {
            return f
                .debug_struct("UnresolvedDealtDamage")
                .field("representative", &self.representative)
                .field("dependencies", &self.dependencies)
                .finish();
        }
        std::fmt::Debug::fmt(&self.representative, f)
    }
}

impl DealtDamage {
    pub const ZERO: Self = Self::constant(0);

    pub const fn constant(value: i32) -> Self {
        assert!(value >= 0, "dealt damage is nonnegative");
        Self {
            representative: value,
            #[cfg(feature = "experiment-lazy-ko-damage")]
            dependencies: 0,
        }
    }

    pub fn is_positive(self) -> bool {
        self.representative > 0
    }

    pub fn is_zero(self) -> bool {
        !self.is_positive()
    }

    /// A real numeric observation: request expansion of every dependency (only the first
    /// pending request is accepted); the discarded representative run may finish normally.
    pub fn exact(self, consumer: ExactDamageConsumer) -> i32 {
        let _ = consumer.contract();
        #[cfg(feature = "experiment-lazy-ko-damage")]
        if self.dependencies != 0 {
            RUN.with(|r| {
                let r = r.borrow();
                assert!(r.active, "dependent damage escaped its lazy run");
                for unit in 0..MAX_UNITS {
                    if self.dependencies & (1 << unit) != 0 {
                        assert!(
                            r.spans[unit] > 0,
                            "dependent damage lost its original HP span"
                        );
                    }
                }
            });
        }
        #[cfg(feature = "experiment-lazy-ko-damage")]
        for unit in 0..MAX_UNITS {
            if self.dependencies & (1 << unit) != 0 {
                with_span(tag(unit), |_| Some(Request::Expand));
            }
        }
        self.representative
    }

    /// Before a stage's pending value is compared or hashed, force the old numeric behavior.
    /// This intentionally gives up collapse across suspensions. A request invalidates the run
    /// before frontier key lookup, so clearing this temporary copy's mask cannot hide a read.
    pub fn materialize(&mut self) {
        *self = Self::constant(self.exact(ExactDamageConsumer::PendingState));
    }

    fn assert_concrete(self) {
        #[cfg(feature = "experiment-lazy-ko-damage")]
        assert_eq!(
            self.dependencies, 0,
            "materialize damage before equality or hashing"
        );
    }
}

impl PartialEq for DealtDamage {
    fn eq(&self, other: &Self) -> bool {
        self.assert_concrete();
        other.assert_concrete();
        self.representative == other.representative
    }
}

impl Eq for DealtDamage {}

impl std::hash::Hash for DealtDamage {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.assert_concrete();
        std::hash::Hash::hash(&self.representative, state);
    }
}

impl std::ops::Add for DealtDamage {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            representative: self.representative + other.representative,
            #[cfg(feature = "experiment-lazy-ko-damage")]
            dependencies: self.dependencies | other.dependencies,
        }
    }
}

impl std::ops::AddAssign for DealtDamage {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl std::iter::Sum for DealtDamage {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |total, damage| total + damage)
    }
}

/// Capture a universally fainting lazy unit's previous HP without observing its number.
/// Return the representative instruction amount separately, solely for the collapse write.
/// A threshold-crossing group is never collapsed: its already requested split wins.
#[cfg(feature = "experiment-lazy-ko-damage")]
pub(crate) fn capture_ko(mon: &Pokemon, amount: i32) -> Option<(DealtDamage, i16)> {
    if mon.lazy.0 == 0 || !mon.is_alive() {
        return None;
    }
    let unit = usize::from(mon.lazy.0 - 1);
    RUN.with(|r| {
        let r = r.borrow();
        (r.active && r.spans[unit] > 0 && i32::from(mon.hp) + i32::from(r.spans[unit]) <= amount)
            .then_some((
                DealtDamage {
                    representative: i32::from(mon.hp),
                    dependencies: 1 << unit,
                },
                mon.hp,
            ))
    })
}

/// For [`super::battle::Battle::apply`]: an HP change of `delta` to `mon` (before it is applied)
/// must keep every value of a lazy unit in `1..=max_hp`, or the unit is expanded.
#[inline]
pub(crate) fn check_shift(mon: &Pokemon, delta: i32) {
    if mon.lazy.0 == 0 {
        return;
    }
    let (hp, max) = (i32::from(mon.hp) + delta, i32::from(mon.max_hp));
    with_span(mon.lazy, |span| {
        (hp < 1 || hp + span > max).then_some(Request::Expand)
    });
}

/// Lazy-aware HP reads (see the module documentation). Outside a factored enumeration these are
/// plain reads of `hp`.
impl Pokemon {
    /// The HP as a number, for arithmetic (damage from HP, HP-scaled power, Pain Split, ...).
    #[inline]
    pub fn hp_value(&self) -> i16 {
        value(self.hp, self.lazy)
    }

    /// `hp <= t`.
    #[inline]
    pub fn hp_le(&self, t: i32) -> bool {
        le(self.hp, self.lazy, t)
    }

    /// `k * hp <= bound` for `k > 0` (Showdown's `hp <= maxhp / 2` is `hp_scaled_le(2, maxhp)`).
    #[inline]
    pub fn hp_scaled_le(&self, k: i32, bound: i32) -> bool {
        self.hp_le(bound.div_euclid(k))
    }

    /// `hp >= max_hp`.
    #[inline]
    pub fn hp_full(&self) -> bool {
        !self.hp_le(i32::from(self.max_hp) - 1)
    }

    /// The HP now, to compare with a later HP (Emergency Exit's `pokemonOriginalHP`).
    #[inline]
    pub(crate) fn hp_mark(&self) -> HpMark {
        HpMark {
            hp: self.hp,
            lazy: self.lazy,
        }
    }
}

/// An earlier HP of a Pokémon ([`Pokemon::hp_mark`]), read with the same lazy-aware accessors (a
/// lazy unit's values all move together, so an earlier value is the earlier smallest one plus
/// the same offset).
#[derive(Clone, Copy, Debug)]
pub(crate) struct HpMark {
    hp: i16,
    lazy: LazyTag,
}

impl HpMark {
    /// `hp <= t`.
    pub fn hp_le(self, t: i32) -> bool {
        le(self.hp, self.lazy, t)
    }

    /// `k * hp <= bound` for `k > 0`.
    pub fn hp_scaled_le(self, k: i32, bound: i32) -> bool {
        self.hp_le(bound.div_euclid(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_reads_split_only_across_the_span() {
        let mut mon = Pokemon {
            hp: 50,
            max_hp: 100,
            lazy: tag(3),
            ..Pokemon::default()
        };
        begin(&[(3, 10)]);
        // Values 50..=60: below, above and at the ends are uniform.
        assert!(!mon.hp_le(49));
        assert!(mon.hp_le(60));
        assert!(!mon.hp_full());
        assert_eq!(take_request(), None);
        // 55 splits offsets 0..=5 from 6..=10.
        assert!(mon.hp_le(55));
        assert_eq!(take_request(), Some((3, Request::Split { at: 5 })));
        // Only the first request of a run counts.
        mon.hp_le(52);
        mon.hp_value();
        assert_eq!(take_request(), Some((3, Request::Split { at: 2 })));
        assert_eq!(mon.hp_value(), 50);
        assert_eq!(take_request(), Some((3, Request::Expand)));
        // A move that keeps every value in range is fine; one that does not expands.
        check_shift(&mon, -49);
        assert_eq!(take_request(), None);
        check_shift(&mon, -50);
        assert_eq!(take_request(), Some((3, Request::Expand)));
        check_shift(&mon, 41);
        assert_eq!(take_request(), Some((3, Request::Expand)));
        // A mark taken earlier reads with the same span.
        let mark = mon.hp_mark();
        mon.hp = 20;
        assert!(!mark.hp_scaled_le(4, 100));
        assert_eq!(take_request(), None);
        assert!(mark.hp_scaled_le(1, 55));
        assert_eq!(take_request(), Some((3, Request::Split { at: 5 })));
        end();
        // Outside a group nothing is asked.
        mon.lazy = LazyTag(0);
        assert!(mon.hp_le(20));
        assert_eq!(mon.hp_value(), 20);
        assert_eq!(take_request(), None);
    }

    /// The turn code reads `Pokemon::hp` only through the accessors above (the factored
    /// enumeration is exact only then): no `.hp` field access in `turn/` outside test modules,
    /// except in the files that work on whole positions outside a run.
    #[test]
    fn turn_code_reads_hp_through_the_accessors() {
        const OUTSIDE_RUNS: [&str; 3] = ["lazy.rs", "frontier.rs", "diff.rs"];
        fn visit(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("turn/ is readable") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    visit(&path, out);
                    continue;
                }
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with(".rs") || OUTSIDE_RUNS.contains(&name.as_str()) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("readable");
                for (i, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("#[cfg(test)]") {
                        break;
                    }
                    let code = line.split("//").next().unwrap_or("");
                    let bytes = code.as_bytes();
                    let mut from = 0;
                    while let Some(at) = code[from..].find(".hp") {
                        let end = from + at + 3;
                        let ident = bytes
                            .get(end)
                            .is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_');
                        if !ident {
                            out.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                        }
                        from = end;
                    }
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/turn");
        let mut found = Vec::new();
        visit(&dir, &mut found);
        assert!(
            found.is_empty(),
            "read HP with is_alive / hp_le / hp_scaled_le / hp_full / hp_value / hp_mark:
{}",
            found.join(
                "
"
            )
        );
    }
}

#[cfg(all(test, feature = "experiment-lazy-ko-damage"))]
mod p1e_dealt_damage_tests {
    use super::*;

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            end();
        }
    }

    fn mon(unit: usize, hp: i16) -> Pokemon {
        Pokemon {
            hp,
            max_hp: 100,
            lazy: tag(unit),
            ..Pokemon::default()
        }
    }

    #[test]
    fn positive_sums_retain_each_dependency_until_exactly_observed() {
        begin(&[(2, 10), (7, 30)]);
        let _guard = Guard;
        let a = capture_ko(&mon(2, 20), 100).unwrap().0;
        let b = capture_ko(&mon(7, 40), 100).unwrap().0;
        let total: DealtDamage = [a, b, DealtDamage::constant(3)].into_iter().sum();
        assert!(total.is_positive());
        assert!(!total.is_zero());
        assert_eq!(take_request(), None);
        assert_eq!(total.exact(ExactDamageConsumer::Recoil), 63);
        assert_eq!(take_request(), Some((2, Request::Expand)));
        // Exact reads cannot erase the original provenance on the throwaway run.
        assert_eq!(total.exact(ExactDamageConsumer::ShellBell), 63);
        assert_eq!(take_request(), Some((2, Request::Expand)));
        assert_eq!(b.exact(ExactDamageConsumer::InnardsOut), 40);
        assert_eq!(take_request(), Some((7, Request::Expand)));
    }

    #[test]
    fn collapse_never_captures_a_threshold_crossing_group() {
        begin(&[(2, 20)]);
        let _guard = Guard;
        let original = mon(2, 20);
        assert!(original.hp_le(30));
        assert!(capture_ko(&original, 30).is_none());
        assert_eq!(take_request(), Some((2, Request::Split { at: 10 })));
    }

    #[test]
    fn old_hp_copies_keep_the_original_span_after_the_live_tag_clears() {
        begin(&[(2, 20)]);
        let _guard = Guard;
        let original = mon(2, 20);
        let mark = original.hp_mark();
        let mut live = original.clone();
        let damage = capture_ko(&live, 100).unwrap().0;
        live.lazy = LazyTag::default();
        live.hp = 0;
        assert_eq!(live.hp_value(), 0);
        assert_eq!(take_request(), None);
        assert!(mark.hp_le(25));
        assert_eq!(take_request(), Some((2, Request::Split { at: 5 })));
        assert_eq!(original.hp_value(), 20);
        assert_eq!(take_request(), Some((2, Request::Expand)));
        assert_eq!(damage.exact(ExactDamageConsumer::Drain), 20);
        assert_eq!(take_request(), Some((2, Request::Expand)));
    }

    #[test]
    fn materialization_requests_expansion_before_removing_key_dependencies() {
        begin(&[(2, 20)]);
        let _guard = Guard;
        let mut damage = capture_ko(&mon(2, 20), 100).unwrap().0;
        assert!(format!("{damage:?}").contains("UnresolvedDealtDamage"));
        damage.materialize();
        assert_eq!(take_request(), Some((2, Request::Expand)));
        assert_eq!(format!("{damage:?}"), "20");
        assert_eq!(damage, DealtDamage::constant(20));
    }

    #[test]
    #[should_panic(expected = "materialize damage before equality or hashing")]
    fn unresolved_damage_cannot_silently_enter_a_hashed_key() {
        use std::hash::Hash;
        begin(&[(2, 20)]);
        let _guard = Guard;
        let damage = capture_ko(&mon(2, 20), 100).unwrap().0;
        damage.hash(&mut std::collections::hash_map::DefaultHasher::new());
    }

    #[test]
    fn every_registered_numeric_consumer_requests_original_hp() {
        use ExactDamageConsumer::*;
        begin(&[(2, 20)]);
        let _guard = Guard;
        let damage = capture_ko(&mon(2, 20), 100).unwrap().0;
        for consumer in [
            LegacyDamageReturn,
            Drain,
            Recoil,
            ShellBell,
            InnardsOut,
            Counter,
            LastDamagedBy,
            Berserk,
            EmergencyExit,
            PendingState,
        ] {
            assert_eq!(damage.exact(consumer), 20);
            assert_eq!(take_request(), Some((2, Request::Expand)));
        }
    }

    #[test]
    #[should_panic(expected = "dependent damage escaped its lazy run")]
    fn dependent_damage_cannot_be_read_after_its_lazy_run_ended() {
        begin(&[(2, 20)]);
        let damage = capture_ko(&mon(2, 20), 100).unwrap().0;
        end();
        damage.exact(ExactDamageConsumer::Drain);
    }
}
