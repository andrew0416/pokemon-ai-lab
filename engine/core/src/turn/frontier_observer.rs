//! P17 diagnostic instrumentation only. No battle values are read by this module.
//! Timers are inclusive and overlap: replay/key/component/commit are inside group,
//! group is inside frontier_runs, and compact is inside stage/final_compact.
//! A progress snapshot includes elapsed time of the currently open timers.

use std::cell::RefCell;
use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    FrontierRuns,
    Group,
    Replay,
    PositionKey,
    Component,
    Commit,
    StageCompact,
    FinalCompact,
    Compact,
    OutcomeDiff,
    Distribution,
    Rollback,
}
impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::FrontierRuns => "frontier_runs",
            Self::Group => "group",
            Self::Replay => "replay",
            Self::PositionKey => "position_key",
            Self::Component => "component",
            Self::Commit => "commit",
            Self::StageCompact => "stage_compact",
            Self::FinalCompact => "final_compact",
            Self::Compact => "compact",
            Self::OutcomeDiff => "outcome_diff",
            Self::Distribution => "distribution",
            Self::Rollback => "rollback",
        }
    }
}
const PHASES: [Phase; 13] = [
    Phase::Idle,
    Phase::FrontierRuns,
    Phase::Group,
    Phase::Replay,
    Phase::PositionKey,
    Phase::Component,
    Phase::Commit,
    Phase::StageCompact,
    Phase::FinalCompact,
    Phase::Compact,
    Phase::OutcomeDiff,
    Phase::Distribution,
    Phase::Rollback,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestReason {
    Value,
    Threshold,
    Shift,
}
impl RequestReason {
    fn name(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Threshold => "threshold",
            Self::Shift => "shift",
        }
    }
}

#[derive(Clone, Default)]
struct Counts {
    groups_started: u64,
    groups_completed: u64,
    replay_attempts: u64,
    position_calls: u64,
    components_created: u64,
    components_committed: u64,
    components_discarded: u64,
    live_reached: u64,
    compact_calls: u64,
    merge_passes: u64,
    merge_inputs: u64,
    request_value: u64,
    request_threshold: u64,
    request_shift: u64,
}
#[derive(Clone)]
struct Gate {
    last: Duration,
    checks: u64,
}
impl Gate {
    fn new() -> Self {
        Self {
            last: Duration::ZERO,
            checks: 0,
        }
    }
    fn should_check(&mut self) -> bool {
        self.checks = self.checks.wrapping_add(1);
        self.checks & 255 == 0
    }
    fn due(&mut self, elapsed: Duration) -> bool {
        if elapsed.saturating_sub(self.last) >= Duration::from_secs(2) {
            self.last = elapsed;
            true
        } else {
            false
        }
    }
}
struct Context {
    id: u64,
    stage: u64,
    group: u64,
    compact_input: u64,
    ordinal: u64,
    started: Instant,
    gate: Gate,
    active: Phase,
    elapsed: [u64; 13],
    open: Vec<(Phase, Instant)>,
    counts: Counts,
    input_groups: u64,
    next_components: u64,
    next_groups: u64,
    finished_components: u64,
    emit: bool,
}
#[derive(Default)]
struct State {
    current: Option<Context>,
    completed: Option<Context>,
    next_id: u64,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn ns(value: Duration) -> u64 {
    u64::try_from(value.as_nanos()).unwrap_or(u64::MAX)
}
fn with_context<T>(f: impl FnOnce(&mut Context) -> T) -> Option<T> {
    STATE.with(|s| s.borrow_mut().current.as_mut().map(f))
}
fn event(
    context: &mut Context,
    kind: &str,
    reason: &str,
    unit: i32,
    span: i32,
    caller: Option<&std::panic::Location<'_>>,
) {
    context.ordinal += 1;
    let now = Instant::now();
    let mut elapsed = context.elapsed;
    for &(phase, start) in &context.open {
        elapsed[phase as usize] =
            elapsed[phase as usize].saturating_add(ns(now.duration_since(start)));
    }
    if !context.emit {
        return;
    }
    let c = &context.counts;
    // Only fixed identifiers and primitive numbers enter this JSON. I/O failure must not
    // introduce a new engine error or panic. No HP accessor runs while STATE is borrowed.
    let mut out = std::io::stderr().lock();
    let _ = write!(out, "P17_FRONTIER {{\"schema\":1,\"event\":\"{kind}\",\"enumeration\":{},\"stage\":{},\"ordinal\":{},\"phase\":\"{}\",\"elapsed_ns\":{},\"reason\":\"{reason}\",\"unit\":{unit},\"span\":{span},\"input_groups\":{},\"next_components\":{},\"next_groups\":{},\"finished_components\":{},\"groups_started\":{},\"groups_completed\":{},\"replay_attempts\":{},\"position_calls\":{},\"components_created\":{},\"components_committed\":{},\"components_discarded\":{},\"live_reached\":{},\"compact_calls\":{},\"merge_passes\":{},\"merge_inputs\":{},\"request_value\":{},\"request_threshold\":{},\"request_shift\":{},\"inclusive_ns\":{{",
        context.id, context.stage, context.ordinal, context.active.name(), ns(now.duration_since(context.started)),
        context.input_groups, context.next_components, context.next_groups, context.finished_components,
        c.groups_started, c.groups_completed, c.replay_attempts, c.position_calls,
        c.components_created, c.components_committed, c.components_discarded, c.live_reached,
        c.compact_calls, c.merge_passes, c.merge_inputs, c.request_value, c.request_threshold, c.request_shift);
    for (i, phase) in PHASES.iter().enumerate() {
        let _ = write!(
            out,
            "{}\"{}\":{}",
            if i == 0 { "" } else { "," },
            phase.name(),
            elapsed[i]
        );
    }
    let (file, line, column) = caller.map_or(("", 0, 0), |c| (c.file(), c.line(), c.column()));
    let _ = writeln!(out, "}},\"group\":{},\"compact_input\":{},\"caller_file\":{},\"caller_line\":{},\"caller_column\":{}}}",
        context.group, context.compact_input, json_string(file), line, column);
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub(crate) struct Session {
    previous: Option<Context>,
    enabled: bool,
    completed: bool,
    output: bool,
}
impl Session {
    pub(super) fn begin() -> Self {
        Self::new(
            std::env::var_os("LAB_ENGINE_FRONTIER_OBSERVER").is_some_and(|v| v == "1"),
            true,
        )
    }
    fn new(enabled: bool, emit: bool) -> Self {
        if !enabled {
            STATE.with(|s| s.borrow_mut().completed = None);
            return Self {
                previous: None,
                enabled: false,
                completed: false,
                output: false,
            };
        }
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.next_id += 1;
            let id = s.next_id;
            let previous = s.current.take();
            let mut context = Context {
                id,
                stage: 0,
                group: 0,
                compact_input: 0,
                ordinal: 0,
                started: Instant::now(),
                gate: Gate::new(),
                active: Phase::Idle,
                elapsed: [0; 13],
                open: Vec::new(),
                counts: Counts::default(),
                input_groups: 0,
                next_components: 0,
                next_groups: 0,
                finished_components: 0,
                emit,
            };
            event(&mut context, "enumeration_begin", "none", -1, 0, None);
            s.current = Some(context);
            Self {
                previous,
                enabled: true,
                completed: false,
                output: false,
            }
        })
    }
    pub(super) fn output() -> Self {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            match s.completed.take() {
                None => Self {
                    previous: None,
                    enabled: false,
                    completed: false,
                    output: true,
                },
                Some(mut context) => {
                    let previous = s.current.take();
                    context.active = Phase::OutcomeDiff;
                    event(&mut context, "output_begin", "none", -1, 0, None);
                    s.current = Some(context);
                    Self {
                        previous,
                        enabled: true,
                        completed: false,
                        output: true,
                    }
                }
            }
        })
    }
    pub(crate) fn complete(&mut self) {
        self.completed = true;
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(mut context) = s.current.take() {
                if !self.completed {
                    context.counts.components_discarded += context.counts.live_reached;
                    context.counts.live_reached = 0;
                }
                let kind = if !self.completed {
                    "abort"
                } else if self.output {
                    "output_end"
                } else {
                    "enumeration_end"
                };
                event(&mut context, kind, "none", -1, 0, None);
                s.completed = if self.completed && !self.output {
                    Some(context)
                } else {
                    None
                };
            }
            s.current = self.previous.take();
        });
    }
}

pub(super) struct Timer {
    started: Option<Instant>,
    phase: Phase,
    previous: Phase,
    edge: bool,
}
impl Timer {
    pub(super) fn new(phase: Phase, edge: bool) -> Self {
        let state = with_context(|c| {
            let previous = c.active;
            c.active = phase;
            let start = Instant::now();
            c.open.push((phase, start));
            if edge {
                event(c, "phase_begin", "none", -1, 0, None);
            }
            (start, previous)
        });
        Self {
            started: state.map(|s| s.0),
            previous: state.map_or(Phase::Idle, |s| s.1),
            phase,
            edge,
        }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            with_context(|c| {
                c.elapsed[self.phase as usize] =
                    c.elapsed[self.phase as usize].saturating_add(ns(started.elapsed()));
                c.open.pop();
                if self.edge {
                    event(c, "phase_end", "none", -1, 0, None);
                }
                c.active = self.previous;
            });
        }
    }
}
pub(super) fn stage_begin(groups: usize) {
    with_context(|c| {
        c.stage += 1;
        c.group = 0;
        c.input_groups = groups as u64;
        c.next_components = 0;
        c.next_groups = 0;
        event(c, "stage_begin", "none", -1, 0, None);
    });
}
pub(super) fn stage_end(components: usize, groups: usize, finished: usize) {
    with_context(|c| {
        c.next_components = components as u64;
        c.next_groups = groups as u64;
        c.finished_components = finished as u64;
        event(c, "stage_end", "none", -1, 0, None);
    });
}
pub(super) fn tick() {
    with_context(|c| {
        if c.gate.should_check() && c.gate.due(c.started.elapsed()) {
            event(c, "progress", "none", -1, 0, None);
        }
    });
}
fn group_event(c: &mut Context, kind: &str) {
    if c.group <= 64 || c.group.is_power_of_two() {
        event(c, kind, "none", -1, 0, None);
    }
}
pub(super) fn group_begin() {
    with_context(|c| {
        c.counts.groups_started += 1;
        c.group += 1;
        group_event(c, "group_begin");
    });
}
pub(super) fn replay() {
    with_context(|c| c.counts.replay_attempts += 1);
    tick();
}
pub(super) fn position() {
    with_context(|c| c.counts.position_calls += 1);
}
pub(super) fn component() {
    with_context(|c| {
        c.counts.components_created += 1;
        c.counts.live_reached += 1;
    });
}
pub(super) fn commit() {
    with_context(|c| {
        c.counts.components_committed += 1;
        c.counts.live_reached -= 1;
    });
    tick();
}
pub(super) fn discard(count: usize) {
    with_context(|c| {
        c.counts.components_discarded += count as u64;
        c.counts.live_reached -= count as u64;
        group_event(c, "group_discard");
    });
}
pub(super) fn group_end() {
    with_context(|c| {
        c.counts.groups_completed += 1;
        group_event(c, "group_end");
    });
}
pub(super) fn compact(input: usize) {
    with_context(|c| {
        c.counts.compact_calls += 1;
        c.compact_input = input as u64;
    });
    tick();
}
pub(super) fn merge(input: usize) {
    with_context(|c| {
        c.counts.merge_passes += 1;
        c.counts.merge_inputs += input as u64;
    });
    tick();
}
pub(crate) fn request(
    reason: RequestReason,
    unit: usize,
    span: i32,
    caller: &std::panic::Location<'_>,
) {
    with_context(|c| {
        match reason {
            RequestReason::Value => c.counts.request_value += 1,
            RequestReason::Threshold => c.counts.request_threshold += 1,
            RequestReason::Shift => c.counts.request_shift += 1,
        }
        event(
            c,
            "first_lazy_request",
            reason.name(),
            unit as i32,
            span,
            Some(caller),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_paths_escape_controls_quotes_and_backslashes() {
        assert_eq!(json_string("a\\b\"\n\0"), "\"a\\\\b\\\"\\u000a\\u0000\"");
    }
    #[test]
    fn output_consumes_completed_session_once_and_disabled_begin_clears_stale() {
        let id;
        {
            let mut session = Session::new(true, false);
            id = with_context(|c| c.id).unwrap();
            session.complete();
        }
        {
            let mut output = Session::output();
            assert_eq!(with_context(|c| c.id), Some(id));
            output.complete();
        }
        assert!(!Session::output().enabled);
        {
            let mut session = Session::new(true, false);
            session.complete();
        }
        drop(Session::new(false, false));
        assert!(!Session::output().enabled);
    }
    #[test]
    fn progress_gate_uses_two_seconds_and_256_checks_without_sleep() {
        let mut gate = Gate::new();
        for _ in 0..255 {
            assert!(!gate.should_check());
        }
        assert!(gate.should_check());
        assert!(!gate.due(Duration::from_millis(1999)));
        assert!(gate.due(Duration::from_secs(2)));
        assert!(!gate.due(Duration::from_millis(3999)));
        assert!(gate.due(Duration::from_secs(4)));
    }
    #[test]
    fn nested_timers_restore_phase_and_account_inclusive_open_time() {
        let mut session = Session::new(true, false);
        {
            let _outer = Timer::new(Phase::Group, true);
            {
                let _inner = Timer::new(Phase::Replay, false);
                assert_eq!(with_context(|c| c.active), Some(Phase::Replay));
            }
            assert_eq!(with_context(|c| c.active), Some(Phase::Group));
        }
        assert_eq!(with_context(|c| c.active), Some(Phase::Idle));
        assert!(with_context(
            |c| c.elapsed[Phase::Group as usize] >= c.elapsed[Phase::Replay as usize]
        )
        .unwrap());
        assert!(with_context(|c| c.open.is_empty()).unwrap());
        component();
        component();
        discard(1);
        commit();
        assert_eq!(
            with_context(|c| (
                c.counts.components_created,
                c.counts.components_discarded,
                c.counts.components_committed,
                c.counts.live_reached
            )),
            Some((2, 1, 1, 0))
        );
        session.complete();
    }
    #[test]
    fn nested_sessions_restore_parent_and_unwind_is_observation_only() {
        let mut parent = Session::new(true, false);
        let id = with_context(|c| c.id).unwrap();
        let result = std::panic::catch_unwind(|| {
            let _child = Session::new(true, false);
            let _timer = Timer::new(Phase::Replay, false);
            assert_ne!(with_context(|c| c.id), Some(id));
            panic!("synthetic observer unwind");
        });
        assert!(result.is_err());
        assert_eq!(with_context(|c| c.id), Some(id));
        parent.complete();
    }
    pub(crate) fn quiet_session() -> Session {
        Session::new(true, false)
    }
    pub(crate) fn requests() -> (u64, u64, u64) {
        with_context(|c| {
            (
                c.counts.request_value,
                c.counts.request_threshold,
                c.counts.request_shift,
            )
        })
        .unwrap()
    }
}

#[cfg(test)]
pub(crate) use tests::{quiet_session, requests};
