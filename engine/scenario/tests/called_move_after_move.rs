//! Parity of a called move's own `onAfterMove` (Opus CC unit B18). Sleep Talk, Copycat and
//! Mirror Move call a move through `useMove` inside their own hit; `runMove` then runs AfterMove
//! with the battle's active move, the called one (`if (this.battle.activeMove) move =
//! this.battle.activeMove`), for the caller's user. Sparkling Aria's AfterMove reads the called
//! move's `hitTargets`, Spit Up's ends the caller's stockpile. Both were refused as called moves.

mod common;

use common::assert_exact_parity;

/// Sleep Talk calls Sparkling Aria; the burned ally is its one hit target and is cured.
#[test]
fn sleep_talk_calling_sparkling_aria_cures_the_burned_ally_matches_showdown() {
    assert_exact_parity("cc-sleep-talk-sparkling-aria");
}

/// Copycat calls Spit Up (battle.lastMove); the Copycat user's stockpile and its raises end.
#[test]
fn copycat_calling_spit_up_ends_the_callers_stockpile_matches_showdown() {
    assert_exact_parity("cc-copycat-spit-up");
}

/// Mirror Move calls Sparkling Aria (the target's lastMove); the burned foe is cured.
#[test]
fn mirror_move_calling_sparkling_aria_cures_the_burned_target_matches_showdown() {
    assert_exact_parity("cc-mirror-move-sparkling-aria");
}
