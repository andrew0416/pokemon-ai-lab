//! Parity of Transform and Imposter (Opus EE unit EE1): Showdown `pokemon.transformInto` as
//! state (`Pokemon::transformed` keeps the base species and move slots) and its reversal in
//! `clearVolatile`, the move's `onHit` and Imposter's `onSwitchIn`.

mod common;

use common::assert_exact_parity;

/// Imposter at the battle start: the opposite foe, its stored stats and moves (5 PP), its
/// Intimidate started; the foe's own Intimidate after it lowers the transformed Ditto.
#[test]
fn imposter_lead_transforms_into_the_opposite_foe() {
    assert_exact_parity("ee-imposter-lead");
}

/// Imposter switching in during a turn opposite a healthy foe.
#[test]
fn imposter_switch_in_transforms_into_a_present_foe() {
    assert_exact_parity("ee-imposter-present");
}

/// Imposter switching in opposite a fainted foe that was not replaced: `transformInto` fails.
#[test]
fn imposter_switch_in_opposite_a_fainted_foe_does_nothing() {
    assert_exact_parity("ee-imposter-fainted");
}

/// Transform into a Mega Evolved, Dragon Danced, Soaked Charizard behind Protect: species,
/// stored stats, stages, the changed type, ability, 5-PP moves; HP stays.
#[test]
fn transform_copies_a_boosted_mega_with_a_changed_type() {
    assert_exact_parity("ee-transform-mega");
}

/// A roosting target's real types (`roost.typeWas`) and its added type are copied.
#[test]
fn transform_copies_roost_types_and_the_added_type() {
    assert_exact_parity("ee-transform-roost-added-type");
}

/// Dragon Cheer and Focus Energy: the user's go, the target's come.
#[test]
fn transform_replaces_the_critical_hit_volatiles() {
    assert_exact_parity("ee-transform-crit-volatiles");
}

/// A substitute stops Transform.
#[test]
fn transform_fails_against_a_substitute() {
    assert_exact_parity("ee-transform-substitute");
}

/// A transformed user or target makes Transform fail (Imposter into a Ditto, then both
/// Transforms).
#[test]
fn transform_fails_when_either_pokemon_is_transformed() {
    assert_exact_parity("ee-transform-twice");
}

/// Switching out restores the species, stats, ability and own move slots (with their PP).
#[test]
fn a_transformed_pokemon_switching_out_and_back_is_itself_again() {
    assert_exact_parity("ee-transform-switch-back");
}

/// Fainting reverts the transformation too (`faintMessages` → `clearVolatile`).
#[test]
fn a_transformed_pokemon_faints_as_itself() {
    assert_exact_parity("ee-transform-faint");
}

/// A `notransform` ability (Disguise) is ignored while transformed; the copied weight counts
/// (Heavy Slam).
#[test]
fn a_transformed_pokemon_ignores_disguise() {
    assert_exact_parity("ee-transform-disguise");
}

/// Transform into Castform-Sunny: the copied Forecast does nothing (`pokemon.transformed`)
/// when the weather turns to rain.
#[test]
fn a_transformed_castform_keeps_its_copied_forme() {
    assert_exact_parity("ee-transform-forecast");
}

/// Stance Change does nothing for a transformed Aegislash: its attack uses the Shield Forme
/// stats it copied.
#[test]
fn a_transformed_aegislash_does_not_change_stance() {
    assert_exact_parity("ee-transform-stance-change");
}
