//! Parity of Champions Mega abilities added by Opus Q unit 8 (Spicy Spray, Healer) with
//! Showdown, through an actual Mega Evolution: the scenario's exact outcome distribution must
//! equal its oracle fixture.

mod common;

use common::assert_exact_parity;

/// Mega Pyroar (Fire Mane) and Mega Starmie (Huge Power), refused before unit 1.
#[test]
fn mega_pyroar_and_mega_starmie_match_showdown() {
    assert_exact_parity("q-mega-pyroar-starmie");
}

/// Mega Scovillain's Spicy Spray burns a non-contact attacker; Mega Audino's Healer (1/2 in
/// Champions) may cure its ally before the burn damage.
#[test]
fn mega_spicy_spray_and_healer_match_showdown() {
    assert_exact_parity("q-mega-spicy-spray-healer");
}
