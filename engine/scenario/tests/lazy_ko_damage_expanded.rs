//! P1e2: full-State/Suspension differential corpus and observable mechanic activation.
//! Same test-only files run against immutable original a4 and candidate OFF/ON.
//! No canonical-state projection substitutes for the real Eq/Hash keys below.
mod p1e2_cases;
use lab_engine::dex::{items, moves};
use lab_engine::instruction::Outcome;
use lab_engine::rules::Ruleset;
use lab_engine::state::{LazyTag, State};
use lab_engine::turn::{sample_turn, EnumerateOptions, FactoredScope, Suspension};
use lab_engine::volatile::Volatile;
use lab_scenario::{run_decision_mid_turn_with, Decision};
use p1e2_cases::{corpus, Case, Witness};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::Write;

fn hash(value: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}
fn distribution(original: &State<2>, outcomes: &[Outcome], id: &str) -> BTreeMap<String, f64> {
    let mut state = original.clone();
    let before = format!("{state:?}");
    let before_hash = hash(&state);
    let mut exact: HashMap<(State<2>, Option<Suspension>), f64> = HashMap::new();
    for outcome in outcomes {
        assert!(
            outcome.probability.is_finite() && outcome.probability >= 0.0,
            "{id}: bad probability"
        );
        state.apply(&outcome.instructions);
        for side in &state.sides {
            for mon in &side.party {
                assert_eq!(
                    format!("{:?}", mon.lazy),
                    format!("{:?}", LazyTag::default()),
                    "{id}: escaped live lazy tag"
                );
            }
        }
        // Eq/Hash also invokes the concrete-only pending damage contract when a key is inserted.
        *exact
            .entry((state.clone(), outcome.suspension.clone()))
            .or_insert(0.0) += outcome.probability;
        state.reverse(&outcome.instructions);
        assert_eq!(state, *original, "{id}: rollback state");
        assert_eq!(hash(&state), before_hash, "{id}: rollback hash");
        assert_eq!(format!("{state:?}"), before, "{id}: rollback debug");
    }
    assert!(!exact.is_empty(), "{id}: no outcomes");
    assert!(
        exact.len() <= 200_000,
        "{id}: bounded corpus unexpectedly grew"
    );
    let mut out = BTreeMap::new();
    for ((state, suspension), probability) in exact {
        let key = format!("{state:?}\n{suspension:?}");
        assert!(
            out.insert(key, probability).is_none(),
            "{id}: Debug collided across actual Eq keys"
        );
    }
    let mass: f64 = out.values().sum();
    assert!(
        mass.is_finite() && (mass - 1.0).abs() <= 1e-12,
        "{id}: mass {mass}"
    );
    // Use the same explicitly normalized probability contract in every binary.
    for value in out.values_mut() {
        *value /= mass;
    }
    out
}
fn saw(witness: &Witness, original: &State<2>, outcome: &Outcome) -> bool {
    let mut end = original.clone();
    end.apply(&outcome.instructions);
    let move_used = |mv| {
        original
            .sides
            .iter()
            .zip(&end.sides)
            .any(|(before, after)| {
                before.party.iter().zip(&after.party).any(|(old, new)| {
                    old.moves
                        .iter()
                        .zip(&new.moves)
                        .any(|(a, b)| a.id == mv && b.id == mv && b.pp < a.pp)
                })
            })
    };
    // Outcome instructions are final-state diffs, not chronological battle events.
    // Observe lasting effects, or explicitly pause while the numeric record is still live.
    match *witness {
        Witness::Damaged(who) => end.pokemon(who).hp < original.pokemon(who).hp,
        Witness::LostHp(who, amount) => original.pokemon(who).hp - end.pokemon(who).hp == amount,
        Witness::SideDamaged(side) => original
            .side(side)
            .party
            .iter()
            .zip(&end.side(side).party)
            .any(|(a, b)| b.hp < a.hp),
        Witness::Healed(who) => end.pokemon(who).hp > original.pokemon(who).hp,
        Witness::CounterStored | Witness::MirrorStored => {
            let v = end.slot(p1e2_cases::TS).volatiles.get(
                if matches!(witness, Witness::CounterStored) {
                    Volatile::Counter
                } else {
                    Volatile::MirrorCoat
                },
            );
            outcome.suspension.is_some() && v.active && v.counter > 0 && v.hidden > 0
        }
        Witness::LastDamageStored => {
            outcome.suspension.is_some()
                && end
                    .slot(p1e2_cases::TS)
                    .history
                    .last_damaged_by
                    .is_some_and(|d| d.damage > 0)
        }
        Witness::Boost {
            target,
            stat,
            positive,
        } => {
            let change = end.slot(target).boosts[usize::from(stat)]
                - original.slot(target).boosts[usize::from(stat)];
            change != 0 && ((change > 0) == positive)
        }
        Witness::OneHp(who) => end.pokemon(who).hp == 1,
        Witness::Fainted(who) => end.pokemon(who).hp == 0,
        Witness::SashSpent(who) => {
            original.pokemon(who).item == items::FOCUS_SASH
                && end.pokemon(who).item == lab_engine::dex::ItemId::NONE
        }
        Witness::Paused => outcome.suspension.is_some(),
        Witness::CalledPaused => {
            outcome.suspension.is_some()
                && move_used(moves::COPYCAT)
                && end
                    .sides
                    .iter()
                    .any(|side| side.slots.iter().any(|slot| slot.must_switch_out()))
        }
        Witness::Completed => outcome.suspension.is_none() && end.turn > original.turn,
        Witness::Switched => original.sides.iter().zip(&end.sides).any(|(a, b)| {
            a.slots
                .iter()
                .zip(&b.slots)
                .any(|(old, new)| new.party_index.is_some() && old.party_index != new.party_index)
        }),
        Witness::MoveUsed(mv) => move_used(mv),
        Witness::MultipleHits(who) => end
            .side(who.side)
            .slots
            .iter()
            .any(|slot| slot.party_index == Some(who.party) && slot.history.times_attacked >= 3),
    }
}

fn activation(case: &Case, outcomes: &[Outcome]) -> BTreeMap<&'static str, bool> {
    let mut result = BTreeMap::new();
    for (label, witness) in &case.witnesses {
        let present = outcomes
            .iter()
            .any(|o| o.probability > 0.0 && saw(witness, &case.state, o));
        assert!(
            present,
            "{}: intended activation missing: {label} ({witness:?})",
            case.id
        );
        assert!(result.insert(*label, present).is_none());
    }
    // Negative half-threshold controls are checked in every outcome, not only a witness.
    if case.id.starts_with("derived-berserk-hp99") || case.id.starts_with("derived-berserk-hp100") {
        assert!(
            !outcomes.iter().any(|o| saw(
                &Witness::Boost {
                    target: p1e2_cases::TS,
                    stat: 2,
                    positive: true
                },
                &case.state,
                o
            )),
            "{}: below-half Berserk spuriously activated",
            case.id
        );
        result.insert("below-half-no-berserk", true);
    }
    if case.id.starts_with("derived-anger-shell-hp99")
        || case.id.starts_with("derived-anger-shell-hp100")
    {
        assert!(
            !outcomes.iter().any(|o| saw(
                &Witness::Boost {
                    target: p1e2_cases::TS,
                    stat: 4,
                    positive: true
                },
                &case.state,
                o
            )),
            "{}: below-half Anger Shell spuriously activated",
            case.id
        );
        result.insert("below-half-no-anger-shell", true);
    }
    if case.id.starts_with("derived-emergency-exit-hp99")
        || case.id.starts_with("derived-emergency-exit-hp100")
    {
        assert!(
            outcomes.iter().all(|o| o.suspension.is_none()),
            "{}: below-half Exit spuriously requested a switch",
            case.id
        );
        result.insert("below-half-no-emergency-exit", true);
    }
    result
}
fn run(case: &Case, factored: bool) -> Vec<Outcome> {
    let _scope = FactoredScope::new(factored);
    let mut work = case.state.clone();
    let result = run_decision_mid_turn_with(
        &mut work,
        &case.order,
        &case.decision,
        &case.mid_turn,
        EnumerateOptions { rolls: case.rolls },
    )
    .unwrap_or_else(|e| panic!("{}: factored={factored}: {e:?}", case.id));
    assert_eq!(work, case.state, "{}: enumeration mutated input", case.id);
    assert_eq!(hash(&work), hash(&case.state));
    assert_eq!(format!("{work:?}"), format!("{:?}", case.state));
    result
}
#[test]
fn p1e2_corpus_contract() {
    let cases = corpus();
    let ids: HashSet<_> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "case IDs must be unique");
    assert!(
        cases.len() >= 100 && cases.len() <= 200,
        "{} cases",
        cases.len()
    );
    let inputs: HashSet<_> = cases
        .iter()
        .map(|c| format!("{:?}|{:?}", c.state, c.decision))
        .collect();
    assert!(
        inputs.len() >= 85,
        "new HP/mechanic inputs, not repeated executions: {}",
        inputs.len()
    );
    assert!(cases.iter().all(|c| !c.witnesses.is_empty()));
    if let Some(path) = std::env::var_os("LAB_P1E2_CORPUS_MANIFEST") {
        let rows: Vec<_> = cases.iter().map(|c| serde_json::json!({
            "case":c.id,"origin":c.origin,"rolls":format!("{:?}",c.rolls),
            "resume_policy":c.resume_policy,"mid_turn":c.mid_turn,"smoke":c.smoke,
            "input_hash":hash(&c.state),"decision":format!("{:?}",c.decision),
            "required_activation":c.witnesses.iter().map(|(label,_)| *label).collect::<Vec<_>>()
        })).collect();
        let manifest = serde_json::json!({"schema":2,"cases":rows,"case_count":cases.len(),
            "distinct_input_count":inputs.len(),"smoke_count":cases.iter().filter(|c| c.smoke).count(),
            "scope":"derived mechanic/HP inputs plus oracle-based real calling and suspension cases"});
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        writeln!(file, "{}", serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    }
    println!(
        "P1E2_CONTRACT cases={} distinct_inputs={} smoke={}",
        cases.len(),
        inputs.len(),
        cases.iter().filter(|c| c.smoke).count()
    );
}
#[test]
fn p1e2_full_state_corpus() {
    let scope = std::env::var("LAB_P1E2_SCOPE").unwrap_or_else(|_| "smoke".into());
    assert!(
        matches!(scope.as_str(), "smoke" | "expanded"),
        "unknown LAB_P1E2_SCOPE"
    );
    let cases: Vec<_> = corpus()
        .into_iter()
        .filter(|c| scope == "expanded" || c.smoke)
        .collect();
    let mut file = std::env::var_os("LAB_P1E2_PUBLIC_RECORDS").map(|path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
    });
    let mut total_keys = 0usize;
    let mut suspended_keys = 0usize;
    for case in &cases {
        println!("P1E2_CASE_START {}", case.id);
        let flat_outcomes = run(case, false);
        let activated = activation(case, &flat_outcomes);
        let expected = distribution(&case.state, &flat_outcomes, &case.id);
        let factored_outcomes = run(case, true);
        let actual = distribution(&case.state, &factored_outcomes, &case.id);
        assert_eq!(
            expected.len(),
            actual.len(),
            "{}: full support count",
            case.id
        );
        for (key, p) in &expected {
            let q = actual
                .get(key)
                .unwrap_or_else(|| panic!("{}: missing actual full State/Suspension", case.id));
            assert!(
                (p - q).abs() <= 1e-12,
                "{}: probability {p} != {q}",
                case.id
            );
        }
        let samples = if let Decision::Turn(choices) = case.decision {
            let _scope = FactoredScope::new(false);
            let mut work = case.state.clone();
            let out = sample_turn(&mut work, Ruleset::CHAMPIONS_MC, choices, 8, 2917).unwrap();
            assert_eq!(work, case.state);
            distribution(&case.state, &out, &case.id)
        } else {
            panic!("{}: only actual turn corpus is supported", case.id)
        };
        total_keys += actual.len();
        suspended_keys += actual.keys().filter(|key| !key.ends_with("\nNone")).count();
        let row = serde_json::json!({"schema":2,"case":case.id,"origin":case.origin,
            "input":format!("{:?}",case.state),"input_hash":hash(&case.state),
            "decision":format!("{:?}",case.decision),"rolls":format!("{:?}",case.rolls),
            "resume_policy":case.resume_policy,"mid_turn":case.mid_turn,
            "activation":activated,"distribution":actual,"samples":samples,
            "sample_scope":"first midturn suspension, Full sampled rolls; compared across binaries only",
            "flat_factored_full_state_agreement":true});
        if let Some(file) = &mut file {
            writeln!(file, "{}", serde_json::to_string(&row).unwrap()).unwrap();
            file.flush().unwrap();
        }
        println!("P1E2_CASE_PASS {} keys={}", case.id, expected.len());
    }
    if scope == "expanded" {
        assert!(suspended_keys > 0, "actual suspension keys required");
    }
    println!("P1E2_PUBLIC scope={scope} cases={} full_state_keys={total_keys} suspension_keys={suspended_keys}",cases.len());
}
