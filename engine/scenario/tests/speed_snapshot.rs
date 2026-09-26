//! Parity of `pokemon.speed` inside an action (board B2): Showdown refreshes it only between
//! actions (`updateSpeed`), at the residual and at the turn start, so Speed-sorted handlers
//! within one action (Eject Pack at the AfterMove, Symbiosis in the Update pass, ...) use the
//! Speed the action started with. The engine keeps a per-stage snapshot for `event_speed`.

mod common;

use common::assert_extremes_parity;

/// Icy Wind drops two Eject Pack holders from Speeds 100 and 99 to a 66/66 tie; Showdown still
/// ejects the pre-drop faster Swampert first every time (no draw).
#[test]
fn eject_pack_order_uses_the_speed_the_action_started_with() {
    assert_extremes_parity("f-speed-snapshot-eject-pack");
}
