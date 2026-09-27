# 이벤트 감사 (A1, 2026-09-27)

작성: WW(웨이브 15 L5). 기준: 엔진 HEAD `9ff1081`, Showdown `9e317a6`(`data/mods/champions` 포함). 읽기 전용 감사이며 엔진 코드는 바꾸지 않았다. 기계 판독용: [`events-audit-2026-09-27.json`](events-audit-2026-09-27.json). 이 문서가 `SHOWDOWN-GAPS.md` §3(2026-09-25 스냅숏)을 대체한다.

## 방법

1. `sim/*.ts`, `data/*.ts`, `data/mods/champions/*.ts`에서 `runEvent`/`singleEvent`/`priorityEvent`/`eachEvent`/`fieldEvent`로 발생하는 이벤트를 전부 뽑았다(`'Modify' + stat`는 ModifyAtk/Def/SpA/SpD/Spe로 펼침, 문서용 `Blah` 제외) — 118종. 여기에 이벤트가 아닌 콜백 6종(`basePowerCallback`·`damageCallback`·`beforeMoveCallback`·`beforeTurnCallback`·`priorityChargeCallback`·`durationCallback`)을 더해 124행.
2. Champions dex(`Dex.mod('champions')`)의 모든 기술·특성·도구, `data/conditions.ts`의 조건 전부, 기술의 `condition`/`self`/`secondary` 핸들러 이름을 기본 이벤트로 묶었다(`Ally`/`Foe`/`Source`/`Any` 접두 제거; `onFieldResidual` 등은 그 자체가 이벤트).
3. 표준 범위 = `engine/scripts/refusals.universe.json`(기술 515·특성 217·도구 166). 표준 반응자가 하나라도 있거나 도달 가능한 조건이 반응하면 그 이벤트는 엔진이 구현해야 한다. 엔진 쪽 근거는 (a) `support.rs`의 고정 핸들러 표(`handler_tables_match_the_dex` 테스트가 dex와 1:1 대조) — 핸들러가 있는 표준 항목 599개 중 표에 없는 것은 메가스톤 81(`onTakeItem`만: `item_can_be_taken`)과 반감열매 18(`resist_berry`)뿐, (b) 엔진 소스에서 그 이벤트를 다루는 위치, (c) 오라클 fixture.
4. 반응자가 전부 비표준이거나 없으면 `unnecessary`이며 근거를 한 줄로 적었다. 근거 없는 '불필요'는 없다.

**한계.** `implemented`는 '표준 반응자가 전부 고정 표에 있고 엔진에 그 이벤트의 처리 위치가 있다'는 뜻이다. 이벤트의 모든 발생 지점이 Showdown과 같은 순서로 호출됨을 증명하지는 않는다 — 그것은 fixture(오라클 913+)와 말뭉치 1,776국면 전수 일치가 경험적으로 확인하는 부분이다. 아래 'fixture 없음' 열은 그 확인이 빠진 반응자다.

## 결과 요약

- 이벤트·콜백 124행: implemented 102, unnecessary 22, refused 0, **missing 0**.
- 이벤트가 아닌 sim 메커니즘 18행: implemented 13, unnecessary 4, **missing 1**(턴 1000 무승부).
- `sim/pokemon.ts` 필드 44묶음: implemented 27, unnecessary 15, refused 2(파티 순서 R9, 시럽 플래그 R10 — 진행 중), missing 0.
- Champions 모드 오버라이드 30행: implemented 28, unnecessary 2(프로토콜 문구만; 기본 함수와 diff로 확인).
- 진행 중(다른 차선, HEAD 기준 미반영): R2·R3·R5·R9·R10·R13·B25·B41–B46 — 해당 행에 보드 id로 표시.

## 새 todo (보드 등록용)

| 제안 id | 내용 | 의존 | 재현 힌트 |
|---|---|---|---|
| A1-t1-turn-limit-tie | Showdown은 턴 1000을 넘기면 무승부(`maybeTriggerEndlessBattleClause`, sim/battle.ts:1839; 클로즈와 무관)인데 엔진은 끝내지 않음 | 없음 | startState `turn: 1000`, 양쪽 아무 행동 → Showdown `tie`; 엔진 `result` Ongoing |
| A1-t2-fixture-gaps | 고유 핸들러가 있는데 오라클 fixture와 라이브러리 사용이 모두 없는 표준 항목의 정확 일치 검증: Upper Hand(onTry 큐 읽기), Illuminate(onTryBoost + 회피 무시), Rattled(위협 AfterBoost), Gluttony(핀치 문턱), Leppa Berry(PP 0 Update), Misty Explosion, Skill Link, Axe Kick·Supercell Slam(crash), Temper Flare, Dragonize(Champions 신규 -ize), Phantom Force·Solar Blade·Sky Attack(2턴), Flail·Water Spout·Power Trip·Infernal Parade(위력 콜백), Big Pecks·White Smoke·Queenly Majesty | 없음(G7과 병행 가능) | `engine/reports/events-audit-2026-09-27.json` `reactors_without_oracle_fixture`; 형제 항목 fixture를 복제해 한 원자씩 |
| A1-t3-support-stale-comments | support.rs 주석이 Baton Pass의 `condition.onCopy`를 '거부/미지원'이라 함(Gastro Acid·Power Trick·Power Shift 행) — 실제로는 `switching::copy_volatile_from`이 구현 | 없음 | support.rs `GASTRO_ACID`, `POWER_TRICK` 주석 |
| A1-t4-refusals-dead-pattern | refusals.py가 사라진 `refused.get_or_insert_with(` 패턴을 아직 검색(B40은 문구만 고침) — 패턴 제거 | B40-refusals-stale-text | scripts/refusals.py `patterns` |

## §3.1 행동 시작·순서

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| BeforeMove | implemented | 9: attract, chillyreception, destinybond, disable, glaiverush, gravity, imprison, taunt … / 조건: choicelock, confusion, flinch, frz, mustrecharge, par, slp | turn/moves.rs, turn/conditions.rs, turn/moves/handlers.rs |  |  |
| BeforeTurn | unnecessary | — | conditions unreachable: commanding (Commander (Tatsugiri) not standard); only Commander's `commanding` (not standard); the `beforeTurnMove` action itself (Counter, Mirror Coat) is implemented |  |  |
| ChargeMove | unnecessary | — | only non-standard reactors: powerherb |  |  |
| DeductPP | implemented | 1: pressure | turn/moves.rs deduct PP (Pressure, `mustpressure`) |  |  |
| FractionalPriority | implemented | 2: quickclaw, quickdraw | turn/order.rs `fractional_priority_tenths`, items.rs (Quick Claw), abilities.rs (Quick Draw) |  |  |
| LockMove | implemented | — / 조건: lockedmove, twoturnmove | turn/lock.rs |  |  |
| ModifyMove | implemented | 28: battlebond, beatup, blizzard, choicescarf, curse, expandingforce, gravity, growth … / 조건: frz | turn/moves.rs, turn/moves/handlers.rs, turn/abilities.rs | hurricane, illuminate, skilllink |  |
| ModifyPriority | implemented | 3: galewings, grassyglide, prankster | turn/moves.rs, turn/order.rs, turn/queue.rs |  |  |
| ModifyType | implemented | 10: aerilate, aurawheel, dragonize, electrify, liquidvoice, pixilate, ragingbull, refrigerate … | turn/abilities.rs, turn/moves.rs, turn/moves/ability_hooks.rs | dragonize |  |
| MoveAborted | implemented | 2: charge, destinybond / 조건: twoturnmove | turn/moves.rs, turn/abilities.rs, volatile.rs |  |  |
| OverrideAction | implemented | 1: encore | turn/battle.rs `encore_change_action`, lock.rs (Champions Encore) |  | B43-encore-after-you-callback |
| PrepareHit | implemented | 13: allyswitch, banefulbunker, destinybond, detect, endure, fling, kingsshield, libero … | turn/moves.rs, turn/moves/handlers.rs, volatile.rs | detect |  |
| SemiLockMove | unnecessary | — | no handler in the dex |  |  |
| StallMove | implemented | — / 조건: stall | turn/moves.rs |  |  |
| Try | implemented | 30: aurawheel, auroraveil, belch, clangoroussoul, comeuppance, counter, fakeout, firstimpression … | turn/moves.rs, turn/moves/handlers.rs, turn/conditions.rs | upperhand |  |
| TryMove | implemented | 17: armortail, bounce, burnup, damp, dig, dive, doubleshock, electroshot … | turn/moves/handlers.rs, turn/moves.rs, turn/moves/ability_hooks.rs (도달 불가 조건: desolateland, primordialsea) | phantomforce, queenlymajesty, skyattack, solarblade |  |
| UseMoveMessage | unnecessary | — | only non-standard reactors: magnitude; only Magnitude (Past); log only |  |  |
| beforeMoveCallback | implemented | 1: focuspunch | volatile.rs, turn/moves.rs, turn/moves/handlers.rs |  |  |
| beforeTurnCallback | implemented | 2: counter, mirrorcoat | turn/moves/handlers.rs, turn/moves.rs, volatile.rs |  |  |
| priorityChargeCallback | implemented | 3: beakblast, chillyreception, focuspunch | volatile.rs, turn/moves/handlers.rs, turn/moves.rs |  |  |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| beforeTurnMove / priorityChargeMove actions (Counter, Mirror Coat, Metal Burst; Focus Punch, Beak Blast, Chilly Reception) | implemented | turn/queue.rs, moves/handlers.rs |  |
| Instruct / After You / Quash queue edits, called-move callback actions | implemented | turn/queue.rs, order.rs | B42-instructed-callback-order, B43-encore-after-you-callback |

## §3.2 대상 결정

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| ModifyTarget | implemented | 2: comeuppance, metalburst | turn/moves.rs, turn/moves/handlers.rs |  |  |
| RedirectTarget | implemented | 5: counter, followme, lightningrod, mirrorcoat, ragepowder | turn/moves.rs `get_target` + abilities.rs (Lightning Rod / Storm Drain), `Slot::ability_order` ties (R4) |  | B45-pinned-ability-order |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| getMoveTargets smartTarget (Dragon Darts) | implemented | turn/moves.rs `smart_targets` |  |
| tracksTarget / originalTarget (Snipe Shot, Stalwart, Propeller Tail) through Ally Switch | implemented | turn/moves.rs `get_target` (R7) | B46-original-target-rebuilt-actions |
| spread modifier 0.75 lost when one target remains | implemented | turn/moves.rs |  |

## §3.3 명중 단계

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| Accuracy | implemented | 4: glaiverush, lockon, minimize, noguard | turn/moves.rs, turn/moves/ability_hooks.rs, turn/moves/handlers.rs |  |  |
| HitProtect | implemented | 2: piercingdrill, unseenfist | turn/abilities.rs (Unseen Fist Champions / Piercing Drill) + moves.rs `bypassProtect` 0.25 |  |  |
| Immunity | implemented | 11: dig, dive, icebody, magmaarmor, magnetrise, oblivious, overcoat, sandforce … / 조건: sunnyday | turn/battle.rs, turn/switching.rs, volatile.rs (도달 불가 조건: desolateland) |  |  |
| Invulnerability | implemented | 6: bounce, dig, dive, fly, lockon, noguard | turn/moves/handlers.rs, volatile.rs, turn/moves.rs |  |  |
| ModifyAccuracy | implemented | 9: brightpowder, compoundeyes, gravity, hustle, sandveil, snowcloak, tangledfeet, widelens … | turn/abilities.rs, turn/items.rs, turn/moves.rs |  |  |
| ModifyBoost | implemented | 1: unaware | turn/battle.rs, volatile.rs, turn/moves.rs |  |  |
| NegateImmunity | unnecessary | — | only non-standard reactors: foresight, miracleeye |  |  |
| TryHit | implemented | 49: banefulbunker, brickbreak, bulletproof, clangoroussoul, curse, disable, dryskin, eartheater … | turn/moves.rs, turn/moves/handlers.rs, turn/moves/ability_hooks.rs |  |  |
| TryHitField | unnecessary | — | no handler in the dex (Showdown's default result is used) |  |  |
| TryHitSide | implemented | 3: magicbounce, sapsipper, soundproof | turn/moves.rs |  |  |
| TryImmunity | implemented | 7: attract, endeavor, leechseed, octolock, switcheroo, trick, worryseed | turn/moves/handlers.rs, turn/moves.rs |  |  |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| OHKO accuracy rule, Blunder Policy | implemented | turn/moves.rs `accuracy_check`, items.rs `blunder_policy` |  |
| multiaccuracy (Triple Axel) | implemented | turn/moves.rs hit loop |  |
| StealBoosts (Spectral Thief) | unnecessary | Spectral Thief not standard (`steals_boosts` stays refused in support.rs) |  |

## §3.4 타격 루프

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| AfterBoost | implemented | 2: opportunist, rattled | turn/battle.rs, turn/items.rs, turn/abilities.rs | rattled |  |
| AfterEachBoost | implemented | 2: competitive, defiant | turn/battle.rs |  |  |
| AfterFaint | implemented | 3: battlebond, eelevate, moxie | turn/battle.rs, turn/switching.rs, turn/abilities.rs |  |  |
| AfterHit | implemented | 8: ceaselessedge, covet, icespinner, knockoff, mortalspin, rapidspin, stoneaxe, thief | turn/moves.rs, turn/moves/handlers.rs |  |  |
| AfterMove | implemented | 6: beakblast, charge, opportunist, sparklingaria, spitup, whiteherb / 조건: lockedmove | turn/moves.rs, turn/moves/handlers.rs, turn/items.rs |  |  |
| AfterMoveSecondary | implemented | 4: berserk, ejectbutton, pickpocket, redcard / 조건: frz | turn/items.rs, turn/abilities.rs, turn/moves.rs |  | R5-future-move-edges |
| AfterMoveSecondarySelf | implemented | 4: fellstinger, lifeorb, magician, shellbell | turn/moves.rs, turn/abilities.rs, turn/items.rs |  |  |
| AfterSetStatus | implemented | 2: lumberry, synchronize | turn/battle.rs, turn/update.rs, turn/abilities.rs |  |  |
| AfterSubDamage | implemented | 8: airballoon, ceaselessedge, icespinner, mortalspin, rapidspin, shellsidearm, steelroller, stoneaxe | turn/moves/handlers.rs, turn/items.rs, turn/moves.rs |  |  |
| Attract | unnecessary | — | only non-standard reactors: destinyknot |  |  |
| BasePower | implemented | 58: aerilate, analytic, barbbarrage, blackbelt, blackglasses, bounce, charcoal, charge … / 조건: gem | turn/abilities.rs, turn/moves/handlers.rs, turn/moves.rs (도달 불가 조건: rolloutstorage) | blackbelt, charcoal, dragonfang, dragonize, fairyfeather, hardstone … |  |
| BeforeFaint | unnecessary | — | no handler in the dex |  |  |
| ChangeBoost | implemented | 3: contrary, ripen, simple | turn/battle.rs, turn/abilities.rs |  |  |
| CriticalHit | implemented | 1: disguise | turn/forme.rs, turn/moves.rs, turn/switching.rs |  |  |
| Damage | implemented | 12: berserk, damp, disguise, endure, focusband, focussash, gluttony, heatproof … | turn/moves.rs, turn/battle.rs, turn/forme.rs | gluttony |  |
| DamagingHit | implemented | 28: aftermath, airballoon, counter, cursedbody, cutecharm, effectspore, electromorphosis, flamebody … / 조건: frz | turn/moves.rs, turn/moves/ability_hooks.rs, turn/abilities.rs | rattled |  |
| DragOut | implemented | 3: guarddog, ingrain, suctioncups | turn/moves.rs, volatile.rs, turn/conditions.rs (도달 불가 조건: commanded, commanding, dynamax) |  |  |
| Drive | unnecessary | — | no handler (Techno Blast / Genesect not standard) |  |  |
| Effectiveness | implemented | 4: disguise, flyingpress, freezedry, ironball | turn/forme.rs, turn/switching.rs, turn/moves/handlers.rs (도달 불가 조건: deltastream) |  |  |
| EmergencyExit | implemented | 1: emergencyexit | turn/moves.rs, turn/switching.rs |  | R3-emergency-exit-replacement |
| Faint | implemented | 3: destinybond, illusion, receiver | turn/battle.rs, turn/abilities.rs, turn/conditions.rs |  |  |
| Flinch | implemented | 1: steadfast | volatile.rs, instruction.rs, turn/abilities.rs |  |  |
| Heal | unnecessary | — | no handler in the dex (the heal itself is implemented: `Battle::heal`) |  |  |
| Hit | implemented | 78: acupressure, afteryou, alluringvoice, allyswitch, angerpoint, banefulbunker, batonpass, beakblast … | turn/moves.rs, turn/moves/handlers.rs, turn/items.rs | detect | B41-copycat-uturn-switch-flag |
| HitField | implemented | 4: courtchange, haze, perishsong, teatime | turn/moves.rs, volatile.rs, turn/moves/handlers.rs |  |  |
| HitSide | implemented | 3: magneticflux, quickguard, wideguard | turn/moves.rs, turn/moves/handlers.rs |  |  |
| Memory | unnecessary | — | no handler (Multi-Attack / Silvally not standard) |  |  |
| ModifyAtk | implemented | 16: blaze, firemane, flashfire, guts, heatproof, hugepower, hustle, lightball … | turn/abilities.rs, turn/switching.rs, turn/moves.rs |  |  |
| ModifyCritRatio | implemented | 6: dragoncheer, focusenergy, leek, merciless, scopelens, superluck | volatile.rs, turn/abilities.rs, turn/items.rs |  |  |
| ModifyDamage | implemented | 38: auraguard, auroraveil, babiriberry, chartiberry, chilanberry, chopleberry, cobaberry, colburberry … | turn/abilities.rs, turn/items.rs, turn/moves.rs (도달 불가 조건: dynamax) | babiriberry, chartiberry, chopleberry, cobaberry, habanberry, kasibberry … |  |
| ModifyDef | implemented | 3: furcoat, grasspelt, marvelscale / 조건: snowscape | turn/abilities.rs, turn/items.rs, turn/switching.rs |  |  |
| ModifySTAB | implemented | 1: adaptability | turn/abilities.rs |  |  |
| ModifySecondaries | implemented | 2: parentalbond, shielddust | turn/moves.rs, turn/moves/ability_hooks.rs, turn/items.rs |  |  |
| ModifySpA | implemented | 15: blaze, firemane, flashfire, heatproof, lightball, minus, overgrow, plus … | turn/abilities.rs, turn/switching.rs, turn/items.rs |  |  |
| ModifySpD | implemented | — / 조건: sandstorm | turn/abilities.rs, turn/switching.rs, turn/items.rs |  |  |
| ModifyWeight | implemented | 2: heavymetal, lightmetal | state.rs, turn/items.rs, turn/moves/handlers.rs |  |  |
| MoveFail | implemented | 4: axekick, highjumpkick, steelbeam, supercellslam | turn/moves.rs, turn/moves/handlers.rs | axekick, supercellslam |  |
| SetStatus | implemented | 14: electricterrain, flowerveil, immunity, insomnia, leafguard, limber, mistyterrain, purifyingsalt … | turn/battle.rs, turn/switching.rs, turn/abilities.rs |  |  |
| TryAddVolatile | implemented | 13: aromaveil, electricterrain, flowerveil, focuspunch, innerfocus, insomnia, leafguard, mistyterrain … | turn/battle.rs, turn/switching.rs, turn/forme.rs (도달 불가 조건: dynamax) |  |  |
| TryBoost | implemented | 13: bigpecks, clearbody, flowerveil, guarddog, hypercutter, illuminate, innerfocus, keeneye … | turn/abilities.rs, turn/battle.rs, turn/items.rs | bigpecks, illuminate, whitesmoke |  |
| TryHeal | implemented | 3: bigroot, liquidooze, ripen | turn/battle.rs, turn/abilities.rs, turn/items.rs |  |  |
| TryPrimaryHit | implemented | 2: normalgem, substitute | turn/moves.rs, turn/moves/ability_hooks.rs, volatile.rs |  |  |
| Type | implemented | 1: roost | turn/conditions.rs Roost filter (`Volatile::Roost`) (도달 불가 조건: arceus, silvally) |  |  |
| WeatherModifyDamage | implemented | 1: megasol / 조건: raindance, sunnyday | turn/moves.rs damage chain + items.rs (Mega Sol's sun view) (도달 불가 조건: desolateland, primordialsea) |  | B25-mega-sol-duration |
| basePowerCallback | implemented | 29: acrobatics, assurance, avalanche, beatup, electroball, eruption, flail, grassknot … | turn/moves/handlers.rs, turn/moves.rs | flail, infernalparade, powertrip, temperflare, waterspout |  |
| damageCallback | implemented | 7: comeuppance, counter, endeavor, finalgambit, metalburst, mirrorcoat, superfang | turn/moves.rs, turn/items.rs, turn/moves/handlers.rs |  |  |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| Champions modifyDamage (bypassProtect 0.25 for Unseen Fist / Piercing Drill; 4x/0.25x messages only) | implemented | turn/moves.rs damage |  |
| Champions spreadMoveHit: AfterHit even if the user fainted | implemented | turn/moves.rs (`knock-off-fainted-user`) |  |
| Stellar / Tera STAB, Z / Max bypassProtect | unnecessary | Terastallization, Z-Moves and Dynamax are off in the ruleset |  |

## §3.5 교체·등장·도구

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| AfterMega | implemented | 2: opportunist, whiteherb | turn/items.rs, turn/mega.rs, turn/abilities.rs |  |  |
| AfterTakeItem | unnecessary | — | no handler in the dex |  |  |
| AfterTerastallization | implemented | 1: opportunist | turn/switching.rs |  |  |
| AfterUseItem | implemented | 2: symbiosis, unburden | turn/abilities.rs, turn/items.rs, turn/update.rs |  | B44-fling-used-item-pickup |
| BattleStart | unnecessary | — | conditions unreachable: zacian (Zacian not standard); zamazenta (Zamazenta not standard); only the Zacian / Zamazenta species conditions (not standard) |  |  |
| BeforeSwitchIn | implemented | 1: illusion | turn/switching.rs, turn/abilities.rs, state.rs |  |  |
| BeforeSwitchOut | unnecessary | — | only non-standard reactors: pursuit; conditions unreachable: dynamax (Dynamax is off in the ruleset); only Pursuit (Past) and Dynamax (off) |  |  |
| CheckShow | unnecessary | — | Champions removes Natural Cure's `onCheckShow` (`data/mods/champions/abilities.ts`); log only |  |  |
| Copy | implemented | 3: gastroacid, powershift, powertrick | turn/switching.rs `copy_volatile_from` (Power Trick / Power Shift stored-stat swap, Gastro Acid on cantsuppress) |  |  |
| Eat | implemented | 28: aspearberry, babiriberry, chartiberry, cheriberry, chestoberry, chilanberry, chopleberry, cobaberry … | turn/update.rs, turn/moves/handlers.rs, turn/items.rs | aspearberry, babiriberry, chartiberry, chestoberry, chopleberry, cobaberry … |  |
| EatItem | implemented | 3: cheekpouch, cudchew, ripen | turn/moves/handlers.rs, turn/abilities.rs, turn/battle.rs |  |  |
| End | implemented | 25: attract, charge, cloudnine, disable, encore, flashfire, illusion, magnetrise … / 조건: confusion, futuremove, lockedmove, partiallytrapped, twoturnmove | turn/switching.rs, turn/abilities.rs, turn/battle.rs (도달 불가 조건: dynamax) |  |  |
| ModifySpecies | unnecessary | — | no handler in the dex |  |  |
| Restart | implemented | 8: allyswitch, charge, helpinghand, minimize, powershift, powertrick, smackdown, stockpile / 조건: lockedmove, stall | turn/battle.rs, volatile.rs, turn/conditions.rs |  |  |
| SetAbility | unnecessary | — | only non-standard reactors: abilityshield |  |  |
| Start | implemented | 80: airballoon, allyswitch, anticipation, aquaring, attract, banefulbunker, beakblast, charge … / 조건: brn, choicelock, confusion, frz, futuremove, lockedmove, mustrecharge, par, partiallytrapped, psn, slp, stall, tox, trapped, twoturnmove | turn/switching.rs, turn/abilities.rs, turn/items.rs (도달 불가 조건: commanded, dynamax, healreplacement) | anticipation, forewarn, gluttony |  |
| Swap | implemented | 1: healingwish | turn/moves/handlers.rs (Ally Switch) + conditions.rs (Healing Wish `onSwitchIn` → `onSwap`) |  |  |
| SwitchIn | implemented | 10: cloudnine, healingwish, imposter, opportunist, spikes, stealthrock, stickyweb, toxicspikes … / 조건: tox | turn/switching.rs `run_switch_in` (fieldEvent: hazards, slot conditions, ability/item start, onAnySwitchIn) (도달 불가 조건: healreplacement) |  | R2-hazard-effect-order |
| SwitchOut | implemented | 3: naturalcure, regenerator, zerotohero | turn/switching.rs, turn/forme.rs, turn/abilities.rs |  |  |
| TakeItem | implemented | 83: abomasite, absolite, absolitez, aerodactylite, aggronite, alakazite, altarianite, ampharosite … | turn/moves/handlers.rs, turn/abilities.rs, turn/battle.rs |  |  |
| TryEatItem | implemented | 5: berserk, oranberry, ripen, sitrusberry, unnerve | turn/update.rs, turn/switching.rs, turn/abilities.rs |  |  |
| Update | implemented | 25: aspearberry, attract, cheriberry, chestoberry, disguise, fling, immunity, insomnia … | turn/update.rs `update_event` (11 call sites: mod.rs, moves.rs hit loop, residual.rs, switching.rs) | aspearberry, chestoberry, leppaberry, pechaberry, rawstberry |  |
| Use | implemented | 1: whiteherb | turn/items.rs (White Herb `onUse`) |  |  |
| UseItem | unnecessary | — | only non-standard reactors: ejectpack |  |  |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| formeChange / setSpecies / Transform / Illusion | implemented | turn/forme.rs, transform.rs (EE1, EE2) |  |
| simultaneous switch-in order (speed sort, one draw per event) | implemented | turn/switching.rs `run_switch_in` (B30) |  |

## §3.6 턴 종료

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| DisableMove | implemented | 9: disable, encore, fakeout, firstimpression, gravity, imprison, taunt, throatchop … / 조건: choicelock | turn/conditions.rs, turn/items.rs, turn/mod.rs |  |  |
| FieldResidual | implemented | — / 조건: raindance, sandstorm, snowscape, sunnyday | turn/residual.rs (도달 불가 조건: deltastream, desolateland, hail, primordialsea) |  |  |
| FoeMaybeTrapPokemon | implemented | 1: shadowtag | turn/abilities.rs |  |  |
| MaybeTrapPokemon | implemented | 2: runaway, shedshell | turn/abilities.rs |  |  |
| ModifySpe | implemented | 10: chlorophyll, choicescarf, ironball, quickfeet, sandrush, slushrush, surgesurfer, swiftswim … / 조건: par | turn/switching.rs, turn/order.rs, turn/abilities.rs | slushrush, swiftswim |  |
| Residual | implemented | 24: aquaring, cudchew, curse, encore, grassyterrain, harvest, healer, hungerswitch … / 조건: brn, futuremove, lockedmove, partiallytrapped, psn, tox | turn/residual.rs (handler list sorted by order/priority/speed/sub-order) (도달 불가 조건: dynamax) |  | R5-future-move-edges, B44-fling-used-item-pickup |
| TrapPokemon | implemented | 7: fairylock, ingrain, noretreat, octolock, runaway, shadowtag, shedshell / 조건: partiallytrapped, trapped | volatile.rs, turn/abilities.rs, turn/conditions.rs (도달 불가 조건: commanded, commanding) |  |  |
| Weather | implemented | 4: dryskin, icebody, raindish, solarpower / 조건: sandstorm | turn/moves/handlers.rs, turn/battle.rs, turn/residual.rs (도달 불가 조건: hail) |  |  |
| durationCallback | implemented | 13: auroraveil, electricterrain, grassyterrain, gravity, lightscreen, magicroom, mistyterrain, psychicterrain … / 조건: partiallytrapped, raindance, sandstorm, snowscape, sunnyday | volatile.rs, turn/conditions.rs (도달 불가 조건: hail) |  |  |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| endTurn bookkeeping (moveThisTurnResult, newlySwitched, statsRaised/LoweredThisTurn, hurtThisTurn, attackedBy, faintedThisTurn/LastTurn, usedItemThisTurn) | implemented | turn/history.rs, state.rs `SlotHistory` / `SideHistory` | B44-fling-used-item-pickup |
| Endless Battle Clause (staleness: Leppa, Recycle, Harvest, Pickup) | unnecessary | not in the M-C formats' rulesets: `Flat Rules` (data/mods/champions/rulesets.ts) has no `Endless Battle Clause`, nor does `gen9championsdoublescustomgame`; `maybeTriggerEndlessBattleClause` returns before it after `!ruleTable.has('endlessbattleclause')` |  |
| turn limit: tie after turn 1000 (`maybeTriggerEndlessBattleClause`, independent of the clause) | missing |  |  |
| knownType / apparentType (type reveal, MaybeTrapPokemon's `!knownType` branch) | unnecessary | information only: they set `maybeTrapped` / request flags; the full-information engine uses `trapped`, checked against the oracle's `trapped.cjs` fixtures |  |

## §3.7 필드·진영

| 이벤트 | 상태 | 표준 반응자 | 엔진 위치 / 불필요 근거 | fixture 없음 | 진행 중 |
|---|---|---|---|---|---|
| FieldEnd | implemented | 8: electricterrain, grassyterrain, gravity, magicroom, mistyterrain, psychicterrain, trickroom, wonderroom / 조건: raindance, sandstorm, snowscape, sunnyday | turn/moves.rs (도달 불가 조건: deltastream, desolateland, hail, primordialsea) |  |  |
| FieldRestart | implemented | 3: magicroom, trickroom, wonderroom | turn/moves.rs |  |  |
| FieldStart | implemented | 9: electricterrain, fairylock, grassyterrain, gravity, magicroom, mistyterrain, psychicterrain, trickroom … / 조건: raindance, sandstorm, snowscape, sunnyday | turn/items.rs, turn/moves.rs (도달 불가 조건: deltastream, desolateland, hail, primordialsea) |  |  |
| PseudoWeatherChange | unnecessary | — | only non-standard reactors: roomservice |  |  |
| SetWeather | unnecessary | — | only non-standard reactors: deltastream, desolateland, primordialsea |  |  |
| SideConditionStart | unnecessary | — | only non-standard reactors: windpower, windrider |  |  |
| SideEnd | implemented | 5: auroraveil, lightscreen, reflect, safeguard, tailwind | turn/switching.rs |  |  |
| SideRestart | implemented | 2: spikes, toxicspikes | turn/conditions.rs, turn/moves.rs |  |  |
| SideStart | implemented | 11: auroraveil, lightscreen, quickguard, reflect, safeguard, spikes, stealthrock, stickyweb … |  |  |  |
| TerrainChange | implemented | 5: electricseed, grassyseed, mimicry, mistyseed, psychicseed | turn/field_events.rs, turn/switching.rs, turn/forme.rs |  |  |
| TryTerrain | unnecessary | — | no handler in the dex |  |  |
| WeatherChange | implemented | 1: forecast | turn/switching.rs, turn/field_events.rs, turn/forme.rs |  |  |

| 메커니즘(이벤트 아님) | 상태 | 엔진 위치 / 근거 | 진행 중 |
|---|---|---|---|
| Magic Room / Wonder Room / Fairy Lock, weather rocks, Terrain Extender, Utility Umbrella | implemented | turn/moves.rs, battle.rs `weather_for` |  |

## §3.8 `sim/pokemon.ts` 상태 필드

필드 목록은 `class Pokemon`의 선언 전부(47–307행). 읽는 곳은 `data/`·`sim/`에서 `.필드` 참조로 셌다.

| 필드 | 상태 | 엔진 표현 / 불필요 근거 | 진행 중 |
|---|---|---|---|
| set, name, fullname, details, pokeball, happiness(Return etc. not standard) | unnecessary | identity / log; happiness only read by Return, Frustration, Pika Papow, Veevee Volley (not standard) |  |
| level, gender | implemented | `Pokemon::level`, `Pokemon::gender` (undecided gender refused) | R13-attract-gender |
| baseSpecies, species, speciesState | implemented | `Pokemon::species` + `transformed.species`; speciesState has no reader (no species handlers) |  |
| baseMoveSlots, moveSlots, ppUps | implemented | `Pokemon::moves`, `TransformBase::moves`; ppUps always 0 in Champions (`calculatePP` in `state::champions_max_pp`); the only other ppUps reader is Zacian's Behemoth Blade swap (not standard) |  |
| hpType, hpPower, baseHpType, baseHpPower | unnecessary | Hidden Power is not standard; Champions `clearVolatile` only copies them |  |
| position | implemented | `Slot` index; Ally Switch swaps slots |  |
| side.pokemon order (party order after switches) | refused | Beat Up with benched allies of different power refuses (`beat_up_powers`) | R9-beat-up-order |
| status, statusState (time, stage, startTime) | implemented | `Pokemon::status`, `status_turns` (Champions slp `sample([2,3,3])`, frz 3 turns / 1-in-4 thaw) |  |
| volatiles (+ effectState) | implemented | `Slot::volatiles` (`volatile.rs`) |  |
| showCure | unnecessary | protocol message flag only |  |
| baseStoredStats, storedStats | implemented | `Pokemon::stats` + stored-stat swaps (Power Trick / Shift, Guard / Power Split, Speed Swap) in conditions.rs; baseStoredStats only read by rulesets | R5-future-move-edges |
| boosts | implemented | `Slot::boosts` |  |
| baseAbility, ability, abilityState | implemented | `Pokemon::ability`, `base_ability`; abilityState.effectOrder = `Slot::ability_order` (R4) | B45-pinned-ability-order |
| item, itemState, lastItem | implemented | `Pokemon::item`, `last_item`; itemState via volatiles (Micle, Metronome counter) |  |
| usedItemThisTurn, ateBerry | implemented | `SlotHistory::used_item_this_turn`, `SideHistory::ate_berry` | B44-fling-used-item-pickup |
| itemKnockedOff | unnecessary | Gens 3-4 only |  |
| trapped, maybeTrapped | implemented | `turn::trapped` + oracle `trapped.cjs` fixtures; maybeTrapped is request information |  |
| maybeDisabled, maybeLocked | unnecessary | request information (Imprison's hidden disable is a rejected choice, i.e. not legal) |  |
| illusion, transformed | implemented | `Pokemon::illusion`, `Pokemon::transformed` (EE1, EE2) |  |
| maxhp, baseMaxhp, hp, fainted | implemented | `Pokemon::hp`, `max_hp` (baseMaxhp = max_hp without Dynamax), `Status::Fainted`, `Slot::fainted_occupant` |  |
| faintQueued, subFainted | unnecessary | internal to faint processing (engine faint queue in `Battle`); subFainted is Gen 1 only |  |
| formeRegression | implemented | Champions drops the Mega one (state.rs doc); Power Construct's is modelled in battle.rs |  |
| types, addedType, baseTypes | implemented | `Pokemon::types` + `Volatile::AddedType` (Trick-or-Treat, Forest's Curse); baseTypes is Tera only |  |
| knownType, apparentType | unnecessary | information only (see §3.6 row) |  |
| switchFlag, forceSwitchFlag, skipBeforeSwitchOutEventFlag | implemented | `Slot::switch_flag` (`SwitchFlag`), forced switches, `skip_before_switch_out` | B41-copycat-uturn-switch-flag, R3-emergency-exit-replacement |
| draggedIn, beingCalledBack | implemented | event speed of dragged-in Pokémon (`Battle::event_speed`); beingCalledBack for Eject Button / Illusion |  |
| newlySwitched | implemented | `SlotHistory::newly_switched` |  |
| lastMove, lastMoveTargetLoc, lastMoveUsed | implemented | `Slot::last_move`, `last_move_target_loc`; lastMoveUsed only read by Conversion 2 (not standard) |  |
| lastMoveEncore | unnecessary | Gen 2 only |  |
| moveThisTurn, moveThisTurnResult, moveLastTurnResult | implemented | `SlotHistory` + queue state |  |
| statsRaisedThisTurn, statsLoweredThisTurn | implemented | `SlotHistory` |  |
| hurtThisTurn, lastDamage, attackedBy, timesAttacked | implemented | `SlotHistory::hurt_this_turn`, `last_damaged_by`, `damaged_by_this_turn`, `times_attacked` (Champions resets on switch) |  |
| isActive, activeTurns, activeMoveActions | implemented | `Slot::party_index`, `Slot::move_actions`; activeTurns > 0 is derived from the slot history's `newly_switched` (battle.rs; Speed Boost, Slow Start, Stakeout, Taunt) |  |
| previouslySwitchedIn | unnecessary | written by switchIn, read by nothing in data/ or sim/ |  |
| truantTurn | unnecessary | Truant not standard |  |
| syrupTriggered | refused | Supersweet Syrup after the battle start refuses (`once_per_battle`) | R10-once-per-battle-flags |
| bondTriggered, heroMessageDisplayed, swordBoost, shieldBoost | unnecessary | Battle Bond only on Past formes, Zero to Hero's message is log-only, Intrepid Sword / Dauntless Shield not standard (R10 note) |  |
| stellarBoostedTypes, canTerastallize, teraType, terastallized, canUltraBurst, canGigantamax, dynamaxLevel, gigantamax | unnecessary | gimmicks off in the ruleset (kept as structure: `GimmickSet`, `DynamaxState`) |  |
| canMegaEvo, canMegaEvoX, canMegaEvoY | implemented | `turn/mega.rs` (Champions `canMegaEvo`: `item.megaStone[species.name]`; X/Y are separate stones) |  |
| isStarted, duringMove | unnecessary | internal: start handlers run once per switch-in in `run_switch_in`; duringMove has no reader in data/ |  |
| weighthg | implemented | `Pokemon::weight_hg` (Autotomize) |  |
| speed | implemented | `Battle::event_speed` / `update_speed` (B30) |  |
| staleness, pendingStaleness, volatileStaleness | unnecessary | only Endless Battle Clause reads them; not in the M-C rulesets (§3.6 row) |  |
| modifiedStats, modifyStat, recalculateStats, m | unnecessary | Gen 1 / Stadium / mods only |  |

## §3.9 콜백 우주와 Champions 모드 오버라이드

- 핸들러가 있는 표준 항목 599개(기술·특성·도구): 전부 `support.rs` 고정 표(`MOVES_WITH_HANDLERS`·`ITEMS_WITH_HANDLERS`·`ABILITIES_WITH_HANDLERS`·`TYPE_BOOST_ITEMS`·`switching::START_HANDLERS`) 또는 일반 규칙(메가스톤 81, 반감열매 18)에 있다. 표의 핸들러 목록은 테스트가 dex와 정확히 대조하므로 dex가 바뀌면 실패한다. 표에 없는 핸들러가 붙은 항목은 `move_unsupported`/`check_state`가 거부한다(도달 가능한 거부는 `REFUSALS.md`의 12개, 전부 진행 중 보드).
- 표준 종의 `species` 핸들러: 없음(`check_state`가 있으면 거부).
- `data/conditions.ts` 조건 35개 중 도달 가능 21개(상태 7·휘발 11·날씨 4 — brn par slp frz psn tox confusion flinch trapped partiallytrapped lockedmove twoturnmove choicelock mustrecharge futuremove stall gem raindance sunnyday sandstorm snowscape), 도달 불가 13개(근거는 JSON `unreachable_conditions`), 핸들러 없음 1(trapper).

| Champions 오버라이드 | 상태 | 엔진 |
|---|---|---|
| scripts.ts init: PP capped at 20 | implemented | `state::champions_max_pp` (test: every move ≤ 20) |
| scripts.ts statModify: SP formula (level 50, +75 HP / +20) | implemented | `stats.rs` `champions_stats` |
| scripts.ts calculatePP: (pp/5+1)*4 | implemented | `state::champions_max_pp` |
| scripts.ts getActionSpeed: Trick Room negates (no 10000 − speed) | implemented | `Battle::trick_room_speed` (V6, fixtures f-trick-room-*) |
| scripts.ts formeChange: no Mega formeRegression | implemented | state.rs `Pokemon::species` doc, forme.rs |
| scripts.ts clearVolatile: resets timesAttacked | implemented | `SlotHistory` reset on switch (Rage Fist) |
| scripts.ts canTerastallize → null | implemented | ruleset gimmicks (Tera off) |
| scripts.ts canMegaEvo: item.megaStone[species.name] only | implemented | `turn/mega.rs` |
| scripts.ts modifyDamage: 4x / 0.25x messages | unnecessary | protocol text only; arithmetic identical to base (diffed) |
| scripts.ts spreadMoveHit: AfterHit without `pokemon.hp` check | implemented | moves.rs (`knock-off-fainted-user`) |
| scripts.ts hitStepMoveHitLoop: no -hitcount for single Parental Bond hit | unnecessary | protocol text only (diffed; aa-parental-bond* fixtures) |
| abilities.ts Anger Shell / Berserk onDamage (multihit check) | implemented | abilities.rs (Champions note) |
| abilities.ts Healer (each adjacent ally, 1/2) | implemented | residual.rs (Champions note) |
| abilities.ts Natural Cure (no onCheckShow, silent cure) | implemented | switching.rs switch-out cure |
| abilities.ts Regenerator (baseMaxhp/3) | implemented | switching.rs |
| abilities.ts Run Away (escapes trapping) | implemented | abilities.rs / trapped (Champions note) |
| abilities.ts Unseen Fist (onHitProtect, bypassProtect 0.25; no onModifyMove) | implemented | abilities.rs, moves.rs |
| abilities.ts Dragonize / Eelevate / Fire Mane / Mega Sol / Piercing Drill / Spicy Spray made standard | implemented | pinned in support.rs; Dragonize has no oracle fixture |
| items.ts Eject Button (ignores future moves) | implemented | items.rs (`rr-future-sight-eject-button`) |
| items.ts isNonstandard flips (Mega Stones standard; Assault Vest, Eviolite, Eject Pack, … Past) | implemented | data/champions.json is exported from the Champions dex |
| moves.ts Dire Claw secondary (psn/par/slp sample) | implemented | moves/handlers.rs |
| moves.ts Disable condition.onBeforeMove (cantusetwice exception) | implemented | conditions.rs |
| moves.ts Encore condition.onStart (changeAction) | implemented | battle.rs `encore_change_action` (R8) |
| moves.ts Fake Out / First Impression onDisableMove (activeMoveActions) | implemented | mod.rs `disabled` |
| moves.ts Salt Cure condition.onResidual | implemented | residual.rs (Champions note) |
| moves.ts data changes (power, accuracy, flags, PP, isNonstandard) | implemented | exported data |
| conditions.ts par 1/8 full paralysis | implemented | moves.rs `chance(1, 8)` |
| conditions.ts slp startTime sample([2,3,3]) | implemented | moves.rs / conditions.rs |
| conditions.ts frz 3-turn cap + 1/4 thaw | implemented | moves.rs |
| rulesets.ts Flat Rules (Team Preview, Adjust Level 50, Item/Species Clause, Picked Team Size Auto) | implemented | scenario loader (VGC bring 6 pick 4, O104); clauses are validator rules |

## 재현

추출 스크립트는 세션 scratchpad에 두었다(보고서만 커밋): Node로 Champions dex 핸들러를 뽑고(`Dex.mod('champions')`, 기술·특성·도구·`dex.data.Conditions`), Python으로 sim 이벤트와 결합해 `support.rs` 표·엔진 소스 언급·`oracle/scenarios`(expected가 있는 것)의 이름 등장으로 분류했다.
