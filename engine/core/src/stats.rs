//! Pokémon Champions stat calculation.
//!
//! Champions uses Stat Points (SP), not the main-series EV formula: at level 50 HP is
//! `base + SP + 75`, while every other stat is `base + SP + 20` followed by nature.

use crate::dex::{Nature, SpeciesId, Stat};

pub const STAT_COUNT: usize = 6;
pub const MAX_STAT_POINT: u8 = 32;
pub const MAX_TOTAL_STAT_POINTS: u16 = 66;

/// hp, atk, def, spa, spd, spe.
pub type StatPoints = [u8; STAT_COUNT];
/// hp, atk, def, spa, spd, spe.
pub type CalculatedStats = [i16; STAT_COUNT];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatPointError {
    PerStat { stat: Stat, value: u8 },
    Total { value: u16 },
}

/// Calculates level-50 Champions stats after validating the SP limits.
pub fn champions_stats(
    species: SpeciesId,
    nature: Nature,
    stat_points: StatPoints,
) -> Result<CalculatedStats, StatPointError> {
    for (index, &value) in stat_points.iter().enumerate() {
        if value > MAX_STAT_POINT {
            return Err(StatPointError::PerStat {
                stat: stat_from_index(index),
                value,
            });
        }
    }
    let total = stat_points.iter().map(|&value| u16::from(value)).sum();
    if total > MAX_TOTAL_STAT_POINTS {
        return Err(StatPointError::Total { value: total });
    }

    let data = species.data();
    let mut result = [0i16; STAT_COUNT];
    result[0] = if data.fixed_max_hp != 0 {
        data.fixed_max_hp as i16
    } else {
        i16::from(data.base_stats[0]) + i16::from(stat_points[0]) + 75
    };

    let (raised, lowered) = nature.modifiers();
    for index in 1..STAT_COUNT {
        let stat = stat_from_index(index);
        let raw = u32::from(data.base_stats[index]) + u32::from(stat_points[index]) + 20;
        let modified = if raised == Some(stat) {
            nature_modify(raw, 110)
        } else if lowered == Some(stat) {
            nature_modify(raw, 90)
        } else {
            raw
        };
        result[index] = modified as i16;
    }
    Ok(result)
}

const fn stat_from_index(index: usize) -> Stat {
    match index {
        0 => Stat::Hp,
        1 => Stat::Atk,
        2 => Stat::Def,
        3 => Stat::Spa,
        4 => Stat::Spd,
        5 => Stat::Spe,
        _ => unreachable!(),
    }
}

/// Champions applies nature using 16-bit truncation before division.
const fn nature_modify(stat: u32, percent: u32) -> u32 {
    ((stat * percent) & 0xffff) / 100
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dex::{species, Nature};

    #[test]
    fn oracle_fixture_stats_match() {
        assert_eq!(
            champions_stats(species::RILLABOOM, Nature::Brave, [32, 32, 2, 0, 0, 0]),
            Ok([207, 194, 112, 80, 90, 94])
        );
        assert_eq!(
            champions_stats(species::TYRANITAR, Nature::Adamant, [32, 32, 0, 0, 2, 0]),
            Ok([207, 204, 130, 103, 122, 81])
        );
    }

    #[test]
    fn stat_point_limits_are_enforced() {
        assert_eq!(
            champions_stats(species::RILLABOOM, Nature::Hardy, [33, 0, 0, 0, 0, 0]),
            Err(StatPointError::PerStat {
                stat: Stat::Hp,
                value: 33
            })
        );
        assert_eq!(
            champions_stats(species::RILLABOOM, Nature::Hardy, [32, 32, 3, 0, 0, 0]),
            Err(StatPointError::Total { value: 67 })
        );
    }
}
