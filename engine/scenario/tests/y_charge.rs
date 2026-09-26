//! The move Charge (Opus Y unit 5; Magic Coat, Snatch and Powder are Past in Champions). Fixtures
//! from Showdown's exact enumeration (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Charge raises SpD and starts `charge`, which its own use does not end.
#[test]
fn charge_starts_its_condition() {
    assert_exact_parity("y-charge");
}

/// The next Electric move has double power and ends `charge`.
#[test]
fn charge_doubles_the_next_electric_move() {
    assert_exact_parity("y-charge-thunderbolt");
}
