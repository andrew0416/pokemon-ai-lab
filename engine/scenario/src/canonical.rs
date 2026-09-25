//! Canonical state JSON, schema 1: the comparison format of `engine/oracle/canonical.cjs`.
//!
//! [`canonical_json`] writes the exact string `canonicalKey(canonical(battle))` gives for the
//! same position: same field names and ids, same key order, Pokémon sorted by name. Showdown
//! data the `State` does not hold is derived from the dex and the sidecar ([`ScenarioMeta`]):
//! display names from the meta, species names and item/ability/move ids from the dex.
//!
//! Derived fields follow Showdown: `request` is `switch` for a side that must replace a
//! fainted Pokémon (the other side waits, `""`), `""` for both once the battle ended, and
//! `move` otherwise; `winner` is the side's player name (`p1`/`p2`, as the oracle names
//! them).
//!
//! State the engine can hold but whose canonical form is not settled yet is an error
//! ([`CanonicalError`]), never dropped: substitutes, Dynamax, Magic/Wonder Room, primal
//! weathers, permanent effects, disabled moves.

use std::fmt;
use std::fmt::Write as _;

use lab_engine::dex::Type;
use lab_engine::field::{Effect, FieldEffect, SideEffect, Terrain, Weather, FIELD_EFFECT_COUNT};
use lab_engine::gimmick::Gimmick;
use lab_engine::rules::Ruleset;
use lab_engine::state::{BattleResult, Pokemon, SideId, State, Status, PARTY_SIZE};
use lab_engine::volatile::VolatileState;

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
    let (ended, winner) = match state.result {
        BattleResult::Ongoing => (false, ""),
        BattleResult::Tie => (true, ""),
        BattleResult::Win(side) => (true, side_name(side)),
    };
    let mut out = String::new();
    write!(
        out,
        r#"{{"schema":{SCHEMA},"turn":{},"ended":{ended},"winner":"{winner}","field":"#,
        state.turn
    )
    .unwrap();
    field(&mut out, &state.field)?;
    let requests = requests(state);
    out.push_str(r#","sides":["#);
    for (i, side) in [SideId::One, SideId::Two].into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        side_json(
            &mut out,
            state,
            side,
            &meta.sides[side.index()],
            ruleset,
            requests[i],
        )?;
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

/// Showdown `side.requestState` after the position was reached.
fn requests<const N: usize>(state: &State<N>) -> [&'static str; 2] {
    if state.result.is_over() {
        return ["", ""];
    }
    let needs = [SideId::One, SideId::Two].map(|side| {
        let s = state.side(side);
        let empty = s.slots.iter().any(|slot| slot.party_index.is_none());
        let bench = (0..s.party.len() as u8).any(|i| {
            s.party[i as usize].hp > 0 && !s.slots.iter().any(|slot| slot.party_index == Some(i))
        });
        empty && bench
    });
    if needs.iter().any(|&n| n) {
        needs.map(|n| if n { "switch" } else { "" })
    } else {
        ["move", "move"]
    }
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

/// A timed effect: `Some(duration)` where `Effect::turns` is Showdown's remaining `duration`.
fn timed(effect: Effect, name: &str) -> Result<Option<u8>, CanonicalError> {
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
    Ok(Some(effect.turns))
}

fn field(out: &mut String, effects: &[Effect; FIELD_EFFECT_COUNT]) -> Result<(), CanonicalError> {
    // Pseudo-weathers the schema can write, in id order.
    const PSEUDO: [(FieldEffect, &str); 2] = [
        (FieldEffect::Gravity, "gravity"),
        (FieldEffect::TrickRoom, "trickroom"),
    ];
    for (i, effect) in effects.iter().enumerate() {
        let known = i == FieldEffect::Weather as usize
            || i == FieldEffect::Terrain as usize
            || PSEUDO.iter().any(|&(e, _)| e as usize == i);
        if !known && *effect != Effect::NONE {
            return Err(unrepresentable(format!("pseudo-weather field effect #{i}")));
        }
    }
    let weather = effects[FieldEffect::Weather as usize];
    let terrain = effects[FieldEffect::Terrain as usize];
    let weather = match timed(weather, "weather")? {
        Some(turns) => Some((weather_id(weather.value)?, turns)),
        None => None,
    };
    let terrain = match timed(terrain, "terrain")? {
        Some(turns) => Some((terrain_id(terrain.value)?, turns)),
        None => None,
    };
    for (sep, key, value) in [('{', "weather", weather), (',', "terrain", terrain)] {
        let written = match value {
            Some((id, turns)) => write!(out, r#"{sep}"{key}":"{id}","{key}Duration":{turns}"#),
            None => write!(out, r#"{sep}"{key}":"","{key}Duration":null"#),
        };
        written.unwrap();
    }
    out.push_str(r#","pseudoWeather":{"#);
    let mut first = true;
    for (effect, id) in PSEUDO {
        if let Some(turns) = timed(effects[effect as usize], id)? {
            let sep = if first { "" } else { "," };
            write!(out, r#"{sep}"{id}":{{"duration":{turns}}}"#).unwrap();
            first = false;
        }
    }
    out.push_str("}}");
    Ok(())
}

/// Side conditions the schema can write, in id order.
const SIDE_CONDITIONS: [(SideEffect, &str); 9] = [
    (SideEffect::AuroraVeil, "auroraveil"),
    (SideEffect::LightScreen, "lightscreen"),
    (SideEffect::LuckyChant, "luckychant"),
    (SideEffect::Mist, "mist"),
    (SideEffect::QuickGuard, "quickguard"),
    (SideEffect::Reflect, "reflect"),
    (SideEffect::Safeguard, "safeguard"),
    (SideEffect::Tailwind, "tailwind"),
    (SideEffect::WideGuard, "wideguard"),
];

fn side_json<const N: usize>(
    out: &mut String,
    state: &State<N>,
    side_id: SideId,
    meta: &SideMeta,
    ruleset: Ruleset,
    request: &str,
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
    for (i, effect) in side.effects.iter().enumerate() {
        if *effect != Effect::NONE && !SIDE_CONDITIONS.iter().any(|&(e, _)| e as usize == i) {
            return Err(unrepresentable(format!(
                "{} side effect #{i}",
                side_name(side_id)
            )));
        }
    }

    // Active slot of each party member.
    let mut slot_of: [Option<u8>; PARTY_SIZE] = [None; PARTY_SIZE];
    for (slot, s) in side.slots.iter().enumerate() {
        let Some(index) = s.party_index else {
            continue;
        };
        let index = index as usize;
        if index >= members || slot_of[index].is_some() {
            return Err(meta_error(format!("slot {slot} holds party[{index}]")));
        }
        slot_of[index] = Some(slot as u8);
        if s.substitute_hp != 0 || s.dynamax.is_active() {
            return Err(unrepresentable(format!(
                "{} slot {slot} substitute or Dynamax",
                side_name(side_id)
            )));
        }
    }

    // canMegaEvo is cleared for the whole side once it Mega Evolves.
    let mega_open = ruleset.allows(Gimmick::Mega) && !side.gimmicks_used.contains(Gimmick::Mega);

    write!(out, r#"{{"request":"{request}","conditions":{{"#).unwrap();
    let mut first = true;
    for (effect, id) in SIDE_CONDITIONS {
        if let Some(turns) = timed(side.effects[effect as usize], id)? {
            let sep = if first { "" } else { "," };
            write!(out, r#"{sep}"{id}":{{"duration":{turns}}}"#).unwrap();
            first = false;
        }
    }
    out.push_str(r#"},"slotConditions":["#);
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
        let active = slot_of[index].map(|slot| (slot, &side.slots[slot as usize]));
        pokemon(out, &side.party[index], name, active, mega_open)
            .map_err(|what| unrepresentable(format!("{} {name:?}: {what}", side_name(side_id))))?;
    }
    out.push_str("]}");
    Ok(())
}

fn status_id(status: Status) -> &'static str {
    match status {
        Status::None => "",
        Status::Burn => "brn",
        Status::Freeze => "frz",
        Status::Paralyze => "par",
        Status::Poison => "psn",
        Status::Toxic => "tox",
        Status::Sleep => "slp",
        Status::Fainted => "fnt",
    }
}

/// Showdown effect-state fields in `EFFECT_FIELDS` order (`duration`, `counter`).
fn volatile_fields(out: &mut String, state: VolatileState) {
    out.push('{');
    let mut first = true;
    if state.duration != 0 {
        write!(out, r#""duration":{}"#, state.duration).unwrap();
        first = false;
    }
    if state.counter != 0 {
        let sep = if first { "" } else { "," };
        write!(out, r#"{sep}"counter":{}"#, state.counter).unwrap();
    }
    out.push('}');
}

fn pokemon(
    out: &mut String,
    mon: &Pokemon,
    name: &str,
    active: Option<(u8, &lab_engine::state::Slot)>,
    mega_open: bool,
) -> Result<(), String> {
    if let Some(m) = mon.moves.iter().find(|m| m.disabled) {
        return Err(format!("disabled move {:?}", m.id));
    }

    out.push_str(r#"{"name":"#);
    string(out, name);
    out.push_str(r#","species":"#);
    string(out, mon.species.data().name);
    write!(
        out,
        r#","hp":{},"maxhp":{},"status":"{}","item":"{}","ability":"{}","slot":"#,
        mon.hp,
        mon.max_hp,
        status_id(mon.status),
        mon.item.id(),
        mon.ability.id()
    )
    .unwrap();
    match active {
        Some((slot, _)) => write!(out, "{slot}").unwrap(),
        None => out.push_str("null"),
    }
    out.push_str(r#","pp":{"#);
    for (i, m) in mon.moves.iter().filter(|m| !m.id.is_none()).enumerate() {
        let sep = if i == 0 { "" } else { "," };
        write!(out, r#"{sep}"{}":{}"#, m.id.id(), m.pp).unwrap();
    }
    out.push('}');
    match mon.status {
        Status::Sleep => write!(out, r#","statusTime":{}"#, mon.status_turns).unwrap(),
        Status::Toxic => write!(out, r#","statusStage":{}"#, mon.status_turns).unwrap(),
        _ => {}
    }
    if !mon.last_item.is_none() {
        write!(out, r#","lastItem":"{}""#, mon.last_item.id()).unwrap();
    }
    if mega_open && mon.gimmicks.contains(Gimmick::Mega) {
        out.push_str(r#","canMega":true"#);
    }
    if let Some((_, slot)) = active {
        out.push_str(r#","boosts":{"#);
        let mut first = true;
        for (name, &stage) in BOOST_NAMES.iter().zip(&slot.boosts) {
            if stage != 0 {
                let sep = if first { "" } else { "," };
                write!(out, r#"{sep}"{name}":{stage}"#).unwrap();
                first = false;
            }
        }
        out.push_str(r#"},"volatiles":{"#);
        // Showdown's `pokemon.volatiles` only (engine-only kinds and payload left out).
        let mut volatiles: Vec<_> = slot
            .volatiles
            .iter()
            .filter_map(|(v, state)| Some((v, v.showdown_state(state)?)))
            .collect();
        volatiles.sort_by_key(|(v, _)| v.id());
        for (i, (volatile, state)) in volatiles.into_iter().enumerate() {
            let sep = if i == 0 { "" } else { "," };
            write!(out, r#"{sep}"{}":"#, volatile.id()).unwrap();
            volatile_fields(out, state);
        }
        out.push('}');
        if !slot.last_move.is_none() {
            write!(out, r#","lastMove":"{}""#, slot.last_move.id()).unwrap();
        }
        // `getTypes(true)` joined, only when it differs from the species' types.
        if mon.types != mon.species.data().types {
            out.push_str(r#","types":"#);
            string(out, &types_string(mon.types)?);
        }
    }
    out.push('}');
    Ok(())
}

/// Showdown `getTypes().join('/')`. A Pokémon with no type at all is `???` there; the
/// engine has no such state yet, so it is an error rather than a guess.
fn types_string(types: [Type; 2]) -> Result<String, String> {
    let names: Vec<&str> = types
        .iter()
        .filter(|&&t| t != Type::None)
        .map(|t| t.name())
        .collect();
    if names.is_empty() {
        return Err("no types (Showdown `???`)".into());
    }
    Ok(names.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `effects()` in `canonical.cjs` writes side conditions sorted by id.
    #[test]
    fn side_conditions_are_in_id_order() {
        assert!(SIDE_CONDITIONS.windows(2).all(|w| w[0].1 < w[1].1));
    }
}
