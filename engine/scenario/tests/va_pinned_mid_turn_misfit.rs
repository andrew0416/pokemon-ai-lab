//! Opus VA (V11): a pinned setup turn whose recorded mid-turn switches fit only some branches.

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
