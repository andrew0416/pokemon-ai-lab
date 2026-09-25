//! Parity of move-specific callbacks (`core/src/turn/moves/handlers.rs`, WORKPLAN §2.1) with
//! Showdown: each scenario's exact outcome distribution must equal its oracle fixture
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

#[test]
fn o2_grav_apple_under_gravity() {
    assert_exact_parity("o2-grav-apple");
}
