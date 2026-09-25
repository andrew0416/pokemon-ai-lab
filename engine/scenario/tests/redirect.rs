//! Target redirection parity (WORKPLAN F7): Follow Me / Rage Powder volatiles and Lightning
//! Rod / Storm Drain (`RedirectTarget` priority event, then the absorbing `TryHit`). Fixtures
//! made by `enumerate.cjs` on the fixed Showdown commit; see `oracle/scenarios/redirect.p*.json`.

mod common;

use common::assert_exact_parity;

/// Follow Me (priority +2) is up before Hypnosis and pulls it onto Indeedee-F.
#[test]
fn follow_me_redirects_hypnosis() {
    assert_exact_parity("followme-hypnosis");
}

/// Rage Powder pulls Focus Blast but not a Grass type's Wood Hammer (powder immunity).
#[test]
fn rage_powder_skips_grass_types() {
    assert_exact_parity("ragepowder-grass");
}

/// A foe's Lightning Rod pulls Thunderbolt off its partner and absorbs it (+1 SpA).
#[test]
fn lightning_rod_pulls_a_foes_electric_move() {
    assert_exact_parity("lightningrod-foe");
}

/// An ally's Lightning Rod pulls the partner's own Thunderbolt too.
#[test]
fn lightning_rod_pulls_an_allys_electric_move() {
    assert_exact_parity("lightningrod-ally");
}
