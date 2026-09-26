//! Parental Bond (Opus AA unit 5). Fixtures from Showdown's exact enumeration
//! (`engine/oracle/expected/<name>.turn.json`).

mod common;

use common::assert_exact_parity;

/// Mega Kangaskhan's Seismic Toss hits twice for its fixed damage.
#[test]
fn parental_bond_hits_twice() {
    assert_exact_parity("aa-parental-bond");
}

/// Each hit makes contact: Rocky Helmet hurts the user twice.
#[test]
fn parental_bond_hits_make_contact_twice() {
    assert_exact_parity("aa-parental-bond-contact");
}

/// The second hit's base damage is quartered.
#[test]
fn parental_bond_second_hit_is_quartered() {
    assert_exact_parity("aa-parental-bond-damage");
}
