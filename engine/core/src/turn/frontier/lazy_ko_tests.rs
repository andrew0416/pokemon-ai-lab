//! Seeded lazy groups exercise collapse itself without waiting for a large battle to create
//! an HP product. The same tests run OFF and ON, with exact distribution and replay counts.
use super::super::lazy::ExactDamageConsumer;
use super::*;
use crate::dex::{species, Type};
use crate::state::{Pokemon, SlotRef};

const TARGET: PokemonRef = PokemonRef {
    side: SideId::Two,
    party: 0,
};
const SLOT: SlotRef = SlotRef {
    side: SideId::Two,
    slot: 0,
};

fn field() -> State<1> {
    let mut state = State::default();
    for side in [SideId::One, SideId::Two] {
        state.side_mut(side).slots[0].party_index = Some(0);
        state.side_mut(side).party[0] = Pokemon {
            species: species::PIKACHU,
            hp: if side == SideId::Two { 20 } else { 100 },
            max_hp: 100,
            level: 50,
            stats: [100; 5],
            types: [Type::Normal, Type::None],
            ..Pokemon::default()
        };
    }
    state
}

fn seeded() -> Group<1, ()> {
    Group {
        state: field(),
        pending: (),
        weight: 1.0,
        lazy: vec![(
            unit_index(TARGET),
            Rc::new(Dist {
                points: vec![(0, 1.0 / 3.0), (10, 1.0 / 3.0), (20, 1.0 / 3.0)],
            }),
        )],
    }
}

fn run_seeded(
    mut stage: impl FnMut(&mut Battle<'_, 1>, &mut ()) -> Result<StageEnd, TurnError>,
) -> (Stats, Positions<1, Option<()>>) {
    let mut stats = Stats::default();
    let mut next = Positions::new();
    let mut finished = Positions::new();
    let mut buffers = RunBuffers::default();
    let mut stack = vec![seeded()];
    while let Some(group) = stack.pop() {
        stats.groups += 1;
        let parts = run_group(
            group,
            &mut next,
            &mut finished,
            EnumerateOptions::default(),
            &mut stage,
            &mut buffers,
            &mut stats,
        )
        .unwrap();
        stack.extend(parts.into_iter().rev());
    }
    assert!(next.entries.is_empty());
    (stats, finished)
}

fn hp_mass(finished: &Positions<1, Option<()>>, hp: i16) -> f64 {
    finished
        .entries
        .iter()
        .map(|entry| {
            if hp == 0 && entry.key.pokemon(TARGET).hp_value() == 0 {
                entry.components.iter().map(|c| c.weight).sum()
            } else if let Some(index) = entry.units.iter().position(|&u| u == unit_index(TARGET)) {
                entry
                    .components
                    .iter()
                    .map(|c| {
                        let unit = &c.hps[index];
                        c.weight
                            * unit
                                .dist
                                .points
                                .iter()
                                .filter(|&&(d, _)| unit.base + d == hp)
                                .map(|&(_, p)| p)
                                .sum::<f64>()
                    })
                    .sum()
            } else {
                0.0
            }
        })
        .sum()
}

#[test]
fn lazy_ko_collapse_reduces_replays_and_restores_live_tags() {
    let mut seen_tags = Vec::new();
    let (stats, finished) = run_seeded(|b, _| {
        seen_tags.push(b.mon(TARGET).lazy.0);
        b.rng.uniform(2);
        let damage = b.move_damage(SLOT, 100.0);
        assert!(damage.is_positive());
        assert_eq!(b.mon(TARGET).hp_value(), 0);
        Ok(StageEnd::Finished)
    });
    assert!((hp_mass(&finished, 0) - 1.0).abs() < 1e-15);
    #[cfg(feature = "experiment-lazy-ko-damage")]
    {
        assert_eq!(stats.runs, 2);
        assert_eq!(stats.expansions, 0);
        // The second chooser replay must see the original tag, not the previous KO's zero.
        assert_eq!(
            seen_tags,
            vec![lazy::tag(usize::from(unit_index(TARGET))).0; 2]
        );
    }
    #[cfg(not(feature = "experiment-lazy-ko-damage"))]
    {
        assert_eq!(stats.runs, 7);
        assert_eq!(stats.expansions, 1);
    }
    for entry in finished.entries {
        assert_eq!(entry.key.pokemon(TARGET).lazy.0, 0);
    }
}

#[test]
fn lazy_ko_mixed_threshold_preserves_ko_and_survivor_mass() {
    let (stats, finished) = run_seeded(|b, _| {
        b.rng.uniform(2);
        b.move_damage(SLOT, 30.0);
        Ok(StageEnd::Finished)
    });
    assert!((hp_mass(&finished, 0) - 2.0 / 3.0).abs() < 1e-15);
    assert!((hp_mass(&finished, 10) - 1.0 / 3.0).abs() < 1e-15);
    assert_eq!(stats.splits, 1);
    #[cfg(feature = "experiment-lazy-ko-damage")]
    {
        assert_eq!(stats.runs, 5);
        assert_eq!(stats.expansions, 0);
    }
    #[cfg(not(feature = "experiment-lazy-ko-damage"))]
    {
        assert_eq!(stats.runs, 8);
        assert_eq!(stats.expansions, 1);
    }
}

#[test]
fn lazy_ko_old_copy_numeric_read_still_forces_expansion() {
    let (stats, finished) = run_seeded(|b, _| {
        let old = b.mon(TARGET).clone();
        b.rng.uniform(2);
        b.move_damage(SLOT, 100.0);
        let _ = old.hp_value();
        Ok(StageEnd::Finished)
    });
    assert_eq!(stats.expansions, 1);
    assert_eq!(stats.runs, 7);
    assert!((hp_mass(&finished, 0) - 1.0).abs() < 1e-15);
}

#[test]
fn lazy_ko_exact_sink_and_public_damage_keep_legacy_expansion() {
    for legacy in [false, true] {
        let (stats, finished) = run_seeded(|b, _| {
            b.rng.uniform(2);
            if legacy {
                b.damage(SLOT, 100.0, super::super::battle::DamageSource::Move);
            } else {
                b.move_damage(SLOT, 100.0).exact(ExactDamageConsumer::Drain);
            }
            Ok(StageEnd::Finished)
        });
        assert_eq!(stats.expansions, 1);
        assert_eq!(stats.runs, 7);
        assert!((hp_mass(&finished, 0) - 1.0).abs() < 1e-15);
    }
}
#[test]
fn lazy_ko_error_discards_private_state_and_closes_tls() {
    let original = field();
    let before = format!("{original:?}");
    let mut old = original.pokemon(TARGET).clone();
    old.lazy = lazy::tag(usize::from(unit_index(TARGET)));
    let mut stats = Stats::default();
    let mut next = Positions::new();
    let mut finished = Positions::new();
    let mut buffers = RunBuffers::default();
    let error = run_group(
        seeded(),
        &mut next,
        &mut finished,
        EnumerateOptions::default(),
        &mut |b, _| {
            b.move_damage(SLOT, 100.0);
            // Leave an original-HP request pending even after current HP collapsed. The error
            // must discard both the private partial state and this thread-local request.
            let _ = old.hp_value();
            Err(TurnError::Unsupported("synthetic post-KO error".into()))
        },
        &mut buffers,
        &mut stats,
    );
    assert!(matches!(error, Err(TurnError::Unsupported(ref s)) if s == "synthetic post-KO error"));
    assert_eq!(stats.runs, 1);
    assert!(next.entries.is_empty() && finished.entries.is_empty());
    assert_eq!(lazy::take_request(), None);
    assert_eq!(old.hp_value(), 20);
    assert_eq!(
        lazy::take_request(),
        None,
        "the group guard ended the old lazy session"
    );
    assert_eq!(format!("{original:?}"), before);
    // The partial ordinary instruction log still applies/reverses correctly, while the
    // failed group's private work is discarded instead of becoming a frontier entry.
    let mut replay = original.clone();
    replay.apply(&buffers.log);
    assert_eq!(replay.pokemon(TARGET).hp_value(), 0);
    replay.reverse(&buffers.log);
    assert_eq!(replay, original);
    assert_eq!(format!("{replay:?}"), before);
    // A later group on the same thread starts with its own original span and no stale request.
    lazy::begin(&[(usize::from(unit_index(TARGET)), 5)]);
    assert_eq!(lazy::take_request(), None);
    assert_eq!(old.hp_value(), 20);
    let request = lazy::take_request();
    lazy::end();
    assert_eq!(
        request,
        Some((usize::from(unit_index(TARGET)), Request::Expand))
    );
}
