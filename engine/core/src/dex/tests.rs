use super::*;

#[test]
fn none_entries_are_index_zero() {
    assert_eq!(SpeciesId::default(), SpeciesId::NONE);
    assert_eq!(SPECIES[0], SpeciesData::NONE);
    assert_eq!(MOVES[0], MoveData::NONE);
    assert_eq!(ITEMS[0], ItemData::NONE);
    assert_eq!(ABILITIES[0], AbilityData::NONE);
    assert_eq!(CONDITIONS[0], ConditionData::NONE);
    assert_eq!(format!("{:?}", ItemId::NONE), "ItemId::NONE");
}

#[test]
fn every_id_round_trips_through_lookup() {
    // Tables must be sorted by id for the binary search; this also catches duplicates.
    for s in SpeciesId::all() {
        assert_eq!(SpeciesId::from_id(s.id()), Some(s));
    }
    for m in MoveId::all() {
        assert_eq!(MoveId::from_id(m.id()), Some(m));
    }
    for i in ItemId::all() {
        assert_eq!(ItemId::from_id(i.id()), Some(i));
    }
    for a in AbilityId::all() {
        assert_eq!(AbilityId::from_id(a.id()), Some(a));
    }
    for c in ConditionId::all() {
        assert_eq!(ConditionId::from_id(c.id()), Some(c));
    }
    assert_eq!(SpeciesId::from_id(""), None);
    assert_eq!(MoveId::from_id("notamove"), None);
}

#[test]
fn names_resolve_in_any_spelling() {
    assert_eq!(
        SpeciesId::from_name("Gardevoir-Mega"),
        Some(species::GARDEVOIR_MEGA)
    );
    assert_eq!(SpeciesId::from_name("Nidoran-F"), Some(species::NIDORAN_F));
    assert_eq!(MoveId::from_name("U-turn"), Some(moves::U_TURN));
    assert_eq!(ItemId::from_name("Focus Sash"), Some(items::FOCUS_SASH));
    assert_eq!(Type::from_name("fairy"), Some(Type::Fairy));
    assert_eq!(Type::from_name(""), None);
}

#[test]
fn species_data() {
    let g = species::GARDEVOIR.data();
    assert_eq!(g.name, "Gardevoir");
    assert_eq!(g.types, [Type::Psychic, Type::Fairy]);
    assert_eq!(g.base_stats, [68, 65, 65, 125, 115, 80]);
    assert_eq!(g.abilities[2], abilities::TELEPATHY);

    let mega = species::GARDEVOIR_MEGA.data();
    assert!(mega.is_mega);
    assert_eq!(mega.base_species, species::GARDEVOIR);
    assert_eq!(mega.abilities[0], abilities::PIXILATE);
    assert_eq!(mega.required_items, &[items::GARDEVOIRITE]);
    assert_eq!(mega.battle_only, &[species::GARDEVOIR]);

    assert_eq!(species::SHEDINJA.data().fixed_max_hp, 1);
    assert_eq!(species::MAROWAK.data().types[1], Type::None);
    assert!(EXCLUDED_SPECIES.iter().any(|&(id, _)| id == "missingno"));
}

#[test]
fn move_data() {
    let h = moves::HYPNOSIS.data();
    assert_eq!(h.accuracy, Some(60));
    assert_eq!(h.status, Status::Sleep);
    assert_eq!(h.category, MoveCategory::Status);
    assert!(h.flags.contains(MoveFlags::REFLECTABLE));

    let g = moves::GRAVITY.data();
    assert_eq!(g.pseudo_weather, conditions::GRAVITY);
    assert_eq!(g.condition_duration, 5);
    assert_eq!(g.accuracy, None);
    assert_eq!(g.target, MoveTarget::All);

    let slider = moves::GRASSY_GLIDE.data();
    assert_eq!(slider.move_type, Type::Grass);
    assert_eq!(slider.category, MoveCategory::Physical);

    let rock_slide = moves::ROCK_SLIDE.data();
    assert_eq!(rock_slide.target, MoveTarget::AllAdjacentFoes);
    assert_eq!(rock_slide.secondaries.len(), 1);
    assert_eq!(rock_slide.secondaries[0].chance, 30);
    assert_eq!(
        rock_slide.secondaries[0].volatile_status,
        conditions::FLINCH
    );

    let fire_fang = moves::FIRE_FANG.data();
    assert_eq!(fire_fang.secondaries.len(), 2);

    assert_eq!(moves::FOLLOW_ME.data().priority, 2);
    assert_eq!(moves::PROTECT.data().target, MoveTarget::User);
    assert_eq!(moves::DRAIN_PUNCH.data().drain, Some(Fraction(1, 2)));
    assert_eq!(
        moves::THOUSAND_ARROWS.data().ignore_immunity,
        IgnoreImmunity::Type(Type::Ground)
    );
    assert_eq!(moves::SHEER_COLD.data().ohko, Ohko::Typed(Type::Ice));
}

#[test]
fn champions_pp_cap() {
    for m in MoveId::all() {
        assert!(m.data().pp <= 20, "{:?} has {} PP", m, m.data().pp);
    }
}

#[test]
fn item_data() {
    let stone = items::GARDEVOIRITE.data();
    assert_eq!(
        stone.mega_stone,
        &[(species::GARDEVOIR, species::GARDEVOIR_MEGA)]
    );
    // Mega Stones decide removability in a callback; Z-Crystals are plain `false`.
    assert!(stone.handlers.contains(&"onTakeItem"));
    assert!(!stone.cannot_be_taken);
    assert!(items::FIRIUM_Z.data().cannot_be_taken);
    assert_eq!(
        items::FIRIUM_Z.data().z_crystal.unwrap().move_type,
        Type::Fire
    );
    assert!(items::SITRUS_BERRY.data().is_berry);
    assert!(items::CHOICE_SCARF.data().is_choice);
    assert_eq!(items::LAGGING_TAIL.data().fractional_priority_tenths, -1);
}

#[test]
fn ability_data() {
    assert!(abilities::MOLD_BREAKER
        .data()
        .handlers
        .contains(&"onModifyMove"));
    assert!(abilities::SHELL_ARMOR.data().cannot_be_crit);
    assert!(abilities::LEVITATE
        .data()
        .flags
        .contains(AbilityFlags::BREAKABLE));
}

#[test]
fn type_chart() {
    assert_eq!(Type::Electric.against(Type::Ground), TypeRelation::Immune);
    assert_eq!(Type::Fire.against(Type::Grass), TypeRelation::Super);
    assert_eq!(Type::Fire.against(Type::Water), TypeRelation::Resist);
    assert_eq!(Type::Normal.against(Type::Normal), TypeRelation::Neutral);
    assert_eq!(Type::Dragon.against(Type::Fairy), TypeRelation::Immune);
    assert_eq!(Type::Fire.against(Type::None), TypeRelation::Neutral);
    assert!(Type::Electric.immunities().contains(TypeImmunities::PAR));
    assert!(Type::Grass.immunities().contains(TypeImmunities::POWDER));
    assert!(Type::Dark.immunities().contains(TypeImmunities::PRANKSTER));
    assert!(!Type::Water.immunities().contains(TypeImmunities::PAR));
}

/// Showdown's `???` (Double Shock): neutral both ways, no immunity, not parsed or listed.
#[test]
fn unknown_type_is_neutral() {
    assert_eq!(Type::Unknown.name(), "???");
    assert!(!Type::ALL.contains(&Type::Unknown));
    assert_eq!(Type::from_name("???"), None);
    assert_eq!(Type::Unknown.immunities(), TypeImmunities::EMPTY);
    for t in Type::ALL {
        assert_eq!(t.against(Type::Unknown), TypeRelation::Neutral, "{t:?}");
        assert_eq!(Type::Unknown.against(t), TypeRelation::Neutral, "{t:?}");
    }
    assert_eq!(Type::Unknown.against(Type::Unknown), TypeRelation::Neutral);
}

#[test]
fn natures() {
    assert_eq!(
        Nature::Adamant.modifiers(),
        (Some(Stat::Atk), Some(Stat::Spa))
    );
    assert_eq!(
        Nature::Brave.modifiers(),
        (Some(Stat::Atk), Some(Stat::Spe))
    );
    assert_eq!(Nature::Hardy.modifiers(), (None, None));
    assert_eq!(Nature::ALL.len(), 25);
}
