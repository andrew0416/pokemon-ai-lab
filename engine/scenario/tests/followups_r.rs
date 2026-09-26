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
