//! Canonical state JSON, schema 1: the comparison format of `engine/oracle/canonical.cjs`.
//!
//! [`canonical_json`] writes the exact string `canonicalKey(canonical(battle))` gives for the
//! same position: same field names and ids, same key order, Pokémon sorted by name. Showdown
//! data the `State` does not hold is derived from the dex and the sidecar ([`ScenarioMeta`]):
//! display names from the meta, species names and item/ability/move ids from the dex.
//!
//! State the engine can hold but whose canonical form is not settled yet is an error
//! ([`CanonicalError`]), never dropped: volatile bits, substitutes, Dynamax, side effects,
//! pseudo-weathers, sleep/toxic counters, type changes, disabled moves, fainted Pokémon.
//! `lastMove`, `lastItem`, `statusTime`/`statusStage`, and non-empty `volatiles`/`conditions`
//! are therefore never written.

use std::fmt;
use std::fmt::Write as _;

use lab_engine::field::{Effect, FieldEffect, Terrain, Weather, FIELD_EFFECT_COUNT};
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::Ruleset;
use lab_engine::state::{Pokemon, SideId, State, Status, PARTY_SIZE};

use crate::meta::{ScenarioMeta, SideMeta};

/// `canonical.cjs` `SCHEMA`.
pub const SCHEMA: u32 = 1;

const BOOST_NAMES: [&str; 7] = ["atk", "def", "spa", "spd", "spe", "accuracy", "evasion"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalError {
    /// The ruleset (for `canMega`) is only known for the oracle's Champions format.
    UnknownFormat(String),
    /// `ScenarioMeta` and `State` disagree (party size, names).
    Meta { side: SideId, reason: String },
    /// State with no canonical schema-1 form yet.
    Unrepresentable { what: String },
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::UnknownFormat(format) => {
                write!(f, "no ruleset known for format {format:?}")
            }
            CanonicalError::Meta { side, reason } => {
                write!(
                    f,
                    "{}: meta does not match the state: {reason}",
                    side_name(*side)
                )
            }
            CanonicalError::Unrepresentable { what } => {
                write!(f, "not representable in canonical schema {SCHEMA}: {what}")
            }
        }
    }
}

impl std::error::Error for CanonicalError {}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

fn unrepresentable(what: String) -> CanonicalError {
    CanonicalError::Unrepresentable { what }
}

/// The gimmick rules of a scenario format (they decide `canMega`).
pub fn format_ruleset(format: &str) -> Result<Ruleset, CanonicalError> {
    if format == crate::DOUBLES_FORMAT {
        // The Champions mod's `canMegaEvo` applies; no other gimmick exists there.
        Ok(Ruleset::CHAMPIONS_MC)
    } else {
        Err(CanonicalError::UnknownFormat(format.to_owned()))
    }
}

/// `canonical(battle)` as a compact JSON string, byte-identical to `canonicalKey`.
pub fn canonical_json<const N: usize>(
    state: &State<N>,
    meta: &ScenarioMeta,
) -> Result<String, CanonicalError> {
    let ruleset = format_ruleset(&meta.format)?;
    let mut out = String::new();
    write!(
        out,
        r#"{{"schema":{SCHEMA},"turn":{},"ended":false,"winner":"","field":"#,
        state.turn
    )
    .unwrap();
    field(&mut out, &state.field)?;
    out.push_str(r#","sides":["#);
    for (i, side) in [SideId::One, SideId::Two].into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        side_json(&mut out, state, side, &meta.sides[side.index()], ruleset)?;
    }
    out.push_str("]}");
    Ok(out)
}

/// [`canonical_json`] parsed, for comparing with oracle output regardless of key order.
pub fn canonical_value<const N: usize>(
    state: &State<N>,
    meta: &ScenarioMeta,
) -> Result<serde_json::Value, CanonicalError> {
    let text = canonical_json(state, meta)?;
    Ok(serde_json::from_str(&text).expect("canonical_json writes valid JSON"))
}

fn string(out: &mut String, s: &str) {
    // serde_json escapes exactly like JSON.stringify for valid UTF-8 strings.
    out.push_str(&serde_json::to_string(s).expect("strings always serialize"));
}

fn weather_id(value: u8) -> Result<&'static str, CanonicalError> {
    Ok(match value {
        v if v == Weather::Sun as u8 => "sunnyday",
        v if v == Weather::Rain as u8 => "raindance",
        v if v == Weather::Sand as u8 => "sandstorm",
        v if v == Weather::Snow as u8 => "snowscape",
        // Primal weathers have no duration in Showdown; `turns` cannot say that yet.
        _ => return Err(unrepresentable(format!("weather value {value}"))),
    })
}

fn terrain_id(value: u8) -> Result<&'static str, CanonicalError> {
    Ok(match value {
        v if v == Terrain::Electric as u8 => "electricterrain",
        v if v == Terrain::Grassy as u8 => "grassyterrain",
        v if v == Terrain::Misty as u8 => "mistyterrain",
        v if v == Terrain::Psychic as u8 => "psychicterrain",
        _ => return Err(unrepresentable(format!("terrain value {value}"))),
    })
}

/// A weather or terrain slot: `(id, duration)` where `Effect::turns` is Showdown's remaining
/// `duration`.
fn timed(
    effect: Effect,
    name: &str,
    id: fn(u8) -> Result<&'static str, CanonicalError>,
) -> Result<Option<(&'static str, u8)>, CanonicalError> {
    if !effect.is_active() {
        if effect != Effect::NONE {
            return Err(unrepresentable(format!(
                "inactive {name} with value {}",
                effect.value
            )));
        }
        return Ok(None);
    }
    if effect.turns == Effect::PERMANENT {
        return Err(unrepresentable(format!("{name} without a duration")));
    }
    Ok(Some((id(effect.value)?, effect.turns)))
}

fn field(out: &mut String, effects: &[Effect; FIELD_EFFECT_COUNT]) -> Result<(), CanonicalError> {
    let weather = FieldEffect::Weather as usize;
    let terrain = FieldEffect::Terrain as usize;
    for (i, effect) in effects.iter().enumerate() {
        if i != weather && i != terrain && *effect != Effect::NONE {
            return Err(unrepresentable(format!("pseudo-weather field effect #{i}")));
        }
    }
    let weather = timed(effects[weather], "weather", weather_id)?;
    let terrain = timed(effects[terrain], "terrain", terrain_id)?;
    for (sep, key, value) in [('{', "weather", weather), (',', "terrain", terrain)] {
        let written = match value {
            Some((id, turns)) => write!(out, r#"{sep}"{key}":"{id}","{key}Duration":{turns}"#),
            None => write!(out, r#"{sep}"{key}":"","{key}Duration":null"#),
        };
        written.unwrap();
    }
    out.push_str(r#","pseudoWeather":{}}"#);
    Ok(())
}

fn side_json<const N: usize>(
    out: &mut String,
    state: &State<N>,
    side_id: SideId,
    meta: &SideMeta,
    ruleset: Ruleset,
) -> Result<(), CanonicalError> {
    let side = state.side(side_id);
    let meta_error = |reason: String| CanonicalError::Meta {
        side: side_id,
        reason,
    };
    let members = meta.members.len();
    if members == 0 || members > PARTY_SIZE {
        return Err(meta_error(format!("{members} members")));
    }
    for (i, mon) in side.party.iter().enumerate() {
        if (i < members) == mon.species.is_none() {
            return Err(meta_error(format!(
                "party[{i}] does not match the member list"
            )));
        }
    }
    if let Some(i) = side.effects.iter().position(|e| *e != Effect::NONE) {
        return Err(unrepresentable(format!(
            "{} side effect #{i}",
            side_name(side_id)
        )));
    }

    // Active slot of each party member.
    let mut slot_of: [Option<u8>; PARTY_SIZE] = [None; PARTY_SIZE];
    for (slot, s) in side.slots.iter().enumerate() {
        let Some(index) = s.party_index else {
            return Err(unrepresentable(format!(
                "{} slot {slot} is empty (a switch request)",
                side_name(side_id)
            )));
        };
        let index = index as usize;
        if index >= members || slot_of[index].is_some() {
            return Err(meta_error(format!("slot {slot} holds party[{index}]")));
        }
        slot_of[index] = Some(slot as u8);
        if s.volatiles != 0 {
            return Err(unrepresentable(format!(
                "{} slot {slot} volatile bits {:#x}",
                side_name(side_id),
                s.volatiles
            )));
        }
        if s.substitute_hp != 0 || s.dynamax.is_active() {
            return Err(unrepresentable(format!(
                "{} slot {slot} substitute or Dynamax",
                side_name(side_id)
            )));
        }
    }

    // canMegaEvo is cleared for the whole side once it Mega Evolves.
    let mega_open = ruleset.allows(Gimmick::Mega) && !side.gimmicks_used.contains(Gimmick::Mega);

    out.push_str(r#"{"request":"move","conditions":{},"slotConditions":["#);
    for slot in 0..N {
        out.push_str(if slot == 0 { "{}" } else { ",{}" });
    }
    out.push_str(r#"],"pokemon":["#);
    for (n, index) in meta.canonical_order().into_iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        let index = index as usize;
        let name = &meta.members[index].name;
        let boosts = slot_of[index].map(|slot| &side.slots[slot as usize].boosts);
        pokemon(
            out,
            &side.party[index],
            name,
            slot_of[index],
            boosts,
            mega_open,
        )
        .map_err(|what| unrepresentable(format!("{} {name:?}: {what}", side_name(side_id))))?;
    }
    out.push_str("]}");
    Ok(())
}

fn status_id(status: Status) -> Result<&'static str, String> {
    Ok(match status {
        Status::None => "",
        Status::Burn => "brn",
        Status::Freeze => "frz",
        Status::Paralyze => "par",
        Status::Poison => "psn",
        // `statusTime` / `statusStage` need a settled meaning of `status_turns`.
        Status::Sleep | Status::Toxic => return Err(format!("status {status:?}")),
    })
}

fn pokemon(
    out: &mut String,
    mon: &Pokemon,
    name: &str,
    slot: Option<u8>,
    boosts: Option<&[i8; 7]>,
    mega_open: bool,
) -> Result<(), String> {
    if !mon.is_alive() {
        return Err("fainted (needs `fnt` and switch requests)".into());
    }
    let status = status_id(mon.status)?;
    if mon.status_turns != 0 {
        return Err(format!("status counter {}", mon.status_turns));
    }
    if slot.is_some() && mon.types != mon.species.data().types {
        return Err(format!("types {:?} differ from the species", mon.types));
    }
    if let Some(m) = mon.moves.iter().find(|m| m.disabled) {
        return Err(format!("disabled move {:?}", m.id));
    }

    out.push_str(r#"{"name":"#);
    string(out, name);
    out.push_str(r#","species":"#);
    string(out, mon.species.data().name);
    write!(
        out,
        r#","hp":{},"maxhp":{},"status":"{status}","item":"{}","ability":"{}","slot":"#,
        mon.hp,
        mon.max_hp,
        mon.item.id(),
        mon.ability.id()
    )
    .unwrap();
    match slot {
        Some(slot) => write!(out, "{slot}").unwrap(),
        None => out.push_str("null"),
    }
    out.push_str(r#","pp":{"#);
    for (i, m) in mon.moves.iter().filter(|m| !m.id.is_none()).enumerate() {
        let sep = if i == 0 { "" } else { "," };
        write!(out, r#"{sep}"{}":{}"#, m.id.id(), m.pp).unwrap();
    }
    out.push('}');
    if mega_open && mon.gimmicks.contains(Gimmick::Mega) {
        out.push_str(r#","canMega":true"#);
    }
    if let Some(boosts) = boosts {
        out.push_str(r#","boosts":{"#);
        let mut first = true;
        for (name, &stage) in BOOST_NAMES.iter().zip(boosts) {
            if stage != 0 {
                let sep = if first { "" } else { "," };
                write!(out, r#"{sep}"{name}":{stage}"#).unwrap();
                first = false;
            }
        }
        out.push_str(r#"},"volatiles":{}"#);
    }
    out.push('}');
    Ok(())
}
