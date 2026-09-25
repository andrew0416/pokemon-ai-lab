//! Team JSON → party `Pokemon` + sidecar, and team preview → `Side<N>`.

use lab_engine::dex::{to_id, AbilityId, Gender, ItemId, MoveId, Nature, SpeciesId, Type};
use lab_engine::gimmick::structural_gimmicks;
use lab_engine::state::{MoveSlot, Pokemon, Side, SideId, Status, PARTY_SIZE};
use lab_engine::stats::champions_stats;

use crate::error::{LoadError, SetProblem, TeamProblem};
use crate::json::TeamSet;
use crate::meta::{MemberMeta, SideMeta};

/// Showdown truncates set names to 20 characters (`set.name.substr(0, 20)`).
const MAX_NAME_CHARS: usize = 20;
const LEVEL: u8 = 50;

/// The display name Showdown gives a set: the nickname, or, when there is none or it equals
/// the species string, the *base* species name (`Indeedee-F` → `Indeedee`, `Tyranitar-Mega`
/// → `Tyranitar`; `sim/pokemon.ts` constructor). An unknown species keeps the string as
/// written; `build_pokemon` rejects it anyway.
pub fn display_name(set: &TeamSet) -> String {
    let name = match set.name.as_deref() {
        Some(name) if !name.is_empty() && name != set.species => name.to_owned(),
        _ => SpeciesId::from_name(&set.species)
            .map(|s| s.data().base_species.data().name.to_owned())
            .unwrap_or_else(|| set.species.clone()),
    };
    name.chars().take(MAX_NAME_CHARS).collect()
}

/// Builds one party member at full HP and PP, with Mega eligibility derived from its species
/// and item. The returned sidecar entry has `team_index` 0; [`build_side`] sets it.
pub fn build_pokemon(set: &TeamSet) -> Result<(Pokemon, MemberMeta), SetProblem> {
    let species = SpeciesId::from_name(&set.species)
        .ok_or_else(|| SetProblem::UnknownSpecies(set.species.clone()))?;

    let item = match set.item.as_deref() {
        None | Some("") => ItemId::NONE,
        Some(name) => {
            ItemId::from_name(name).ok_or_else(|| SetProblem::UnknownItem(name.to_owned()))?
        }
    };
    let ability = match set.ability.as_deref() {
        None | Some("") => return Err(SetProblem::MissingAbility),
        Some(name) => {
            AbilityId::from_name(name).ok_or_else(|| SetProblem::UnknownAbility(name.to_owned()))?
        }
    };
    let nature = match set.nature.as_deref() {
        None | Some("") => return Err(SetProblem::MissingNature),
        Some(name) => {
            nature_from_name(name).ok_or_else(|| SetProblem::UnknownNature(name.to_owned()))?
        }
    };

    let level = set.level.unwrap_or(LEVEL);
    if level != LEVEL {
        return Err(SetProblem::UnsupportedLevel(level));
    }
    if let Some(ivs) = set.ivs {
        const NAMES: [&str; 6] = ["hp", "atk", "def", "spa", "spd", "spe"];
        let ivs = ivs.to_array();
        if let Some(i) = ivs.iter().position(|&v| v > 31) {
            return Err(SetProblem::InvalidIv {
                stat: NAMES[i],
                value: ivs[i],
            });
        }
    }
    let gender = match set.gender.as_deref() {
        None | Some("") => Gender::Random,
        Some("M") => Gender::Male,
        Some("F") => Gender::Female,
        Some("N") => Gender::Genderless,
        Some(other) => return Err(SetProblem::UnknownGender(other.to_owned())),
    };
    let tera_type = match set.tera_type.as_deref() {
        None | Some("") => Type::None,
        Some(name) => {
            Type::from_name(name).ok_or_else(|| SetProblem::UnknownTeraType(name.to_owned()))?
        }
    };

    if set.moves.is_empty() {
        return Err(SetProblem::NoMoves);
    }
    if set.moves.len() > 4 {
        return Err(SetProblem::TooManyMoves(set.moves.len()));
    }
    let mut moves = [MoveSlot::default(); 4];
    for (i, name) in set.moves.iter().enumerate() {
        let id = MoveId::from_name(name).ok_or_else(|| SetProblem::UnknownMove(name.clone()))?;
        if moves[..i].iter().any(|slot| slot.id == id) {
            return Err(SetProblem::DuplicateMove(name.clone()));
        }
        moves[i] = MoveSlot::full(id);
    }

    let stat_points = set.evs.to_array();
    let stats = champions_stats(species, nature, stat_points).map_err(SetProblem::StatPoints)?;

    let pokemon = Pokemon {
        species,
        level,
        types: species.data().types,
        hp: stats[0],
        max_hp: stats[0],
        stats: [stats[1], stats[2], stats[3], stats[4], stats[5]],
        nature,
        stat_points,
        status: Status::None,
        status_turns: 0,
        item,
        last_item: ItemId::NONE,
        ability,
        base_ability: ability,
        moves,
        gimmicks: structural_gimmicks(species, item),
        gigantamax_factor: false,
    };
    let meta = MemberMeta {
        name: display_name(set),
        team_index: 0,
        gender,
        tera_type,
    };
    Ok((pokemon, meta))
}

fn nature_from_name(name: &str) -> Option<Nature> {
    let id = to_id(name);
    Nature::ALL.into_iter().find(|n| to_id(n.name()) == id)
}

/// Team preview as Showdown's `Side.chooseTeam` does it for a format that brings every
/// member: positions split on commas (or per character), cut to the team size, missing
/// members appended in team order. Returns 0-based team indices in party order.
pub fn preview_order(order: Option<&str>, team_len: usize) -> Result<Vec<usize>, String> {
    let order = order.map(str::trim).unwrap_or("");
    if order.starts_with('[') {
        return Err("bracketed team preview choices are not supported".into());
    }
    let mut positions = Vec::new();
    if !order.is_empty() {
        let parts: Vec<String> = if order.contains(',') {
            order.split(',').map(str::to_owned).collect()
        } else {
            order.chars().map(String::from).collect()
        };
        for part in parts.into_iter().take(team_len) {
            let pos: usize = part
                .trim()
                .parse()
                .map_err(|_| format!("{part:?} is not a team position"))?;
            if pos == 0 || pos > team_len {
                return Err(format!("no Pokémon in slot {pos}"));
            }
            if positions.contains(&(pos - 1)) {
                return Err(format!("the Pokémon in slot {pos} can only switch in once"));
            }
            positions.push(pos - 1);
        }
    }
    for i in 0..team_len {
        if !positions.contains(&i) {
            positions.push(i);
        }
    }
    Ok(positions)
}

/// Builds a side: party in team preview order, the first `N` members active.
pub fn build_side<const N: usize>(
    side: SideId,
    team: &[TeamSet],
    order: Option<&str>,
) -> Result<(Side<N>, SideMeta), LoadError> {
    let team_err = |problem| LoadError::Team { side, problem };
    if team.is_empty() {
        return Err(team_err(TeamProblem::Empty));
    }
    if team.len() > PARTY_SIZE {
        return Err(team_err(TeamProblem::TooLarge(team.len())));
    }
    let positions = preview_order(order, team.len()).map_err(|reason| {
        team_err(TeamProblem::Order {
            order: order.unwrap_or("").to_owned(),
            reason,
        })
    })?;

    let mut built: Vec<(Pokemon, MemberMeta)> = Vec::with_capacity(team.len());
    for (index, set) in team.iter().enumerate() {
        let (pokemon, mut meta) = build_pokemon(set).map_err(|problem| LoadError::Set {
            side,
            index,
            name: display_name(set),
            problem,
        })?;
        if built.iter().any(|(_, m)| m.name == meta.name) {
            return Err(team_err(TeamProblem::DuplicateName(meta.name)));
        }
        meta.team_index = index as u8;
        built.push((pokemon, meta));
    }

    let mut result = Side::<N>::default();
    let mut meta = SideMeta::default();
    for (party_index, &team_index) in positions.iter().enumerate() {
        let (pokemon, member) = built[team_index].clone();
        result.party[party_index] = pokemon;
        meta.members.push(member);
    }
    for (slot_index, slot) in result.slots.iter_mut().enumerate() {
        slot.party_index = (slot_index < positions.len()).then_some(slot_index as u8);
    }
    Ok((result, meta))
}
