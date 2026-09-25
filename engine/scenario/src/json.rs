//! Serde shapes of the oracle's scenario and team JSON (`engine/oracle/scenarios/`).
//!
//! Unknown fields are errors: a field the loader does not understand could change the battle,
//! so it is rejected instead of dropped.

use serde::Deserialize;
use serde_json::Value;

/// One team member in Showdown's JSON set format.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TeamSet {
    /// Display name; defaults to `species` as written (Showdown: `set.name || set.species`).
    #[serde(default)]
    pub name: Option<String>,
    pub species: String,
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub ability: Option<String>,
    #[serde(default)]
    pub nature: Option<String>,
    /// Champions Stat Points (the key is Showdown's `evs`).
    #[serde(default)]
    pub evs: StatTable,
    /// Range-checked only: the Champions stat formula has no IV term.
    #[serde(default)]
    pub ivs: Option<StatTable>,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(default)]
    pub moves: Vec<String>,
    /// Kept as sidecar data only; Terastallization is locked under Champions M-C.
    #[serde(default, rename = "teraType")]
    pub tera_type: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StatTable {
    #[serde(default)]
    pub hp: u8,
    #[serde(default)]
    pub atk: u8,
    #[serde(default)]
    pub def: u8,
    #[serde(default)]
    pub spa: u8,
    #[serde(default)]
    pub spd: u8,
    #[serde(default)]
    pub spe: u8,
}

impl StatTable {
    /// hp, atk, def, spa, spd, spe.
    pub fn to_array(self) -> [u8; 6] {
        [self.hp, self.atk, self.def, self.spa, self.spd, self.spe]
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioJson {
    #[serde(default)]
    pub description: Option<String>,
    pub format: String,
    /// Showdown PRNG seed. Irrelevant to an engine that enumerates every outcome; accepted
    /// and ignored.
    #[serde(default)]
    pub seed: Option<Value>,
    pub p1: SideJson,
    pub p2: SideJson,
    /// Rejected unless absent or empty (see `crate::load_scenario_str`).
    #[serde(default, rename = "setupTurns")]
    pub setup_turns: Option<Value>,
    /// Rejected unless absent or empty.
    #[serde(default)]
    pub patch: Option<Value>,
    #[serde(default)]
    pub turn: Option<TurnJson>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideJson {
    /// A path relative to the scenario file, or an inline team array.
    pub team: Value,
    /// Team preview choice (`"12"`, `"2,1"`); missing members follow in team order.
    #[serde(default)]
    pub order: Option<String>,
}

/// The decision the oracle checks, as Showdown choice strings. Parsing choices belongs to the
/// turn engine; the loader keeps them verbatim.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnJson {
    pub p1: String,
    pub p2: String,
}
