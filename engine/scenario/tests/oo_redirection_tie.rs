//! Opus OO R4-redirection-tie: two `RedirectTarget` handlers of one priority at equal Speed.
//! Showdown's `priorityEvent` sorts them with `compareRedirectOrder` in a stable sort (no
//! Speed-tie shuffle): priority, Speed, then the holders' `abilityState.effectOrder` (who
//! switched in or last had an ability set first; `Slot::ability_order`). The fixtures were
//! enumerated with `enumerate.cjs --staged` (full mode; the Speed ties of the unstaged runs are
//! 2^23 branches). `rr-redirect-tie` (`refusals_rr.rs`) is the p2a-first case.

mod common;

use common::assert_exact_parity;

/// Ariados (Rage Powder) leads in p2a, Clefable (Follow Me) in p2b: Ariados's ability state
/// started first, so it takes Garchomp's Dragon Claw aimed at Clefable, whoever moved first.
#[test]
fn the_holder_that_switched_in_first_wins() {
    assert_exact_parity("oo-redirect-tie-swapped");
}

/// Worry Seed redirected to Clefable (Follow Me) restarts its ability state, so Garchomp's Dragon
/// Claw later in the turn goes to Ariados (Rage Powder) instead.
#[test]
fn set_ability_moves_the_holder_to_the_back() {
    assert_exact_parity("oo-redirect-tie-worry-seed");
}

/// Lightning Rod on both sides at equal Speed: the foe that switched in first (Raichu, p1b) takes
/// Pikachu's Thunderbolt, not the user's own ally Manectric.
#[test]
fn lightning_rod_tie_across_sides() {
    assert_exact_parity("oo-lightningrod-tie");
}
