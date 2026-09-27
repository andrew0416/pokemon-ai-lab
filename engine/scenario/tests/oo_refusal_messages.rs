//! Opus OO B36-refusal-messages: refusal texts say what is missing. `berry_problem` refused a
//! Figy-type berry whose flavour the holder's nature dislikes "because confusion is not
//! implemented", which was stale (the berry's Eat confuses): the refusal is gone and the holder's
//! own Update eating matches the oracle. The Mirror Herb and Beat Up messages lost the runs of
//! spaces their wrapped string literals had.

mod common;

use common::{assert_exact_parity, engine_dir, start};
use lab_scenario::{run_decision_mid_turn, scenario_decision};
use serde_json::Value;

/// A Timid Snorlax at 60 HP eats its Figy Berry at the turn-start Update: a third of its HP back,
/// and confusion, which its Harden then checks.
#[test]
fn a_disliked_figy_berry_confuses_its_holder() {
    assert_exact_parity("oo-figy-berry-confusion");
}

/// The Beat Up refusal (`rr-beat-up-bench`, refused until board R9 implements the bench order) is
/// one sentence with single spaces.
#[test]
fn the_beat_up_refusal_has_no_runs_of_spaces() {
    let name = "rr-beat-up-bench";
    let path = engine_dir().join(format!("oracle/expected/{name}.extremes.json"));
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (loaded, position) = start(name, &fixture);
    let mut state = position.state.clone();
    let decision = scenario_decision(&loaded, &position).unwrap();
    // Once R9 simulates it there is no message left to check.
    if let Err(e) = run_decision_mid_turn(&mut state, &position.order, &decision, &loaded.mid_turn)
    {
        assert!(e.is_unsupported(), "{e}");
        let why = e.message();
        assert!(
            why.contains(
                "Beat Up with benched allies of different power [18, 12] (their order in \
                 Showdown's side.pokemon depends on the switches so far, which the state does not \
                 keep)"
            ),
            "{why}"
        );
        assert!(!why.contains("  "), "a run of spaces in `{why}`");
    }
}
