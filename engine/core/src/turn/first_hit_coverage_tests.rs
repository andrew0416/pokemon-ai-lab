//! P7f additional Full adoption coverage. Diagnostic errors are not canonicalized.
#[cfg(test)]
mod tests {
    use crate::action::{Gimmick, SlotAction};
    use crate::dex::{
        abilities, items, moves, species, ItemId, MoveFlags, MoveId, SelfSwitch, Type,
    };
    use crate::field::{Effect, FieldEffect, Terrain};
    use crate::instruction::Outcome;
    use crate::rules::Ruleset;
    use crate::state::{MoveSlot, Pokemon, PokemonRef, SideId, SlotRef, State};
    use crate::turn::{
        self, first_hit, lazy, EnumerateOptions, FactoredOptions, FactoredScope, RollMode,
        Suspension, TurnError,
    };
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};

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
    const TARGET_SLOT: SlotRef = SlotRef {
        side: SideId::Two,
        slot: 0,
    };
    #[derive(Clone, Copy, Debug)]
    struct Counts {
        captures: u64,
        resumes: u64,
        prefixes: u64,
        fallbacks: u64,
    }
    fn counts() -> Counts {
        let c = first_hit::test_observer_snapshot();
        Counts {
            captures: c.captures as u64,
            resumes: c.resumes as u64,
            prefixes: c.prefix_entries as u64,
            fallbacks: c.fallbacks as u64,
        }
    }
    fn hash(value: &impl Hash) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut h);
        h.finish()
    }
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
    fn unchanged(work: &State<2>, original: &State<2>) {
        assert_eq!(work, original);
        assert_eq!(hash(work), hash(original));
        assert_eq!(
            format!("{work:?}"),
            format!("{original:?}"),
            "Eq ignores lazy tags"
        );
    }
    fn distribution(original: &State<2>, outcomes: Vec<Outcome>) -> Distribution {
        assert!(
            outcomes.len() <= 100_000,
            "bounded test unexpectedly expanded"
        );
        let mut out = Distribution::new();
        let mut work = original.clone();
        for outcome in outcomes {
            assert!(outcome.probability.is_finite() && outcome.probability > 0.0);
            work.apply(&outcome.instructions);
            for side in &work.sides {
                for mon in &side.party {
                    assert_eq!(mon.lazy.0, 0, "public outcomes retain no lazy tags");
                }
            }
            *out.entry((work.clone(), outcome.suspension)).or_default() += outcome.probability;
            work.reverse(&outcome.instructions);
            unchanged(&work, original);
        }
        // Actual Eq/Hash is primary; additionally check the export Debug view is injective.
        let mut texts = HashSet::new();
        for key in out.keys() {
            assert!(texts.insert(format!("{key:?}")));
        }
        let mass: f64 = out.values().sum();
        assert!((mass - 1.0).abs() <= 1e-12, "raw mass {mass:.17}");
        out
    }
    fn same(name: &str, expected: &Distribution, actual: &Distribution) {
        assert_eq!(
            expected.len(),
            actual.len(),
            "{name}: full State/Suspension support"
        );
        for (key, p) in expected {
            let q = actual
                .get(key)
                .unwrap_or_else(|| panic!("{name}: missing key {key:?}"));
            assert!((p - q).abs() <= 1e-12, "{name}: {p:.17} != {q:.17}");
        }
    }
    fn tls_closed(state: &State<2>) {
        assert_eq!(lazy::take_request(), None);
        let mut probe = state.pokemon(ATTACKER).clone();
        probe.lazy = lazy::tag(0);
        let expected = probe.hp;
        assert_eq!(probe.hp_value(), expected);
        assert_eq!(lazy::take_request(), None, "driver must close TLS");
        lazy::begin(&[(0, 3)]);
        assert_eq!(lazy::take_request(), None);
        assert_eq!(probe.hp_value(), expected);
        let request = lazy::take_request();
        lazy::end();
        assert_eq!(request, Some((0, lazy::Request::Expand)));
    }
    fn factored(
        state: &State<2>,
        actions: Choices,
        options: FactoredOptions,
        disabled: bool,
    ) -> (Distribution, Counts) {
        let _disabled = first_hit::TestDisableGuard::new(disabled);
        first_hit::test_observer_reset();
        let mut work = state.clone();
        let result =
            turn::enumerate_turn_factored_with(&mut work, Ruleset::CHAMPIONS_MC, actions, options)
                .unwrap();
        unchanged(&work, state);
        if options.max_support.is_none() {
            assert_eq!(result.tv_bound, 0.0);
        }
        let expanded = result.outcomes.iter().flat_map(|o| o.expand()).collect();
        let seen = counts();
        let out = distribution(state, expanded);
        tls_closed(state);
        (out, seen)
    }
    fn compare(
        name: &str,
        state: &State<2>,
        actions: Choices,
        flat: bool,
    ) -> (Distribution, Counts, Counts) {
        let options = FactoredOptions {
            rolls: RollMode::Full,
            max_support: None,
        };
        let (off, off_counts) = factored(state, actions, options, true);
        let (on, on_counts) = factored(state, actions, options, false);
        same(name, &off, &on);
        assert_eq!(off_counts.captures, 0);
        if flat {
            let _scope = FactoredScope::new(false);
            let _disabled = first_hit::TestDisableGuard::new(false);
            first_hit::test_observer_reset();
            let mut work = state.clone();
            let outcomes = turn::enumerate_turn_with(
                &mut work,
                Ruleset::CHAMPIONS_MC,
                actions,
                EnumerateOptions {
                    rolls: RollMode::Full,
                },
            )
            .unwrap();
            unchanged(&work, state);
            assert_eq!(
                counts().captures,
                0,
                "ordinary flat enumeration is outside P7d"
            );
            same(name, &distribution(state, outcomes), &on);
            tls_closed(state);
        }
        println!(
            "P7F_CASE {name} keys={} OFF={off_counts:?} ON={on_counts:?}",
            on.len()
        );
        (on, off_counts, on_counts)
    }

    const ALLY: PokemonRef = PokemonRef {
        side: SideId::One,
        party: 1,
    };
    const FOE_OTHER: PokemonRef = PokemonRef {
        side: SideId::Two,
        party: 1,
    };
    const FAINT_ERROR: &str = "Zygarde-Complete fainting";
    const TRICK_ERROR: &str = "Trick moving Booster Energy";

    fn refuse(state: &State<2>, actions: Choices, disabled: bool) -> (String, Counts) {
        let _disabled = first_hit::TestDisableGuard::new(disabled);
        first_hit::test_observer_reset();
        let mut work = state.clone();
        let error = turn::enumerate_turn_factored_with(
            &mut work,
            Ruleset::CHAMPIONS_MC,
            actions,
            FactoredOptions {
                rolls: RollMode::Full,
                max_support: None,
            },
        )
        .unwrap_err();
        let TurnError::Unsupported(message) = error else {
            panic!("not a runtime Unsupported: {error:?}");
        };
        let seen = counts();
        unchanged(&work, state);
        tls_closed(state);
        (message, seen)
    }

    #[test]
    fn distinct_reachable_unsupported_paths_preserve_refusal_and_cleanup() {
        let mut state = field(moves::EARTHQUAKE);
        state.pokemon_mut(ALLY).moves[0] = MoveSlot::full(moves::TRICK);
        state.pokemon_mut(ALLY).item = items::BOOSTER_ENERGY;
        let tied_speed = state.pokemon(ATTACKER).stats[4];
        state.pokemon_mut(ALLY).stats[4] = tied_speed;
        state.pokemon_mut(TARGET).species = species::ZYGARDE_COMPLETE;
        state.pokemon_mut(TARGET).hp = 1;
        let actions = [[action(0, 0), action(0, 1)], [action(0, 0), action(0, 0)]];
        let mut faint_only = state.clone();
        faint_only.pokemon_mut(ALLY).item = ItemId::NONE;
        let mut trick_only = state.clone();
        trick_only.pokemon_mut(TARGET).species = species::PIKACHU;
        trick_only.pokemon_mut(TARGET).hp = 160;
        for disabled in [true, false] {
            let (faint, seen) = refuse(&faint_only, actions, disabled);
            assert!(
                faint.starts_with(FAINT_ERROR),
                "single-refusal control: {faint}"
            );
            if !disabled {
                assert!(
                    seen.captures > 0 && seen.resumes > 0,
                    "error after real resumed work"
                );
            }
            let (trick, _) = refuse(&trick_only, actions, disabled);
            assert!(
                trick.starts_with(TRICK_ERROR),
                "single-refusal control: {trick}"
            );
        }
        let (off, off_seen) = refuse(&state, actions, true);
        let (on, on_seen) = refuse(&state, actions, false);
        for message in [&off, &on] {
            assert!(
                message.starts_with(FAINT_ERROR) || message.starts_with(TRICK_ERROR),
                "unexpected refusal {message}"
            );
        }
        assert_eq!(off_seen.captures, 0);
        assert!(
            on_seen.captures > 0,
            "must reach a real checkpoint before the alternate prefix"
        );
        // No equality/canonicalization assumption: both public calls must refuse and restore.
        println!(
            "P7F_ERROR_ORDER off={off:?} on={on:?} same_message={} OFF={off_seen:?} ON={on_seen:?}",
            off == on
        );
        let healthy = field(moves::EARTHQUAKE);
        let (_, _, seen) = compare(
            "success-after-two-natural-refusals",
            &healthy,
            choices(moves::EARTHQUAKE),
            false,
        );
        assert!(seen.captures > 0 && seen.resumes > 0);
    }

    #[test]
    fn newly_registered_callbacks_and_event_orders_fail_checkpoint_capability() {
        let plain = moves::EARTHQUAKE.data().clone();
        assert!(first_hit::data_capable(&plain));
        // A future callback string must not need a newly maintained move-name denylist.
        let mut callback = plain.clone();
        callback.handlers = &["onFutureUnregisteredCheckpointEvent"];
        assert!(!first_hit::data_capable(&callback));
        let mut order = plain.clone();
        order.event_orders = &[("onFutureUnregisteredEventPriority", 17)];
        assert!(!first_hit::data_capable(&order));
        let mut called = plain.clone();
        called.calls_move = true;
        assert!(!first_hit::data_capable(&called));
        let mut multi = plain.clone();
        multi.multihit = Some((2, 2));
        assert!(!first_hit::data_capable(&multi));
        let mut switch = plain.clone();
        switch.self_switch = SelfSwitch::Yes;
        assert!(!first_hit::data_capable(&switch));
        let mut future = plain;
        future.flags = MoveFlags(future.flags.bits() | MoveFlags::FUTUREMOVE.bits());
        assert!(!first_hit::data_capable(&future));
        // These are metadata contract assertions, not execution of an invented handler.
        let state = field(moves::EXPANDING_FORCE);
        assert!(!moves::EXPANDING_FORCE.data().handlers.is_empty());
        let (out, _, seen) = compare(
            "existing-callback-spread",
            &state,
            choices(moves::EXPANDING_FORCE),
            true,
        );
        assert_eq!(seen.captures, 0);
        assert!(out
            .keys()
            .any(|(s, _)| s.pokemon(TARGET).hp < state.pokemon(TARGET).hp));
    }

    #[test]
    fn mega_spread_then_real_pause_resume_preserves_gimmick_and_pending() {
        let mut state = field(moves::HYPER_VOICE);
        let mon = state.pokemon_mut(ATTACKER);
        mon.species = species::SALAMENCE;
        mon.item = items::SALAMENCITE;
        mon.gimmicks = crate::gimmick::structural_gimmicks(mon.species, mon.item);
        mon.level = 1; // Small Full support, while Mega uses real stored stat recomputation.
        mon.set_forme(mon.forme_as(species::SALAMENCE));
        mon.hp = mon.max_hp;
        state.pokemon_mut(TARGET).item = items::EJECT_BUTTON;
        for p in [ALLY, TARGET, FOE_OTHER] {
            state.pokemon_mut(p).stats[3] = 1000;
            state.pokemon_mut(p).stats[4] = 1;
        }
        let mut actions = choices(moves::HYPER_VOICE);
        actions[0][0] = SlotAction::Move {
            index: 0,
            target: 0,
            gimmick: Gimmick::Mega,
        };
        let (first, _, on) = compare("mega-hyper-voice-eject-button", &state, actions, true);
        assert!(on.captures > 0 && on.resumes > 0);
        assert!(first.keys().all(
            |(s, _)| s.pokemon(ATTACKER).species == species::SALAMENCE_MEGA
                && s.side(SideId::One).gimmicks_used.contains(Gimmick::Mega)
        ));
        let mut resumed = 0;
        for ((paused, pending), _) in first {
            let Some(pending) = pending else { continue };
            assert!(paused.slot(TARGET_SLOT).must_switch_out());
            let mut arms = Vec::new();
            for disabled in [true, false] {
                let _disabled = first_hit::TestDisableGuard::new(disabled);
                let mut work = paused.clone();
                let out = turn::resume_turn_factored(
                    &mut work,
                    &pending,
                    [[None, None], [Some(2), None]],
                    EnumerateOptions {
                        rolls: RollMode::Full,
                    },
                )
                .unwrap();
                unchanged(&work, &paused);
                let dist = distribution(&paused, out.iter().flat_map(|o| o.expand()).collect());
                assert!(dist.keys().all(|(s, p)| p.is_none()
                    && s.pokemon(ATTACKER).species == species::SALAMENCE_MEGA
                    && s.side(SideId::One).gimmicks_used.contains(Gimmick::Mega)));
                arms.push(dist);
                tls_closed(&paused);
            }
            same("mega-resume", &arms[0], &arms[1]);
            resumed += 1;
        }
        assert!(
            resumed > 0,
            "must resume an actual public replacement request"
        );
    }

    #[test]
    fn fractional_priority_is_drawn_once_across_first_hit_and_changes_real_survival() {
        for (label, item, ability, expected) in [
            ("quick-claw", items::QUICK_CLAW, abilities::NO_ABILITY, 0.2),
            ("quick-draw", ItemId::NONE, abilities::QUICK_DRAW, 0.3),
            ("custap", items::CUSTAP_BERRY, abilities::NO_ABILITY, 1.0),
        ] {
            let mut state = field(moves::EARTHQUAKE);
            let mon = state.pokemon_mut(ATTACKER);
            mon.hp = 3;
            mon.item = item;
            mon.ability = ability;
            mon.base_ability = ability;
            mon.stats[0] = 100;
            mon.stats[4] = 30;
            let foe = state.pokemon_mut(TARGET);
            foe.hp = 3;
            foe.moves[0] = MoveSlot::full(moves::TACKLE);
            foe.stats[0] = 100;
            foe.stats[4] = 250;
            state.pokemon_mut(ALLY).stats[4] = 20;
            state.pokemon_mut(FOE_OTHER).stats[4] = 10;
            let actions = [[action(0, 0), action(0, 0)], [action(0, 1), action(0, 0)]];
            let (out, _, seen) = compare(label, &state, actions, true);
            assert!(seen.captures > 0 && seen.resumes > 0);
            let survives: f64 = out
                .iter()
                .filter(|((s, _), _)| s.pokemon(ATTACKER).hp > 0)
                .map(|(_, p)| p)
                .sum();
            assert!(
                (survives - expected).abs() < 1e-12,
                "{label}: survival mass {survives}"
            );
            assert!(out
                .keys()
                .filter(|(s, _)| s.pokemon(ATTACKER).hp > 0)
                .all(|(s, _)| s.pokemon(TARGET).hp == 0));
            if item == items::CUSTAP_BERRY {
                assert!(out
                    .keys()
                    .all(|(s, _)| s.pokemon(ATTACKER).item == ItemId::NONE));
            }
        }
    }

    #[test]
    fn called_spread_and_called_multihit_execute_but_never_capture_direct_checkpoint() {
        let mut sleep = field(moves::SLEEP_TALK);
        let mon = sleep.pokemon_mut(ATTACKER);
        mon.moves = [
            MoveSlot::full(moves::SLEEP_TALK),
            MoveSlot::full(moves::EARTHQUAKE),
            MoveSlot::full(moves::REST),
            MoveSlot::full(moves::REST),
        ];
        mon.status = crate::state::Status::Sleep;
        mon.status_turns = 3;
        let (out, _, seen) = compare(
            "sleep-talk-calls-spread",
            &sleep,
            choices(moves::SLEEP_TALK),
            true,
        );
        assert_eq!(seen.captures, 0);
        assert!(out
            .keys()
            .any(|(s, _)| s.pokemon(TARGET).hp < sleep.pokemon(TARGET).hp));
        assert!(out.keys().all(|(s, _)| s.pokemon(ATTACKER).moves[0].pp
            == sleep.pokemon(ATTACKER).moves[0].pp - 1
            && s.pokemon(ATTACKER).moves[1].pp == sleep.pokemon(ATTACKER).moves[1].pp));
        let mut copy = field(moves::COPYCAT);
        copy.last_move = moves::DOUBLE_HIT;
        copy.side_mut(SideId::One).party[2].moves[2] = MoveSlot::full(moves::RAGE_FIST);
        let (out, _, seen) = compare(
            "copycat-calls-two-hit",
            &copy,
            choices(moves::COPYCAT),
            true,
        );
        assert_eq!(seen.captures, 0);
        assert!(
            out.keys().any(|(s, _)| s.sides.iter().any(|side| side
                .slots
                .iter()
                .any(|slot| slot.history.times_attacked >= 2))),
            "a real called multi-hit must reach both hits; mere move registration is insufficient"
        );
    }
}
