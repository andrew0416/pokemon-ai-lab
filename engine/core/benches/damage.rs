use std::hint::black_box;
use std::time::Instant;

use lab_engine::damage::{
    chain_modifiers, damage_rolls, DamageInput, MOD_DOUBLE, MOD_ONE_POINT_FIVE,
    MOD_ONE_POINT_THREE, MOD_ONE_POINT_TWO,
};

const ITERATIONS: u64 = 20_000_000;

fn main() {
    let mut input = DamageInput::neutral(55, 194, 130);
    input.base_power_modifier =
        chain_modifiers(&[MOD_ONE_POINT_THREE, MOD_ONE_POINT_TWO], 41, 2_097_152);
    input.stab_modifier = MOD_ONE_POINT_FIVE;
    input.type_effectiveness = MOD_DOUBLE;

    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..ITERATIONS {
        let rolls = damage_rolls(black_box(input));
        checksum = checksum.wrapping_add(u64::from(black_box(rolls[15])));
    }
    let elapsed = started.elapsed();
    let ns_per_distribution = elapsed.as_nanos() as f64 / ITERATIONS as f64;
    let million_distributions_per_second = ITERATIONS as f64 / elapsed.as_secs_f64() / 1_000_000.0;

    println!("damage distributions: {ITERATIONS}");
    println!("elapsed: {elapsed:.3?}");
    println!("{ns_per_distribution:.2} ns/distribution");
    println!("{million_distributions_per_second:.2} million distributions/s");
    println!("checksum: {checksum}");
}
