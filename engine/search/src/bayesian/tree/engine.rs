//! Explicit boundary-snapshot information abstraction for the current engine.
//! The engine merges some histories before exposing outcomes and has no complete public
//! event log. This observer deliberately omits within-segment events/order. It is NOT a
//! Showdown battle-log information model or a safety certificate for unabstracted VGC.
//! ObservedDomain is replaceable when an event-preserving engine interface is available.

use super::builder::{self, Built, Limits, Observation, ObservedDomain, Seed};
use crate::bayesian::{
    engine::{visible, EngineWorld, Knowledge},
    Error,
};
use crate::budgeted::{Domain, EngineDomain, Phase, Position};
use crate::{Choice, Pruning};
use lab_engine::{
    eval::Evaluator,
    rules::Ruleset,
    state::{SideId, State},
    turn::EnumerateOptions,
};

pub struct SnapshotDomain<'a, const N: usize, E: Evaluator<N> + ?Sized> {
    pub inner: EngineDomain<'a, N, E>,
}
impl<const N: usize, E: Evaluator<N> + ?Sized> Domain for SnapshotDomain<'_, N, E> {
    type Position = Position<N>;
    type Action = Choice<N>;
    fn phase(&self, p: &Self::Position) -> Result<Phase, String> {
        self.inner.phase(p)
    }
    fn actions(&self, p: &Self::Position, player: usize) -> Result<Vec<Self::Action>, String> {
        self.inner.actions(p, player)
    }
    fn value(&self, p: &Self::Position) -> f32 {
        self.inner.value(p)
    }
    fn transitions(
        &self,
        p: &Self::Position,
        a: [&Self::Action; 2],
    ) -> Result<Vec<(f64, Self::Position)>, String> {
        self.inner.transitions(p, a)
    }
}

/// Champions public HP uses floor, minimum one for living Pokémon. Boundary-color
/// distinctions at 20/50 are intentionally coarsened, never replaced with exact HP.
fn hp_bucket(hp: i16, max: i16) -> Result<i32, String> {
    if max <= 0 || hp < 0 || hp > max {
        return Err("invalid active HP".into());
    }
    Ok(if hp == 0 {
        0
    } else {
        (100 * i32::from(hp) / i32::from(max)).max(1)
    })
}

fn snapshot<const N: usize>(
    state: &State<N>,
    us: SideId,
    phase: &str,
) -> Result<Observation, String> {
    snapshot_impl::<N, false>(state, us, phase)
}
fn snapshot_impl<const N: usize, const DIRECT: bool>(
    state: &State<N>,
    us: SideId,
    phase: &str,
) -> Result<Observation, String> {
    // A single disclosure allowlist feeds both formatting paths. Adding a mechanic's
    // observation changes both paths together; DIRECT changes storage only.
    macro_rules! append {
        ($out:expr, $($args:tt)*) => {{
            if DIRECT {
                use std::fmt::Write;
                write!($out, $($args)*).expect("String formatting failed");
            } else {
                ($out).push_str(&format!($($args)*));
            }
        }};
    }

    // Explicit allowlist. Never Debug/hash State, Side, Slot, Pokemon or Suspension here:
    // they include hidden counters, bench identity, queue order and committed actions.
    let mut public = format!("{phase}|{}|{:?}|", state.turn, state.result);
    for effect in &state.field {
        append!(&mut public, "{},{};", effect.is_active(), effect.value);
    }
    let mut private = [String::new(), String::new()];
    for (physical, side) in state.sides.iter().enumerate() {
        public.push('|');
        for effect in &side.effects {
            append!(&mut public, "{},{};", effect.is_active(), effect.value);
        }
        for slot in &side.slots {
            // A last-move field is not a public event log: aborted/called moves need
            // disclosure semantics that this engine API does not preserve.
            append!(&mut public, "{:?};{};", slot.boosts, slot.must_switch_out());
            if let Some(index) = slot.party_index.or(slot.fainted_occupant) {
                let p = &side.party[index as usize];
                if p.illusion {
                    return Err("Illusion needs an appearance-aware observer".into());
                }
                append!(
                    &mut public,
                    "{:?},{},{:?};",
                    p.species,
                    hp_bucket(p.hp, p.max_hp)?,
                    p.status
                );
            } else {
                public.push_str("empty;");
            }
        }
        let player = if physical == us.index() { 0 } else { 1 };
        // The owner's request supplies HP, item/ability and PP, not random status timers.
        for p in &side.party {
            append!(
                &mut private[player],
                "{:?},{},{},{:?},{:?},{:?};",
                p.species,
                p.hp,
                p.max_hp,
                p.status,
                p.item,
                p.ability
            );
            for m in &p.moves {
                append!(&mut private[player], "{:?},{};", m.id, m.pp);
            }
        }
    }
    Ok(Observation { public, private })
}

impl<const N: usize, E: Evaluator<N> + ?Sized> ObservedDomain for SnapshotDomain<'_, N, E> {
    fn observation(&self, p: &Position<N>) -> Result<Observation, String> {
        let phase = crate::decision(&p.state, p.suspension.as_ref()).map_err(|e| e.to_string())?;
        snapshot(&p.state, self.inner.us, &format!("{phase:?}"))
    }
    fn action_id(&self, _: &Position<N>, _: usize, a: &Choice<N>) -> String {
        format!("{a:?}")
    }
}

pub fn build<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
) -> Result<Built, Error> {
    let first = worlds.first().ok_or_else(|| Error("empty worlds".into()))?;
    let reference = visible(&first.position.state, us, knowledge)?;
    for w in worlds {
        if w.position.suspension.is_some() {
            return Err(Error("root suspended state needs prior action memory; resume using this tree's continuation policy".into()));
        }
        if visible(&w.position.state, us, knowledge)? != reference {
            return Err(Error("worlds disagree on declared known root state".into()));
        }
    }
    let domain = SnapshotDomain {
        inner: EngineDomain {
            ruleset,
            options: EnumerateOptions::default(),
            pruning: Pruning::All,
            us,
            evaluator,
        },
    };
    let seeds: Vec<_> = worlds
        .iter()
        .map(|w| Seed {
            id: w.id.clone(),
            weight: w.weight,
            position: w.position.clone(),
        })
        .collect();
    builder::build(&domain, &seeds, limits)
}

#[cfg(feature = "experiment-belief-workspace")]
struct WritingDomain<'a, const N: usize, E: Evaluator<N> + ?Sized, const OWNED: bool> {
    inner: SnapshotDomain<'a, N, E>,
    direct: bool,
}
#[cfg(feature = "experiment-belief-workspace")]
impl<const N: usize, E: Evaluator<N> + ?Sized, const OWNED: bool> Domain
    for WritingDomain<'_, N, E, OWNED>
{
    type Position = Position<N>;
    type Action = Choice<N>;
    fn phase(&self, p: &Self::Position) -> Result<Phase, String> {
        self.inner.phase(p)
    }
    fn actions(&self, p: &Self::Position, player: usize) -> Result<Vec<Self::Action>, String> {
        self.inner.actions(p, player)
    }
    fn value(&self, p: &Self::Position) -> f32 {
        self.inner.value(p)
    }
    fn transitions(
        &self,
        p: &Self::Position,
        a: [&Self::Action; 2],
    ) -> Result<Vec<(f64, Self::Position)>, String> {
        #[cfg(feature = "experiment-owned-transitions")]
        if OWNED {
            return self.inner.inner.transitions_owned(p, a);
        }
        self.inner.transitions(p, a)
    }
}
#[cfg(feature = "experiment-belief-workspace")]
impl<const N: usize, E: Evaluator<N> + ?Sized, const OWNED: bool> ObservedDomain
    for WritingDomain<'_, N, E, OWNED>
{
    fn observation(&self, p: &Position<N>) -> Result<Observation, String> {
        if !self.direct {
            return self.inner.observation(p);
        }
        let phase = crate::decision(&p.state, p.suspension.as_ref()).map_err(|e| e.to_string())?;
        snapshot_impl::<N, true>(&p.state, self.inner.inner.us, &format!("{phase:?}"))
    }
    fn action_id(&self, p: &Position<N>, player: usize, a: &Choice<N>) -> String {
        self.inner.action_id(p, player, a)
    }
}

/// Selective expansion uses exactly the same root knowledge and observation contract.
#[cfg(feature = "experiment-growing-belief")]
pub fn growing<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    config: builder::growing::Config,
) -> Result<builder::growing::ResultTree, Error> {
    let first = worlds.first().ok_or_else(|| Error("empty worlds".into()))?;
    let reference = visible(&first.position.state, us, knowledge)?;
    for w in worlds {
        if w.position.suspension.is_some() {
            return Err(Error(
                "root suspended state needs prior action memory".into(),
            ));
        }
        if visible(&w.position.state, us, knowledge)? != reference {
            return Err(Error("worlds disagree on declared known root state".into()));
        }
    }
    let domain = SnapshotDomain {
        inner: EngineDomain {
            ruleset,
            options: EnumerateOptions::default(),
            pruning: Pruning::All,
            us,
            evaluator,
        },
    };
    let seeds: Vec<_> = worlds
        .iter()
        .map(|w| Seed {
            id: w.id.clone(),
            weight: w.weight,
            position: w.position.clone(),
        })
        .collect();
    builder::growing::search(&domain, &seeds, limits, config, &builder::growing::Uniform)
}

/// Allocation-only candidate; same root information checks as `growing`.
#[cfg(feature = "experiment-belief-workspace")]
pub fn growing_reusing<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: builder::growing::reuse::Settings,
) -> Result<builder::growing::ResultTree, Error> {
    growing_storage::<N, E, false, false, false>(
        worlds, us, ruleset, evaluator, knowledge, limits, settings,
    )
}

#[cfg(feature = "experiment-owned-transitions")]
pub fn growing_owned<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: builder::growing::reuse::Settings,
) -> Result<builder::growing::ResultTree, Error> {
    growing_storage::<N, E, true, false, false>(
        worlds, us, ruleset, evaluator, knowledge, limits, settings,
    )
}

#[cfg(feature = "experiment-shared-final-passes")]
pub fn growing_shared<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: builder::growing::reuse::Settings,
) -> Result<builder::growing::ResultTree, Error> {
    growing_storage::<N, E, true, true, false>(
        worlds, us, ruleset, evaluator, knowledge, limits, settings,
    )
}

#[cfg(feature = "experiment-incremental-compilation")]
pub fn growing_incremental<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: builder::growing::reuse::Settings,
) -> Result<builder::growing::ResultTree, Error> {
    growing_storage::<N, E, true, false, true>(
        worlds, us, ruleset, evaluator, knowledge, limits, settings,
    )
}

#[cfg(feature = "experiment-belief-workspace")]
fn growing_storage<
    const N: usize,
    E: Evaluator<N> + ?Sized,
    const OWNED: bool,
    const SHARED: bool,
    const INCREMENTAL: bool,
>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
    settings: builder::growing::reuse::Settings,
) -> Result<builder::growing::ResultTree, Error> {
    let first = worlds.first().ok_or_else(|| Error("empty worlds".into()))?;
    let reference = visible(&first.position.state, us, knowledge)?;
    for w in worlds {
        if w.position.suspension.is_some() {
            return Err(Error(
                "root suspended state needs prior action memory".into(),
            ));
        }
        if visible(&w.position.state, us, knowledge)? != reference {
            return Err(Error("worlds disagree on declared known root state".into()));
        }
    }
    let domain = SnapshotDomain {
        inner: EngineDomain {
            ruleset,
            options: EnumerateOptions::default(),
            pruning: Pruning::All,
            us,
            evaluator,
        },
    };
    let seeds: Vec<_> = worlds
        .iter()
        .map(|w| Seed {
            id: w.id.clone(),
            weight: w.weight,
            position: w.position.clone(),
        })
        .collect();
    let domain = WritingDomain::<N, E, OWNED> {
        inner: domain,
        direct: settings.storage.direct_write,
    };
    #[cfg(feature = "experiment-incremental-compilation")]
    if INCREMENTAL {
        return builder::growing::reuse::search_incremental(
            &domain,
            &seeds,
            limits,
            settings.growth,
            &builder::growing::Uniform,
            settings.storage,
        );
    }
    #[cfg(feature = "experiment-shared-final-passes")]
    if SHARED {
        return builder::growing::reuse::search_shared(
            &domain,
            &seeds,
            limits,
            settings.growth,
            &builder::growing::Uniform,
            settings.storage,
        );
    }
    builder::growing::reuse::search(
        &domain,
        &seeds,
        limits,
        settings.growth,
        &builder::growing::Uniform,
        settings.storage,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_writers_share_disclosure_and_error_contracts() {
        let mut state = State::<2>::default();
        state.sides[0].slots[0].party_index = Some(0);
        for side in [SideId::One, SideId::Two] {
            for hp in [0, 1, 50, 100, 101] {
                state.sides[0].party[0].max_hp = 100;
                state.sides[0].party[0].hp = hp;
                assert_eq!(
                    snapshot_impl::<2, false>(&state, side, "Turn"),
                    snapshot_impl::<2, true>(&state, side, "Turn")
                );
            }
        }
        assert!(snapshot_impl::<2, true>(&state, SideId::One, "Turn").is_err());
        state.sides[0].party[0].hp = 50;
        state.sides[0].party[0].illusion = true;
        assert_eq!(
            snapshot_impl::<2, false>(&state, SideId::One, "Switch"),
            snapshot_impl::<2, true>(&state, SideId::One, "Switch")
        );
        assert!(snapshot_impl::<2, true>(&state, SideId::One, "Switch").is_err());
    }
    #[test]
    fn hp_is_not_exact_and_invalid_health_fails() {
        assert_eq!(hp_bucket(51, 101).unwrap(), 50);
        assert_eq!(hp_bucket(50, 100).unwrap(), 50);
        assert_eq!(hp_bucket(1, 1000).unwrap(), 1);
        assert!(hp_bucket(10, 0).is_err());
    }
    #[test]
    fn owner_requests_follow_the_selected_seat_and_counters_are_not_observations() {
        let mut state = State::<2>::default();
        state.sides[0].party[0].hp = 77;
        state.sides[0].party[0].max_hp = 100;
        state.sides[1].party[0].hp = 88;
        state.sides[1].party[0].max_hp = 100;
        let a = snapshot(&state, SideId::One, "Turn").unwrap();
        let b = snapshot(&state, SideId::Two, "Turn").unwrap();
        assert_eq!(a.public, b.public);
        assert_eq!(a.private[0], b.private[1]);
        assert_eq!(a.private[1], b.private[0]);
        state.sides[1].party[0].status_turns = 4;
        state.sides[1].party[0].stats[4] = 201;
        state.sides[1].slots[0].ability_order = 3;
        state.sides[1].slots[0].substitute_hp = 43;
        assert_eq!(a, snapshot(&state, SideId::One, "Turn").unwrap());
    }
}

#[cfg(feature = "experiment-belief-workspace")]
pub fn build_writing<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
) -> Result<Built, Error> {
    build_storage::<N, E, false>(worlds, us, ruleset, evaluator, knowledge, limits)
}

#[cfg(feature = "experiment-owned-transitions")]
pub fn build_owned<const N: usize, E: Evaluator<N> + ?Sized>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
) -> Result<Built, Error> {
    build_storage::<N, E, true>(worlds, us, ruleset, evaluator, knowledge, limits)
}

#[cfg(feature = "experiment-belief-workspace")]
fn build_storage<const N: usize, E: Evaluator<N> + ?Sized, const OWNED: bool>(
    worlds: &[EngineWorld<N>],
    us: SideId,
    ruleset: Ruleset,
    evaluator: &E,
    knowledge: &Knowledge,
    limits: Limits,
) -> Result<Built, Error> {
    let first = worlds.first().ok_or_else(|| Error("empty worlds".into()))?;
    let reference = visible(&first.position.state, us, knowledge)?;
    for w in worlds {
        if w.position.suspension.is_some() {
            return Err(Error("root suspended state needs prior action memory; resume using this tree's continuation policy".into()));
        }
        if visible(&w.position.state, us, knowledge)? != reference {
            return Err(Error("worlds disagree on declared known root state".into()));
        }
    }
    let domain = SnapshotDomain {
        inner: EngineDomain {
            ruleset,
            options: EnumerateOptions::default(),
            pruning: Pruning::All,
            us,
            evaluator,
        },
    };
    let seeds: Vec<_> = worlds
        .iter()
        .map(|w| Seed {
            id: w.id.clone(),
            weight: w.weight,
            position: w.position.clone(),
        })
        .collect();
    let domain = WritingDomain::<N, E, OWNED> {
        inner: domain,
        direct: true,
    };
    builder::build(&domain, &seeds, limits)
}

#[cfg(feature = "experiment-growth-cadence")]
pub mod cadence;
#[cfg(feature = "experiment-paper-solvers")]
pub mod paper;
#[cfg(feature = "experiment-parallel-transitions")]
pub mod parallel;
