# 라이브러리 더블 팀 지원 현황 (자동 생성)

`cargo run -p lab-scenario --release --bin lab-library -- --out <이 파일>`로 만든다. `teams/library/doubles/m-c/*/team.json` 28팀을 VGC 형식(`gen9championsvgc2026regmc`, 팀 프리뷰에서 4마리 선출)으로 고정 상대(무효 특성·도구 없음, 매 턴 방어)와 붙인다. 팀마다 선출 `1234`·`3456`·`5612`(모든 멤버가 한 번씩 선두)에서 (1) 배틀 시작 등장 효과, (2) 선두 둘이 방어(없으면 첫 기술)하는 첫 턴, (3) 선두 각각의 기술 4개, (4) 메가진화 가능한 선두의 메가진화를 엔진으로 실행한다. 이유는 엔진이 낸 `TurnError::Unsupported` 문자열 그대로이고, 묶을 때 앞의 `<포켓몬>: `만 뗐다. 벤치 멤버의 등장·교체, 선택하지 않은 기술 조합, 둘째 턴 이후는 검사하지 않는다. 직접 편집하지 않는다.

## 요약

- 팀 28개, 로더 통과 28개.
- 세 선출 모두 시작 + 방어 턴이 실행되는 팀: 0개 (거부 28개).
- 선출 84개(팀 × 3) 중 시작 + 방어 턴이 실행되는 선출: 41개.
- 모든 검사(기술·메가진화 포함) 통과: 0개.

## 거부 이유 (팀 수순)

| 이유 | 팀 수 | 팀 (포켓몬) |
|---|---:|---|
| ability Unburden (["condition.onModifySpe", "onAfterUseItem", "onEnd", "onTakeItem"]) | 11 | crown-tachyon112358 (Sneasler), kickoff-aveornot (Sneasler), kickoff-gerard (Sneasler), kickoff-hollowedhollowed (Sneasler), kickoff-jhinting (Sneasler), kickoff-sableyevgc (Sneasler), kickoff-wolfey (Sneasler), psy-cona (Sneasler), psy-lello (Sneasler), psy-sand-udon (Sneasler), sand-owen (Sneasler) |
| move Helping Hand: callbacks ["condition.onBasePower", "condition.onRestart", "condition.onStart", "onTryHit"] are not implemented | 8 | kickoff-aveornot, kickoff-jhinting, kickoff-karlin22, kickoff-shadezero, kickoff-thosewhoknow, kickoff-wolfey, psy-cona, psy-sand-udon |
| move Last Respects: callbacks ["basePowerCallback"] are not implemented | 8 | coaching-panda, kickoff-balmung, kickoff-gerard, kickoff-sableyevgc, kickoff-thepostmanp, kickoff-thosewhoknow, kickoff-wolfey, psy-cona |
| move Parting Shot: callbacks ["onHit"] are not implemented | 8 | balance-ddee, kickoff-conkledonk, kickoff-gerard, kickoff-hollowedhollowed, kickoff-jhinting, kickoff-joshawott, kickoff-prongs, perish-mrada |
| ability Aerilate (["onBasePower", "onModifyType"]) | 6 | kickoff-gwendolyte (Salamence-Mega), kickoff-jhinting (Salamence-Mega), kickoff-sableyevgc (Salamence-Mega), psy-lello (Salamence-Mega), psy-sand-udon (Salamence-Mega), sand-owen (Salamence-Mega) |
| ability Fairy Aura (["onAnyBasePower", "onStart"]) | 6 | balance-ddee (Floette-Mega), crown-cecil9 (Floette-Mega), kickoff-balmung (Floette-Mega), kickoff-beedrillvgc (Floette-Mega), kickoff-hollowedhollowed (Floette-Mega), kickoff-thosewhoknow (Floette-Mega) |
| ability Flower Veil (["onAllySetStatus", "onAllyTryAddVolatile", "onAllyTryBoost"]) | 6 | balance-ddee (Floette-Eternal), crown-cecil9 (Floette-Eternal), kickoff-balmung (Floette-Eternal), kickoff-beedrillvgc (Floette-Eternal), kickoff-hollowedhollowed (Floette-Eternal), kickoff-thosewhoknow (Floette-Eternal) |
| ability No Guard (["onAnyAccuracy", "onAnyInvulnerability"]) | 6 | balance-ddee (Raichu-Mega-Y), crown-cecil9 (Raichu-Mega-Y), crown-eternalton (Raichu-Mega-Y), crown-tachyon112358 (Raichu-Mega-Y), kickoff-hollowedhollowed (Raichu-Mega-Y), kickoff-jhinting (Raichu-Mega-Y) |
| move Throat Chop: callbacks ["condition.onBeforeMove", "condition.onDisableMove", "condition.onEnd", "condition.onModifyMove", "condition.onStart", "secondaries.onHit", "secondary.onHit"] are not implemented | 6 | balance-ddee, crown-tachyon112358, kickoff-hollowedhollowed, kickoff-jhinting, kickoff-joshawott, kickoff-prongs |
| move U-turn: switching | 6 | crown-cecil9, kickoff-jhinting, kickoff-joshawott, kickoff-sableyevgc, kickoff-shadezero, perish-mrada |
| ability Cursed Body (["onDamagingHit"]) | 5 | coaching-panda (Froslass), kickoff-joshawott (Froslass), kickoff-prongs (Gengar), kickoff-thepostmanp (Froslass), perish-mrada (Gengar) |
| ability Emergency Exit (["onEmergencyExit"]) | 4 | kickoff-shadezero (Golisopod), kickoff-wolfey (Golisopod), psy-cona (Golisopod), psy-nihat (Golisopod) |
| move Stomping Tantrum: callbacks ["basePowerCallback"] are not implemented | 4 | crown-cecil9, crown-ryukeivgc, kickoff-beedrillvgc, kickoff-conkledonk |
| ability Pixilate (["onBasePower", "onModifyType"]) | 3 | crown-eternalton (Sylveon), crown-ryukeivgc (Sylveon), kickoff-wolfey (Gardevoir-Mega) |
| ability Thermal Exchange switch-in handler (["onDamagingHit", "onSetStatus", "onUpdate"]) | 3 | coaching-panda (Baxcalibur), kickoff-gerard (Baxcalibur), psy-nihat (Baxcalibur) |
| move Flip Turn: switching | 3 | kickoff-prongs, kickoff-thosewhoknow, kickoff-wolfey |
| ability Shadow Tag (["onFoeMaybeTrapPokemon", "onFoeTrapPokemon"]) | 2 | kickoff-prongs (Gengar-Mega), perish-mrada (Gengar-Mega) |
| ability Stamina (["onDamagingHit"]) | 2 | kickoff-aveornot (Archaludon), kickoff-gwendolyte (Archaludon) |
| move Double Shock: callbacks ["onTryMove", "self.onHit"] are not implemented | 2 | kickoff-shadezero, kickoff-thepostmanp |
| move Electro Shot: callbacks ["onTryMove"] are not implemented | 2 | kickoff-aveornot, kickoff-gwendolyte |
| move Imprison: callbacks ["condition.onFoeBeforeMove", "condition.onFoeDisableMove", "condition.onStart"] are not implemented | 2 | kickoff-thosewhoknow, psy-lello |
| move Revival Blessing: callbacks ["onTryHit"] are not implemented | 2 | kickoff-shadezero, kickoff-thepostmanp |
| move Spiky Shield: callbacks ["condition.onHit", "condition.onStart", "condition.onTryHit", "onHit", "onPrepareHit"] are not implemented | 2 | kickoff-joshawott, kickoff-thepostmanp |
| ability Hospitality switch-in handler (["onStart"]) | 1 | kickoff-beedrillvgc (Sinistcha) |
| ability Magic Bounce (["onAllyTryHitSide", "onTryHit"]) | 1 | kickoff-karlin22 (Hatterene) |
| ability Poison Touch (["onSourceDamagingHit"]) | 1 | kickoff-gwendolyte (Sneasler) |
| ability Regenerator (["onSwitchOut"]) | 1 | kickoff-conkledonk (Toxapex) |
| ability Weak Armor (["onDamagingHit"]) | 1 | psy-sand-udon (Armarouge) |
| item Eject Button (["onAfterMoveSecondary"]) | 1 | perish-mrada (Rillaboom) |
| item Miracle Berry switch-in handler onUpdate | 1 | crown-ryukeivgc (Rillaboom) |
| move Baneful Bunker: callbacks ["condition.onHit", "condition.onStart", "condition.onTryHit", "onHit", "onPrepareHit"] are not implemented | 1 | kickoff-conkledonk |
| move Belly Drum: callbacks ["onHit"] are not implemented | 1 | kickoff-joshawott |
| move Disable: callbacks ["condition.onBeforeMove", "condition.onDisableMove", "condition.onEnd", "condition.onStart", "onTryHit"] are not implemented | 1 | kickoff-prongs |
| move Feint: a special mechanic | 1 | psy-sand-udon |
| move Fissure: OHKO | 1 | perish-mrada |
| move Gigaton Hammer: can't use twice | 1 | kickoff-prongs |
| move Infestation: volatile partiallytrapped | 1 | kickoff-conkledonk |
| move Psychic Fangs: callbacks ["onTryHit"] are not implemented | 1 | psy-lello |
| move Rage Fist: callbacks ["basePowerCallback"] are not implemented | 1 | kickoff-prongs |
| move Solar Beam: callbacks ["onBasePower", "onTryMove"] are not implemented | 1 | kickoff-balmung |

## 팀별

| 팀 | 라이브러리 상태 | 시작+방어 턴 | 전체 | 거부 검사 수 | 불법 선택 | 첫 거부 |
|---|---|---|---|---:|---:|---|
| balance-ddee | source-complete-sp-unknown | 거부 | 거부 | 13 | 0 | 1234 방어 턴: Floette-Eternal: ability Flower Veil (["onAllySetStatus", "onAllyTryAddVolatile", "onAllyTryBoost"]) |
| coaching-panda | validated | 거부 | 거부 | 11 | 0 | 3456 시작: Baxcalibur: ability Thermal Exchange switch-in handler (["onDamagingHit", "onSetStatus", "onUpdate"]) |
| crown-cecil9 | source-complete-sp-unknown | 거부 | 거부 | 13 | 0 | 1234 U-turn: move U-turn: switching |
| crown-eternalton | source-complete-sp-unknown | 거부 | 거부 | 10 | 0 | 1234 메가진화: Raichu-Mega-Y: ability No Guard (["onAnyAccuracy", "onAnyInvulnerability"]) |
| crown-ryukeivgc | needs-review | 거부 | 거부 | 11 | 0 | 1234 방어 턴: Sylveon: ability Pixilate (["onBasePower", "onModifyType"]) |
| crown-tachyon112358 | source-complete-sp-unknown | 거부 | 거부 | 10 | 0 | 3456 방어 턴: Sneasler: ability Unburden (["condition.onModifySpe", "onAfterUseItem", "onEnd", "onTakeItem"]) |
| kickoff-aveornot | source-complete-sp-unknown | 거부 | 거부 | 19 | 0 | 1234 Helping Hand: move Helping Hand: callbacks ["condition.onBasePower", "condition.onRestart", "condition.onStart", "onTryHit"] are not implemented |
| kickoff-balmung | source-complete-sp-unknown | 거부 | 거부 | 12 | 0 | 1234 방어 턴: Floette-Eternal: ability Flower Veil (["onAllySetStatus", "onAllyTryAddVolatile", "onAllyTryBoost"]) |
| kickoff-beedrillvgc | source-complete-sp-unknown | 거부 | 거부 | 11 | 0 | 3456 시작: Sinistcha: ability Hospitality switch-in handler (["onStart"]) |
| kickoff-conkledonk | source-complete-sp-unknown | 거부 | 거부 | 11 | 0 | 1234 Stomping Tantrum: move Stomping Tantrum: callbacks ["basePowerCallback"] are not implemented |
| kickoff-gerard | source-complete-sp-unknown | 거부 | 거부 | 11 | 0 | 1234 시작: Baxcalibur: ability Thermal Exchange switch-in handler (["onDamagingHit", "onSetStatus", "onUpdate"]) |
| kickoff-gwendolyte | source-complete-sp-unknown | 거부 | 거부 | 19 | 0 | 1234 방어 턴: Sneasler: ability Poison Touch (["onSourceDamagingHit"]) |
| kickoff-hollowedhollowed | source-complete-sp-unknown | 거부 | 거부 | 22 | 0 | 1234 방어 턴: Floette-Eternal: ability Flower Veil (["onAllySetStatus", "onAllyTryAddVolatile", "onAllyTryBoost"]) |
| kickoff-jhinting | source-complete-sp-unknown | 거부 | 거부 | 14 | 0 | 1234 메가진화: Salamence-Mega: ability Aerilate (["onBasePower", "onModifyType"]) |
| kickoff-joshawott | source-complete-sp-unknown | 거부 | 거부 | 21 | 0 | 1234 방어 턴: Froslass: ability Cursed Body (["onDamagingHit"]) |
| kickoff-karlin22 | source-complete-sp-unknown | 거부 | 거부 | 10 | 0 | 1234 Helping Hand: move Helping Hand: callbacks ["condition.onBasePower", "condition.onRestart", "condition.onStart", "onTryHit"] are not implemented |
| kickoff-prongs | source-complete-sp-unknown | 거부 | 거부 | 14 | 0 | 1234 방어 턴: Gengar: ability Cursed Body (["onDamagingHit"]) |
| kickoff-sableyevgc | source-complete-sp-unknown | 거부 | 거부 | 12 | 0 | 1234 메가진화: Salamence-Mega: ability Aerilate (["onBasePower", "onModifyType"]) |
| kickoff-shadezero | source-complete-sp-unknown | 거부 | 거부 | 15 | 0 | 1234 방어 턴: Golisopod: ability Emergency Exit (["onEmergencyExit"]) |
| kickoff-thepostmanp | source-complete-sp-unknown | 거부 | 거부 | 18 | 0 | 1234 방어 턴: Froslass: ability Cursed Body (["onDamagingHit"]) |
| kickoff-thosewhoknow | source-complete-sp-unknown | 거부 | 거부 | 14 | 0 | 1234 방어 턴: Floette-Eternal: ability Flower Veil (["onAllySetStatus", "onAllyTryAddVolatile", "onAllyTryBoost"]) |
| kickoff-wolfey | source-complete-sp-unknown | 거부 | 거부 | 27 | 0 | 1234 방어 턴: Golisopod: ability Emergency Exit (["onEmergencyExit"]) |
| perish-mrada | source-complete-sp-unknown | 거부 | 거부 | 20 | 0 | 1234 방어 턴: Gengar: ability Cursed Body (["onDamagingHit"]) |
| psy-cona | validated | 거부 | 거부 | 21 | 0 | 1234 Helping Hand: move Helping Hand: callbacks ["condition.onBasePower", "condition.onRestart", "condition.onStart", "onTryHit"] are not implemented |
| psy-lello | source-complete-sp-unknown | 거부 | 거부 | 12 | 0 | 1234 Psychic Fangs: move Psychic Fangs: callbacks ["onTryHit"] are not implemented |
| psy-nihat | source-complete-sp-unknown | 거부 | 거부 | 12 | 0 | 1234 시작: Baxcalibur: ability Thermal Exchange switch-in handler (["onDamagingHit", "onSetStatus", "onUpdate"]) |
| psy-sand-udon | validated | 거부 | 거부 | 19 | 0 | 1234 방어 턴: Armarouge: ability Weak Armor (["onDamagingHit"]) |
| sand-owen | validated | 거부 | 거부 | 10 | 0 | 1234 메가진화: Salamence-Mega: ability Aerilate (["onBasePower", "onModifyType"]) |
