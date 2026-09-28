//! Opus VA (V11): fixtures from the unseen-feature parity positions.

mod common;

/// Turn 1 of a V11 lab-parity game: U-turn into an Eject Button ally on a Speed tie. The recorded
/// branch asked p1 twice for one switch; the other order asks both slots in one request (the
/// Champions Eject Button keeps U-turn's flag), which the recorded choices do not fit. The pinned
/// replay drops those branches instead of failing (it used to: "2 mid-turn switches needed, 1
/// given"), and the replacement after the turn matches the oracle.
#[test]
fn pinned_setup_turn_drops_branches_the_mid_turn_choices_do_not_fit() {
    common::assert_exact_parity("va-pinned-mid-turn-misfit");
}

/// V11 (never in the parity corpus): Normal Gem's `gem` volatile on the Fake Out user and the
/// target's `flinch`, seen in the state U-turn pauses the turn in (extremes rolls).
#[test]
fn gem_and_flinch_in_a_paused_turn() {
    common::assert_extremes_parity("va-gem-flinch-uturn-pause");
}

/// V11 (never in the parity corpus): Revival Blessing's slot condition while the turn waits for
/// the fainted member to revive, with Wide Guard on the user's side and Quick Guard on the foe's.
#[test]
fn revival_blessing_and_guards_in_a_paused_turn() {
    common::assert_exact_parity("va-revival-blessing-guards-pause");
}

/// V11f (Past content the engine implements): Geomancy's second turn from a lock, the choice
/// written with the target Showdown requires and ignores.
#[test]
fn geomancy_second_turn() {
    common::assert_exact_parity("va-geomancy-charge-lock");
}
