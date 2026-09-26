//! Parity of Leek (Opus W unit 4) with Showdown: +2 critical-hit stages for Farfetch'd and
//! Sirfetch'd only.

mod common;

use common::assert_exact_parity;

#[test]
fn leek_raises_the_crit_ratio_of_sirfetchd_only() {
    assert_exact_parity("w-leek");
}
