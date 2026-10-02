// Included as a child of frontier, so tests can seed HP products without widening the API.
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::action::{Gimmick, SlotAction};
    use crate::dex::{abilities, items, moves, species, MoveId, Type};
    use crate::field::{Effect, FieldEffect, Terrain};
    use crate::instruction::{Instruction, Outcome};
    use crate::rules::Ruleset;
    use crate::state::{MoveSlot, Pokemon, PokemonRef, SideId, State, Status};
    use crate::turn::{
        self, first_hit, hit_suffix, lazy, FactoredScope, Pending, RollMode, Suspension,
    };
    use std::collections::HashMap;
    type Distribution = HashMap<(State<2>, Option<Suspension>), f64>;
    type Choices = [[SlotAction; 2]; 2];
    const ATTACKER: PokemonRef = PokemonRef {
        side: SideId::One,
        party: 0,
    };
    const TARGET: PokemonRef = PokemonRef {
        side: SideId::Two,
        party: 0,
    };
    const LAST: PokemonRef = PokemonRef {
        side: SideId::Two,
        party: 1,
    };
    // Low attack/high defense retain nonuniform Full roll multiplicities with small support.
    fn field(mv: MoveId) -> State<2> {
        let mut state = State::<2> {
            turn: 1,
            ..State::default()
        };
        for side in [SideId::One, SideId::Two] {
            for party in 0..3 {
                let speed = match (side, party) {
                    (SideId::One, 0) => 250,
                    (SideId::One, 1) => 180,
                    (SideId::Two, 0) => 130,
                    _ => 80,
                };
                state.side_mut(side).party[party] = Pokemon {
                    species: species::PIKACHU,
                    hp: 160,
                    max_hp: 200,
                    level: 50,
                    stats: [20, 200, 20, 200, speed],
                    types: [Type::Normal, Type::None],
                    ability: abilities::NO_ABILITY,
                    base_ability: abilities::NO_ABILITY,
                    moves: [MoveSlot::full(moves::SWORDS_DANCE); 4],
                    ..Pokemon::default()
                };
            }
            for slot in 0..2 {
                state.side_mut(side).slots[slot].party_index = Some(slot as u8);
            }
            // Unused reader keeps battle.lastMove observable in the exact State key.
            state.side_mut(side).party[2].moves[3] = MoveSlot::full(moves::COPYCAT);
        }
        state.pokemon_mut(ATTACKER).moves[0] = MoveSlot::full(mv);
        if mv == moves::EXPANDING_FORCE {
            state.field[FieldEffect::Terrain as usize] = Effect {
                value: Terrain::Psychic as u8,
                turns: 5,
            };
        }
        state
    }
    fn action(index: u8, target: i8) -> SlotAction {
        SlotAction::Move {
            index,
            target,
            gimmick: Gimmick::None,
        }
    }
    fn choices(mv: MoveId) -> Choices {
        let target = if [
            moves::TACKLE,
            moves::DOUBLE_HIT,
            moves::DRAGON_DARTS,
            moves::U_TURN,
            moves::EXPANDING_FORCE,
        ]
        .contains(&mv)
        {
            1
        } else {
            0
        };
        [
            [action(0, target), action(0, 0)],
            [action(0, 0), action(0, 0)],
        ]
    }

    fn unchanged(a: &State<2>, b: &State<2>) {
        assert_eq!(a, b);
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "including lazy tags");
    }
    fn closed() {
        assert!(!lazy::request_pending());
        let mut probe = Pokemon::default();
        probe.hp = 20;
        probe.lazy = lazy::tag(0);
        assert_eq!(probe.hp_value(), 20);
        assert!(!lazy::request_pending(), "TLS must be inactive");
    }
    fn distribution(state: &State<2>, outcomes: Vec<Outcome>) -> Distribution {
        assert!(outcomes.len() <= 100_000);
        let mut out = Distribution::new();
        let mut work = state.clone();
        for row in outcomes {
            assert!(row.probability.is_finite() && row.probability > 0.0);
            work.apply(&row.instructions);
            for side in &work.sides {
                for mon in &side.party {
                    assert_eq!(mon.lazy.0, 0);
                }
            }
            *out.entry((work.clone(), row.suspension)).or_default() += row.probability;
            work.reverse(&row.instructions);
            unchanged(&work, state);
        }
        assert!((out.values().sum::<f64>() - 1.0).abs() <= 1e-12);
        out
    }
    fn same(a: &Distribution, b: &Distribution) {
        assert_eq!(a.len(), b.len());
        for (key, p) in a {
            assert_eq!(
                p.to_bits(),
                b.get(key).expect("complete state key").to_bits(),
                "probability bits"
            );
        }
    }
    fn factored(
        state: &State<2>,
        actions: Choices,
        options: FactoredOptions,
        disabled: bool,
    ) -> (Distribution, hit_suffix::Counts) {
        let _off = hit_suffix::TestDisableGuard::new(disabled);
        hit_suffix::test_observer_reset();
        let mut work = state.clone();
        let result =
            turn::enumerate_turn_factored_with(&mut work, Ruleset::CHAMPIONS_MC, actions, options)
                .unwrap();
        unchanged(&work, state);
        closed();
        let rows = result
            .outcomes
            .into_iter()
            .flat_map(|o| o.expand())
            .collect();
        (
            distribution(state, rows),
            hit_suffix::test_observer_snapshot(),
        )
    }
    fn compare(
        state: &State<2>,
        actions: Choices,
    ) -> (Distribution, hit_suffix::Counts, hit_suffix::Counts) {
        let (off, a) = factored(state, actions, FactoredOptions::default(), true);
        let (on, b) = factored(state, actions, FactoredOptions::default(), false);
        same(&off, &on);
        assert_eq!(a.captures, 0);
        println!("P7H keys={} off={a:?} on={b:?}", on.len());
        (on, a, b)
    }

    #[test]
    fn full_spread_reuses_real_damage_prefix_and_preserves_probability_bits() {
        for mv in [moves::EARTHQUAKE, moves::SURF, moves::ROCK_SLIDE] {
            let state = field(mv);
            let (out, off, on) = compare(&state, choices(mv));
            assert!(out.len() > 1);
            assert!(on.captures > 0 && on.resumes > 0 && on.invalidations > 0);
            assert!(
                on.damage_entries < off.damage_entries,
                "actual get_damage work must be saved"
            );
            assert!(out
                .keys()
                .any(|(s, _)| s.pokemon(TARGET).hp < state.pokemon(TARGET).hp));
        }
    }
    #[test]
    fn resist_berry_prefix_is_retained_once_and_full_tails_preserve_items_history() {
        let mut state = field(moves::EARTHQUAKE);
        state.pokemon_mut(LAST).types = [Type::Electric, Type::None];
        state.pokemon_mut(LAST).item = items::SHUCA_BERRY;
        state.pokemon_mut(ATTACKER).item = items::LIFE_ORB;
        let (out, off, on) = compare(&state, choices(moves::EARTHQUAKE));
        assert!(on.resumes > 0 && on.saved_prefix_instructions > 0);
        assert!(on.damage_entries < off.damage_entries);
        assert!(out
            .keys()
            .all(|(s, _)| s.pokemon(LAST).item != items::SHUCA_BERRY));
        assert!(out
            .keys()
            .any(|(s, _)| s.pokemon(ATTACKER).hp < state.pokemon(ATTACKER).hp));
    }
    #[test]
    fn fallback_modes_callbacks_multihit_and_flat_do_not_capture() {
        for mv in [moves::TACKLE, moves::DOUBLE_HIT, moves::EXPANDING_FORCE] {
            let state = field(mv);
            let (_, _, on) = compare(&state, choices(mv));
            assert_eq!(on.captures, 0);
        }
        let state = field(moves::EARTHQUAKE);
        let actions = choices(moves::EARTHQUAKE);
        for options in [
            FactoredOptions {
                rolls: RollMode::Median,
                max_support: None,
            },
            FactoredOptions {
                rolls: RollMode::Full,
                max_support: Some(2),
            },
        ] {
            let (off, _) = factored(&state, actions, options, true);
            let (on, c) = factored(&state, actions, options, false);
            same(&off, &on);
            assert_eq!(c.captures, 0);
        }
        let _scope = FactoredScope::new(false);
        hit_suffix::test_observer_reset();
        let mut work = state.clone();
        let rows = turn::enumerate_turn_with(
            &mut work,
            Ruleset::CHAMPIONS_MC,
            actions,
            EnumerateOptions::default(),
        )
        .unwrap();
        assert_eq!(hit_suffix::test_observer_snapshot().captures, 0);
        unchanged(&work, &state);
        closed();
        let (on, _, _) = compare(&state, actions);
        // Flat and factored have different existing accumulation orders. P7h OFF/ON
        // above is bit-strict; this older representation boundary uses its 1e-12 contract.
        let flat = distribution(&state, rows);
        assert_eq!(flat.len(), on.len());
        for (key, p) in flat {
            assert!((p - on.get(&key).expect("flat/factored full key")).abs() <= 1e-12);
        }
    }
    #[test]
    fn called_move_and_real_midturn_suspension_keep_existing_contract() {
        let mut state = field(moves::SLEEP_TALK);
        state.pokemon_mut(ATTACKER).status = Status::Sleep;
        state.pokemon_mut(ATTACKER).status_turns = 2;
        state.pokemon_mut(ATTACKER).moves = [
            MoveSlot::full(moves::SLEEP_TALK),
            MoveSlot::full(moves::EARTHQUAKE),
            MoveSlot::full(moves::SLEEP_TALK),
            MoveSlot::full(moves::SLEEP_TALK),
        ];
        let (_, _, c) = compare(&state, choices(moves::SLEEP_TALK));
        assert_eq!(c.captures, 0);
        let mut state = field(moves::EARTHQUAKE);
        state.pokemon_mut(TARGET).item = items::EJECT_BUTTON;
        let (out, _, c) = compare(&state, choices(moves::EARTHQUAKE));
        assert!(c.resumes > 0);
        assert!(out.keys().any(|(_, s)| s.is_some()));
    }
    #[test]
    fn error_after_second_retained_suffix_rolls_back_everything_and_closes_tls() {
        let state = field(moves::EARTHQUAKE);
        let actions = choices(moves::EARTHQUAKE);
        let _on = hit_suffix::TestDisableGuard::new(false);
        hit_suffix::test_observer_reset();
        let mut injected = false;
        let result = enumerate_factored(
            &state,
            Pending::new(turn::initial_queue(&state, &actions)),
            FactoredOptions::default(),
            |b, p| {
                let end = turn::run_stage(b, p)?;
                if hit_suffix::test_observer_snapshot().resumes >= 2 {
                    assert!(b.hit_suffix_frame.is_some() && !b.log.is_empty());
                    injected = true;
                    return Err(TurnError::Unsupported(
                        "p7h injected after retained work".into(),
                    ));
                }
                Ok(end)
            },
        );
        assert!(
            matches!(result,Err(TurnError::Unsupported(ref s)) if s=="p7h injected after retained work")
        );
        assert!(injected);
        assert_eq!(hit_suffix::test_observer_snapshot().error_rollbacks, 1);
        closed();
        let (_, _, c) = compare(&state, actions);
        assert!(c.resumes > 0);
    }
    #[test]
    fn actual_unsupported_faint_error_matches_first_error_without_canonicalization() {
        let mut state = field(moves::EARTHQUAKE);
        state.pokemon_mut(TARGET).species = species::ZYGARDE_COMPLETE;
        state.pokemon_mut(TARGET).hp = 1;
        let mut errors = Vec::new();
        for disabled in [true, false] {
            let _guard = hit_suffix::TestDisableGuard::new(disabled);
            hit_suffix::test_observer_reset();
            let mut work = state.clone();
            let error = turn::enumerate_turn_factored_with(
                &mut work,
                Ruleset::CHAMPIONS_MC,
                choices(moves::EARTHQUAKE),
                FactoredOptions::default(),
            )
            .unwrap_err();
            unchanged(&work, &state);
            closed();
            assert!(matches!(error, TurnError::Unsupported(_)));
            if !disabled {
                assert!(hit_suffix::test_observer_snapshot().captures > 0);
            }
            errors.push(format!("{error:?}"));
        }
        assert_eq!(errors[0], errors[1]);
        assert!(errors[0].contains("Zygarde"));
    }

    // Prepare the actual P7d boundary, then seed a product at that existing internal API.
    fn first_hit_input(mut state: State<2>) -> (State<2>, Pending) {
        let actions = choices(moves::EARTHQUAKE);
        let mut pending = Pending::new(turn::initial_queue(&state, &actions));
        let mut rng = Chooser::with_rolls(RollMode::Full);
        // BeforeTurn is an earlier stage. Cross only existing successful stage boundaries,
        // rebuilding Battle scratch as the real driver does, until the actual FirstHit.
        for _ in 0..8 {
            rng.begin_run();
            {
                let mut b = Battle::new(&mut state, &mut rng);
                b.first_hit_policy = first_hit::Policy::ExactFull;
                assert_eq!(
                    turn::run_stage(&mut b, &mut pending).unwrap(),
                    StageEnd::Continue
                );
            }
            if pending
                .in_progress
                .as_ref()
                .is_some_and(|p| p.is_first_hit())
            {
                break;
            }
        }
        assert!(
            pending
                .in_progress
                .as_ref()
                .is_some_and(|p| p.is_first_hit()),
            "bounded fixture must reach real FirstHit"
        );
        (state, pending)
    }
    type StageDistribution = HashMap<(State<2>, bool, Option<Pending>), f64>;
    fn expand_entry<P: Clone>(
        entry: &Entry<2, P>,
        pending: Option<Pending>,
        done: bool,
        out: &mut StageDistribution,
    ) {
        for component in &entry.components {
            let mut rows = vec![(entry.key.clone(), component.weight)];
            for (&unit, hp) in entry.units.iter().zip(&component.hps) {
                let mut next = Vec::new();
                for (state, p) in rows {
                    for &(offset, mass) in &hp.dist.points {
                        let mut s = state.clone();
                        s.pokemon_mut(unit_ref(unit)).hp = hp.base + offset;
                        next.push((s, p * mass));
                    }
                }
                rows = next;
            }
            for (s, p) in rows {
                *out.entry((s, done, pending.clone())).or_default() += p;
            }
        }
    }
    fn seeded(
        state: &State<2>,
        pending: &Pending,
        disabled: bool,
        base: i16,
        span: i16,
    ) -> (StageDistribution, Stats, hit_suffix::Counts) {
        let _guard = hit_suffix::TestDisableGuard::new(disabled);
        hit_suffix::test_observer_reset();
        let mut state = state.clone();
        state.pokemon_mut(TARGET).hp = base;
        let group = Group {
            state,
            pending: pending.clone(),
            weight: 1.0,
            lazy: vec![(
                unit_index(TARGET),
                Rc::new(Dist {
                    points: vec![(0, 0.25), (span, 0.75)],
                }),
            )],
        };
        let mut next = Positions::new();
        let mut finished = Positions::new();
        let mut stats = Stats::default();
        let mut buffers = RunBuffers::default();
        let mut stack = vec![group];
        while let Some(group) = stack.pop() {
            stats.groups += 1;
            let parts = run_group(
                group,
                &mut next,
                &mut finished,
                EnumerateOptions::default(),
                &mut turn::run_stage,
                &mut buffers,
                &mut stats,
                first_hit::Policy::ExactFull,
            )
            .unwrap();
            stack.extend(parts.into_iter().rev());
        }
        let mut out = StageDistribution::new();
        for entry in &next.entries {
            expand_entry(entry, Some(entry.rest.clone()), false, &mut out);
        }
        for entry in &finished.entries {
            expand_entry(entry, entry.rest.clone(), true, &mut out);
        }
        assert!((out.values().sum::<f64>() - 1.0).abs() < 1e-12);
        closed();
        (out, stats, hit_suffix::test_observer_snapshot())
    }
    #[test]
    fn seeded_lazy_ko_and_threshold_split_match_exact_stage_keys_and_weights() {
        let (state, pending) = first_hit_input(field(moves::EARTHQUAKE));
        for (base, span) in [(1, 1), (3, 5), (4, 1)] {
            let (off, a, _) = seeded(&state, &pending, true, base, span);
            let (on, b, c) = seeded(&state, &pending, false, base, span);
            assert_eq!(off.len(), on.len());
            for (k, p) in &off {
                assert_eq!(p.to_bits(), on.get(k).expect("full stage key").to_bits());
            }
            assert!(c.captures > 0 && c.resumes > 0);
            assert_eq!(a.splits, b.splits);
            assert_eq!(a.expansions, b.expansions);
            if base == 1 {
                assert!(on.keys().all(|(s, _, _)| s.pokemon(TARGET).hp == 0));
                #[cfg(feature = "experiment-lazy-ko-damage")]
                assert_eq!(b.expansions, 0);
            } else {
                // Without P1e, a universally lethal critical branch may request exact
                // HP expansion before the later mixed-threshold branch is reached.
                assert!(b.splits + b.expansions > 0 && c.lazy_discards > 0);
                #[cfg(feature = "experiment-lazy-ko-damage")]
                assert!(b.splits > 0);
                #[cfg(feature = "experiment-lazy-ko-damage")]
                if base == 4 {
                    assert!(
                        c.discarded_reached > 0,
                        "a later threshold must discard earlier buffered results"
                    );
                }
            }
            println!(
                "P7H_SEEDED base={base} span={span} runs={} splits={} expands={} counts={c:?}",
                b.runs, b.splits, b.expansions
            );
        }
    }
    #[test]
    fn pending_request_blocks_capture_and_error_still_wins_after_capture() {
        let (mut state, pending) = first_hit_input(field(moves::EARTHQUAKE));
        state.pokemon_mut(TARGET).hp = 100;
        for request_before in [true, false] {
            let _on = hit_suffix::TestDisableGuard::new(false);
            hit_suffix::test_observer_reset();
            let group = Group {
                state: state.clone(),
                pending: pending.clone(),
                weight: 1.0,
                lazy: vec![(
                    unit_index(TARGET),
                    Rc::new(Dist {
                        points: vec![(0, 0.5), (10, 0.5)],
                    }),
                )],
            };
            let mut next = Positions::new();
            let mut finished = Positions::new();
            let mut stats = Stats::default();
            let mut buffers = RunBuffers::default();
            let mut injected = false;
            let mut completed_leaves = 0;
            let result = run_group(
                group,
                &mut next,
                &mut finished,
                EnumerateOptions::default(),
                &mut |b, p| {
                    if request_before {
                        let _ = b.mon(TARGET).hp_value();
                        assert!(lazy::request_pending());
                    }
                    let end = turn::run_stage(b, p)?;
                    assert_eq!(b.hit_suffix_frame.is_some(), !request_before);
                    if !request_before && hit_suffix::test_observer_snapshot().resumes == 0 {
                        completed_leaves += 1;
                        return Ok(end);
                    }
                    let _ = b.mon(TARGET).hp_value();
                    assert!(lazy::request_pending());
                    injected = true;
                    Err(TurnError::Unsupported("p7h request then error".into()))
                },
                &mut buffers,
                &mut stats,
                first_hit::Policy::ExactFull,
            );
            assert!(
                matches!(result,Err(TurnError::Unsupported(ref s)) if s=="p7h request then error")
            );
            assert!(injected);
            if !request_before {
                assert!(completed_leaves > 0);
                assert!(hit_suffix::test_observer_snapshot().resumes > 0);
                assert!(
                    stats.runs > 1,
                    "discard follows a buffered leaf and a retained suffix"
                );
            }
            assert_eq!(
                stats.splits + stats.expansions,
                0,
                "error precedes request handling"
            );
            assert!(next.entries.iter().all(|e| e.components.is_empty()));
            assert!(finished.entries.iter().all(|e| e.components.is_empty()));
            closed();
        }
    }
    #[test]
    fn generic_pending_callback_has_no_concrete_frame_and_preserves_exact_branch_mass() {
        let state = field(moves::EARTHQUAKE);
        let mut calls = 0;
        let (rows, tv) = enumerate_factored(&state, 7_u8, FactoredOptions::default(), |b, p| {
            assert_eq!(*p, 7);
            assert!(b.hit_suffix_frame.is_none() && !b.hit_suffix_retained);
            let choice = b.rng.uniform(2);
            calls += 1;
            b.apply(Instruction::Damage {
                target: TARGET,
                amount: 1 + choice as i16,
            });
            *p = 8;
            Ok(StageEnd::Finished)
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(tv, 0.0);
        assert!((rows.iter().map(|r| r.probability).sum::<f64>() - 1.0).abs() < 1e-15);
        closed();
    }
    #[test]
    fn chooser_retention_keeps_nonuniform_roll_probability_bits_and_dfs_order() {
        let rolls = [3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 5, 5];
        fn run(
            rolls: &crate::damage::DamageRolls,
            retain: bool,
        ) -> (Vec<(usize, u16, usize, u64)>, usize) {
            let mut rng = Chooser::with_rolls(RollMode::Full);
            let mut rows = Vec::new();
            let mut entries = 0;
            let mut saved = None;
            let mut first = 0;
            loop {
                rng.begin_run();
                if let Some(s) = &saved {
                    rng.resume_checkpoint(s);
                } else {
                    first = rng.uniform(3);
                    entries += 1;
                    if retain {
                        saved = Some(rng.checkpoint());
                    }
                }
                let damage = rng.roll(rolls, SideId::One);
                let tail = rng.uniform(2);
                rows.push((first, damage, tail, rng.probability().to_bits()));
                if !rng.advance() {
                    break;
                }
                if saved.as_ref().is_some_and(|s| !rng.matches_checkpoint(s)) {
                    saved = None;
                }
            }
            (rows, entries)
        }
        let (off, a) = run(&rolls, false);
        let (on, b) = run(&rolls, true);
        assert_eq!(off, on);
        assert_eq!(on.len(), 18);
        assert_eq!(a, 18);
        assert_eq!(b, 3);
    }
}
