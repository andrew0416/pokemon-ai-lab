//! The scenario's position and decision: the oracle's `patch` applied to a state, and the
//! `turn` choice strings parsed into joint actions.
//!
//! `patch` follows `applyPatch` in `engine/oracle/enumerate.cjs`: it runs after the leads'
//! switch-in effects, so it is applied to each state [`crate::initial_outcomes`] returns.
//! Where Showdown would draw a random number (a sleep patch without `statusTime`), or where
//! the patch would do something the oracle's own code does not do cleanly, it is an error.

use std::collections::BTreeMap;

use serde::Deserialize;

use lab_engine::action::{Gimmick, JointAction, SlotAction};
use lab_engine::dex::{to_id, ItemId, MoveId, NO_BOOSTS};
use lab_engine::field::{Effect, FieldEffect, SideEffect, Terrain, Weather};
use lab_engine::instruction::Instruction;
use lab_engine::state::{SideId, SlotRef, State, Status, BOOST_COUNT};

use crate::meta::ScenarioMeta;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PatchJson {
    #[serde(default)]
    pub p1: BTreeMap<String, MonPatch>,
    #[serde(default)]
    pub p2: BTreeMap<String, MonPatch>,
    /// Side conditions: `{ "p1": { "tailwind": 3 } }`; `null` keeps the natural duration.
    #[serde(default)]
    pub sides: BTreeMap<String, BTreeMap<String, Option<u8>>>,
    #[serde(default)]
    pub field: FieldPatch,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MonPatch {
    pub hp: Option<i16>,
    pub status: Option<String>,
    #[serde(rename = "statusTime")]
    pub status_time: Option<i8>,
    pub boosts: Option<BTreeMap<String, i8>>,
    pub item: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FieldPatch {
    pub weather: Option<String>,
    #[serde(rename = "weatherDuration")]
    pub weather_duration: Option<u8>,
    pub terrain: Option<String>,
    #[serde(rename = "terrainDuration")]
    pub terrain_duration: Option<u8>,
    #[serde(default, rename = "pseudoWeather")]
    pub pseudo_weather: BTreeMap<String, Option<u8>>,
}

const BOOST_NAMES: [&str; BOOST_COUNT] = ["atk", "def", "spa", "spd", "spe", "accuracy", "evasion"];

/// Applies an oracle patch to a state after the leads' switch-in effects.
pub fn apply_patch<const N: usize>(
    state: &mut State<N>,
    meta: &ScenarioMeta,
    patch: &PatchJson,
) -> Result<(), String> {
    for (side, mons) in [(SideId::One, &patch.p1), (SideId::Two, &patch.p2)] {
        for (name, p) in mons {
            patch_mon(state, meta, side, name, p)?;
        }
    }
    for (side_name, conditions) in &patch.sides {
        let side = match side_name.as_str() {
            "p1" => SideId::One,
            "p2" => SideId::Two,
            other => return Err(format!("unknown side {other:?}")),
        };
        for (id, duration) in conditions {
            let (effect, natural) = match id.as_str() {
                "tailwind" => (SideEffect::Tailwind, 4),
                "reflect" => (SideEffect::Reflect, 5),
                "lightscreen" => (SideEffect::LightScreen, 5),
                "auroraveil" => (SideEffect::AuroraVeil, 5),
                "safeguard" => (SideEffect::Safeguard, 5),
                "mist" => (SideEffect::Mist, 5),
                "luckychant" => (SideEffect::LuckyChant, 5),
                "wideguard" => (SideEffect::WideGuard, 1),
                "quickguard" => (SideEffect::QuickGuard, 1),
                other => return Err(format!("side condition {other:?} is not supported")),
            };
            // The screens' natural duration depends on the source's Light Clay.
            let screen = matches!(
                effect,
                SideEffect::Reflect | SideEffect::LightScreen | SideEffect::AuroraVeil
            );
            if duration.is_none() && screen {
                return Err(format!(
                    "side condition {id}: give a duration (Light Clay on the source changes it)"
                ));
            }
            state.side_mut(side).effects[effect as usize] = Effect {
                value: 0,
                turns: duration.unwrap_or(natural),
            };
        }
    }
    patch_field(state, &patch.field)
}

fn patch_mon<const N: usize>(
    state: &mut State<N>,
    meta: &ScenarioMeta,
    side: SideId,
    name: &str,
    p: &MonPatch,
) -> Result<(), String> {
    let index = meta.sides[side.index()]
        .party_index(name)
        .ok_or_else(|| format!("{side:?} has no Pokémon named {name}"))?;
    let slot = state
        .side(side)
        .slots
        .iter()
        .position(|s| s.party_index == Some(index));
    let mon = &mut state.side_mut(side).party[index as usize];
    if let Some(hp) = p.hp {
        // `sethp`: no effect on a fainted Pokémon; at least 1, at most max HP.
        if mon.hp > 0 {
            mon.hp = hp.clamp(1, mon.max_hp);
        }
    }
    if let Some(status) = &p.status {
        mon.status = Status::None;
        mon.status_turns = 0;
        let status = match status.as_str() {
            "" => Status::None,
            "brn" => Status::Burn,
            "par" => Status::Paralyze,
            "psn" => Status::Poison,
            "tox" => Status::Toxic,
            "frz" => Status::Freeze,
            "slp" => Status::Sleep,
            other => return Err(format!("{name}: status {other:?}")),
        };
        mon.status = status;
        mon.status_turns = match status {
            Status::Sleep => p
                .status_time
                .ok_or_else(|| format!("{name}: a sleep patch needs statusTime"))?,
            Status::Freeze => p.status_time.unwrap_or(3),
            _ => 0,
        };
    }
    if let Some(item) = &p.item {
        mon.item = if item.is_empty() {
            ItemId::NONE
        } else {
            ItemId::from_name(item).ok_or_else(|| format!("{name}: unknown item {item:?}"))?
        };
    }
    if let Some(boosts) = &p.boosts {
        let slot = slot.ok_or_else(|| format!("boosts on inactive {name}"))?;
        let mut stages = NO_BOOSTS;
        for (stat, &stage) in boosts {
            let i = BOOST_NAMES
                .iter()
                .position(|n| n == stat)
                .ok_or_else(|| format!("{name}: unknown boost {stat:?}"))?;
            stages[i] = stage;
        }
        // `Object.assign(mon.boosts, p.boosts)`: only the listed stats change.
        let current = &mut state.side_mut(side).slots[slot].boosts;
        for (i, stat) in BOOST_NAMES.iter().enumerate() {
            if boosts.contains_key(*stat) {
                current[i] = stages[i];
            }
        }
    }
    Ok(())
}

fn patch_field<const N: usize>(state: &mut State<N>, f: &FieldPatch) -> Result<(), String> {
    if let Some(weather) = &f.weather {
        let value = match weather.as_str() {
            "sunnyday" => Weather::Sun,
            "raindance" => Weather::Rain,
            "sandstorm" => Weather::Sand,
            "snowscape" => Weather::Snow,
            other => return Err(format!("weather {other:?} is not supported")),
        };
        let turns = f
            .weather_duration
            .ok_or("a weather patch needs weatherDuration")?;
        state.field[FieldEffect::Weather as usize] = Effect {
            value: value as u8,
            turns,
        };
    }
    if let Some(terrain) = &f.terrain {
        let value = match terrain.as_str() {
            "electricterrain" => Terrain::Electric,
            "grassyterrain" => Terrain::Grassy,
            "mistyterrain" => Terrain::Misty,
            "psychicterrain" => Terrain::Psychic,
            other => return Err(format!("terrain {other:?} is not supported")),
        };
        let turns = f
            .terrain_duration
            .ok_or("a terrain patch needs terrainDuration")?;
        state.field[FieldEffect::Terrain as usize] = Effect {
            value: value as u8,
            turns,
        };
    }
    for (id, duration) in &f.pseudo_weather {
        let effect = match id.as_str() {
            "gravity" => FieldEffect::Gravity,
            "trickroom" => FieldEffect::TrickRoom,
            other => return Err(format!("field effect {other:?} is not supported")),
        };
        if state.field[effect as usize].is_active() {
            return Err(format!(
                "{id} is already up (Showdown would fail or end it)"
            ));
        }
        state.field[effect as usize] = Effect {
            value: 0,
            turns: duration.unwrap_or(5),
        };
    }
    Ok(())
}

/// Showdown's `side.pokemon` order as party indices: the party order at the start, then every
/// switch swaps the newcomer's position with the one it replaces (`switchIn`). Choice strings
/// (`switch N`) count positions in this order.
pub type PartyOrder = Vec<u8>;

/// The party order at the start of the battle: the party itself.
pub fn initial_order<const N: usize>(state: &State<N>, side: SideId) -> PartyOrder {
    let party = &state.side(side).party;
    (0..party.len() as u8)
        .filter(|&i| !party[i as usize].species.is_none())
        .collect()
}

/// Updates both sides' party orders for the switches in `instructions` (an outcome's, in
/// order): a `Switch` that brings in a Pokémon swaps its position with the one leaving (an
/// occupant, or the fainted occupant a replacement relieves). A slot that emptied by a faint
/// swaps nothing; a slot that changed hands and then emptied brought in the Pokémon that its
/// following `SetFaintedOccupant` names.
pub fn advance_order(order: &mut [PartyOrder; 2], instructions: &[Instruction]) {
    for (i, instruction) in instructions.iter().enumerate() {
        let Instruction::Switch {
            slot,
            previous,
            party_index,
        } = instruction
        else {
            continue;
        };
        let outgoing = previous.party_index.or(previous.fainted_occupant);
        let incoming = party_index.or_else(|| {
            instructions[i + 1..].iter().find_map(|later| match later {
                Instruction::SetFaintedOccupant { slot: s, new, .. } if s == slot => *new,
                _ => None,
            })
        });
        let (Some(outgoing), Some(incoming)) = (outgoing, incoming) else {
            continue;
        };
        if outgoing == incoming {
            continue;
        }
        let order = &mut order[slot.side.index()];
        let a = order.iter().position(|&p| p == outgoing);
        let b = order.iter().position(|&p| p == incoming);
        if let (Some(a), Some(b)) = (a, b) {
            order.swap(a, b);
        }
    }
}

/// The party index `switch N` (1-based position in `order`) names.
fn switch_position(order: &[u8], n: &str, part: &str) -> Result<u8, String> {
    let n: usize = n.parse().map_err(|_| format!("{part:?}: bad switch"))?;
    if n == 0 {
        return Err(format!("{part:?}: switch positions start at 1"));
    }
    order
        .get(n - 1)
        .copied()
        .ok_or_else(|| format!("{part:?}: no Pokémon in position {n}"))
}

/// Parses one side's replacement choice after faints (`"switch 3"`, `"switch 3, switch 4"`,
/// `""` for a side that waits): the switches fill the side's empty slots in slot order;
/// `pass` leaves one empty. Returns the party index per active slot.
pub fn parse_replacement<const N: usize>(
    state: &State<N>,
    side: SideId,
    order: &[u8],
    text: &str,
) -> Result<[Option<u8>; N], String> {
    let mut out = [None; N];
    let text = text.trim();
    if text.is_empty() {
        return Ok(out);
    }
    let mut empty = (0..N).filter(|&i| state.side(side).slots[i].party_index.is_none());
    for part in text.split(',').map(str::trim) {
        let words: Vec<&str> = part.split_whitespace().collect();
        let Some(slot) = empty.next() else {
            return Err(format!("{part:?}: no empty slot left to fill"));
        };
        match words.as_slice() {
            ["pass"] => {}
            ["switch", n] => out[slot] = Some(switch_position(order, n, part)?),
            _ => return Err(format!("{part:?}: not a replacement choice")),
        }
    }
    Ok(out)
}

/// Parses one side's Showdown choice string (`"move protect, move grassyglide 1"`,
/// `"switch 3"`, `"pass"`, `"move 2 -1 mega"`) against `state`.
///
/// A move is its id, name or 1-based slot; the target is Showdown's (positive = foe,
/// negative = ally). `switch N` is the Nth Pokémon in `order`, Showdown's current party order
/// (see [`advance_order`]).
pub fn parse_choice<const N: usize>(
    state: &State<N>,
    side: SideId,
    order: &[u8],
    text: &str,
) -> Result<JointAction<N>, String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    if parts.len() != N {
        return Err(format!("{text:?}: expected {N} actions"));
    }
    let mut out = [SlotAction::Pass; N];
    for (i, part) in parts.iter().enumerate() {
        let slot = SlotRef {
            side,
            slot: i as u8,
        };
        let words: Vec<&str> = part.split_whitespace().collect();
        out[i] = match words.as_slice() {
            ["pass"] => SlotAction::Pass,
            ["switch", n] => SlotAction::Switch {
                party_index: switch_position(order, n, part)?,
            },
            // A recharging Pokémon's only choice; the turn engine substitutes the locked
            // action for whatever move is named, as Showdown does.
            ["move", "recharge", ..] => SlotAction::Move {
                index: 0,
                target: 0,
                gimmick: Gimmick::None,
            },
            ["move", mv, rest @ ..] => {
                let mon = state
                    .active(slot)
                    .ok_or_else(|| format!("{part:?}: empty slot"))?;
                let index = match mv.parse::<u8>() {
                    Ok(n) if (1..=4).contains(&n) => n - 1,
                    _ => {
                        let id = MoveId::from_id(&to_id(mv))
                            .ok_or_else(|| format!("{part:?}: unknown move"))?;
                        mon.moves
                            .iter()
                            .position(|m| m.id == id)
                            .ok_or_else(|| format!("{part:?}: not a known move"))?
                            as u8
                    }
                };
                let mut target = 0i8;
                let mut gimmick = Gimmick::None;
                for word in rest {
                    match *word {
                        "mega" => gimmick = Gimmick::Mega,
                        "terastallize" => gimmick = Gimmick::Tera,
                        "dynamax" | "max" => gimmick = Gimmick::Dynamax,
                        "zmove" => gimmick = Gimmick::ZMove,
                        "ultra" => gimmick = Gimmick::UltraBurst,
                        t => {
                            target = t
                                .parse()
                                .map_err(|_| format!("{part:?}: unknown word {t:?}"))?
                        }
                    }
                }
                SlotAction::Move {
                    index,
                    target,
                    gimmick,
                }
            }
            _ => return Err(format!("{part:?}: not a choice")),
        };
    }
    Ok(out)
}
