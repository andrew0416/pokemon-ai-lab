//! P1e2 derived input construction and observable mechanic contracts.
//! Ordinary activation does not imply that a symbolic damage dependency reached the sink.
use lab_engine::action::{Gimmick, SlotAction};
use lab_engine::dex::{abilities, items, moves, AbilityId, ItemId, MoveId, Type};
use lab_engine::field::Effect;
use lab_engine::state::{MoveSlot, PokemonRef, SideId, SlotRef, State, Status};
use lab_engine::turn::{EnumerateOptions, RollMode};
use lab_scenario::{
    load_scenario_file, scenario_decision, scenario_positions_with, Decision, PartyOrder,
};
use std::path::Path;

pub const A: PokemonRef = PokemonRef {
    side: SideId::One,
    party: 0,
};
pub const B: PokemonRef = PokemonRef {
    side: SideId::One,
    party: 1,
};
pub const T: PokemonRef = PokemonRef {
    side: SideId::Two,
    party: 0,
};
pub const TS: SlotRef = SlotRef {
    side: SideId::Two,
    slot: 0,
};

#[derive(Clone, Debug)]
pub enum Witness {
    Damaged(PokemonRef),
    LostHp(PokemonRef, i16),
    SideDamaged(SideId),
    Healed(PokemonRef),
    CounterStored,
    MirrorStored,
    LastDamageStored,
    Boost {
        target: SlotRef,
        stat: u8,
        positive: bool,
    },
    OneHp(PokemonRef),
    Fainted(PokemonRef),
    SashSpent(PokemonRef),
    Paused,
    CalledPaused,
    Completed,
    Switched,
    MoveUsed(MoveId),
    MultipleHits(PokemonRef),
}
#[derive(Clone)]
pub struct Case {
    pub id: String,
    pub origin: String,
    pub state: State<2>,
    pub decision: Decision<2>,
    pub order: [PartyOrder; 2],
    pub mid_turn: [Vec<String>; 2],
    pub rolls: RollMode,
    pub witnesses: Vec<(&'static str, Witness)>,
    pub smoke: bool,
    pub resume_policy: &'static str,
}
fn action(target: i8) -> SlotAction {
    SlotAction::Move {
        index: 0,
        target,
        gimmick: Gimmick::None,
    }
}
fn base() -> Case {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/scenarios/single-hit.json");
    let loaded = load_scenario_file(path).unwrap();
    let mut p = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Fixed(0),
        },
    )
    .unwrap()
    .remove(0);
    p.state.field.fill(Effect::NONE);
    for side in &mut p.state.sides {
        side.effects.fill(Effect::NONE);
        if side.party[2].max_hp == 0 {
            side.party[2] = side.party[0].clone();
        }
        for mon in &mut side.party {
            if mon.max_hp == 0 {
                continue;
            }
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
    p.state.pokemon_mut(A).moves[0] = MoveSlot::full(moves::TACKLE);
    p.state.pokemon_mut(A).stats = [50, 100, 50, 100, 250];
    p.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::TACKLE);
    p.state.pokemon_mut(B).stats = [250, 100, 250, 100, 200];
    p.state.pokemon_mut(B).hp = 90;
    Case {
        id: String::new(),
        origin: "derived:single-hit".into(),
        state: p.state,
        decision: Decision::Turn([[action(1), action(1)], [action(0), action(0)]]),
        order: p.order,
        mid_turn: [vec![], vec![]],
        rolls: RollMode::Extremes,
        witnesses: vec![("target-damaged", Witness::Damaged(T))],
        smoke: false,
        resume_policy: "stop-at-first-pause",
    }
}
fn ability(c: &mut Case, value: AbilityId) {
    c.state.pokemon_mut(T).ability = value;
    c.state.pokemon_mut(T).base_ability = value;
}
fn derived_cases() -> Vec<Case> {
    let template = base();
    let mut cases = Vec::new();
    for family in [
        "plain",
        "drain",
        "recoil",
        "shell-bell",
        "innards-out",
        "liquid-ooze",
        "life-orb",
        "counter",
        "mirror-coat",
        "metal-burst",
        "comeuppance",
        "u-turn",
    ] {
        for hp in [45, 60, 100, 180] {
            let mut c = template.clone();
            c.id = format!("derived-{family}-hp{hp}");
            c.state.pokemon_mut(T).hp = hp;
            c.smoke = hp == 60 && matches!(family, "plain" | "counter" | "drain");
            match family {
                "plain" => ability(&mut c, abilities::BATTLE_ARMOR),
                "drain" => {
                    c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::GIGA_DRAIN);
                    c.witnesses.push(("drain-healed-user", Witness::Healed(B)));
                }
                "recoil" => {
                    c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::DOUBLE_EDGE);
                    c.witnesses
                        .push(("recoil-damaged-user", Witness::Damaged(B)));
                }
                "shell-bell" => {
                    c.state.pokemon_mut(B).item = items::SHELL_BELL;
                    c.witnesses
                        .push(("shell-bell-healed-user", Witness::Healed(B)));
                }
                "innards-out" => {
                    ability(&mut c, abilities::INNARDS_OUT);
                    c.state.pokemon_mut(B).stats[0] = 500;
                    c.witnesses.extend([
                        ("innards-out-damaged-attacker", Witness::Damaged(B)),
                        ("holder-fainted", Witness::Fainted(T)),
                    ]);
                }
                "liquid-ooze" => {
                    ability(&mut c, abilities::LIQUID_OOZE);
                    c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::GIGA_DRAIN);
                    c.witnesses
                        .push(("ooze-damaged-drainer", Witness::Damaged(B)));
                }
                "life-orb" => {
                    c.state.pokemon_mut(B).item = items::LIFE_ORB;
                    c.witnesses.push(("orb-damaged-user", Witness::Damaged(B)));
                }
                "counter" | "mirror-coat" | "metal-burst" | "comeuppance" => {
                    let mv = match family {
                        "counter" => moves::COUNTER,
                        "mirror-coat" => moves::MIRROR_COAT,
                        "metal-burst" => moves::METAL_BURST,
                        _ => moves::COMEUPPANCE,
                    };
                    c.state.pokemon_mut(T).moves[0] = MoveSlot::full(mv);
                    if family == "mirror-coat" {
                        c.state.pokemon_mut(A).moves[0] = MoveSlot::full(moves::WATER_GUN);
                        c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::WATER_GUN);
                    }
                    // End-of-turn output is a state diff: numeric history has reset.
                    // Actual returned damage witnesses live reflection; dedicated paused cases
                    // below expose the numeric volatile/history before it is reset.
                    if hp == 180 {
                        c.witnesses.push(("returned-damage", Witness::Damaged(B)));
                    }
                }
                "u-turn" => {
                    c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::U_TURN);
                    c.witnesses.push(("real-midturn-pause", Witness::Paused));
                }
                _ => unreachable!(),
            }
            cases.push(c);
        }
    }
    for family in ["counter", "mirror-coat", "metal-burst", "comeuppance"] {
        let mut c = cases
            .iter()
            .find(|c| c.id == format!("derived-{family}-hp180"))
            .unwrap()
            .clone();
        c.id = format!("derived-{family}-paused-history");
        c.state.pokemon_mut(B).stats = [100, 100, 100, 100, 200];
        c.state.pokemon_mut(B).moves[0] = MoveSlot::full(if family == "mirror-coat" {
            moves::VOLT_SWITCH
        } else {
            moves::U_TURN
        });
        c.witnesses = vec![
            ("real-pause-before-reflection", Witness::Paused),
            match family {
                "counter" => ("paused-counter-numeric-record", Witness::CounterStored),
                "mirror-coat" => ("paused-mirror-numeric-record", Witness::MirrorStored),
                _ => ("paused-last-damage-record", Witness::LastDamageStored),
            },
        ];
        c.smoke = family == "counter";
        cases.push(c);
    }
    // Explicit half-HP boundary inputs include non-activation controls below/at half.
    for (family, ab) in [
        ("berserk", abilities::BERSERK),
        ("anger-shell", abilities::ANGER_SHELL),
        ("emergency-exit", abilities::EMERGENCY_EXIT),
    ] {
        for hp in [99, 100, 101, 120, 180] {
            let mut c = template.clone();
            c.id = format!("derived-{family}-hp{hp}");
            c.state.pokemon_mut(T).hp = hp;
            c.state.pokemon_mut(B).stats[0] = 100;
            ability(&mut c, ab);
            c.smoke = hp == 120 && family == "berserk";
            if hp > 100 && hp < 180 {
                c.witnesses.push(if family == "emergency-exit" {
                    ("crossing-requested-switch", Witness::Paused)
                } else if family == "berserk" {
                    (
                        "crossing-raised-spa",
                        Witness::Boost {
                            target: TS,
                            stat: 2,
                            positive: true,
                        },
                    )
                } else {
                    (
                        "crossing-raised-speed",
                        Witness::Boost {
                            target: TS,
                            stat: 4,
                            positive: true,
                        },
                    )
                });
            }
            cases.push(c);
        }
    }
    for (family, mv) in [
        ("endure", moves::ENDURE),
        ("false-swipe", moves::FALSE_SWIPE),
        ("hold-back", moves::HOLD_BACK),
    ] {
        for hp in [1, 2, 40, 100] {
            let mut c = template.clone();
            c.id = format!("derived-{family}-hp{hp}");
            c.state.pokemon_mut(T).hp = hp;
            c.state.pokemon_mut(B).stats[0] = 1000;
            if family == "endure" {
                c.state.pokemon_mut(T).moves[0] = MoveSlot::full(mv);
            } else {
                c.state.pokemon_mut(A).moves[0] = MoveSlot::full(mv);
                c.state.pokemon_mut(B).moves[0] = MoveSlot::full(mv);
            }
            c.witnesses = vec![
                ("protected-at-one-hp", Witness::OneHp(T)),
                ("protecting-move-used", Witness::MoveUsed(mv)),
            ];
            c.smoke = family == "endure" && hp == 40;
            cases.push(c);
        }
    }
    for family in ["sturdy", "focus-sash", "focus-band"] {
        for hp in [1, 199, 200] {
            let mut c = template.clone();
            c.id = format!("derived-{family}-hp{hp}");
            c.state.pokemon_mut(T).hp = hp;
            c.state.pokemon_mut(A).stats[0] = 2000;
            c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::SWORDS_DANCE);
            if let Decision::Turn(ref mut choices) = c.decision {
                choices[0][1] = action(0);
            }
            match family {
                "sturdy" => ability(&mut c, abilities::STURDY),
                "focus-sash" => c.state.pokemon_mut(T).item = items::FOCUS_SASH,
                _ => c.state.pokemon_mut(T).item = items::FOCUS_BAND,
            }
            c.witnesses.clear();
            if hp == 200 || family == "focus-band" {
                c.witnesses.push(("survival-at-one-hp", Witness::OneHp(T)));
                if family == "focus-sash" {
                    c.witnesses.push(("sash-consumed", Witness::SashSpent(T)));
                }
            } else {
                c.witnesses
                    .push(("partial-hp-not-protected", Witness::Fainted(T)));
            }
            if family == "focus-band" {
                c.witnesses.push(("band-can-fail", Witness::Fainted(T)));
            }
            c.smoke = family == "focus-sash" && hp == 200;
            cases.push(c);
        }
    }
    for hp in [100, 150, 180] {
        let mut c = template.clone();
        c.id = format!("derived-double-hit-hp{hp}");
        // Rage Fist is an unused history reader: it keeps times_attacked observable at EOT.
        c.state
            .pokemon_mut(PokemonRef {
                side: SideId::Two,
                party: 2,
            })
            .moves[3] = MoveSlot::full(moves::RAGE_FIST);
        c.state.pokemon_mut(T).hp = hp;
        c.state.pokemon_mut(B).moves[0] = MoveSlot::full(moves::DOUBLE_HIT);
        c.state.pokemon_mut(B).stats[0] = 100;
        c.witnesses
            .push(("multiple-damage-applications", Witness::MultipleHits(T)));
        cases.push(c);
    }
    let mut full = cases[1].clone();
    full.id = "derived-plain-hp60-full".into();
    full.rolls = RollMode::Full;
    full.smoke = true;
    cases.push(full);
    cases
}
fn oracle_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, witnesses) in [
        (
            "counter-mirror-coat",
            vec![(
                "mirror-returned-exact100",
                Witness::LostHp(
                    PokemonRef {
                        side: SideId::Two,
                        party: 1,
                    },
                    100,
                ),
            )],
        ),
        (
            "counter-follow-me",
            vec![(
                "counter-redirected-exact100",
                Witness::LostHp(
                    PokemonRef {
                        side: SideId::Two,
                        party: 0,
                    },
                    100,
                ),
            )],
        ),
        (
            "history-ragefist-metalburst",
            vec![(
                "metal-burst-exact75",
                Witness::LostHp(
                    PokemonRef {
                        side: SideId::Two,
                        party: 1,
                    },
                    75,
                ),
            )],
        ),
        (
            "ll-staged-comeuppance",
            vec![(
                "comeuppance-returned-damage",
                Witness::SideDamaged(SideId::Two),
            )],
        ),
        (
            "o55-anger-shell-berserk",
            vec![(
                "anger-shell-speed",
                Witness::Boost {
                    target: SlotRef {
                        side: SideId::One,
                        slot: 0,
                    },
                    stat: 4,
                    positive: true,
                },
            )],
        ),
        (
            "nn-sleep-talk-double-hit-life-orb",
            vec![
                ("sleep-talk-used", Witness::MoveUsed(moves::SLEEP_TALK)),
                ("orb-recoil-on-caller", Witness::Damaged(A)),
            ],
        ),
        (
            "nn-copycat-bullet-seed",
            vec![
                ("copycat-used", Witness::MoveUsed(moves::COPYCAT)),
                ("called-seed-damaged-foe", Witness::SideDamaged(SideId::Two)),
            ],
        ),
        (
            "emergency-exit",
            vec![
                ("resumed-switch", Witness::Switched),
                ("resumed-to-end", Witness::Completed),
            ],
        ),
        (
            "dd-emergency-exit-recoil-paused",
            vec![("real-pause", Witness::Paused)],
        ),
    ] {
        let loaded = load_scenario_file(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../oracle/scenarios/{name}.json")),
        )
        .unwrap();
        let positions = scenario_positions_with(
            &loaded,
            EnumerateOptions {
                rolls: RollMode::Extremes,
            },
        )
        .unwrap();
        assert!(
            positions.len() <= 64,
            "{name}: setup corpus unexpectedly grew"
        );
        for (index, p) in positions.into_iter().enumerate() {
            let decision = scenario_decision(&loaded, &p).unwrap();
            cases.push(Case {
                id: format!("oracle-{name}-position{index}"),
                origin: format!("oracle/scenarios/{name}.json"),
                state: p.state,
                decision,
                order: p.order,
                mid_turn: loaded.mid_turn.clone(),
                rolls: RollMode::Extremes,
                witnesses: witnesses.clone(),
                smoke: name == "counter-mirror-coat",
                resume_policy: "scenario-recorded-switches",
            });
        }
    }
    let name = "uu-copycat-uturn-hits";
    let loaded = load_scenario_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../oracle/scenarios/{name}.json")),
    )
    .unwrap();
    let p = scenario_positions_with(
        &loaded,
        EnumerateOptions {
            rolls: RollMode::Extremes,
        },
    )
    .unwrap()
    .remove(0);
    for (label, limit) in [("initial-pause", 0), ("called-pause", 1), ("completed", 2)] {
        let mut mid = loaded.mid_turn.clone();
        mid[0].truncate(limit);
        let witnesses = match limit {
            0 => vec![("initial-uturn-paused", Witness::Paused)],
            1 => vec![
                ("copycat-used", Witness::MoveUsed(moves::COPYCAT)),
                ("called-uturn-paused", Witness::CalledPaused),
            ],
            _ => vec![
                ("copycat-used", Witness::MoveUsed(moves::COPYCAT)),
                ("switched", Witness::Switched),
                ("completed", Witness::Completed),
            ],
        };
        cases.push(Case {
            id: format!("oracle-{name}-{label}"),
            origin: format!("oracle/scenarios/{name}.json"),
            state: p.state.clone(),
            decision: scenario_decision(&loaded, &p).unwrap(),
            order: p.order.clone(),
            mid_turn: mid,
            rolls: RollMode::Extremes,
            witnesses,
            smoke: false,
            resume_policy: label,
        });
    }
    cases
}
pub fn corpus() -> Vec<Case> {
    let mut cases = derived_cases();
    cases.extend(oracle_cases());
    cases
}
