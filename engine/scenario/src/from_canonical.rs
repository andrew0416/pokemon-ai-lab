//! Canonical state JSON (schema 1) → `State<N>`: hand-built and edited positions (board PY2),
//! and the engine's optional hidden-state extension (`x-hidden`, board R2-t1).
//!
//! [`state_from_canonical`] is the inverse of [`crate::canonical_json`]: it rebuilds the state a
//! canonical JSON object describes, so a position can be written by hand, or read with
//! `canonical_json` / `Position.state_json()`, edited, and put back. The canonical form keys
//! Pokémon by display name and leaves out everything a set fixes (level, nature, Stat Points,
//! gender, moves' order), so the rebuild takes that set data from a `base` state of the same
//! battle (the scenario's loaded state, or any position of it) and its [`ScenarioMeta`].
//!
//! # What the canonical form does not say
//!
//! Showdown keeps state the canonical form leaves out (`canonical.cjs` prints only what the
//! oracle compares). Without the extension the rebuild uses these defaults:
//!
//! | State | Default |
//! |---|---|
//! | Showdown's party order (`switch N`; `Side::party_order` where Beat Up is in a party) | the active (or fainted, unreplaced) Pokémon in slot order, then the rest in team preview order |
//! | which fainted Pokémon still holds an empty slot (`Slot::fainted_occupant`) | the `fnt` Pokémon in team preview order fill the empty slots in slot order |
//! | entry hazard order (`SideHistory::hazard_order`) | id order (as the oracle's `patch`) |
//! | faint counters (`totalFainted`, `faintedThisTurn`, `faintedLastTurn`) | the fainted members counted; `false`; `false` |
//! | once-per-battle flags (Intrepid Sword, Dauntless Shield, Supersweet Syrup), `ateBerry` | unset |
//! | `battle.lastMove` (Copycat), `lastMoveTargetLoc` (Instruct), `abilityState.effectOrder` | none / 0 / 0 (battle-start order) |
//! | `activeMoveActions` (Fake Out) | 1 if the Pokémon has a `lastMove`, else 0 |
//! | the slot's damage and move history (`SlotHistory`) | Showdown's switch-in values; `newlySwitched` only on turn 1 |
//! | `switchFlag` | none (a canonical `request` for a mid-turn switch is refused) |
//! | freeze's remaining turns | 3 (as a freeze starts) |
//! | stored stats | the set's for its current forme (Power Trick / Power Shift swapped); Power Split, Guard Split and Speed Swap are lost |
//! | base ability | the set's; a permanent forme's first ability (Mega Evolution) |
//! | Wish's starting turn | the previous turn (a Wish seen at a decision always heals at the next residual) |
//! | engine-only volatiles (`Volatile::showdown_state` is `None`: Protean's use, Slow Start's count, an added type, ...), Autotomize, Illusion | absent |
//!
//! A few things have no safe default and are refused ([`ScenarioError::Unsupported`]) unless
//! the extension gives them: a volatile whose canonical fields drop state the engine reads
//! (Leech Seed's source, partial trapping's source, Protosynthesis's stat, a two-turn move's
//! target, Stockpile's raises, ... — [`volatile_is_lossy`]), a transformed Pokémon, and a pending
//! Future Sight / Doom Desire (its user and turn).
//!
//! # The `x-hidden` extension
//!
//! [`canonical_json_hidden`] writes the canonical string followed by one more top-level member,
//! `"x-hidden"`, holding exactly the hidden values that differ from the defaults above. The
//! canonical part is byte-identical to [`crate::canonical_json`] (the oracle's `canonicalKey`
//! never sees the extension: strip the member and the bytes are the same), and
//! `state_from_canonical(canonical_json_hidden(s))` gives back `s` itself (and the party
//! orders when they were passed). The rebuild checks that the result prints the canonical part
//! it was given; anything that does not is an error, not a silent difference.

use serde_json::{json, Map, Value};

use lab_engine::dex::{abilities, AbilityId, Gender, ItemId, MoveId, SpeciesId, Type};
use lab_engine::field::{
    Effect, FieldEffect, HazardOrder, SideEffect, SlotCondition, SlotEffect, Terrain, Weather,
    FUTURE_MOVE_DOOM_DESIRE, HAZARDS,
};
use lab_engine::gimmick::{Gimmick, GimmickSet};
use lab_engine::state::{
    DamagedBy, MoveResult, MoveSlot, Pokemon, PokemonRef, SideId, Slot, SlotHistory, SlotRef,
    State, Status, SwitchFlag, TransformBase, IDENTITY_ORDER, PARTY_SIZE,
};
use lab_engine::volatile::{Volatile, VolatileState};

use crate::canonical::{canonical_json, canonical_value, format_ruleset};
use crate::decision::PartyOrder;
use crate::error::ScenarioError;
use crate::meta::ScenarioMeta;

/// The top-level key of the hidden-state extension.
pub const HIDDEN_KEY: &str = "x-hidden";

/// A rebuilt position: the state and Showdown's party order per side (what `switch N` counts).
#[derive(Clone, Debug, PartialEq)]
pub struct Rebuilt<const N: usize> {
    pub state: State<N>,
    pub order: [PartyOrder; 2],
}

/// Something the canonical form cannot rebuild without the extension.
#[derive(Clone, Debug, PartialEq)]
enum Need {
    Volatile(SlotRef, Volatile),
    Transformed(PokemonRef),
    FutureMove(SlotRef),
    /// A `switch` request on a side with no empty slot: a mid-turn switch (U-turn, Eject
    /// Button), whose flagged slot the canonical form does not name.
    MidTurn(SideId),
}

impl Need {
    fn describe(&self, meta: &ScenarioMeta) -> String {
        match self {
            Need::Volatile(slot, v) => format!(
                "{} slot {}: volatile {:?} carries state its canonical fields leave out",
                side_name(slot.side),
                slot.slot,
                v.id()
            ),
            Need::Transformed(p) => format!(
                "{} {:?}: a transformed Pokémon (its own species and move slots are not canonical)",
                side_name(p.side),
                meta.sides[p.side.index()].name(p.party).unwrap_or("?")
            ),
            Need::FutureMove(slot) => format!(
                "{} slot {}: a pending future move (its user and turn are not canonical)",
                side_name(slot.side),
                slot.slot
            ),
            Need::MidTurn(side) => format!(
                "{}: a mid-turn switch request (which slot must switch out is not canonical)",
                side_name(*side)
            ),
        }
    }
}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::One => "p1",
        SideId::Two => "p2",
    }
}

fn invalid(what: impl Into<String>) -> ScenarioError {
    ScenarioError::Invalid(what.into())
}

fn unsupported(what: impl Into<String>) -> ScenarioError {
    ScenarioError::Unsupported(what.into())
}

// ---- small JSON readers ------------------------------------------------------------------------

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, ScenarioError> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{what}: expected an object")))
}

fn get<'a>(obj: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a Value, ScenarioError> {
    obj.get(key)
        .ok_or_else(|| invalid(format!("{what}: missing {key:?}")))
}

fn int(value: &Value, what: &str) -> Result<i64, ScenarioError> {
    value
        .as_i64()
        .ok_or_else(|| invalid(format!("{what}: expected an integer, got {value}")))
}

fn int_in<T: TryFrom<i64>>(value: &Value, what: &str) -> Result<T, ScenarioError> {
    let n = int(value, what)?;
    T::try_from(n).map_err(|_| invalid(format!("{what}: {n} is out of range")))
}

fn text<'a>(value: &'a Value, what: &str) -> Result<&'a str, ScenarioError> {
    value
        .as_str()
        .ok_or_else(|| invalid(format!("{what}: expected a string, got {value}")))
}

fn boolean(value: &Value, what: &str) -> Result<bool, ScenarioError> {
    value
        .as_bool()
        .ok_or_else(|| invalid(format!("{what}: expected true or false, got {value}")))
}

fn move_id(id: &str, what: &str) -> Result<MoveId, ScenarioError> {
    if id.is_empty() {
        return Ok(MoveId::NONE);
    }
    MoveId::from_id(id).ok_or_else(|| invalid(format!("{what}: unknown move {id:?}")))
}

fn item_id(id: &str, what: &str) -> Result<ItemId, ScenarioError> {
    if id.is_empty() {
        return Ok(ItemId::NONE);
    }
    ItemId::from_id(id).ok_or_else(|| invalid(format!("{what}: unknown item {id:?}")))
}

fn ability_id(id: &str, what: &str) -> Result<AbilityId, ScenarioError> {
    if id.is_empty() {
        return Ok(AbilityId::NONE);
    }
    AbilityId::from_id(id).ok_or_else(|| invalid(format!("{what}: unknown ability {id:?}")))
}

fn type_by_name(name: &str) -> Option<Type> {
    Type::ALL
        .into_iter()
        .chain([Type::Unknown])
        .find(|t| t.name() == name)
}

/// `getTypes().join('/')` read back.
fn parse_types(joined: &str, what: &str) -> Result<[Type; 2], ScenarioError> {
    let parts: Vec<&str> = joined.split('/').collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(unsupported(format!(
            "{what}: types {joined:?} (the engine holds at most two)"
        )));
    }
    let mut out = [Type::None; 2];
    for (i, part) in parts.iter().enumerate() {
        out[i] = type_by_name(part).ok_or_else(|| invalid(format!("{what}: type {part:?}")))?;
    }
    Ok(out)
}

fn status_from_id(id: &str, what: &str) -> Result<Status, ScenarioError> {
    Ok(match id {
        "" => Status::None,
        "brn" => Status::Burn,
        "frz" => Status::Freeze,
        "par" => Status::Paralyze,
        "psn" => Status::Poison,
        "tox" => Status::Toxic,
        "slp" => Status::Sleep,
        "fnt" => Status::Fainted,
        other => return Err(invalid(format!("{what}: status {other:?}"))),
    })
}

/// The Showdown volatile with canonical id `id` (engine-only kinds have none).
fn volatile_by_id(id: &str) -> Option<Volatile> {
    let probe = VolatileState {
        active: true,
        ..VolatileState::NONE
    };
    Volatile::ALL
        .into_iter()
        .find(|v| v.id() == id && v.showdown_state(probe).is_some())
}

/// Whether the canonical fields of `volatile` leave out state the engine keeps (and reads):
/// the kinds whose `Volatile::showdown_state` clears a field, and Stockpile (which Stat raises
/// took is written nowhere).
pub fn volatile_is_lossy(volatile: Volatile) -> bool {
    let probe = VolatileState {
        active: true,
        duration: 1,
        counter: 1,
        time: 1,
        mv: MoveId(1),
        hidden: 1,
    };
    volatile == Volatile::Stockpile
        || volatile
            .showdown_state(probe)
            .is_some_and(|shown| shown != probe)
}

// ---- rebuilding ----------------------------------------------------------------------------------

/// Rebuilds the state a canonical JSON object describes (see the module documentation), with
/// `base` (a state of the same battle: the scenario's loaded state or any of its positions) and
/// `meta` for the set data the canonical form leaves out. An `x-hidden` member, as
/// [`canonical_json_hidden`] writes it, overrides the defaults.
pub fn state_from_canonical<const N: usize>(
    base: &State<N>,
    meta: &ScenarioMeta,
    canonical: &Value,
) -> Result<Rebuilt<N>, ScenarioError> {
    let obj = object(canonical, "canonical state")?;
    let (mut rebuilt, mut needs) = rebuild(base, meta, obj)?;
    if let Some(hidden) = obj.get(HIDDEN_KEY) {
        apply_hidden(&mut rebuilt, meta, hidden, &mut needs)?;
    }
    if let Some(need) = needs.first() {
        return Err(unsupported(format!(
            "{}; give it in \"{HIDDEN_KEY}\" (canonical_json_hidden / state_json(hidden=True) writes it)",
            need.describe(meta)
        )));
    }
    // The rebuilt state must print what it was built from.
    let mut expected = obj.clone();
    expected.remove(HIDDEN_KEY);
    let expected = Value::Object(expected);
    let printed = canonical_value(&rebuilt.state, meta)?;
    if printed != expected {
        let diffs = crate::parity::json_diff(&expected, &printed, 5);
        return Err(invalid(format!(
            "the rebuilt state does not print the given canonical state: {}",
            diffs.join(", ")
        )));
    }
    Ok(rebuilt)
}

/// [`crate::canonical_json`] plus the `x-hidden` member (omitted when every hidden value is
/// its default): what [`state_from_canonical`] needs to give back `state` exactly. `order` is
/// Showdown's party order per side (a `lab_search` node's), written when it differs from the
/// default; `None` leaves it out.
pub fn canonical_json_hidden<const N: usize>(
    state: &State<N>,
    meta: &ScenarioMeta,
    order: Option<&[PartyOrder; 2]>,
) -> Result<String, ScenarioError> {
    let text = canonical_json(state, meta)?;
    let value: Value = serde_json::from_str(&text).expect("canonical_json writes valid JSON");
    let (rebuilt, needs) = rebuild(state, meta, object(&value, "canonical state")?)?;
    let order = order.cloned().unwrap_or_else(|| rebuilt.order.clone());
    let actual = Rebuilt {
        state: state.clone(),
        order,
    };
    let hidden = write_hidden(&actual, &rebuilt, meta, &needs);
    // Everything hidden must be carried: the extension read back gives the state itself.
    let mut check = rebuilt;
    if let Some(hidden) = &hidden {
        let mut needs = Vec::new();
        apply_hidden(&mut check, meta, hidden, &mut needs)?;
    }
    if check != actual {
        return Err(unsupported(format!(
            "hidden state the {HIDDEN_KEY:?} extension does not carry: {}",
            first_difference(&check.state, &actual.state)
        )));
    }
    Ok(match hidden {
        None => text,
        Some(hidden) => {
            let mut out = text;
            out.pop();
            out.push_str(&format!(r#","{HIDDEN_KEY}":{hidden}}}"#));
            out
        }
    })
}

fn first_difference<const N: usize>(a: &State<N>, b: &State<N>) -> String {
    for s in 0..2 {
        let (x, y) = (&a.sides[s], &b.sides[s]);
        for i in 0..PARTY_SIZE {
            if x.party[i] != y.party[i] {
                return format!(
                    "p{} party[{i}]: {:?} vs {:?}",
                    s + 1,
                    x.party[i],
                    y.party[i]
                );
            }
        }
        for i in 0..N {
            if x.slots[i] != y.slots[i] {
                return format!("p{} slot {i}: {:?} vs {:?}", s + 1, x.slots[i], y.slots[i]);
            }
        }
        if x != y {
            return format!("p{} side", s + 1);
        }
    }
    "state".into()
}

/// The canonical fields → state, defaults for the rest; what needs the extension is listed.
fn rebuild<const N: usize>(
    base: &State<N>,
    meta: &ScenarioMeta,
    obj: &Map<String, Value>,
) -> Result<(Rebuilt<N>, Vec<Need>), ScenarioError> {
    let schema = int(get(obj, "schema", "state")?, "schema")?;
    if schema != i64::from(crate::canonical::SCHEMA) {
        return Err(invalid(format!(
            "canonical schema {schema}; this engine reads {}",
            crate::canonical::SCHEMA
        )));
    }
    // The format must be one whose gimmick rules are known (they decide `canMega`).
    format_ruleset(&meta.format)?;
    let mut state = State::<N>::default();
    let mut needs = Vec::new();
    state.turn = int_in(get(obj, "turn", "state")?, "turn")?;
    let ended = boolean(get(obj, "ended", "state")?, "ended")?;
    let winner = text(get(obj, "winner", "state")?, "winner")?;
    state.result = match (ended, winner) {
        (false, "") => lab_engine::state::BattleResult::Ongoing,
        (true, "") => lab_engine::state::BattleResult::Tie,
        (true, "p1") => lab_engine::state::BattleResult::Win(SideId::One),
        (true, "p2") => lab_engine::state::BattleResult::Win(SideId::Two),
        (e, w) => return Err(invalid(format!("ended {e} with winner {w:?}"))),
    };
    read_field(&mut state, object(get(obj, "field", "state")?, "field")?)?;

    let sides = get(obj, "sides", "state")?
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| invalid("sides: expected two sides"))?;
    let mut order: [PartyOrder; 2] = Default::default();
    for (side, value) in [SideId::One, SideId::Two].into_iter().zip(sides) {
        order[side.index()] = read_side(&mut state, base, meta, side, value, &mut needs)?;
    }
    // A request the state's empty slots do not explain is a mid-turn switch (U-turn, Eject
    // Button, Emergency Exit): `switch` for the sides asked, `""` for the others, and which
    // slot must switch out is not canonical.
    let requests: Vec<&str> = sides
        .iter()
        .map(|s| s["request"].as_str().unwrap_or(""))
        .collect();
    let derived = replacement_requests(&state);
    if requests != derived {
        if !requests.contains(&"switch") {
            return Err(invalid(format!(
                "requests {requests:?}; the position gives {derived:?}"
            )));
        }
        for side in [SideId::One, SideId::Two] {
            if requests[side.index()] == "switch" {
                needs.push(Need::MidTurn(side));
            }
        }
    }
    // Showdown's `side.pokemon` order is state only Beat Up reads; the engine records it only
    // while one is in a party (`HistoryReaders::party_order`), and the identity otherwise.
    if beat_up_in_battle(&state) {
        for side in [SideId::One, SideId::Two] {
            let o = &order[side.index()];
            let mut party_order = IDENTITY_ORDER;
            party_order[..o.len()].copy_from_slice(o);
            state.side_mut(side).party_order = party_order;
        }
    }
    Ok((Rebuilt { state, order }, needs))
}

/// `canonical::requests` without mid-turn switches: `switch` for a side with an empty slot and
/// a healthy bench member (the other side waits), `move` otherwise, nothing once it is over.
fn replacement_requests<const N: usize>(state: &State<N>) -> Vec<&'static str> {
    if state.result.is_over() {
        return vec!["", ""];
    }
    let needs: Vec<bool> = [SideId::One, SideId::Two]
        .into_iter()
        .map(|side| crate::side_must_replace(state, side))
        .collect();
    if needs.iter().any(|&n| n) {
        needs
            .iter()
            .map(|&n| if n { "switch" } else { "" })
            .collect()
    } else {
        vec!["move", "move"]
    }
}

fn beat_up_in_battle<const N: usize>(state: &State<N>) -> bool {
    use lab_engine::dex::moves::BEAT_UP;
    state.sides.iter().flat_map(|s| s.party.iter()).any(|mon| {
        let own = mon.transformed.map(|base| base.moves);
        mon.moves
            .iter()
            .chain(own.iter().flatten())
            .any(|m| m.id == BEAT_UP)
    })
}

const PSEUDO: [(&str, FieldEffect); 5] = [
    ("fairylock", FieldEffect::FairyLock),
    ("gravity", FieldEffect::Gravity),
    ("magicroom", FieldEffect::MagicRoom),
    ("trickroom", FieldEffect::TrickRoom),
    ("wonderroom", FieldEffect::WonderRoom),
];

fn duration_of(value: &Value, what: &str) -> Result<u8, ScenarioError> {
    match value {
        Value::Null => Err(unsupported(format!(
            "{what} without a duration (primal weather, a permanent effect)"
        ))),
        v => {
            let turns: u8 = int_in(v, what)?;
            if turns == 0 || turns == Effect::PERMANENT {
                return Err(invalid(format!("{what}: duration {turns}")));
            }
            Ok(turns)
        }
    }
}

fn read_field<const N: usize>(
    state: &mut State<N>,
    field: &Map<String, Value>,
) -> Result<(), ScenarioError> {
    let weather = text(get(field, "weather", "field")?, "weather")?;
    if !weather.is_empty() {
        let value = match weather {
            "sunnyday" => Weather::Sun,
            "raindance" => Weather::Rain,
            "sandstorm" => Weather::Sand,
            "snowscape" => Weather::Snow,
            other => return Err(unsupported(format!("weather {other:?}"))),
        };
        let turns = duration_of(get(field, "weatherDuration", "field")?, "weather")?;
        state.field[FieldEffect::Weather as usize] = Effect {
            value: value as u8,
            turns,
        };
    }
    let terrain = text(get(field, "terrain", "field")?, "terrain")?;
    if !terrain.is_empty() {
        let value = match terrain {
            "electricterrain" => Terrain::Electric,
            "grassyterrain" => Terrain::Grassy,
            "mistyterrain" => Terrain::Misty,
            "psychicterrain" => Terrain::Psychic,
            other => return Err(unsupported(format!("terrain {other:?}"))),
        };
        let turns = duration_of(get(field, "terrainDuration", "field")?, "terrain")?;
        state.field[FieldEffect::Terrain as usize] = Effect {
            value: value as u8,
            turns,
        };
    }
    for (id, effect) in object(get(field, "pseudoWeather", "field")?, "pseudoWeather")? {
        let Some(&(_, kind)) = PSEUDO.iter().find(|(i, _)| i == id) else {
            return Err(unsupported(format!("pseudo-weather {id:?}")));
        };
        let effect = object(effect, id)?;
        let turns = duration_of(get(effect, "duration", id)?, id)?;
        state.field[kind as usize] = Effect { value: 0, turns };
    }
    Ok(())
}

const TIMED_SIDE: [(&str, SideEffect); 11] = [
    ("auroraveil", SideEffect::AuroraVeil),
    ("craftyshield", SideEffect::CraftyShield),
    ("lightscreen", SideEffect::LightScreen),
    ("luckychant", SideEffect::LuckyChant),
    ("matblock", SideEffect::MatBlock),
    ("mist", SideEffect::Mist),
    ("quickguard", SideEffect::QuickGuard),
    ("reflect", SideEffect::Reflect),
    ("safeguard", SideEffect::Safeguard),
    ("tailwind", SideEffect::Tailwind),
    ("wideguard", SideEffect::WideGuard),
];

const HAZARD_IDS: [(&str, SideEffect); 4] = [
    ("spikes", SideEffect::Spikes),
    ("stealthrock", SideEffect::StealthRock),
    ("stickyweb", SideEffect::StickyWeb),
    ("toxicspikes", SideEffect::ToxicSpikes),
];

fn hazard_id(effect: SideEffect) -> &'static str {
    HAZARD_IDS
        .iter()
        .find(|(_, e)| *e == effect)
        .map(|(id, _)| *id)
        .expect("a hazard")
}

/// Reads one side; returns its default party order.
fn read_side<const N: usize>(
    state: &mut State<N>,
    base: &State<N>,
    meta: &ScenarioMeta,
    side: SideId,
    value: &Value,
    needs: &mut Vec<Need>,
) -> Result<PartyOrder, ScenarioError> {
    let what = side_name(side);
    let obj = object(value, what)?;
    let side_meta = &meta.sides[side.index()];
    let members = side_meta.members.len();
    if members == 0 || members > PARTY_SIZE {
        return Err(invalid(format!("{what}: the meta has {members} members")));
    }
    // Checked against the state in `rebuild` (a mid-turn switch).
    text(get(obj, "request", what)?, "request")?;

    // Side conditions.
    let conditions = object(get(obj, "conditions", what)?, "conditions")?;
    let mut hazards = Vec::new();
    for (id, effect) in conditions {
        let effect = object(effect, id)?;
        if let Some(&(_, kind)) = TIMED_SIDE.iter().find(|(i, _)| i == id) {
            let turns = duration_of(get(effect, "duration", id)?, id)?;
            state.side_mut(side).effects[kind as usize] = Effect { value: 0, turns };
        } else if let Some(&(_, kind)) = HAZARD_IDS.iter().find(|(i, _)| i == id) {
            let layered = matches!(kind, SideEffect::Spikes | SideEffect::ToxicSpikes);
            let value = if layered {
                int_in(get(effect, "layers", id)?, id)?
            } else {
                0
            };
            state.side_mut(side).effects[kind as usize] = Effect {
                value,
                turns: Effect::PERMANENT,
            };
            hazards.push(kind);
        } else {
            return Err(unsupported(format!("{what}: side condition {id:?}")));
        }
    }
    // Default hazard order: id order (the canonical object's order, as the oracle's patch).
    let mut hazard_order = HazardOrder::default();
    {
        let mut active = [Effect::NONE; lab_engine::field::SIDE_EFFECT_COUNT];
        for &kind in &hazards {
            hazard_order = hazard_order.changed(&active, kind, true);
            active[kind as usize] = state.side(side).effects[kind as usize];
        }
    }
    state.side_mut(side).history.hazard_order = hazard_order;

    // Slot conditions.
    let slot_conditions = get(obj, "slotConditions", what)?
        .as_array()
        .ok_or_else(|| invalid(format!("{what}: slotConditions is not a list")))?;
    if slot_conditions.len() != N {
        return Err(invalid(format!(
            "{what}: {} slot condition entries for {N} slots",
            slot_conditions.len()
        )));
    }
    for (slot, conditions) in slot_conditions.iter().enumerate() {
        for (id, effect) in object(conditions, "slotConditions")? {
            let effect = object(effect, id)?;
            let at = SlotRef {
                side,
                slot: slot as u8,
            };
            let (kind, value) = match id.as_str() {
                "wish" => {
                    let hp = get(effect, "hp", id)?
                        .as_f64()
                        .ok_or_else(|| invalid("wish hp: expected a number"))?;
                    let doubled = hp * 2.0;
                    if doubled.fract() != 0.0 || !(1.0..=65535.0).contains(&doubled) {
                        return Err(invalid(format!("wish hp {hp}")));
                    }
                    let turn = state.turn.saturating_sub(1);
                    (
                        SlotCondition::Wish,
                        SlotEffect {
                            value: doubled as u16,
                            turn,
                        },
                    )
                }
                "healingwish" => (SlotCondition::HealingWish, SlotEffect { value: 1, turn: 0 }),
                "revivalblessing" => (
                    SlotCondition::RevivalBlessing,
                    SlotEffect {
                        value: 1,
                        turn: int_in(get(effect, "duration", id)?, id)?,
                    },
                ),
                "futuremove" => {
                    let mv = text(get(effect, "move", id)?, id)?;
                    let value = match mv {
                        "futuresight" => 0,
                        "doomdesire" => FUTURE_MOVE_DOOM_DESIRE,
                        other => return Err(invalid(format!("future move {other:?}"))),
                    };
                    needs.push(Need::FutureMove(at));
                    // A placeholder the extension replaces (`value` must be non-zero).
                    (
                        SlotCondition::FutureMove,
                        SlotEffect {
                            value: value | 1,
                            turn: 0,
                        },
                    )
                }
                other => return Err(unsupported(format!("{what}: slot condition {other:?}"))),
            };
            state.side_mut(side).slot_conditions[slot][kind as usize] = value;
        }
    }

    // Pokémon.
    let list = get(obj, "pokemon", what)?
        .as_array()
        .ok_or_else(|| invalid(format!("{what}: pokemon is not a list")))?;
    if list.len() != members {
        return Err(invalid(format!(
            "{what}: {} Pokémon for {members} members",
            list.len()
        )));
    }
    let mut seen = [false; PARTY_SIZE];
    let mut slot_of: [Option<u8>; PARTY_SIZE] = [None; PARTY_SIZE];
    let mut can_mega: [bool; PARTY_SIZE] = [false; PARTY_SIZE];
    let mut entries: [Option<&Map<String, Value>>; PARTY_SIZE] = [None; PARTY_SIZE];
    for entry in list {
        let entry = object(entry, "pokemon")?;
        let name = text(get(entry, "name", "pokemon")?, "name")?;
        let index = side_meta
            .party_index(name)
            .ok_or_else(|| invalid(format!("{what} has no Pokémon named {name:?}")))?
            as usize;
        if seen[index] {
            return Err(invalid(format!("{what}: {name:?} twice")));
        }
        seen[index] = true;
        entries[index] = Some(entry);
        let slot = get(entry, "slot", name)?;
        if !slot.is_null() {
            let slot: u8 = int_in(slot, name)?;
            if usize::from(slot) >= N {
                return Err(invalid(format!("{what} {name:?}: slot {slot} of {N}")));
            }
            if slot_of.contains(&Some(slot)) {
                return Err(invalid(format!("{what}: two Pokémon in slot {slot}")));
            }
            slot_of[index] = Some(slot);
            state.side_mut(side).slots[slot as usize].party_index = Some(index as u8);
        }
        if let Some(flag) = entry.get("canMega") {
            can_mega[index] = boolean(flag, "canMega")?;
        }
    }

    for index in 0..members {
        let entry = entries[index].expect("every member seen");
        let name = &side_meta.members[index].name;
        let at = PokemonRef {
            side,
            party: index as u8,
        };
        let slot = slot_of[index].map(|slot| SlotRef { side, slot });
        let mon = read_pokemon(
            &base.side(side).party[index],
            meta,
            at,
            name,
            entry,
            slot.is_some(),
            needs,
        )?;
        state.side_mut(side).party[index] = mon;
        if let Some(slot) = slot {
            read_slot(state, slot, name, entry, needs)?;
        }
    }

    // canMega: every eligible member shows it until the side Mega Evolves.
    let eligible: Vec<usize> = (0..members)
        .filter(|&i| state.side(side).party[i].gimmicks.contains(Gimmick::Mega))
        .collect();
    if let Some(i) = (0..members).find(|&i| can_mega[i] && !eligible.contains(&i)) {
        return Err(invalid(format!(
            "{what} {:?}: canMega on a Pokémon that cannot Mega Evolve",
            side_meta.members[i].name
        )));
    }
    let open = eligible.iter().any(|&i| can_mega[i]);
    if open && eligible.iter().any(|&i| !can_mega[i]) {
        return Err(invalid(format!(
            "{what}: canMega on some eligible Pokémon but not on others"
        )));
    }
    let mega_used = ruleset_allows_mega(meta) && !eligible.is_empty() && !open;
    state.side_mut(side).gimmicks_used = if mega_used {
        GimmickSet::MEGA
    } else {
        GimmickSet::EMPTY
    };

    // Fainted Pokémon that still hold an empty slot: the `fnt` ones (a replacement clears the
    // status), in team preview order, fill the empty slots in slot order.
    let fainted: Vec<usize> = (0..members)
        .filter(|&i| slot_of[i].is_none() && state.side(side).party[i].status == Status::Fainted)
        .collect();
    let mut fainted = fainted.into_iter();
    for slot in 0..N {
        if state.side(side).slots[slot].party_index.is_none() {
            state.side_mut(side).slots[slot].fainted_occupant = fainted.next().map(|i| i as u8);
        }
    }
    if let Some(i) = fainted.next() {
        return Err(invalid(format!(
            "{what} {:?}: fainted (fnt) without an empty slot to hold",
            side_meta.members[i].name
        )));
    }
    state.side_mut(side).history.total_fainted = (0..members)
        .filter(|&i| state.side(side).party[i].hp == 0)
        .count() as u8;

    // Showdown's party order: slot holders first, then the rest in team preview order.
    let mut order: PartyOrder = Vec::with_capacity(members);
    for slot in &state.side(side).slots {
        if let Some(p) = slot.party_index.or(slot.fainted_occupant) {
            order.push(p);
        }
    }
    for i in 0..members as u8 {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    Ok(order)
}

fn ruleset_allows_mega(meta: &ScenarioMeta) -> bool {
    format_ruleset(&meta.format).is_ok_and(|r| r.allows(Gimmick::Mega))
}

fn read_pokemon(
    base: &Pokemon,
    meta: &ScenarioMeta,
    at: PokemonRef,
    name: &str,
    entry: &Map<String, Value>,
    active: bool,
    needs: &mut Vec<Need>,
) -> Result<Pokemon, ScenarioError> {
    let member = &meta.sides[at.side.index()].members[at.party as usize];
    let species_name = text(get(entry, "species", name)?, "species")?;
    let species = SpeciesId::from_name(species_name)
        .ok_or_else(|| invalid(format!("{name:?}: unknown species {species_name:?}")))?;
    if species.data().name != species_name {
        return Err(invalid(format!(
            "{name:?}: species {species_name:?} is not a species name"
        )));
    }
    // The set as loaded: nature, Stat Points, level, gender and Mega eligibility from `base`;
    // its own move slots even if `base` is transformed.
    let mut set = base.clone();
    if let Some(own) = base.transformed {
        set.moves = own.moves;
    }
    set.species = member.species;
    set.base_ability = member.ability;

    let mut mon = Pokemon {
        transformed: None,
        illusion: false,
        autotomized: 0,
        last_item: ItemId::NONE,
        status: Status::None,
        status_turns: 0,
        ..set.clone()
    };
    mon.species = species;
    let forme = set.forme_as(species);
    mon.stats = forme.stats;
    mon.base_ability = if species == member.species
        || lab_engine::turn::temporary_forme_base(species).is_some()
        || member.ability == abilities::DISGUISE
        || member.ability == abilities::ICE_FACE
    {
        member.ability
    } else {
        forme.base_ability
    };
    mon.types = species.data().types;
    if active {
        if let Some(types) = entry.get("types") {
            mon.types = parse_types(text(types, "types")?, name)?;
        }
    }
    mon.max_hp = int_in(get(entry, "maxhp", name)?, "maxhp")?;
    mon.hp = int_in(get(entry, "hp", name)?, "hp")?;
    if mon.hp < 0 || mon.hp > mon.max_hp {
        return Err(invalid(format!("{name:?}: hp {}/{}", mon.hp, mon.max_hp)));
    }
    mon.status = status_from_id(text(get(entry, "status", name)?, "status")?, name)?;
    mon.status_turns = match mon.status {
        Status::Sleep => int_in(get(entry, "statusTime", name)?, "statusTime")?,
        Status::Toxic => int_in(get(entry, "statusStage", name)?, "statusStage")?,
        Status::Freeze => 3,
        _ => 0,
    };
    mon.item = item_id(text(get(entry, "item", name)?, "item")?, name)?;
    if let Some(last) = entry.get("lastItem") {
        mon.last_item = item_id(text(last, "lastItem")?, name)?;
    }
    mon.ability = ability_id(text(get(entry, "ability", name)?, "ability")?, name)?;

    // Move slots: the set's moves with the given PP; anything else is a Transform copy.
    let pp = object(get(entry, "pp", name)?, "pp")?;
    let own: Vec<MoveId> = set
        .moves
        .iter()
        .map(|m| m.id)
        .filter(|id| !id.is_none())
        .collect();
    let same = pp.len() == own.len() && own.iter().all(|id| pp.contains_key(id.id()));
    if same {
        for slot in mon.moves.iter_mut().filter(|m| !m.id.is_none()) {
            slot.pp = int_in(&pp[slot.id.id()], "pp")?;
            slot.disabled = false;
        }
    } else {
        // Showdown's `moveSlots` of a transformed Pokémon; the extension gives the order and
        // what it returns to.
        needs.push(Need::Transformed(at));
        let mut slots = [MoveSlot::default(); 4];
        if pp.len() > 4 {
            return Err(invalid(format!("{name:?}: {} move slots", pp.len())));
        }
        for (i, (id, value)) in pp.iter().enumerate() {
            slots[i] = MoveSlot {
                id: move_id(id, name)?,
                pp: int_in(value, "pp")?,
                disabled: false,
            };
        }
        mon.moves = slots;
    }
    Ok(mon)
}

fn read_slot<const N: usize>(
    state: &mut State<N>,
    at: SlotRef,
    name: &str,
    entry: &Map<String, Value>,
    needs: &mut Vec<Need>,
) -> Result<(), ScenarioError> {
    let mut slot = Slot {
        party_index: state.slot(at).party_index,
        ..Slot::default()
    };
    slot.history.newly_switched = state.turn == 1;
    const BOOSTS: [&str; 7] = ["atk", "def", "spa", "spd", "spe", "accuracy", "evasion"];
    for (stat, stage) in object(get(entry, "boosts", name)?, "boosts")? {
        let i = BOOSTS
            .iter()
            .position(|b| b == stat)
            .ok_or_else(|| invalid(format!("{name:?}: boost {stat:?}")))?;
        slot.boosts[i] = int_in(stage, "boost")?;
    }
    if let Some(last) = entry.get("lastMove") {
        slot.last_move = move_id(text(last, "lastMove")?, name)?;
    }
    slot.move_actions = u8::from(!slot.last_move.is_none());
    for (id, fields) in object(get(entry, "volatiles", name)?, "volatiles")? {
        let volatile =
            volatile_by_id(id).ok_or_else(|| unsupported(format!("{name:?}: volatile {id:?}")))?;
        let fields = object(fields, id)?;
        let mut v = VolatileState {
            active: true,
            ..VolatileState::NONE
        };
        let what = format!("{name:?} {id}");
        match volatile {
            Volatile::Substitute => {
                slot.substitute_hp = int_in(get(fields, "hp", &what)?, &what)?;
            }
            Volatile::ChoiceLock => {
                v.counter = move_id(text(get(fields, "move", &what)?, &what)?, &what)?.0;
            }
            Volatile::Stockpile => {
                v.counter = int_in(get(fields, "layers", &what)?, &what)?;
            }
            _ => {
                for (key, value) in fields {
                    match key.as_str() {
                        "duration" => v.duration = int_in(value, &what)?,
                        "counter" => v.counter = int_in(value, &what)?,
                        "time" => v.time = int_in(value, &what)?,
                        "move" => v.mv = move_id(text(value, &what)?, &what)?,
                        "trueDuration" if volatile == Volatile::LockedMove => {
                            v.hidden = int_in(value, &what)?
                        }
                        other => {
                            return Err(invalid(format!("{what}: field {other:?}")));
                        }
                    }
                }
            }
        }
        if volatile_is_lossy(volatile) {
            needs.push(Need::Volatile(at, volatile));
        }
        slot.volatiles.set(volatile, v);
    }
    // Power Trick and Power Shift swapped the stored Attack and Defense.
    if slot.volatiles.has(Volatile::PowerTrick) != slot.volatiles.has(Volatile::PowerShift) {
        let index = slot.party_index.expect("an active slot") as usize;
        state.side_mut(at.side).party[index].stats.swap(0, 1);
    }
    *state.slot_mut(at) = slot;
    Ok(())
}

// ---- the extension -------------------------------------------------------------------------------

fn names_of(meta: &ScenarioMeta, side: SideId, bits: u8) -> Value {
    let members = &meta.sides[side.index()].members;
    Value::Array(
        (0..members.len())
            .filter(|&i| bits & (1 << i) != 0)
            .map(|i| Value::String(members[i].name.clone()))
            .collect(),
    )
}

fn bits_of(
    meta: &ScenarioMeta,
    side: SideId,
    value: &Value,
    what: &str,
) -> Result<u8, ScenarioError> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid(format!("{what}: expected a list of names")))?;
    let mut bits = 0u8;
    for name in list {
        bits |= 1 << party_of(meta, side, name, what)?;
    }
    Ok(bits)
}

fn party_of(
    meta: &ScenarioMeta,
    side: SideId,
    name: &Value,
    what: &str,
) -> Result<u8, ScenarioError> {
    let name = text(name, what)?;
    meta.sides[side.index()]
        .party_index(name)
        .ok_or_else(|| invalid(format!("{what}: {} has no {name:?}", side_name(side))))
}

fn side_of(value: &Value, what: &str) -> Result<SideId, ScenarioError> {
    match text(value, what)? {
        "p1" => Ok(SideId::One),
        "p2" => Ok(SideId::Two),
        other => Err(invalid(format!("{what}: side {other:?}"))),
    }
}

fn move_result_name(r: MoveResult) -> &'static str {
    match r {
        MoveResult::Undefined => "undefined",
        MoveResult::Null => "null",
        MoveResult::Failed => "false",
        MoveResult::Succeeded => "true",
    }
}

fn move_result(value: &Value, what: &str) -> Result<MoveResult, ScenarioError> {
    Ok(match text(value, what)? {
        "undefined" => MoveResult::Undefined,
        "null" => MoveResult::Null,
        "false" => MoveResult::Failed,
        "true" => MoveResult::Succeeded,
        other => return Err(invalid(format!("{what}: {other:?}"))),
    })
}

fn switch_flag_name(f: SwitchFlag) -> &'static str {
    match f {
        SwitchFlag::None => "none",
        SwitchFlag::Move => "move",
        SwitchFlag::Effect => "effect",
        SwitchFlag::CopyVolatile => "copyvolatile",
        SwitchFlag::ShedTail => "shedtail",
    }
}

fn switch_flag(value: &Value, what: &str) -> Result<SwitchFlag, ScenarioError> {
    Ok(match text(value, what)? {
        "none" => SwitchFlag::None,
        "move" => SwitchFlag::Move,
        "effect" => SwitchFlag::Effect,
        "copyvolatile" => SwitchFlag::CopyVolatile,
        "shedtail" => SwitchFlag::ShedTail,
        other => return Err(invalid(format!("{what}: switch flag {other:?}"))),
    })
}

fn gender_name(g: Gender) -> &'static str {
    match g {
        Gender::Random => "",
        Gender::Male => "M",
        Gender::Female => "F",
        Gender::Genderless => "N",
    }
}

fn gender(value: &Value, what: &str) -> Result<Gender, ScenarioError> {
    Ok(match text(value, what)? {
        "" => Gender::Random,
        "M" => Gender::Male,
        "F" => Gender::Female,
        "N" => Gender::Genderless,
        other => return Err(invalid(format!("{what}: gender {other:?}"))),
    })
}

fn gimmick_bits(set: GimmickSet) -> Value {
    json!(set.bits())
}

fn gimmicks(value: &Value, what: &str) -> Result<GimmickSet, ScenarioError> {
    let bits: u8 = int_in(value, what)?;
    let mut set = GimmickSet::EMPTY;
    for (i, g) in Gimmick::ACTIVATIONS.into_iter().enumerate() {
        if bits & (1 << i) != 0 {
            set = set.with(g);
        }
    }
    if set.bits() != bits {
        return Err(invalid(format!("{what}: gimmick bits {bits}")));
    }
    Ok(set)
}

fn moves_json(moves: &[MoveSlot; 4]) -> Value {
    Value::Array(
        moves
            .iter()
            .map(|m| json!([m.id.id(), m.pp, m.disabled]))
            .collect(),
    )
}

fn moves_from(value: &Value, what: &str) -> Result<[MoveSlot; 4], ScenarioError> {
    let list = value.as_array().filter(|l| l.len() == 4).ok_or_else(|| {
        invalid(format!(
            "{what}: expected four [id, pp, disabled] move slots"
        ))
    })?;
    let mut out = [MoveSlot::default(); 4];
    for (i, slot) in list.iter().enumerate() {
        let parts = slot
            .as_array()
            .filter(|p| p.len() == 3)
            .ok_or_else(|| invalid(format!("{what}: move slot {i}")))?;
        out[i] = MoveSlot {
            id: move_id(text(&parts[0], what)?, what)?,
            pp: int_in(&parts[1], what)?,
            disabled: boolean(&parts[2], what)?,
        };
    }
    Ok(out)
}

fn volatile_json(v: VolatileState) -> Value {
    json!([v.active, v.duration, v.counter, v.time, v.mv.id(), v.hidden])
}

fn volatile_from(value: &Value, what: &str) -> Result<VolatileState, ScenarioError> {
    let parts = value.as_array().filter(|p| p.len() == 6).ok_or_else(|| {
        invalid(format!(
            "{what}: expected [active, duration, counter, time, move, hidden]"
        ))
    })?;
    Ok(VolatileState {
        active: boolean(&parts[0], what)?,
        duration: int_in(&parts[1], what)?,
        counter: int_in(&parts[2], what)?,
        time: int_in(&parts[3], what)?,
        mv: move_id(text(&parts[4], what)?, what)?,
        hidden: int_in(&parts[5], what)?,
    })
}

/// A slot's history as `(key, value)` pairs, every field.
fn history_fields(h: &SlotHistory) -> Vec<(&'static str, Value)> {
    let damaged_by = match h.last_damaged_by {
        None => Value::Null,
        Some(d) => json!({
            "source": [side_name(d.source.side), d.source.party],
            "slot": [side_name(d.slot.side), d.slot.slot],
            "damage": d.damage,
        }),
    };
    vec![
        ("hurtThisTurn", json!(h.hurt_this_turn)),
        ("lastDamagedBy", damaged_by),
        ("damagedByThisTurn", json!(h.damaged_by_this_turn)),
        ("timesAttacked", json!(h.times_attacked)),
        (
            "moveThisTurnResult",
            json!(move_result_name(h.move_this_turn_result)),
        ),
        (
            "moveLastTurnResult",
            json!(move_result_name(h.move_last_turn_result)),
        ),
        ("newlySwitched", json!(h.newly_switched)),
        ("statsRaisedThisTurn", json!(h.stats_raised_this_turn)),
        ("statsLoweredThisTurn", json!(h.stats_lowered_this_turn)),
        ("usedItemThisTurn", json!(h.used_item_this_turn)),
        ("movesUsed", json!(h.moves_used)),
    ]
}

fn set_history_field(h: &mut SlotHistory, key: &str, value: &Value) -> Result<(), ScenarioError> {
    let what = format!("history.{key}");
    let pair = |v: &Value| -> Result<(SideId, u8), ScenarioError> {
        let parts = v
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or_else(|| invalid(format!("{what}: expected [side, index]")))?;
        Ok((side_of(&parts[0], &what)?, int_in(&parts[1], &what)?))
    };
    match key {
        "hurtThisTurn" => {
            h.hurt_this_turn = match value {
                Value::Null => None,
                v => Some(int_in(v, &what)?),
            }
        }
        "lastDamagedBy" => {
            h.last_damaged_by = match value {
                Value::Null => None,
                v => {
                    let o = object(v, &what)?;
                    let (side, party) = pair(get(o, "source", &what)?)?;
                    let (slot_side, slot) = pair(get(o, "slot", &what)?)?;
                    Some(DamagedBy {
                        source: PokemonRef { side, party },
                        slot: SlotRef {
                            side: slot_side,
                            slot,
                        },
                        damage: int_in(get(o, "damage", &what)?, &what)?,
                    })
                }
            }
        }
        "damagedByThisTurn" => h.damaged_by_this_turn = int_in(value, &what)?,
        "timesAttacked" => h.times_attacked = int_in(value, &what)?,
        "moveThisTurnResult" => h.move_this_turn_result = move_result(value, &what)?,
        "moveLastTurnResult" => h.move_last_turn_result = move_result(value, &what)?,
        "newlySwitched" => h.newly_switched = boolean(value, &what)?,
        "statsRaisedThisTurn" => h.stats_raised_this_turn = boolean(value, &what)?,
        "statsLoweredThisTurn" => h.stats_lowered_this_turn = boolean(value, &what)?,
        "usedItemThisTurn" => h.used_item_this_turn = boolean(value, &what)?,
        "movesUsed" => h.moves_used = int_in(value, &what)?,
        other => {
            return Err(invalid(format!(
                "x-hidden: unknown history field {other:?}"
            )))
        }
    }
    Ok(())
}

/// A Pokémon's hidden fields as `(key, value)` pairs, every field.
fn pokemon_fields(mon: &Pokemon) -> Vec<(&'static str, Value)> {
    let transformed = match mon.transformed {
        None => Value::Null,
        Some(t) => json!({"species": t.species.data().name, "moves": moves_json(&t.moves)}),
    };
    vec![
        ("gender", json!(gender_name(mon.gender))),
        ("statusTurns", json!(mon.status_turns)),
        ("autotomized", json!(mon.autotomized)),
        ("illusion", json!(mon.illusion)),
        ("baseAbility", json!(mon.base_ability.id())),
        ("stats", json!(mon.stats)),
        ("types", json!([mon.types[0].name(), mon.types[1].name()])),
        ("moves", moves_json(&mon.moves)),
        ("transformed", transformed),
        ("gimmicks", gimmick_bits(mon.gimmicks)),
        ("gigantamax", json!(mon.gigantamax_factor)),
        ("level", json!(mon.level)),
        ("nature", json!(mon.nature.name())),
        ("statPoints", json!(mon.stat_points)),
    ]
}

fn set_pokemon_field(
    mon: &mut Pokemon,
    at: PokemonRef,
    key: &str,
    value: &Value,
    needs: &mut Vec<Need>,
) -> Result<(), ScenarioError> {
    let what = format!("pokemon.{key}");
    match key {
        "gender" => mon.gender = gender(value, &what)?,
        "statusTurns" => mon.status_turns = int_in(value, &what)?,
        "autotomized" => mon.autotomized = int_in(value, &what)?,
        "illusion" => mon.illusion = boolean(value, &what)?,
        "baseAbility" => mon.base_ability = ability_id(text(value, &what)?, &what)?,
        "stats" => {
            let list = value
                .as_array()
                .filter(|l| l.len() == 5)
                .ok_or_else(|| invalid(format!("{what}: five stats")))?;
            for (i, v) in list.iter().enumerate() {
                mon.stats[i] = int_in(v, &what)?;
            }
        }
        "types" => {
            let list = value
                .as_array()
                .filter(|l| l.len() == 2)
                .ok_or_else(|| invalid(format!("{what}: two type names")))?;
            for (i, v) in list.iter().enumerate() {
                let name = text(v, &what)?;
                mon.types[i] = if name.is_empty() {
                    Type::None
                } else {
                    type_by_name(name).ok_or_else(|| invalid(format!("{what}: {name:?}")))?
                };
            }
        }
        "moves" => mon.moves = moves_from(value, &what)?,
        "transformed" => {
            mon.transformed = match value {
                Value::Null => None,
                v => {
                    let o = object(v, &what)?;
                    let species = text(get(o, "species", &what)?, &what)?;
                    Some(TransformBase {
                        species: SpeciesId::from_name(species)
                            .ok_or_else(|| invalid(format!("{what}: species {species:?}")))?,
                        moves: moves_from(get(o, "moves", &what)?, &what)?,
                    })
                }
            };
            needs.retain(|n| *n != Need::Transformed(at));
        }
        "gimmicks" => mon.gimmicks = gimmicks(value, &what)?,
        "gigantamax" => mon.gigantamax_factor = boolean(value, &what)?,
        "level" => mon.level = int_in(value, &what)?,
        "nature" => {
            let name = text(value, &what)?;
            mon.nature = lab_engine::dex::Nature::ALL
                .into_iter()
                .find(|n| n.name() == name)
                .ok_or_else(|| invalid(format!("{what}: {name:?}")))?;
        }
        "statPoints" => {
            let list = value
                .as_array()
                .filter(|l| l.len() == mon.stat_points.len())
                .ok_or_else(|| invalid(format!("{what}: six Stat Points")))?;
            for (i, v) in list.iter().enumerate() {
                mon.stat_points[i] = int_in(v, &what)?;
            }
        }
        other => {
            return Err(invalid(format!(
                "x-hidden: unknown Pokémon field {other:?}"
            )))
        }
    }
    Ok(())
}

/// The fields of `actual` that differ from `default`, as an object (`None` when none do).
fn diff(actual: Vec<(&'static str, Value)>, default: Vec<(&'static str, Value)>) -> Option<Value> {
    let mut out = Map::new();
    for ((key, a), (_, d)) in actual.into_iter().zip(default) {
        if a != d {
            out.insert(key.to_owned(), a);
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn order_names(meta: &ScenarioMeta, side: SideId, order: &[u8]) -> Value {
    let side_meta = &meta.sides[side.index()];
    Value::Array(
        order
            .iter()
            .map(|&p| json!(side_meta.name(p).unwrap_or("")))
            .collect(),
    )
}

fn order_from(
    meta: &ScenarioMeta,
    side: SideId,
    value: &Value,
    what: &str,
) -> Result<PartyOrder, ScenarioError> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid(format!("{what}: expected a list of names")))?;
    let members = meta.sides[side.index()].members.len();
    let mut out = Vec::with_capacity(list.len());
    for name in list {
        let p = party_of(meta, side, name, what)?;
        if out.contains(&p) {
            return Err(invalid(format!("{what}: a name twice")));
        }
        out.push(p);
    }
    if out.len() != members {
        return Err(invalid(format!(
            "{what}: {} names for {members} members",
            out.len()
        )));
    }
    Ok(out)
}

/// The hidden values of `actual` that differ from `rebuilt`'s defaults.
/// What `needs` lists (no default at all) is written even when it equals the placeholder.
fn write_hidden<const N: usize>(
    actual: &Rebuilt<N>,
    rebuilt: &Rebuilt<N>,
    meta: &ScenarioMeta,
    needs: &[Need],
) -> Option<Value> {
    let (a, r) = (&actual.state, &rebuilt.state);
    let mut top = Map::new();
    if a.last_move != r.last_move {
        top.insert("lastMove".into(), json!(a.last_move.id()));
    }
    let mut sides = Vec::new();
    let mut any_side = false;
    for side in [SideId::One, SideId::Two] {
        let (sa, sr) = (a.side(side), r.side(side));
        let members = meta.sides[side.index()].members.len();
        let mut out = Map::new();
        if actual.order[side.index()] != rebuilt.order[side.index()] {
            out.insert(
                "switchOrder".into(),
                order_names(meta, side, &actual.order[side.index()]),
            );
        }
        if sa.party_order != sr.party_order {
            out.insert(
                "partyOrder".into(),
                order_names(meta, side, &sa.party_order[..members]),
            );
        }
        let (ha, hr) = (&sa.history, &sr.history);
        if ha.hazard_order != hr.hazard_order {
            let order: Vec<&str> = ha
                .hazard_order
                .sorted(&sa.effects)
                .into_iter()
                .map(hazard_id)
                .collect();
            out.insert("hazardOrder".into(), json!(order));
        }
        if ha.total_fainted != hr.total_fainted {
            out.insert("totalFainted".into(), json!(ha.total_fainted));
        }
        if ha.fainted_this_turn != hr.fainted_this_turn {
            out.insert("faintedThisTurn".into(), json!(ha.fainted_this_turn));
        }
        if ha.fainted_last_turn != hr.fainted_last_turn {
            out.insert("faintedLastTurn".into(), json!(ha.fainted_last_turn));
        }
        for (key, x, y) in [
            ("ateBerry", ha.ate_berry, hr.ate_berry),
            ("swordBoost", ha.sword_boost, hr.sword_boost),
            ("shieldBoost", ha.shield_boost, hr.shield_boost),
            ("syrupTriggered", ha.syrup_triggered, hr.syrup_triggered),
        ] {
            if x != y {
                out.insert(key.into(), names_of(meta, side, x));
            }
        }
        if sa.gimmicks_used != sr.gimmicks_used {
            out.insert("gimmicksUsed".into(), gimmick_bits(sa.gimmicks_used));
        }
        let future_needed = needs
            .iter()
            .any(|n| matches!(n, Need::FutureMove(at) if at.side == side));
        if sa.slot_conditions != sr.slot_conditions || future_needed {
            let list: Vec<Value> = sa
                .slot_conditions
                .iter()
                .map(|conditions| {
                    let mut m = Map::new();
                    for c in SlotCondition::ALL {
                        let e = conditions[c as usize];
                        if e != SlotEffect::NONE {
                            m.insert(c.id().into(), json!([e.value, e.turn]));
                        }
                    }
                    Value::Object(m)
                })
                .collect();
            out.insert("slotConditions".into(), Value::Array(list));
        }
        let mut slots = Vec::new();
        let mut any_slot = false;
        for i in 0..N {
            let (xa, xr) = (&sa.slots[i], &sr.slots[i]);
            let mut s = Map::new();
            if xa.fainted_occupant != xr.fainted_occupant {
                let name = xa
                    .fainted_occupant
                    .and_then(|p| meta.sides[side.index()].name(p));
                s.insert("faintedOccupant".into(), json!(name));
            }
            if xa.ability_order != xr.ability_order {
                s.insert("abilityOrder".into(), json!(xa.ability_order));
            }
            if xa.last_move_target_loc != xr.last_move_target_loc {
                s.insert("lastMoveTargetLoc".into(), json!(xa.last_move_target_loc));
            }
            if xa.move_actions != xr.move_actions {
                s.insert("moveActions".into(), json!(xa.move_actions));
            }
            if xa.switch_flag != xr.switch_flag || needs.contains(&Need::MidTurn(side)) {
                s.insert("switchFlag".into(), json!(switch_flag_name(xa.switch_flag)));
            }
            if let Some(h) = diff(history_fields(&xa.history), history_fields(&xr.history)) {
                s.insert("history".into(), h);
            }
            let mut volatiles = Map::new();
            for v in Volatile::ALL {
                let (va, vr) = (xa.volatiles.get(v), xr.volatiles.get(v));
                let at = SlotRef {
                    side,
                    slot: i as u8,
                };
                if va != vr || needs.contains(&Need::Volatile(at, v)) {
                    volatiles.insert(v.id().into(), volatile_json(va));
                }
            }
            if !volatiles.is_empty() {
                s.insert("volatiles".into(), Value::Object(volatiles));
            }
            any_slot |= !s.is_empty();
            slots.push(Value::Object(s));
        }
        if any_slot {
            out.insert("slots".into(), Value::Array(slots));
        }
        let mut pokemon = Map::new();
        for i in 0..members {
            let at = PokemonRef {
                side,
                party: i as u8,
            };
            let mut d = diff(pokemon_fields(&sa.party[i]), pokemon_fields(&sr.party[i]));
            if needs.contains(&Need::Transformed(at)) {
                let map = d.get_or_insert_with(|| Value::Object(Map::new()));
                let map = map.as_object_mut().expect("an object");
                for (key, value) in pokemon_fields(&sa.party[i]) {
                    if key == "transformed" || key == "moves" {
                        map.insert(key.to_owned(), value);
                    }
                }
            }
            if let Some(d) = d {
                pokemon.insert(meta.sides[side.index()].members[i].name.clone(), d);
            }
        }
        if !pokemon.is_empty() {
            out.insert("pokemon".into(), Value::Object(pokemon));
        }
        any_side |= !out.is_empty();
        sides.push(Value::Object(out));
    }
    if any_side {
        top.insert("sides".into(), Value::Array(sides));
    }
    (!top.is_empty()).then_some(Value::Object(top))
}

/// Applies an `x-hidden` object to a rebuilt state; the needs it satisfies are removed.
fn apply_hidden<const N: usize>(
    rebuilt: &mut Rebuilt<N>,
    meta: &ScenarioMeta,
    hidden: &Value,
    needs: &mut Vec<Need>,
) -> Result<(), ScenarioError> {
    let top = object(hidden, HIDDEN_KEY)?;
    for (key, value) in top {
        match key.as_str() {
            "lastMove" => rebuilt.state.last_move = move_id(text(value, key)?, key)?,
            "sides" => {
                let list = value
                    .as_array()
                    .filter(|l| l.len() == 2)
                    .ok_or_else(|| invalid("x-hidden sides: expected two objects"))?;
                for (side, obj) in [SideId::One, SideId::Two].into_iter().zip(list) {
                    apply_side(rebuilt, meta, side, object(obj, "x-hidden side")?, needs)?;
                }
            }
            other => return Err(invalid(format!("x-hidden: unknown field {other:?}"))),
        }
    }
    Ok(())
}

fn apply_side<const N: usize>(
    rebuilt: &mut Rebuilt<N>,
    meta: &ScenarioMeta,
    side: SideId,
    obj: &Map<String, Value>,
    needs: &mut Vec<Need>,
) -> Result<(), ScenarioError> {
    let members = meta.sides[side.index()].members.len();
    for (key, value) in obj {
        let what = format!("x-hidden {} {key}", side_name(side));
        let s = rebuilt.state.side_mut(side);
        match key.as_str() {
            "switchOrder" => {
                rebuilt.order[side.index()] = order_from(meta, side, value, &what)?;
            }
            "partyOrder" => {
                let order = order_from(meta, side, value, &what)?;
                s.party_order = IDENTITY_ORDER;
                s.party_order[..members].copy_from_slice(&order);
            }
            "hazardOrder" => {
                let list = value
                    .as_array()
                    .ok_or_else(|| invalid(format!("{what}: a list of hazard ids")))?;
                let mut kinds = Vec::new();
                for id in list {
                    let id = text(id, &what)?;
                    let kind = HAZARD_IDS
                        .iter()
                        .find(|(i, _)| *i == id)
                        .map(|(_, k)| *k)
                        .ok_or_else(|| invalid(format!("{what}: {id:?}")))?;
                    kinds.push(kind);
                }
                let active: Vec<SideEffect> = HAZARDS
                    .into_iter()
                    .filter(|h| s.effects[*h as usize].is_active())
                    .collect();
                let mut sorted_kinds = kinds.clone();
                sorted_kinds.sort_by_key(|k| *k as u8);
                let mut sorted_active = active.clone();
                sorted_active.sort_by_key(|k| *k as u8);
                if sorted_kinds != sorted_active {
                    return Err(invalid(format!(
                        "{what}: must list exactly the side's active hazards"
                    )));
                }
                let mut order = HazardOrder::default();
                let mut seen = [Effect::NONE; lab_engine::field::SIDE_EFFECT_COUNT];
                for kind in kinds {
                    order = order.changed(&seen, kind, true);
                    seen[kind as usize] = s.effects[kind as usize];
                }
                s.history.hazard_order = order;
            }
            "totalFainted" => s.history.total_fainted = int_in(value, &what)?,
            "faintedThisTurn" => s.history.fainted_this_turn = boolean(value, &what)?,
            "faintedLastTurn" => s.history.fainted_last_turn = boolean(value, &what)?,
            "ateBerry" => s.history.ate_berry = bits_of(meta, side, value, &what)?,
            "swordBoost" => s.history.sword_boost = bits_of(meta, side, value, &what)?,
            "shieldBoost" => s.history.shield_boost = bits_of(meta, side, value, &what)?,
            "syrupTriggered" => s.history.syrup_triggered = bits_of(meta, side, value, &what)?,
            "gimmicksUsed" => s.gimmicks_used = gimmicks(value, &what)?,
            "slotConditions" => {
                let list = value
                    .as_array()
                    .filter(|l| l.len() == N)
                    .ok_or_else(|| invalid(format!("{what}: one object per slot")))?;
                for (slot, conditions) in list.iter().enumerate() {
                    let mut table = [SlotEffect::NONE; lab_engine::field::SLOT_CONDITION_COUNT];
                    for (id, pair) in object(conditions, &what)? {
                        let kind = SlotCondition::ALL
                            .into_iter()
                            .find(|c| c.id() == id)
                            .ok_or_else(|| invalid(format!("{what}: {id:?}")))?;
                        let parts = pair
                            .as_array()
                            .filter(|p| p.len() == 2)
                            .ok_or_else(|| invalid(format!("{what}: [value, turn]")))?;
                        table[kind as usize] = SlotEffect {
                            value: int_in(&parts[0], &what)?,
                            turn: int_in(&parts[1], &what)?,
                        };
                    }
                    s.slot_conditions[slot] = table;
                    if table[SlotCondition::FutureMove as usize].is_active() {
                        needs.retain(|n| {
                            *n != Need::FutureMove(SlotRef {
                                side,
                                slot: slot as u8,
                            })
                        });
                    }
                }
            }
            "slots" => {
                let list = value
                    .as_array()
                    .filter(|l| l.len() == N)
                    .ok_or_else(|| invalid(format!("{what}: one object per slot")))?;
                for (i, slot_obj) in list.iter().enumerate() {
                    let at = SlotRef {
                        side,
                        slot: i as u8,
                    };
                    apply_slot(rebuilt, meta, at, object(slot_obj, &what)?, needs)?;
                }
            }
            "pokemon" => {
                for (name, fields) in object(value, &what)? {
                    let party = party_of(meta, side, &json!(name), &what)?;
                    let at = PokemonRef { side, party };
                    for (field, v) in object(fields, &what)? {
                        let mon = &mut rebuilt.state.side_mut(side).party[party as usize];
                        set_pokemon_field(mon, at, field, v, needs)?;
                    }
                }
            }
            other => return Err(invalid(format!("x-hidden: unknown side field {other:?}"))),
        }
    }
    Ok(())
}

fn apply_slot<const N: usize>(
    rebuilt: &mut Rebuilt<N>,
    meta: &ScenarioMeta,
    at: SlotRef,
    obj: &Map<String, Value>,
    needs: &mut Vec<Need>,
) -> Result<(), ScenarioError> {
    for (key, value) in obj {
        let what = format!("x-hidden {} slot {} {key}", side_name(at.side), at.slot);
        let slot = rebuilt.state.slot_mut(at);
        match key.as_str() {
            "faintedOccupant" => {
                slot.fainted_occupant = match value {
                    Value::Null => None,
                    v => Some(party_of(meta, at.side, v, &what)?),
                }
            }
            "abilityOrder" => slot.ability_order = int_in(value, &what)?,
            "lastMoveTargetLoc" => slot.last_move_target_loc = int_in(value, &what)?,
            "moveActions" => slot.move_actions = int_in(value, &what)?,
            "switchFlag" => {
                slot.switch_flag = switch_flag(value, &what)?;
                if slot.switch_flag != SwitchFlag::None {
                    needs.retain(|n| *n != Need::MidTurn(at.side));
                }
            }
            "history" => {
                for (field, v) in object(value, &what)? {
                    set_history_field(&mut slot.history, field, v)?;
                }
            }
            "volatiles" => {
                for (id, v) in object(value, &what)? {
                    let volatile = Volatile::ALL
                        .into_iter()
                        .find(|k| k.id() == id)
                        .ok_or_else(|| invalid(format!("{what}: {id:?}")))?;
                    slot.volatiles.set(volatile, volatile_from(v, &what)?);
                    needs.retain(|n| *n != Need::Volatile(at, volatile));
                }
            }
            other => return Err(invalid(format!("x-hidden: unknown slot field {other:?}"))),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `x-hidden` names volatiles by id, so the ids must be unique.
    #[test]
    fn volatile_ids_are_unique() {
        let mut ids: Vec<&str> = Volatile::ALL.iter().map(|v| v.id()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn lossy_volatiles() {
        assert!(volatile_is_lossy(Volatile::LeechSeed));
        assert!(volatile_is_lossy(Volatile::Stockpile));
        assert!(!volatile_is_lossy(Volatile::Confusion));
        assert!(!volatile_is_lossy(Volatile::LockedMove));
    }
}
