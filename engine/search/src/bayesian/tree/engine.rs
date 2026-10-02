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
    // Explicit allowlist. Never Debug/hash State, Side, Slot, Pokemon or Suspension here:
    // they include hidden counters, bench identity, queue order and committed actions.
    let mut public = format!("{phase}|{}|{:?}|", state.turn, state.result);
    for effect in &state.field {
        public.push_str(&format!("{},{};", effect.is_active(), effect.value));
    }
    let mut private = [String::new(), String::new()];
    for (physical, side) in state.sides.iter().enumerate() {
        public.push('|');
        for effect in &side.effects {
            public.push_str(&format!("{},{};", effect.is_active(), effect.value));
        }
        for slot in &side.slots {
            // A last-move field is not a public event log: aborted/called moves need
            // disclosure semantics that this engine API does not preserve.
            public.push_str(&format!("{:?};{};", slot.boosts, slot.must_switch_out()));
            if let Some(index) = slot.party_index.or(slot.fainted_occupant) {
                let p = &side.party[index as usize];
                if p.illusion {
                    return Err("Illusion needs an appearance-aware observer".into());
                }
                public.push_str(&format!(
                    "{:?},{},{:?};",
                    p.species,
                    hp_bucket(p.hp, p.max_hp)?,
                    p.status
                ));
            } else {
                public.push_str("empty;");
            }
        }
        let player = if physical == us.index() { 0 } else { 1 };
        // The owner's request supplies HP, item/ability and PP, not random status timers.
        for p in &side.party {
            private[player].push_str(&format!(
                "{:?},{},{},{:?},{:?},{:?};",
                p.species, p.hp, p.max_hp, p.status, p.item, p.ability
            ));
            for m in &p.moves {
                private[player].push_str(&format!("{:?},{};", m.id, m.pp));
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

#[cfg(test)]
mod tests {
    use super::*;
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
