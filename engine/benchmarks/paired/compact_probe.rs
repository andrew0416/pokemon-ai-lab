//! P10 representation-equivalence probe; never a benchmark.
//! The SAME source is compiled against baseline / candidate-off / candidate-on.
//! Dependencies: lab-engine, lab-scenario, serde_json. Argument: common fixture engine root.
//! No clocks, performance counters, thread pools, or remote operations.
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::Path;

use lab_engine::dex::{moves, MoveTarget};
use lab_engine::hash::KeyHasher;
use lab_engine::instruction::Instruction;
use lab_engine::state::{Side, SideId, Slot, SlotRef, State};
use lab_engine::turn::{
    verify_position_hashes, EnumerateOptions, FactoredScope, RollMode, TurnError,
};
use lab_engine::volatile::{Volatile, VolatileState, Volatiles, VOLATILE_COUNT};
use lab_scenario::decision::advance_order;
use lab_scenario::{
    canonical_json_hidden, load_scenario_file, parse_decision, run_decision_mid_turn_with,
    run_decision_with, scenario_positions_with, state_from_canonical,
};
use serde_json::{json, Value};

type Dense = [VolatileState; VOLATILE_COUNT];

/// Records both method identity and bytes: KeyHasher mixes a write_u16 differently
/// from two write_u8 calls, even if their flattened bytes would look equal.
#[derive(Default, Debug, PartialEq, Eq)]
struct TraceHasher(Vec<(&'static str, Vec<u8>)>);
macro_rules! trace_write {
    ($name:ident, $ty:ty) => {
        fn $name(&mut self, value: $ty) {
            self.0
                .push((stringify!($name), value.to_le_bytes().to_vec()));
        }
    };
}
impl Hasher for TraceHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.push(("write", bytes.to_vec()));
    }
    trace_write!(write_u8, u8);
    trace_write!(write_u16, u16);
    trace_write!(write_u32, u32);
    trace_write!(write_u64, u64);
    trace_write!(write_u128, u128);
    trace_write!(write_usize, usize);
    trace_write!(write_i8, i8);
    trace_write!(write_i16, i16);
    trace_write!(write_i32, i32);
    trace_write!(write_i64, i64);
    trace_write!(write_i128, i128);
    trace_write!(write_isize, isize);
}
fn trace<T: Hash>(value: &T) -> TraceHasher {
    let mut h = TraceHasher::default();
    value.hash(&mut h);
    h
}
fn key_hash<T: Hash>(value: &T) -> u64 {
    let mut h = KeyHasher::new();
    value.hash(&mut h);
    h.finish()
}
/// Explicit old logical hash contract, independent of Volatiles' implementation.
fn dense_hash<H: Hasher>(dense: &Dense, h: &mut H) {
    for (i, v) in dense.iter().enumerate() {
        if *v != VolatileState::NONE {
            h.write_u8(i as u8);
            v.active.hash(h);
            v.duration.hash(h);
            v.counter.hash(h);
            v.time.hash(h);
            v.mv.hash(h);
            v.hidden.hash(h);
        }
    }
    h.write_u8(u8::MAX);
}
fn row(v: VolatileState) -> Value {
    json!([v.active, v.duration, v.counter, v.time, v.mv.0, v.hidden])
}
fn emit(record: Value) {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &record).unwrap();
    out.write_all(b"\n").unwrap();
    out.flush().unwrap(); // Preserve completed records if a later assertion fails.
}
fn payload(i: usize) -> VolatileState {
    VolatileState {
        active: i % 3 != 0,
        duration: (i % 255 + 1) as u8,
        counter: (i * 257 + 1) as u16,
        time: (255 - i) as u8,
        mv: if i % 2 == 0 {
            moves::HYPNOSIS
        } else {
            moves::PROTECT
        },
        hidden: (i % 254 + 1) as u8,
    }
}
fn check(value: &Volatiles, dense: &Dense) {
    assert_eq!(
        VOLATILE_COUNT, 112,
        "This probe is pinned to the 112-entry registry"
    );
    for (i, v) in Volatile::ALL.into_iter().enumerate() {
        assert_eq!(v as usize, i, "ALL must remain in dense index order");
        assert_eq!(value.get(v), dense[i], "get at {i}");
        assert_eq!(value.has(v), dense[i].active, "has at {i}");
    }
    let expected: Vec<_> = Volatile::ALL
        .into_iter()
        .filter_map(|v| dense[v as usize].active.then_some((v, dense[v as usize])))
        .collect();
    assert_eq!(value.iter().collect::<Vec<_>>(), expected);
    assert_eq!(value.is_empty(), dense.iter().all(|v| !v.active));
    let mut expected_hash = TraceHasher::default();
    dense_hash(dense, &mut expected_hash);
    assert_eq!(trace(value), expected_hash, "Hash method/byte stream");
    let mut h = KeyHasher::new();
    dense_hash(dense, &mut h);
    assert_eq!(key_hash(value), h.finish());
    // The candidate deliberately preserves public logical Debug despite new storage.
    assert_eq!(format!("{value:?}"), format!("Volatiles({dense:?})"));

    let mut different_order = Volatiles::default();
    for v in Volatile::ALL.into_iter().rev() {
        different_order.set(v, dense[v as usize]);
    }
    assert_eq!(
        value, &different_order,
        "Eq independent of insertion/storage history"
    );
    assert_eq!(
        *value == Volatiles::default(),
        dense.iter().all(|v| *v == VolatileState::NONE)
    );
}
fn snapshot(label: &str, value: &Volatiles, dense: &Dense) {
    check(value, dense);
    emit(json!({
        "kind":"volatiles", "label":label,
        "entries":dense.iter().copied().map(row).collect::<Vec<_>>(),
        "active_indices":value.iter().map(|(v,_)|v as usize).collect::<Vec<_>>(),
        "is_empty":value.is_empty(), "hash_events":trace(value).0,
        "key_hash":key_hash(value)
    }));
}
fn singleton_registry() {
    let patterns = [
        VolatileState {
            duration: 1,
            ..VolatileState::NONE
        },
        VolatileState {
            counter: u16::MAX,
            ..VolatileState::NONE
        },
        VolatileState {
            time: u8::MAX,
            ..VolatileState::NONE
        },
        VolatileState {
            mv: moves::HYPNOSIS,
            ..VolatileState::NONE
        },
        VolatileState {
            hidden: u8::MAX,
            ..VolatileState::NONE
        },
        VolatileState {
            active: true,
            ..VolatileState::NONE
        },
        VolatileState {
            active: true,
            duration: u8::MAX,
            counter: u16::MAX,
            time: u8::MAX,
            mv: moves::PROTECT,
            hidden: u8::MAX,
        },
        VolatileState {
            active: false,
            duration: u8::MAX,
            counter: u16::MAX,
            time: u8::MAX,
            mv: moves::PROTECT,
            hidden: u8::MAX,
        },
    ];
    for (pattern, entry) in patterns.into_iter().enumerate() {
        let mut records = Vec::new();
        for (i, volatile) in Volatile::ALL.into_iter().enumerate() {
            let mut value = Volatiles::default();
            let mut dense = [VolatileState::NONE; VOLATILE_COUNT];
            value.set(volatile, entry);
            dense[i] = entry;
            check(&value, &dense);
            assert_ne!(
                value,
                Volatiles::default(),
                "inactive non-NONE must affect Eq"
            );
            records.push(json!([
                i,
                volatile.id(),
                row(value.get(volatile)),
                value.is_empty(),
                trace(&value).0,
                key_hash(&value)
            ]));
            value.set(volatile, VolatileState::NONE);
            dense[i] = VolatileState::NONE;
            check(&value, &dense);
        }
        emit(json!({"kind":"registry-singletons","pattern":pattern,"records":records}));
    }
}
fn clone_independence(label: &str, value: &Volatiles, dense: &Dense) {
    let first = Volatile::ALL[0];
    let changed = VolatileState {
        hidden: dense[0].hidden.wrapping_add(1),
        ..dense[0]
    };
    let mut expected_changed = *dense;
    expected_changed[0] = changed;

    let mut copied = value.clone();
    copied.set(first, changed);
    check(&copied, &expected_changed);
    check(value, dense);
    assert_ne!(&copied, value);

    for destination_count in [0, 4, 5, VOLATILE_COUNT] {
        let mut destination = Volatiles::default();
        for (i, v) in Volatile::ALL
            .into_iter()
            .take(destination_count)
            .enumerate()
        {
            destination.set(v, payload(i));
        }
        destination.clone_from(value);
        check(&destination, dense);
        destination.set(first, changed);
        check(&destination, &expected_changed);
        check(value, dense);

        let mut source_copy = value.clone();
        destination.clone_from(&source_copy);
        source_copy.set(first, changed);
        check(&destination, dense);
        check(&source_copy, &expected_changed);
    }
    emit(
        json!({"kind":"clone-independence","label":label,"source_entries":
        dense.iter().copied().map(row).collect::<Vec<_>>(),"destination_counts":[0,4,5,112]}),
    );
}
fn registry_mutations() {
    let mut value = Volatiles::default();
    let mut dense = [VolatileState::NONE; VOLATILE_COUNT];
    snapshot("empty", &value, &dense);
    clone_independence("empty", &value, &dense);
    let boundaries = [1, 3, 4, 5, 7, 8, 63, 64, 65, 111, 112];
    // Reverse insertion exercises sorting in both bitmap words.
    for (step, v) in Volatile::ALL.into_iter().rev().enumerate() {
        dense[v as usize] = payload(v as usize);
        value.set(v, dense[v as usize]);
        check(&value, &dense);
        if boundaries.contains(&(step + 1)) {
            snapshot(&format!("reverse-insert-{}", step + 1), &value, &dense);
        }
        if [4, 5, 112].contains(&(step + 1)) {
            clone_independence(&format!("reverse-insert-{}", step + 1), &value, &dense);
        }
    }
    // Non-NONE overwrite does not remove inactive payload.
    for v in Volatile::ALL {
        let i = v as usize;
        dense[i].active = !dense[i].active;
        dense[i].hidden ^= u8::MAX;
        value.set(v, dense[i]);
        check(&value, &dense);
    }
    snapshot("all-overwritten", &value, &dense);
    // 37 is coprime to 112: interleaved removals cover every key, not only ends.
    for step in 0..VOLATILE_COUNT {
        let i = (step * 37) % VOLATILE_COUNT;
        value.set(Volatile::ALL[i], VolatileState::NONE);
        dense[i] = VolatileState::NONE;
        check(&value, &dense);
        let remaining = VOLATILE_COUNT - step - 1;
        if boundaries.contains(&remaining) || remaining == 0 {
            snapshot(&format!("interleaved-delete-{remaining}"), &value, &dense);
        }
        if [4, 5].contains(&remaining) {
            clone_independence(&format!("after-spill-delete-{remaining}"), &value, &dense);
        }
    }
    // Reinsert after complete deletion, then remove an already absent key twice.
    for step in 0..VOLATILE_COUNT {
        let i = (step * 13) % VOLATILE_COUNT;
        value.set(Volatile::ALL[i], payload(i));
        dense[i] = payload(i);
        check(&value, &dense);
    }
    snapshot("reinsert-all", &value, &dense);
    for v in Volatile::ALL {
        value.set(v, VolatileState::NONE);
        value.set(v, VolatileState::NONE);
        dense[v as usize] = VolatileState::NONE;
        check(&value, &dense);
    }
    snapshot("reinsert-clear-all", &value, &dense);
}
fn state_record<const N: usize>(state: &State<N>) -> Value {
    json!({"debug":format!("{state:?}"),"hash_events":trace(state).0,
        "key_hash":key_hash(state),"position_hash":state.position_hash()})
}
fn apply_checked<const N: usize>(state: &mut State<N>, instruction: &Instruction) {
    let before = state.position_hash();
    let cell_before = state.instruction_hash(instruction);
    state.apply_one(instruction);
    let expected = before
        .wrapping_sub(cell_before)
        .wrapping_add(state.instruction_hash(instruction));
    assert_eq!(
        state.position_hash(),
        expected,
        "incremental hash after apply"
    );
}
fn state_instructions<const N: usize>() {
    let original = State::<N>::default();
    let mut state = original.clone();
    let mut instructions = Vec::new();
    for target in State::<N>::slot_refs() {
        for v in Volatile::ALL {
            let instruction = Instruction::SetVolatile {
                target,
                volatile: v,
                old: VolatileState::NONE,
                new: payload(v as usize),
            };
            apply_checked(&mut state, &instruction);
            instructions.push(instruction);
        }
    }
    let populated = state.clone();
    // Exercise SetVolatile replacement and deletion with non-NONE old payloads,
    // including inactive ones, before the whole-slot Switch path.
    for target in State::<N>::slot_refs() {
        for (i, v) in Volatile::ALL.into_iter().enumerate() {
            let old = state.slot(target).volatiles.get(v);
            let new = if i % 2 == 0 {
                VolatileState::NONE
            } else {
                VolatileState {
                    active: !old.active,
                    hidden: old.hidden ^ u8::MAX,
                    ..old
                }
            };
            let instruction = Instruction::SetVolatile {
                target,
                volatile: v,
                old,
                new,
            };
            apply_checked(&mut state, &instruction);
            instructions.push(instruction);
        }
    }
    let replaced = state.clone();
    for target in State::<N>::slot_refs() {
        let previous = state.slot(target).clone();
        let instruction = Instruction::Switch {
            slot: target,
            party_index: Some(0),
            previous: Box::new(previous),
        };
        apply_checked(&mut state, &instruction);
        assert_eq!(state.slot(target).volatiles, Volatiles::default());
        instructions.push(instruction);
    }
    let switched = state.clone();
    for instruction in instructions.iter().rev() {
        let before = state.position_hash();
        let cell_before = state.instruction_hash(instruction);
        state.reverse_one(instruction);
        let expected = before
            .wrapping_sub(cell_before)
            .wrapping_add(state.instruction_hash(instruction));
        assert_eq!(
            state.position_hash(),
            expected,
            "incremental hash after reverse"
        );
    }
    assert_eq!(state, original);
    let mut batch = original.clone();
    batch.apply(&instructions);
    assert_eq!(batch, switched);
    batch.reverse(&instructions);
    assert_eq!(batch, original);
    emit(json!({"kind":"state-instructions","slots_per_side":N,
        "instructions":format!("{instructions:?}"),"populated":state_record(&populated),
        "replaced":state_record(&replaced),
        "switched":state_record(&switched),"restored":state_record(&state)}));
}
fn hidden_inactive(engine: &Path) {
    let loaded =
        load_scenario_file(engine.join("oracle/scenarios/ff-encore-protect-stall.json")).unwrap();
    let positions = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap();
    assert!(!positions.is_empty());
    let base = &positions[0];
    let at = SlotRef {
        side: SideId::One,
        slot: 0,
    };
    for count in [4, 5, VOLATILE_COUNT] {
        let mut state = base.state.clone();
        state.slot_mut(at).volatiles = Volatiles::default();
        for v in Volatile::ALL.into_iter().take(count) {
            state.slot_mut(at).volatiles.set(
                v,
                VolatileState {
                    active: false,
                    ..payload(v as usize)
                },
            );
        }
        assert!(state.slot(at).volatiles.is_empty());
        let text = canonical_json_hidden(&state, &loaded.meta, Some(&base.order)).unwrap();
        let rebuilt =
            state_from_canonical(&state, &loaded.meta, &serde_json::from_str(&text).unwrap())
                .unwrap();
        assert_eq!(rebuilt.state, state);
        assert_eq!(rebuilt.order, base.order);
        emit(json!({"kind":"hidden-inactive","non_none_count":count,
            "canonical_hidden":text,"state":state_record(&state)}));
    }
}
fn fixture(engine: &Path, name: &str, rolls: RollMode, factored: bool) {
    let _scope = FactoredScope::new(factored);
    let path = engine.join(format!("oracle/scenarios/{name}.json"));
    let input: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let loaded = load_scenario_file(&path).unwrap();
    let input_p1 = input["turn"]["p1"].as_str().unwrap();
    let input_p2 = input["turn"]["p2"].as_str().unwrap();
    // The existing oracle regression selects its one locked `before` state, where
    // check_slot discards the written target. This probe intentionally keeps ALL
    // setup positions, including positions with no remaining lock. Outrage is
    // RandomNormal and must have target 0 there; removing the ignored target does
    // not select a foe or remove any target/probability branch. Keep this explicit
    // and fixture-specific rather than silently repairing arbitrary bad choices.
    let (p1, normalization) = if name == "nn-pressure-locked-outrage" {
        assert_eq!(input_p1, "move outrage 1, move curse");
        assert_eq!(moves::OUTRAGE.data().target, MoveTarget::RandomNormal);
        (
            "move outrage, move curse",
            "remove Outrage's redundant chosen-target token; retain all setup positions and RandomNormal target branches",
        )
    } else {
        (input_p1, "none")
    };
    let positions = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Median,
        },
    )
    .unwrap();
    assert!(!positions.is_empty(), "empty setup: {name}");
    let mut records = Vec::new();
    for (index, position) in positions.into_iter().enumerate() {
        let original = position.state;
        let decision = parse_decision(&original, &position.order, p1, input_p2).unwrap();
        // Empty resume choices retain actual suspension; recorded choices exercise resume.
        let variants = if loaded.mid_turn.iter().any(|x| !x.is_empty()) {
            2
        } else {
            1
        };
        for resume in 0..variants {
            let mut work = original.clone();
            let outcomes = if resume == 0 {
                run_decision_with(&mut work, &decision, EnumerateOptions { rolls }).unwrap()
            } else {
                run_decision_mid_turn_with(
                    &mut work,
                    &position.order,
                    &decision,
                    &loaded.mid_turn,
                    EnumerateOptions { rolls },
                )
                .unwrap()
            };
            assert_eq!(work, original, "enumeration input restoration: {name}");
            assert!(!outcomes.is_empty(), "empty outcomes: {name}");
            // Keep the unmodified input's acceptance/rejection as evidence too.
            // Accepted (locked) inputs must produce exactly the normalized result;
            // rejected inputs must retain the precise target error and input State.
            let raw_input_check = if name == "nn-pressure-locked-outrage" {
                assert_eq!(resume, 0);
                let raw_decision =
                    parse_decision(&original, &position.order, input_p1, input_p2).unwrap();
                let mut raw_work = original.clone();
                let raw_result =
                    run_decision_with(&mut raw_work, &raw_decision, EnumerateOptions { rolls });
                assert_eq!(raw_work, original, "raw choice input restoration");
                match raw_result {
                    Ok(raw_outcomes) => {
                        assert_eq!(raw_outcomes, outcomes, "locked target normalization");
                        assert_eq!(
                            raw_outcomes
                                .iter()
                                .map(|o| o.probability.to_bits())
                                .collect::<Vec<_>>(),
                            outcomes
                                .iter()
                                .map(|o| o.probability.to_bits())
                                .collect::<Vec<_>>(),
                        );
                        json!({"status":"accepted","normalized_outcomes_exact":true})
                    }
                    Err(error) => {
                        assert_eq!(
                            error,
                            TurnError::InvalidChoice {
                                side: SideId::One,
                                slot: 0,
                                reason: "target 1 for Outrage (RandomNormal)".into(),
                            }
                        );
                        json!({"status":"rejected","error":format!("{error:?}")})
                    }
                }
            } else {
                Value::Null
            };
            let mut endings = Vec::new();
            for outcome in outcomes {
                work.apply(&outcome.instructions);
                let after = state_record(&work);
                let mut ending_order = position.order.clone();
                advance_order(&mut ending_order, &outcome.instructions);
                let hidden =
                    canonical_json_hidden(&work, &loaded.meta, Some(&ending_order)).unwrap();
                let rebuilt = state_from_canonical(
                    &work,
                    &loaded.meta,
                    &serde_json::from_str(&hidden).unwrap(),
                )
                .unwrap();
                assert_eq!(rebuilt.state, work);
                assert_eq!(rebuilt.order, ending_order);
                endings.push(json!({"probability_bits":outcome.probability.to_bits(),
                    "instructions":format!("{:?}",outcome.instructions),
                    "suspension":format!("{:?}",outcome.suspension),
                    "canonical_hidden":hidden,"state":after}));
                work.reverse(&outcome.instructions);
                assert_eq!(work, original, "outcome reverse: {name}");
            }
            records.push(
                json!({"position":index,"setup_probability_bits":position.probability.to_bits(),
                "resume_recorded":resume==1,"raw_input_check":raw_input_check,
                "before":state_record(&original),"endings":endings}),
            );
        }
    }
    emit(
        json!({"kind":"fixture","fixture":name,"rolls":format!("{rolls:?}"),
        "factored":factored,"input_choices":[input_p1,input_p2],
        "engine_choices":[p1,input_p2],"choice_normalization":normalization,
        "records":records}),
    );
}
fn main() {
    let mut args = std::env::args().skip(1);
    let engine = args
        .next()
        .expect("usage: p10-probe <common fixture engine root> | --layout");
    assert!(args.next().is_none(), "one argument expected");
    if engine == "--layout" {
        emit(json!({"kind":"layout","schema_version":1,
            "scope":"Static inline type sizes only; excludes heap allocations and allocator overhead; not a performance result",
            "volatile_state":std::mem::size_of::<VolatileState>(),
            "volatiles":std::mem::size_of::<Volatiles>(),
            "volatiles_align":std::mem::align_of::<Volatiles>(),
            "slot":std::mem::size_of::<Slot>(),
            "side1":std::mem::size_of::<Side<1>>(),
            "side2":std::mem::size_of::<Side<2>>(),
            "state1":std::mem::size_of::<State<1>>(),
            "state2":std::mem::size_of::<State<2>>(),
            "instruction":std::mem::size_of::<Instruction>()}));
        return;
    }
    let engine = Path::new(&engine);
    verify_position_hashes(true);
    let _scope = FactoredScope::new(false);
    singleton_registry();
    registry_mutations();
    state_instructions::<1>();
    state_instructions::<2>();
    hidden_inactive(engine);
    for factored in [false, true] {
        for name in [
            "ff-encore-protect-stall",
            "nn-pressure-locked-outrage",
            "ee-transform-switch-back",
            "eject-button-uturn",
        ] {
            fixture(engine, name, RollMode::Median, factored);
        }
        fixture(engine, "ff-encore-protect-stall", RollMode::Full, factored);
    }
    emit(
        json!({"kind":"complete","schema_version":1,"registry_count":VOLATILE_COUNT,
        "singleton_patterns":8,"fixture_configurations":10,
        "scope":"bounded logic equivalence only; no performance measurement; inactive diff untested"}),
    );
}
