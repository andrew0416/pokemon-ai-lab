//! Instructions that turn one state into another.
//!
//! The staged enumeration merges paths, so an outcome is not tied to one path's log. Its
//! instructions are rebuilt from the start and end states instead: applying them to the start
//! gives the end, and reversing them on the end gives the start back. They cover every
//! field the turn engine can change; the rest must be equal (checked in debug builds).

use crate::gimmick::Gimmick;
use crate::instruction::Instruction;
use crate::state::{PokemonRef, SideId, Slot, SlotRef, State};
use crate::volatile::VolatileState;

pub(crate) fn instructions<const N: usize>(from: &State<N>, to: &State<N>) -> Vec<Instruction> {
    let mut out = Vec::new();
    for side in [SideId::One, SideId::Two] {
        let (a, b) = (from.side(side), to.side(side));
        for party in 0..a.party.len() as u8 {
            pokemon(&mut out, from, to, PokemonRef { side, party });
        }
        for slot in 0..N as u8 {
            let r = SlotRef { side, slot };
            slot_changes(&mut out, r, from.slot(r), to.slot(r));
        }
        for (i, (&old, &new)) in a.effects.iter().zip(&b.effects).enumerate() {
            if old != new {
                out.push(Instruction::SetSideEffect {
                    side,
                    effect: side_effect(i),
                    old,
                    new,
                });
            }
        }
        for gimmick in Gimmick::ACTIVATIONS {
            if b.gimmicks_used.contains(gimmick) && !a.gimmicks_used.contains(gimmick) {
                out.push(Instruction::UseGimmick { side, gimmick });
            }
            debug_assert!(
                !(a.gimmicks_used.contains(gimmick) && !b.gimmicks_used.contains(gimmick)),
                "a spent gimmick cannot come back"
            );
        }
    }
    for (i, (&old, &new)) in from.field.iter().zip(&to.field).enumerate() {
        if old != new {
            out.push(Instruction::SetField {
                effect: field_effect(i),
                old,
                new,
            });
        }
    }
    if from.turn != to.turn {
        out.push(Instruction::SetTurn {
            old: from.turn,
            new: to.turn,
        });
    }
    if from.result != to.result {
        out.push(Instruction::SetResult {
            old: from.result,
            new: to.result,
        });
    }
    out
}

fn pokemon<const N: usize>(
    out: &mut Vec<Instruction>,
    from: &State<N>,
    to: &State<N>,
    r: PokemonRef,
) {
    let (a, b) = (from.pokemon(r), to.pokemon(r));
    if a == b {
        return;
    }
    if b.hp < a.hp {
        out.push(Instruction::Damage {
            target: r,
            amount: a.hp - b.hp,
        });
    } else if b.hp > a.hp {
        out.push(Instruction::Heal {
            target: r,
            amount: b.hp - a.hp,
        });
    }
    if a.status != b.status {
        out.push(Instruction::ChangeStatus {
            target: r,
            old: a.status,
            new: b.status,
        });
    }
    if a.status_turns != b.status_turns {
        out.push(Instruction::SetStatusTurns {
            target: r,
            old: a.status_turns,
            new: b.status_turns,
        });
    }
    if a.item != b.item {
        out.push(Instruction::SetItem {
            target: r,
            old: a.item,
            new: b.item,
        });
    }
    if a.last_item != b.last_item {
        out.push(Instruction::SetLastItem {
            target: r,
            old: a.last_item,
            new: b.last_item,
        });
    }
    let (fa, fb) = (a.forme(), b.forme());
    if fa.species != fb.species
        || fa.max_hp != fb.max_hp
        || fa.stats != fb.stats
        || fa.base_ability != fb.base_ability
    {
        // A forme change carries the ability and types along.
        out.push(Instruction::SetForme {
            target: r,
            old: fa,
            new: fb,
        });
    } else {
        if a.ability != b.ability {
            out.push(Instruction::SetAbility {
                target: r,
                old: a.ability,
                new: b.ability,
            });
        }
        if a.types != b.types {
            out.push(Instruction::SetTypes {
                target: r,
                old: a.types,
                new: b.types,
            });
        }
    }
    for (i, (ma, mb)) in a.moves.iter().zip(&b.moves).enumerate() {
        if ma.pp != mb.pp {
            out.push(Instruction::SetPp {
                target: r,
                move_index: i as u8,
                old: ma.pp,
                new: mb.pp,
            });
        }
        debug_assert!(ma.id == mb.id && ma.disabled == mb.disabled);
    }
    debug_assert!(
        a.level == b.level
            && a.nature == b.nature
            && a.stat_points == b.stat_points
            && a.gimmicks == b.gimmicks
            && a.gigantamax_factor == b.gigantamax_factor,
        "a field the turn engine never changes differs"
    );
}

/// A changed slot is reset with `Switch` (which restores the old slot on reverse), then its
/// non-default fields are set.
fn slot_changes(out: &mut Vec<Instruction>, r: SlotRef, a: &Slot, b: &Slot) {
    if a == b {
        return;
    }
    out.push(Instruction::Switch {
        slot: r,
        previous: a.clone(),
        party_index: b.party_index,
    });
    for (stat, &amount) in b.boosts.iter().enumerate() {
        if amount != 0 {
            out.push(Instruction::Boost {
                target: r,
                stat: stat as u8,
                amount,
            });
        }
    }
    for (volatile, state) in b.volatiles.iter() {
        out.push(Instruction::SetVolatile {
            target: r,
            volatile,
            old: VolatileState::NONE,
            new: state,
        });
    }
    if !b.last_move.is_none() {
        out.push(Instruction::SetLastMove {
            target: r,
            old: Default::default(),
            new: b.last_move,
        });
    }
    if b.move_actions != 0 {
        out.push(Instruction::SetMoveActions {
            target: r,
            old: 0,
            new: b.move_actions,
        });
    }
    if b.fainted_occupant.is_some() {
        out.push(Instruction::SetFaintedOccupant {
            slot: r,
            old: None,
            new: b.fainted_occupant,
        });
    }
    debug_assert!(
        b.substitute_hp == 0 && !b.dynamax.is_active(),
        "no instruction sets these yet"
    );
}

fn field_effect(i: usize) -> crate::field::FieldEffect {
    use crate::field::FieldEffect::*;
    [Weather, Terrain, TrickRoom, Gravity, MagicRoom, WonderRoom][i]
}

fn side_effect(i: usize) -> crate::field::SideEffect {
    use crate::field::SideEffect::*;
    [
        Reflect,
        LightScreen,
        AuroraVeil,
        Tailwind,
        Safeguard,
        Mist,
        StealthRock,
        Spikes,
        ToxicSpikes,
        StickyWeb,
        WideGuard,
        QuickGuard,
        LuckyChant,
    ][i]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::FieldEffect;
    use crate::volatile::Volatile;

    #[test]
    fn enum_tables_match_the_discriminants() {
        for i in 0..crate::field::FIELD_EFFECT_COUNT {
            assert_eq!(field_effect(i) as usize, i);
        }
        for i in 0..crate::field::SIDE_EFFECT_COUNT {
            assert_eq!(side_effect(i) as usize, i);
        }
    }

    #[test]
    fn diff_applies_and_reverses() {
        let mut from = State::<2>::default();
        for side in [SideId::One, SideId::Two] {
            for (i, p) in from.side_mut(side).party.iter_mut().enumerate() {
                p.species = crate::dex::SpeciesId(i as u16 + 1);
                p.max_hp = 100;
                p.hp = 100;
            }
            for s in 0..2 {
                from.side_mut(side).slots[s].party_index = Some(s as u8);
            }
        }
        from.slot_mut(SlotRef {
            side: SideId::One,
            slot: 0,
        })
        .boosts[0] = 2;
        let mut to = from.clone();
        let r = SlotRef {
            side: SideId::Two,
            slot: 1,
        };
        to.active_mut(r).unwrap().hp = 0;
        to.slot_mut(r).party_index = None;
        to.slot_mut(r).fainted_occupant = Some(1);
        let me = SlotRef {
            side: SideId::One,
            slot: 0,
        };
        to.slot_mut(me).volatiles.set(
            Volatile::Stall,
            VolatileState {
                active: true,
                duration: 1,
                counter: 3,
                ..VolatileState::NONE
            },
        );
        to.slot_mut(me).boosts[0] = 1;
        to.field[FieldEffect::Gravity as usize] = crate::field::Effect { value: 0, turns: 4 };
        to.turn = 2;
        // A forme change on one Pokémon, a bare type change on another.
        let mega = to.active_mut(me).unwrap();
        let new_forme = mega.forme_as(crate::dex::species::TYRANITAR_MEGA);
        mega.set_forme(new_forme);
        mega.hp = new_forme.hp_after(100, 100);
        to.active_mut(SlotRef {
            side: SideId::One,
            slot: 1,
        })
        .unwrap()
        .types = [crate::dex::Type::Water, crate::dex::Type::None];

        let ins = instructions(&from, &to);
        let mut s = from.clone();
        s.apply(&ins);
        assert_eq!(s, to);
        s.reverse(&ins);
        assert_eq!(s, from);
    }
}
