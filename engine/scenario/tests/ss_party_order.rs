//! Opus SS (wave 15 L1, board R9a/R9b): Showdown's `side.pokemon` order in the state
//! (`Side::party_order`). Every switch-in exchanges the newcomer's position with the Pokémon it
//! replaces and Ally Switch exchanges the two active positions; Beat Up hits in that order
//! (`move.allies`). The fixtures put a Stamina Mudsdale in front of the hits, so the order of
//! the hit powers shows in the damage (each hit raises its Defense). `--mode extremes --staged`
//! fixtures (the plain enumeration of four hits takes minutes).

mod common;

use common::{assert_extremes_parity, start};
use lab_engine::state::{SideId, IDENTITY_ORDER};
use serde_json::Value;

/// Snorlax switched out for Clefable: Weavile, Clefable, Garchomp, Snorlax.
#[test]
fn beat_up_after_a_switch() {
    assert_extremes_parity("ss-beat-up-switched-bench");
}

/// A switch, then Ally Switch: Farigiraf, Weavile, Garchomp, Snorlax.
#[test]
fn beat_up_after_a_switch_and_ally_switch() {
    assert_extremes_parity("ss-beat-up-ally-switch");
}

fn fixture(name: &str) -> Value {
    let path = common::engine_dir().join(format!("oracle/expected/{name}.extremes.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The state's order is the scenario's (`advance_order`, which `switch N` is parsed against)
/// where a Beat Up records it, and the actives lead it.
#[test]
fn the_state_order_follows_the_switches() {
    let (_, position) = start(
        "ss-beat-up-switched-bench",
        &fixture("ss-beat-up-switched-bench"),
    );
    let order = position.state.side(SideId::One).party_order;
    // Weavile, Clefable, Garchomp, Snorlax, then the empty entries.
    assert_eq!(order, [0, 3, 2, 1, 4, 5]);
    assert_eq!(&order[..4], position.order[0].as_slice());
    assert_eq!(position.state.side(SideId::Two).party_order, IDENTITY_ORDER);

    let (_, position) = start("ss-beat-up-ally-switch", &fixture("ss-beat-up-ally-switch"));
    let order = position.state.side(SideId::One).party_order;
    // Farigiraf, Weavile, Garchomp, Snorlax: Ally Switch moved the actives. The scenario's
    // order only tracks what `switch N` reads, the bench.
    assert_eq!(order, [3, 0, 2, 1, 4, 5]);
    assert_eq!(&order[2..4], &position.order[0][2..]);
}
