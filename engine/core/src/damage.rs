//! Allocation-free Champions damage primitives.
//!
//! The order and half-down rounding match the Champions calculator. Higher-level move,
//! ability, item and field handlers reduce their effects to the fixed-point modifiers here.

/// Game Freak's fixed-point denominator for damage modifiers.
pub const MOD_ONE: u32 = 4096;
pub const MOD_QUARTER: u32 = 1024;
pub const MOD_HALF: u32 = 2048;
pub const MOD_THREE_QUARTERS: u32 = 3072;
pub const MOD_ONE_POINT_TWO: u32 = 4915;
pub const MOD_ONE_POINT_THREE: u32 = 5325;
pub const MOD_ONE_POINT_FIVE: u32 = 6144;
pub const MOD_DOUBLE: u32 = 8192;

pub const DAMAGE_ROLL_COUNT: usize = 16;
pub type DamageRolls = [u16; DAMAGE_ROLL_COUNT];

/// Inputs after move-specific effects have selected power, offensive and defensive stats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageInput {
    pub level: u8,
    pub base_power: u16,
    pub attack: u16,
    pub defense: u16,
    /// Chained base-power modifier, using [`MOD_ONE`] as 1x.
    pub base_power_modifier: u32,
    pub spread: bool,
    /// Weather modifier, using [`MOD_ONE`] as 1x.
    pub weather_modifier: u32,
    pub critical: bool,
    /// STAB modifier, using [`MOD_ONE`] as 1x.
    pub stab_modifier: u32,
    /// Type effectiveness, using [`MOD_ONE`] as 1x. Zero means immune.
    pub type_effectiveness: u32,
    /// Whether the physical burn penalty applies after type effectiveness.
    pub burned: bool,
    /// Whether the move goes through a protection at a quarter of its damage (Showdown
    /// `getMoveHitData(move).bypassProtect`), applied after the final modifier.
    pub protected: bool,
    /// Chained final modifier, using [`MOD_ONE`] as 1x.
    pub final_modifier: u32,
}

impl DamageInput {
    /// A neutral level-50 hit. Callers only need to replace the modifiers that apply.
    pub const fn neutral(base_power: u16, attack: u16, defense: u16) -> Self {
        Self {
            level: 50,
            base_power,
            attack,
            defense,
            base_power_modifier: MOD_ONE,
            spread: false,
            weather_modifier: MOD_ONE,
            critical: false,
            stab_modifier: MOD_ONE,
            type_effectiveness: MOD_ONE,
            burned: false,
            protected: false,
            final_modifier: MOD_ONE,
        }
    }
}

/// Chains 4096-based modifiers with the in-game rounding step after every factor.
pub fn chain_modifiers(modifiers: &[u32], lower_bound: u32, upper_bound: u32) -> u32 {
    let mut chained = MOD_ONE;
    for &modifier in modifiers {
        if modifier != MOD_ONE {
            chained = chained.wrapping_mul(modifier).wrapping_add(MOD_ONE / 2) >> 12;
        }
    }
    chained.clamp(lower_bound, upper_bound)
}

/// Applies a pre-chained modifier to base power using half-down rounding.
pub fn modified_base_power(base_power: u16, modifier: u32) -> u16 {
    let result = poke_round_fraction_u64(
        u64::from(base_power) * u64::from(modifier),
        u64::from(MOD_ONE),
    );
    result.max(1) as u16
}

/// Returns all 16 equally likely integer damage rolls (85 through 100).
pub fn damage_rolls(input: DamageInput) -> DamageRolls {
    if input.base_power == 0 || input.attack == 0 || input.defense == 0 {
        return [0; DAMAGE_ROLL_COUNT];
    }
    if input.type_effectiveness == 0 {
        return [0; DAMAGE_ROLL_COUNT];
    }

    let power = modified_base_power(input.base_power, input.base_power_modifier);
    let mut base_damage = base_damage(input.level, power, input.attack, input.defense);

    if input.spread {
        base_damage = apply_rounded_modifier(base_damage, MOD_THREE_QUARTERS);
    }
    if input.weather_modifier != MOD_ONE {
        base_damage = apply_rounded_modifier(base_damage, input.weather_modifier);
    }
    if input.critical {
        base_damage = base_damage.wrapping_mul(3) / 2;
    }

    let mut result = [0; DAMAGE_ROLL_COUNT];
    let mut index = 0;
    while index < DAMAGE_ROLL_COUNT {
        let random = 85 + index as u32;
        let mut damage = base_damage.wrapping_mul(random) / 100;

        if input.stab_modifier != MOD_ONE {
            damage = apply_rounded_modifier(damage, input.stab_modifier);
        }
        damage = damage.wrapping_mul(input.type_effectiveness) / MOD_ONE;
        if input.burned {
            damage /= 2;
        }
        damage = apply_rounded_modifier(damage, input.final_modifier);
        // Showdown `modifyDamage`: `bypassProtect` quarters the damage after `ModifyDamage`,
        // then `if (!baseDamage) return 1`.
        if input.protected {
            damage = apply_rounded_modifier(damage, MOD_QUARTER);
        }
        let damage = damage.max(1);
        result[index] = (damage & u32::from(u16::MAX)) as u16;
        index += 1;
    }
    result
}

fn base_damage(level: u8, base_power: u16, attack: u16, defense: u16) -> u32 {
    let level_factor = (2 * u32::from(level)) / 5 + 2;
    let after_power = level_factor.wrapping_mul(u32::from(base_power));
    let after_attack = after_power.wrapping_mul(u32::from(attack));
    (after_attack / u32::from(defense)) / 50 + 2
}

fn apply_rounded_modifier(value: u32, modifier: u32) -> u32 {
    poke_round_fraction(value.wrapping_mul(modifier), MOD_ONE)
}

/// Rounds positive fractions to nearest with exact halves rounded down.
fn poke_round_fraction(numerator: u32, denominator: u32) -> u32 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    quotient + u32::from(remainder > denominator / 2)
}

fn poke_round_fraction_u64(numerator: u64, denominator: u64) -> u64 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    quotient + u64::from(remainder > denominator / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn champions_oracle_grassy_glide_rolls_match() {
        let mut input = DamageInput::neutral(55, 194, 130);
        input.base_power_modifier =
            chain_modifiers(&[MOD_ONE_POINT_THREE, MOD_ONE_POINT_TWO], 41, 2_097_152);
        input.stab_modifier = MOD_ONE_POINT_FIVE;
        input.type_effectiveness = MOD_DOUBLE;

        assert_eq!(input.base_power_modifier, 6390);
        assert_eq!(
            modified_base_power(input.base_power, input.base_power_modifier),
            86
        );
        assert_eq!(
            damage_rolls(input),
            [146, 146, 150, 152, 152, 156, 156, 158, 158, 162, 164, 164, 168, 168, 170, 174]
        );
    }

    #[test]
    fn immunity_is_zero_for_every_roll() {
        let mut input = DamageInput::neutral(100, 200, 100);
        input.type_effectiveness = 0;
        assert_eq!(damage_rolls(input), [0; DAMAGE_ROLL_COUNT]);
    }

    #[test]
    fn modifier_chain_rounds_after_each_factor() {
        assert_eq!(
            chain_modifiers(&[MOD_ONE_POINT_THREE, MOD_ONE_POINT_TWO], 41, 2_097_152),
            6390
        );
    }
}
