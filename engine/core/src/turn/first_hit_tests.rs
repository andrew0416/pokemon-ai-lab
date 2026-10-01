//! Bounded independent differential tests. No candidate support filtering.
#[cfg(test)]
mod tests {
    use crate::action::{Gimmick, SlotAction};
    use crate::dex::{abilities, items, moves, species, ItemId, MoveId, Type};
    use crate::field::{Effect, FieldEffect, Terrain};
    use crate::instruction::Outcome;
    use crate::rules::Ruleset;
    use crate::state::{MoveSlot, Pokemon, PokemonRef, SideId, SlotRef, State};
    use crate::volatile::{Volatile, VolatileState};
    use crate::turn::{self, first_hit, frontier, lazy, EnumerateOptions, FactoredOptions,
        FactoredScope, Pending, RollMode, Suspension, TurnError};
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};

    type Distribution = HashMap<(State<2>, Option<Suspension>), f64>;
    type Choices = [[SlotAction; 2]; 2];
    const ATTACKER: PokemonRef = PokemonRef { side: SideId::One, party: 0 };
    const TARGET: PokemonRef = PokemonRef { side: SideId::Two, party: 0 };
    const TARGET_SLOT: SlotRef = SlotRef { side: SideId::Two, slot: 0 };
    #[derive(Clone, Copy, Debug)]
    struct Counts { captures: u64, resumes: u64, prefixes: u64, fallbacks: u64 }
    fn counts() -> Counts {
        let c = first_hit::test_observer_snapshot();
        Counts { captures: c.captures as u64, resumes: c.resumes as u64,
            prefixes: c.prefix_entries as u64, fallbacks: c.fallbacks as u64 }
    }
    fn hash(value: &impl Hash) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut h); h.finish()
    }
    // Low attack/high defense retain nonuniform Full roll multiplicities with small support.
    fn field(mv: MoveId) -> State<2> {
        let mut state = State::<2> { turn: 1, ..State::default() };
        for side in [SideId::One, SideId::Two] {
            for party in 0..3 {
                let speed = match (side, party) {
                    (SideId::One, 0) => 250, (SideId::One, 1) => 180,
                    (SideId::Two, 0) => 130, _ => 80,
                };
                state.side_mut(side).party[party] = Pokemon {
                    species: species::PIKACHU, hp: 160, max_hp: 200, level: 50,
                    stats: [20, 200, 20, 200, speed], types: [Type::Normal, Type::None],
                    ability: abilities::NO_ABILITY, base_ability: abilities::NO_ABILITY,
                    moves: [MoveSlot::full(moves::SWORDS_DANCE); 4], ..Pokemon::default()
                };
            }
            for slot in 0..2 { state.side_mut(side).slots[slot].party_index = Some(slot as u8); }
            // Unused reader keeps battle.lastMove observable in the exact State key.
            state.side_mut(side).party[2].moves[3] = MoveSlot::full(moves::COPYCAT);
        }
        state.pokemon_mut(ATTACKER).moves[0] = MoveSlot::full(mv);
        if mv == moves::EXPANDING_FORCE {
            state.field[FieldEffect::Terrain as usize] = Effect { value: Terrain::Psychic as u8, turns: 5 };
        }
        state
    }
    fn action(index: u8, target: i8) -> SlotAction {
        SlotAction::Move { index, target, gimmick: Gimmick::None }
    }
    fn choices(mv: MoveId) -> Choices {
        let target = if [moves::TACKLE, moves::DOUBLE_HIT, moves::DRAGON_DARTS,
            moves::U_TURN, moves::EXPANDING_FORCE].contains(&mv) { 1 } else { 0 };
        [[action(0, target), action(0, 0)], [action(0, 0), action(0, 0)]]
    }
    fn unchanged(work: &State<2>, original: &State<2>) {
        assert_eq!(work, original);
        assert_eq!(hash(work), hash(original));
        assert_eq!(format!("{work:?}"), format!("{original:?}"), "Eq ignores lazy tags");
    }
    fn distribution(original: &State<2>, outcomes: Vec<Outcome>) -> Distribution {
        assert!(outcomes.len() <= 100_000, "bounded test unexpectedly expanded");
        let mut out = Distribution::new();
        let mut work = original.clone();
        for outcome in outcomes {
            assert!(outcome.probability.is_finite() && outcome.probability > 0.0);
            work.apply(&outcome.instructions);
            for side in &work.sides { for mon in &side.party {
                assert_eq!(mon.lazy.0, 0, "public outcomes retain no lazy tags");
            }}
            *out.entry((work.clone(), outcome.suspension)).or_default() += outcome.probability;
            work.reverse(&outcome.instructions);
            unchanged(&work, original);
        }
        // Actual Eq/Hash is primary; additionally check the export Debug view is injective.
        let mut texts = HashSet::new();
        for key in out.keys() { assert!(texts.insert(format!("{key:?}"))); }
        let mass: f64 = out.values().sum();
        assert!((mass - 1.0).abs() <= 1e-12, "raw mass {mass:.17}");
        out
    }
    fn same(name: &str, expected: &Distribution, actual: &Distribution) {
        assert_eq!(expected.len(), actual.len(), "{name}: full State/Suspension support");
        for (key, p) in expected {
            let q = actual.get(key).unwrap_or_else(|| panic!("{name}: missing key {key:?}"));
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
    fn factored(state: &State<2>, actions: Choices, options: FactoredOptions,
        disabled: bool) -> (Distribution, Counts) {
        let _disabled = first_hit::TestDisableGuard::new(disabled);
        first_hit::test_observer_reset();
        let mut work = state.clone();
        let result = turn::enumerate_turn_factored_with(&mut work, Ruleset::CHAMPIONS_MC,
            actions, options).unwrap();
        unchanged(&work, state);
        if options.max_support.is_none() { assert_eq!(result.tv_bound, 0.0); }
        let expanded = result.outcomes.iter().flat_map(|o| o.expand()).collect();
        let seen = counts();
        let out = distribution(state, expanded);
        tls_closed(state);
        (out, seen)
    }
    fn compare(name: &str, state: &State<2>, actions: Choices, flat: bool)
        -> (Distribution, Counts, Counts) {
        let options = FactoredOptions { rolls: RollMode::Full, max_support: None };
        let (off, off_counts) = factored(state, actions, options, true);
        let (on, on_counts) = factored(state, actions, options, false);
        same(name, &off, &on);
        assert_eq!(off_counts.captures, 0);
        if flat {
            let _scope = FactoredScope::new(false);
            let _disabled = first_hit::TestDisableGuard::new(false);
            first_hit::test_observer_reset();
            let mut work = state.clone();
            let outcomes = turn::enumerate_turn_with(&mut work, Ruleset::CHAMPIONS_MC,
                actions, EnumerateOptions { rolls: RollMode::Full }).unwrap();
            unchanged(&work, state);
            assert_eq!(counts().captures, 0, "ordinary flat enumeration is outside P7d");
            same(name, &distribution(state, outcomes), &on);
            tls_closed(state);
        }
        println!("P7D_CASE {name} keys={} OFF={off_counts:?} ON={on_counts:?}", on.len());
        (on, off_counts, on_counts)
    }

    #[test]
    fn full_spread_matches_flat_and_strictly_reduces_prefix_execution() {
        for (name, mv) in [("earthquake-three-target", moves::EARTHQUAKE),
            ("rock-slide-accuracy-secondary", moves::ROCK_SLIDE), ("surf-three-target", moves::SURF)] {
            let state = field(mv);
            let (out, off, on) = compare(name, &state, choices(mv), true);
            assert!(out.len() > 1, "Full must actually branch");
            assert!(on.captures > 0 && on.resumes > 0, "{name}: must activate");
            assert!(on.prefixes < off.prefixes, "{name}: prefix work {off:?} -> {on:?}");
            assert!(out.keys().any(|(s, _)| s.pokemon(TARGET).hp < state.pokemon(TARGET).hp));
            assert!(out.keys().all(|(s, _)| s.pokemon(ATTACKER).moves[0].pp == state.pokemon(ATTACKER).moves[0].pp - 1));
        }
    }
    #[test]
    fn pressure_items_substitute_and_ko_boundaries_keep_full_state() {
        for case in 0..8 {
            let mut state = field(moves::EARTHQUAKE);
            match case {
                0 => { state.pokemon_mut(TARGET).ability = abilities::PRESSURE;
                    state.pokemon_mut(TARGET).base_ability = abilities::PRESSURE; }
                1 => { state.pokemon_mut(TARGET).hp = 3; }
                2 => { let m = state.pokemon_mut(TARGET); m.hp = 3; m.max_hp = 3; m.item = items::FOCUS_SASH; }
                3 => { let m = state.pokemon_mut(TARGET); m.hp = 3; m.max_hp = 3;
                    m.ability = abilities::STURDY; m.base_ability = abilities::STURDY; }
                4 => { state.pokemon_mut(ATTACKER).item = items::LIFE_ORB; }
                5 => { let m = state.pokemon_mut(TARGET); m.hp = 101; m.item = items::SITRUS_BERRY; }
                6 => { let slot = state.slot_mut(TARGET_SLOT); slot.substitute_hp = 3;
                    slot.volatiles.set(Volatile::Substitute, VolatileState { active: true, ..VolatileState::NONE }); }
                7 => { state.pokemon_mut(TARGET).item = items::RED_CARD; }
                _ => unreachable!(),
            }
            compare(&format!("edge-{case}"), &state, choices(moves::EARTHQUAKE), false);
        }
    }
    #[test]
    fn previous_damage_lazy_hp_and_prefix_status_branches_match() {
        let mut state = field(moves::TACKLE);
        state.pokemon_mut(ATTACKER).stats[0] = 50;
        state.side_mut(SideId::One).party[1].moves[0] = MoveSlot::full(moves::EARTHQUAKE);
        state.side_mut(SideId::One).party[1].status = crate::state::Status::Paralyze;
        state.side_mut(SideId::One).party[1].stats[4] = 240;
        state.side_mut(SideId::Two).party[0].stats[4] = 40;
        state.side_mut(SideId::Two).party[1].stats[4] = 30;
        state.pokemon_mut(TARGET).hp = 10;
        let actions = [[action(0, 1), action(0, 0)], [action(0, 0), action(0, 0)]];
        let (_, _, on) = compare("prior-damage-paralysis-lazy", &state, actions, true);
        assert!(on.captures > 0 && on.resumes > 0);
    }
    #[test]
    fn real_eject_button_suspension_and_resume_preserve_pending() {
        let mut state = field(moves::EARTHQUAKE);
        state.pokemon_mut(TARGET).item = items::EJECT_BUTTON;
        let (first, _, on) = compare("eject-button", &state, choices(moves::EARTHQUAKE), true);
        assert!(on.captures > 0);
        assert!(first.keys().any(|(_, pending)| pending.is_some()), "real mid-turn pause required");
        let mut checked = 0;
        for ((paused, suspension), _) in &first {
            let Some(suspension) = suspension else { continue };
            assert!(paused.slot(TARGET_SLOT).must_switch_out());
            let replacements = [[None, None], [Some(2), None]];
            let mut arms = Vec::new();
            for disabled in [true, false] {
                let _disabled = first_hit::TestDisableGuard::new(disabled);
                let mut work = paused.clone();
                let out = turn::resume_turn_factored(&mut work, suspension, replacements,
                    EnumerateOptions { rolls: RollMode::Full }).unwrap();
                unchanged(&work, paused);
                let dist = distribution(paused, out.iter().flat_map(|o| o.expand()).collect());
                assert!(dist.keys().all(|(_, p)| p.is_none()));
                arms.push(dist); tls_closed(paused);
            }
            same("resume-real-suspension", &arms[0], &arms[1]);
            checked += 1;
        }
        assert!(checked > 0);
    }
    #[test]
    fn reduced_rolls_approximation_flat_and_sampling_do_not_capture() {
        let state = field(moves::ROCK_SLIDE);
        let actions = choices(moves::ROCK_SLIDE);
        for rolls in [RollMode::Median, RollMode::Extremes, RollMode::Quartiles,
            RollMode::Fixed(3), RollMode::Pessimistic(SideId::One)] {
            let options = FactoredOptions { rolls, max_support: None };
            let (off, _) = factored(&state, actions, options, true);
            let (on, seen) = factored(&state, actions, options, false);
            same("reduced-roll fallback", &off, &on);
            assert_eq!(seen.captures, 0); assert_eq!(seen.resumes, 0);
        }
        let options = FactoredOptions { rolls: RollMode::Full, max_support: Some(1) };
        let (off, _) = factored(&state, actions, options, true);
        let (on, seen) = factored(&state, actions, options, false);
        same("capped fallback", &off, &on); assert_eq!(seen.captures, 0);
        for seed in [7, 2917] {
            let mut arms = Vec::new();
            for disabled in [true, false] {
                let _scope = FactoredScope::new(false);
                let _disabled = first_hit::TestDisableGuard::new(disabled);
                first_hit::test_observer_reset();
                let mut work = state.clone();
                let out = turn::sample_turn(&mut work, Ruleset::CHAMPIONS_MC, actions, 12, seed).unwrap();
                unchanged(&work, &state); assert_eq!(counts().captures, 0);
                arms.push(distribution(&state, out));
            }
            assert_eq!(arms[0], arms[1], "sampling retains seed-to-outcome mapping");
        }
    }
    #[test]
    fn excluded_single_target_multihit_called_and_selfdestruct_use_fallback() {
        for mv in [moves::TACKLE, moves::DOUBLE_HIT, moves::DRAGON_DARTS,
            moves::COPYCAT, moves::EXPLOSION, moves::EXPANDING_FORCE] {
            let mut state = field(mv);
            state.last_move = moves::EARTHQUAKE;
            if mv == moves::EXPLOSION { state.pokemon_mut(ATTACKER).level = 1; }
            let (_, _, seen) = compare(mv.data().name, &state, choices(mv), false);
            assert_eq!(seen.captures, 0, "{} must use old execution", mv.data().name);
        }
    }
    #[test]
    fn speed_ties_and_dirty_mirror_herb_prefix_match_without_early_update() {
        let mut state = field(moves::EARTHQUAKE);
        state.pokemon_mut(ATTACKER).item = items::BLUNDER_POLICY;
        // Three targets: a single miss still leaves two hits at the checkpoint gate.
        state.side_mut(SideId::One).slots[0].boosts[5] = -1;
        state.pokemon_mut(TARGET).item = items::MIRROR_HERB;
        state.pokemon_mut(TARGET).ability = abilities::BATTLE_ARMOR;
        state.pokemon_mut(TARGET).base_ability = abilities::BATTLE_ARMOR;
        state.side_mut(SideId::Two).party[1].ability = abilities::BATTLE_ARMOR;
        state.side_mut(SideId::Two).party[1].base_ability = abilities::BATTLE_ARMOR;
        let (out, _, seen) = compare("dirty-mirror-herb", &state, choices(moves::EARTHQUAKE), true);
        assert!(out.keys().any(|(s, _)| s.pokemon(ATTACKER).item == ItemId::NONE), "Blunder Policy must activate");
        assert!(seen.fallbacks > 0, "pending Mirror Herb scratch must fall back");
        let mut tied = field(moves::EARTHQUAKE);
        tied.side_mut(SideId::One).party[1].stats[4] = 250;
        compare("action-speed-tie", &tied, choices(moves::EARTHQUAKE), true);
    }
    #[test]
    fn errors_after_real_prefix_and_resumed_work_restore_input_and_tls() {
        let state = field(moves::EARTHQUAKE);
        let actions = choices(moves::EARTHQUAKE);
        for after_resume in [false, true] {
            let _disabled = first_hit::TestDisableGuard::new(false);
            first_hit::test_observer_reset();
            let before = state.clone();
            let mut injected = false;
            let mut had_log = false;
            let result = frontier::enumerate_factored(&state,
                Pending::new(turn::initial_queue(&state, &actions)),
                FactoredOptions { rolls: RollMode::Full, max_support: None },
                |b, pending| {
                    let end = turn::run_stage(b, pending)?;
                    let observed = counts();
                    let reached = if after_resume { observed.resumes > 0 }
                        else { observed.captures > 0 && pending.in_progress.is_some() };
                    if reached {
                        had_log = !b.log.is_empty();
                        injected = true;
                        return Err(TurnError::Unsupported("test-only failure after actual first-hit work".into()));
                    }
                    Ok(end)
                });
            assert!(matches!(result, Err(TurnError::Unsupported(ref s))
                if s == "test-only failure after actual first-hit work"));
            assert!(injected && had_log, "error must occur after real mutations");
            unchanged(&state, &before); tls_closed(&state);
        }
        let (_, _, seen) = compare("success-after-errors", &state, actions, false);
        assert!(seen.captures > 0 && seen.resumes > 0);
    }
    #[test]
    fn disabled_scope_is_nested_and_does_not_leak() {
        let state = field(moves::EARTHQUAKE);
        let actions = choices(moves::EARTHQUAKE);
        let _outer = first_hit::TestDisableGuard::new(true);
        let (_, inner) = factored(&state, actions, FactoredOptions::default(), false);
        assert!(inner.captures > 0);
        first_hit::test_observer_reset();
        let mut work = state.clone();
        turn::enumerate_turn_factored_with(&mut work, Ruleset::CHAMPIONS_MC, actions,
            FactoredOptions::default()).unwrap();
        assert_eq!(counts().captures, 0, "inner guard must restore outer state");
        unchanged(&work, &state);
    }
}
