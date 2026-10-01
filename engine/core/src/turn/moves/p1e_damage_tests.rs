//! Recursive pending-state normalization: each typed numeric carrier must independently
//! force expansion, including nested called-move frames and their retained hit arrays.
use super::super::lazy::{self, Request};
use super::*;
use crate::state::Pokemon;
use std::hash::{Hash, Hasher};

struct RunGuard;
impl Drop for RunGuard {
    fn drop(&mut self) {
        lazy::end();
    }
}

fn active(total_damage: DealtDamage) -> ActiveMove {
    let id = moves::TACKLE;
    let data = id.data();
    ActiveMove {
        id,
        data,
        category: data.category,
        priority: 0,
        prankster_boosted: false,
        spread: false,
        accuracy: Some(100),
        has_sheer_force: false,
        secondary_chance_factor: 1,
        added_secondary: None,
        parental_bond: false,
        total_damage,
        target: data.target,
        move_type: data.move_type,
        base_power: 40,
        ignore_evasion: false,
        scrappy: false,
        hit_targets: 1,
        source_effect: MoveId::NONE,
        self_switch: false,
        target_loc: 1,
        type_changer: AbilityId::NONE,
        has_bounced: false,
        future_hit: false,
        bypass_protect: 0,
        beat_up: [0; 6],
    }
}

fn progress(nested_callers: usize) -> MoveProgress {
    let user = SlotRef {
        side: crate::state::SideId::One,
        slot: 0,
    };
    let target = SlotRef {
        side: crate::state::SideId::Two,
        slot: 0,
    };
    MoveProgress {
        user,
        pokemon: PokemonRef {
            side: user.side,
            party: 0,
        },
        mv: active(DealtDamage::constant(1)),
        targets: smallvec::smallvec![target],
        main_target: target,
        hits: 3,
        hit: 1,
        total_damage: DealtDamage::constant(2),
        any_ok: true,
        last_hit: smallvec::smallvec![
            (target, LastHit::Damage(DealtDamage::constant(3))),
            (target, LastHit::Done),
            (target, LastHit::Substitute),
            (target, LastHit::Blocked),
        ],
        ignore_ability: false,
        infiltrates: false,
        raw_speed: Vec::new(),
        speed_snapshot: Vec::new(),
        smart: false,
        caller: (nested_callers > 0).then(|| {
            Box::new(CallerFrame {
                progress: progress(nested_callers - 1),
                results: smallvec::smallvec![
                    Hit::Damage(DealtDamage::constant(4)),
                    Hit::Failed,
                    Hit::Done,
                    Hit::Substitute,
                    Hit::Blocked
                ],
                main_target: target,
            })
        }),
    }
}

#[derive(Clone, Copy, Debug)]
enum Carrier {
    ActiveTotal,
    ProgressTotal,
    LastHit,
    CallerResult,
}

fn carrier(progress: &mut MoveProgress, depth: usize, field: Carrier) -> &mut DealtDamage {
    if depth > 0 {
        return carrier(
            &mut progress.caller.as_mut().unwrap().progress,
            depth - 1,
            field,
        );
    }
    match field {
        Carrier::ActiveTotal => &mut progress.mv.total_damage,
        Carrier::ProgressTotal => &mut progress.total_damage,
        Carrier::LastHit => match &mut progress.last_hit[0].1 {
            LastHit::Damage(d) => d,
            _ => unreachable!(),
        },
        Carrier::CallerResult => match &mut progress.caller.as_mut().unwrap().results[0] {
            Hit::Damage(d) => d,
            _ => unreachable!(),
        },
    }
}

fn hash(value: &MoveProgress) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

#[test]
fn nested_caller_materialization_observes_every_damage_carrier_before_keying() {
    let mon = Pokemon {
        hp: 20,
        max_hp: 100,
        lazy: lazy::tag(3),
        ..Pokemon::default()
    };
    lazy::begin(&[(3, 20)]);
    let _guard = RunGuard;
    let symbolic = lazy::capture_ko(&mon, 100).unwrap().0;
    let mut checked = 0;
    for depth in 0..=2 {
        for field in [
            Carrier::ActiveTotal,
            Carrier::ProgressTotal,
            Carrier::LastHit,
            Carrier::CallerResult,
        ] {
            if depth == 2 && matches!(field, Carrier::CallerResult) {
                continue;
            }
            let mut expected = progress(2);
            *carrier(&mut expected, depth, field) = DealtDamage::constant(20);
            let mut pending = expected.clone();
            *carrier(&mut pending, depth, field) = symbolic;
            assert!(format!("{pending:?}").contains("UnresolvedDealtDamage"));
            assert_eq!(lazy::take_request(), None);
            pending.materialize_damage();
            assert_eq!(
                lazy::take_request(),
                Some((3, Request::Expand)),
                "{depth} {field:?}"
            );
            // Eq and Hash themselves reject any missed symbolic carrier. The numeric fields
            // and all non-damage data must remain equal to the independently built key.
            assert_eq!(pending, expected, "{depth} {field:?}");
            assert_eq!(hash(&pending), hash(&expected), "{depth} {field:?}");
            assert_eq!(format!("{pending:?}"), format!("{expected:?}"));
            assert!(!format!("{pending:?}").contains("UnresolvedDealtDamage"));
            pending.materialize_damage();
            assert_eq!(
                lazy::take_request(),
                None,
                "concrete normalization is idempotent"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 11,
        "three progress levels and both caller result arrays"
    );
}
