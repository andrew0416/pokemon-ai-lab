//! Follow-up fixes reported by earlier sessions (Opus R): event-order details of mid-turn
//! switches, spread AfterMoveSecondary, trapping, guards, accuracy, ties, Speed and weight.
//! Fixtures from Showdown's exact enumeration.

mod common;

use common::assert_exact_parity;

/// Mid-turn switch-outs run no BeforeSwitchOut Update (`skipBeforeSwitchOutEventFlag`): with
/// the Unnerve holder gone first, the U-turn user still leaves without eating its Sitrus Berry.
#[test]
fn instaswitch_skips_the_pre_switch_update() {
    assert_exact_parity("r-instaswitch-skips-update");
}

/// One spread move, two Eject Buttons: the speed-sorted AfterMoveSecondary lets the faster
/// holder switch out; the slower one's button stays unused.
#[test]
fn spread_eject_buttons_act_in_speed_order() {
    assert_exact_parity("r-spread-eject-buttons");
}

/// One spread move, two Red Cards: the faster holder's card drags the attacker; the slower
/// one's card sees the attacker's `forceSwitchFlag` and stays held.
#[test]
fn spread_red_cards_act_in_speed_order() {
    assert_exact_parity("r-spread-red-cards");
}
