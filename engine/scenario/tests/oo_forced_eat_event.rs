//! Opus OO B35-forced-eat-eatitem: Showdown `eatItem(true)` (Teatime, Stuff Cheeks) runs
//! `runEvent('EatItem', this, source, sourceEffect, item)` after the berry's Eat, as every
//! `eatItem` does: Cheek Pouch heals, Cud Chew keeps the berry (the effect is not Bug Bite or
//! Pluck), Ripen sets `berryWeaken` for a resist berry. The engine's forced path skipped it.

mod common;

use common::assert_exact_parity;

/// Dedenne (Cheek Pouch) eats its Lum Berry through Teatime: a third of its HP back.
#[test]
fn teatime_triggers_cheek_pouch() {
    assert_exact_parity("oo-teatime-cheek-pouch");
}

/// Farigiraf (Cud Chew) ate its Sitrus Berry through Teatime in the setup turn; Cud Chew eats it
/// again at this turn's residual. Enumerated with `enumerate.cjs --staged` (full mode; the
/// unstaged Speed ties are about a million branches).
#[test]
fn teatime_berry_comes_back_through_cud_chew() {
    assert_exact_parity("oo-teatime-cud-chew");
}

/// Flapple (Ripen) eats its Occa Berry through Teatime: the next hit on it (Snorlax's Facade) is
/// halved once more.
#[test]
fn teatime_resist_berry_weakens_the_next_hit_through_ripen() {
    assert_exact_parity("oo-teatime-ripen");
}
