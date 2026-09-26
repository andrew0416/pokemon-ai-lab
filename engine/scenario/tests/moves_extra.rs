//! Parity of moves Champions doubles players use that no library team carries (Opus P): Heal
//! Pulse, ... Each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

#[test]
fn heal_pulse_heals_half_or_three_quarters_with_mega_launcher() {
    assert_exact_parity("heal-pulse");
}

#[test]
fn heal_pulse_bounces_and_is_blocked_by_protect() {
    assert_exact_parity("heal-pulse-bounce");
}
