//! Bounded full-State and Suspension distribution checks for the KO-collapse experiment.
//! These are derived synthetic states, not public team sets or performance measurements.
use lab_engine::action::{Gimmick, SlotAction};
use lab_engine::dex::{abilities, items, moves, AbilityId, ItemId, MoveId, Type};
use lab_engine::field::Effect;
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{MoveSlot, PokemonRef, SideId, State, Status};
use lab_engine::turn::{enumerate_turn_factored_with, enumerate_turn_with, sample_turn,
    FactoredOptions, FactoredScope, EnumerateOptions, RollMode};
use lab_scenario::{load_scenario_file, scenario_positions};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;

fn hash(value: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn derived_state(second: MoveId, item: ItemId, target_ability: AbilityId, hp: i16, atk: i16) -> State<2> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios/single-hit.json");
    let loaded = load_scenario_file(file).unwrap();
    let mut state = scenario_positions(&loaded).unwrap().remove(0).state;
    state.field.fill(Effect::NONE);
    for side in &mut state.sides {
        // The original single-hit fixture selects only two members. Add one synthetic
        // reserve so U-turn can actually expose a mid-turn Suspension.
        if side.party[2].max_hp == 0 { side.party[2] = side.party[0].clone(); }
        side.effects.fill(Effect::NONE);
        for mon in &mut side.party {
            if mon.max_hp == 0 { continue; }
            mon.ability = abilities::NO_ABILITY;
            mon.base_ability = abilities::NO_ABILITY;
            mon.item = ItemId::NONE;
            mon.status = Status::None;
            mon.status_turns = 0;
            mon.hp = 180;
            mon.max_hp = 200;
            mon.stats = [100, 100, 100, 100, 20];
            mon.types = [Type::Normal, Type::None];
            mon.moves = [MoveSlot::full(moves::SWORDS_DANCE); 4];
        }
    }
    let first = state.pokemon_mut(PokemonRef { side: SideId::One, party: 0 });
    first.moves[0] = MoveSlot::full(moves::TACKLE);
    first.stats = [50, 100, 50, 100, 250];
    let next = state.pokemon_mut(PokemonRef { side: SideId::One, party: 1 });
    next.moves[0] = MoveSlot::full(second);
    next.stats = [atk, 100, atk, 100, 200];
    next.item = item;
    next.hp = 90; // Room for drain/Shell Bell and recoil to affect full State.
    let target = state.pokemon_mut(PokemonRef { side: SideId::Two, party: 0 });
    target.hp = hp;
    target.ability = target_ability;
    target.base_ability = target_ability;
    state
}

fn choices() -> [[SlotAction; 2]; 2] {
    let attack = SlotAction::Move { index: 0, target: 1, gimmick: Gimmick::None };
    let idle = SlotAction::Move { index: 0, target: 0, gimmick: Gimmick::None };
    [[attack, attack], [idle, idle]]
}

fn distribution(original: &State<2>, outcomes: Vec<Outcome>) -> BTreeMap<String, f64> {
    let mut state = original.clone();
    let before = format!("{state:?}");
    let before_hash = hash(&state);
    // Actual Eq/Hash keys (including hidden fields and pending state) also validate the Debug
    // representation used by the retained cross-binary records.
    let mut exact = HashMap::new();
    for outcome in outcomes {
        assert!(outcome.probability.is_finite() && outcome.probability >= 0.0);
        state.apply(&outcome.instructions);
        for side in &state.sides { for mon in &side.party {
            assert_eq!(format!("{:?}", mon.lazy), format!("{:?}", lab_engine::state::LazyTag::default()));
        }}
        *exact.entry((state.clone(), outcome.suspension)).or_insert(0.0) += outcome.probability;
        state.reverse(&outcome.instructions);
        assert_eq!(state, *original);
        assert_eq!(hash(&state), before_hash);
        assert_eq!(format!("{state:?}"), before);
    }
    let mut out = BTreeMap::new();
    for ((state, suspension), probability) in exact {
        assert!(out.insert(format!("{state:?}\n{suspension:?}"), probability).is_none(), "Debug must identify each Eq key");
    }
    let mass: f64 = out.values().sum();
    assert!((mass - 1.0).abs() <= 1e-12, "probability mass {mass}");
    out
}

#[test]
fn p1e_full_state_distributions_match_flat_and_rollback() {
    let cases = [
        ("all-ko", moves::TACKLE, ItemId::NONE, abilities::BATTLE_ARMOR, 60, 250),
        ("ko-threshold", moves::TACKLE, ItemId::NONE, abilities::BATTLE_ARMOR, 55, 150),
        ("drain", moves::GIGA_DRAIN, ItemId::NONE, abilities::BATTLE_ARMOR, 60, 250),
        ("recoil", moves::DOUBLE_EDGE, ItemId::NONE, abilities::BATTLE_ARMOR, 60, 250),
        ("shell-bell", moves::TACKLE, items::SHELL_BELL, abilities::BATTLE_ARMOR, 60, 250),
        ("innards-out", moves::TACKLE, ItemId::NONE, abilities::INNARDS_OUT, 60, 250),
        ("liquid-ooze", moves::GIGA_DRAIN, ItemId::NONE, abilities::LIQUID_OOZE, 60, 250),
        ("life-orb", moves::TACKLE, items::LIFE_ORB, abilities::BATTLE_ARMOR, 60, 250),
        ("uturn-suspension", moves::U_TURN, ItemId::NONE, abilities::BATTLE_ARMOR, 60, 250),
        ("multihit-survive", moves::DOUBLE_HIT, ItemId::NONE, abilities::BATTLE_ARMOR, 160, 180),
    ];
    let mut records = Vec::new();
    let mut total_keys = 0;
    let mut suspended_keys = 0;
    for (name, second, item, ability, hp, atk) in cases {
        let state = derived_state(second, item, ability, hp, atk);
        // Full on the small ordinary case; reduced rolls keep the mechanic-focused corpus
        // bounded while still creating non-singleton HP supports before the second move.
        let rolls = if name == "all-ko" { RollMode::Full } else { RollMode::Extremes };
        let options = EnumerateOptions { rolls };
        let flat = {
            let _scope = FactoredScope::new(false);
            let mut work = state.clone();
            let out = enumerate_turn_with(&mut work, Ruleset::CHAMPIONS_MC, choices(), options).unwrap();
            assert_eq!(format!("{work:?}"), format!("{state:?}"));
            distribution(&state, out)
        };
        let mut work = state.clone();
        let factored = enumerate_turn_factored_with(&mut work, Ruleset::CHAMPIONS_MC, choices(),
            FactoredOptions { rolls, max_support: None }).unwrap();
        assert_eq!(format!("{work:?}"), format!("{state:?}"));
        assert_eq!(factored.tv_bound, 0.0);
        let components = factored.outcomes.len();
        let expanded = factored.outcomes.into_iter().flat_map(|o| o.expand()).collect();
        let actual = distribution(&state, expanded);
        assert_eq!(flat.len(), actual.len(), "{name}: full State/Suspension support count");
        for (key, p) in &flat {
            let q = actual.get(key).unwrap_or_else(|| panic!("{name}: missing full State/Suspension"));
            assert!((p - q).abs() <= 1e-12, "{name}: probability {p} != {q}");
        }
        total_keys += actual.len();
        suspended_keys += actual.keys().filter(|k| !k.ends_with("\nNone")).count();
        let sampled = {
            let _scope = FactoredScope::new(false);
            let mut work = state.clone();
            let out = sample_turn(&mut work, Ruleset::CHAMPIONS_MC, choices(), 8, 2917).unwrap();
            assert_eq!(format!("{work:?}"), format!("{state:?}"));
            distribution(&state, out)
        };
        records.push(serde_json::json!({"schema":1,"case":name,"rolls":format!("{rolls:?}"),
            "input":format!("{state:?}"),"input_hash":hash(&state),"components":components,
            "distribution":actual,"samples":sampled}));
    }
    assert!(suspended_keys > 0, "at least one real U-turn suspension");
    if let Some(path) = std::env::var_os("LAB_P1E_PUBLIC_RECORDS") {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
        for row in &records { writeln!(file, "{}", serde_json::to_string(row).unwrap()).unwrap(); }
    }
    println!("P1E_PUBLIC cases={} full_state_keys={total_keys} suspension_keys={suspended_keys}", records.len());
}
