# 턴 엔진 구현 작업 계획 (원소 단위, 담당 배분)

작성 2026-09-25. 기준 문서: [SHOWDOWN-GAPS.md](SHOWDOWN-GAPS.md)(무엇이 없는가), [COVERAGE.md](COVERAGE.md)(자동 분류), [CONTEXT.md](CONTEXT.md)(실행법·함정), [DESIGN.md](DESIGN.md). 사용자 지시(2026-09-25): 미구현 항목을 원소 단위로 쪼개고, 어려운 것은 Fable 5.1, 쉬운 것은 Opus 5.5가 맡는다.

## 0. 난이도 기준과 배분 규칙

| 난이도 | 기준 | 담당 |
|---|---|---|
| **상** | 상태 모델·명령(`Instruction`)·정규 출력 schema·열거 구조(`Pending`, 단계)·행동 큐·결정 단계를 바꾸거나, Showdown 이벤트 순서를 새로 재현해야 하는 것. 틀리면 이후 모든 결과가 어긋난다 | **Fable 5.1** |
| **중** | 기존 훅(`BasePower`, `ModifyAtk`, `DamagingHit` 등)에 핸들러를 하나 추가하되 새 휘발·확률 분기·부수 효과가 있는 것 | **Opus 5.5** (전제 조건이 끝난 뒤) |
| **하** | 이미 있는 훅에 조건식 하나를 더하는 것. 오라클 시나리오 하나로 검증 끝 | **Opus 5.5** |

**모든 단위의 완료 조건(공통):**
1. `core/src/turn/support.rs`의 거부 목록/표에서 해당 항목을 빼고, 구현한 콜백 목록을 표에 고정한다(표와 dex가 어긋나면 테스트가 실패한다).
2. 오라클 시나리오 JSON을 `oracle/scenarios/`에 추가하고 `node oracle/enumerate.cjs <s> --out …`(정확 열거가 끝나는 크기로 설계)로 정답을 만든 뒤 `node oracle/strip-report.cjs` → `oracle/expected/<이름>.turn.json`. 결과가 커지는 턴은 `--mode mc` + `oracle/marginals.cjs`로 대신한다.
3. `scenario/tests/turn.rs`에 정확 비교(또는 주변분포) 테스트를 추가한다. 시나리오 대상 번호는 양수=상대·음수=아군, 로그로 확인한다.
4. `cargo fmt --all --check`, `cargo clippy --workspace --exclude lab-engine-py --all-targets -- -D warnings`, `cargo test --workspace --exclude lab-engine-py`, `cargo run -p lab-scenario --release --bin lab-coverage -- --out engine/COVERAGE.md`.
5. 구현하지 않은 분기를 조용히 넘기지 않는다. 확신이 없는 조합은 `TurnError::Unsupported`로 남기고 문서에 적는다.
6. 커밋 여부는 사용자 지시에 따른다(브랜치 `lab-engine`).

**Opus 5.5 세션 시작 절차:** CONTEXT.md → DESIGN.md → SHOWDOWN-GAPS.md → 이 문서. 담당 단위의 "의존" 열이 전부 완료(§4 체크리스트)된 것만 시작한다. Showdown 코드는 `vendor/pokemon-showdown/{sim,data}`와 `data/mods/champions`(Champions 차이)를 반드시 직접 읽는다. 값을 기억으로 쓰지 않는다.

## 1. Fable 5.1 담당 — 상 (기반·구조)

순서대로 한다. 각 항목이 아래 Opus 트랙의 전제다.

| ID | 단위 | 내용 | 바꾸는 곳 | 검증 |
|---|---|---|---|---|
| **F1** | 타입·종 변경 명령 | `Instruction::SetTypes{target, old, new}`, `Instruction::SetSpecies{target, old, new}`(종·타입·능력치·특성을 한 번에 되돌림), `diff.rs` 반영. 정규 출력에 `types`(종과 다를 때)·`species` 변화 반영 | `instruction.rs`, `state.rs`, `turn/diff.rs`, `scenario/canonical.rs` | 단위 테스트(apply/reverse) |
| **F2** | 성격·SP를 `Pokemon`에 | 메가·폼체인지 시 능력치 재계산 근거. `Pokemon { nature, stat_points }` 추가, 로더가 채움, `stats::champions_stats`로 재계산 함수 | `state.rs`, `scenario/team.rs`, `meta.rs`(사이드카에서 이동) | 로더 fixture |
| **F3** | 메가진화 행동 | `megaEvo` 큐 행동(order 104, 우선도 없음, 속도순), `runMegaEvo`: 종 변경(F1)·능력치 재계산(F2)·특성 교체·`gimmicks_used`·정규 `canMega` 소거. 같은 턴 기술 순서 재계산(gen 8+: 매 행동 후 재정렬이라 자동). `AfterMega` 이벤트는 훅 자리만. 매직룸 등 도구 억제와 무관(DESIGN.md) | `turn/mod.rs`(Action 종류), 새 `turn/mega.rs`, `support.rs` | 시나리오: 가디안 메가 후 하이퍼보이스(속도 100 변경 확인), 마기라스 메가 모래날림 재발동 없음 확인 |
| **F4** | 등장 처리 통합 + 동시 등장 순서 | `scenario/switch_in.rs`(트레이스·모래날림·그래스메이커, 속도순 다중 등장)와 `turn/switching.rs`(날씨·필드·위협, 단일)를 `turn/switching.rs` 하나로. `runSwitch`의 일괄 처리: 등장자들 속도 정렬(`speedOrder` 보정), `fieldEvent('SwitchIn')` 핸들러 순서, 트레이스의 `Update` 즉시 시작. 첫 턴 시작(`start`)도 같은 함수 | `turn/switching.rs`, `scenario/switch_in.rs`(위임), `lib.rs` | 기존 `initial.rs` 테스트 유지 + 이중 교체 시나리오 |
| **F5** | 기절 후 교체 결정 단계 | `enumerate_replacements(state, [Option<party_index>;2×N])`: `request: switch` 상태에서 양쪽 `instaswitch`를 속도순 등장(F4), 기절 포켓몬 status `fnt → ''`, 이어서 `endTurn`(턴 증가). 한쪽만 교체하는 경우 다른 쪽은 `wait`. 합법 교체 후보 생성(`Ruleset::joint_actions`와 같은 자리) | `turn/mod.rs`, `rules.rs`(교체 후보), `canonical.rs`(`request`) | 오라클: 1턴 KO 후 교체 → 2턴 결과를 `setupTurns`로 재현(`enumerate.cjs`는 이미 지원). 로더의 `setupTurns` 거부를 이 단계 이후 해제 |
| **F6** | 턴 중단·재개 (교체기·탈출 도구·위기회피) | 유턴·볼트체인지·플립턴·패스트샷·바톤터치·탈출버튼·붉은카드·위기회피·도망태세: 행동 도중 `switchFlag` → 턴이 멈추고 그 편의 교체 결정을 받은 뒤 남은 큐를 이어 간다. `Pending`에 "중단 지점"과 남은 큐를 넣고 `enumerate_turn`이 `TurnSuspended{side, outcomes…}`를 돌려주는 API. 바톤터치는 랭크·휘발 복사(`copyVolatileFrom`) | `turn/mod.rs`(Pending·API), `moves.rs`(`selfSwitch`/`forceSwitch`), `battle.rs` | 유턴 → 교체 → 상대 기술 순서 오라클 |
| **F7** | 대상 유도 | `RedirectTarget` priorityEvent: 날따름·분노가루·스포트라이트(`followme`/`ragepowder`/`spotlight` 휘발, 1턴, `onTry`: `queue.willAct()`), 피뢰침·저수(`onAnyRedirectTarget`, 타입 일치 시 유도 + 면역/랭크), 우선순위·속도·`effectOrder` 동순 처리, `tracksTarget`(스토커·프로펠러테일)·광역기 제외. `getMoveTargets` 재구성 | `moves.rs`(대상 결정), `volatile.rs`(3종), `support.rs` | 중력 파티 필수. 시나리오: 에써르 날따름 + 최면술 |
| **F8** | 큐 조회 API와 순서 조작 | 기술 실행 중 남은 큐를 읽고(`willMove(target)`, `willAct()`) 바꾸는(`prioritizeAction`, `changeAction`) 인터페이스. 명령(Quash)·애프터유·기습(Sucker Punch `onTry`)·도우미(`willMove`)·바크아웃? 의 전제 | `turn/mod.rs`(`Pending.queue`를 `Battle`에 노출) | 명령·애프터유 오라클 |
| **F9** | 강제 선택(잠금) | `lockedmove`(역린·꽃잎댄스·난동, 2~3턴 후 혼란), `twoturnmove`(솔라빔·전자포·하늘로날기·구멍파기·고스트다이브: 반투명 상태 `Invulnerability`), `mustrecharge`(파괴광선), 앵콜(`OverrideAction`, 3턴, PP 0이면 끝). 다음 턴 선택이 강제되므로 `Ruleset::joint_actions`/`check_turn`이 잠금을 반영해야 한다. 파워풀허브 | `state.rs`(잠금 표현), `rules.rs`, `moves.rs`, `volatile.rs` | 역린 2턴 + 혼란, 솔라빔 쾌청/비 |
| **F10** | 연속기 | `hitStepMoveHitLoop` 복수 타격: 고정 횟수(2·3·10), 2~5회 분포(35/35/15/15), 로디드다이스, 스킬링크, `multiaccuracy`(트리플악셀·킥), 타격마다 기합의띠·울퉁불퉁멧·부가효과, 대상 기절 시 중단, 네즈미(10회·명중별). 분기 수 폭증 → 결과 병합 확인 | `moves.rs`(hit loop), `support.rs` | 씨기관총(스킬링크 유/무) 오라클(`mc`) |
| **F11** | 대타출동 | `substitute` 휘발(HP 저장), `TryPrimaryHit`로 피해 우회, `bypasssub` 플래그, 소리 기술, 대타 파괴, 정규 출력 `volatiles.substitute.hp` | `volatile.rs`(payload), `moves.rs`, `canonical.rs` | 대타 + 광역기 |
| **F12** | 슬롯 조건 표 | `Side.slot_conditions[N][kind]`(소원·치유소원·달의춤·리바이벌블레싱·미래예지·파멸의소원), 되돌릴 명령, 정규 `slotConditions`, `Residual`/`SwitchIn` 훅 | `state.rs`, `instruction.rs`, `diff.rs`, `canonical.rs`, `residual.rs` | 소원 오라클 |
| **F13** | 피해 이력 필드 | `Slot`에 `last_damage/attacked_by(source, damage, category, this_turn)`, `hurt_this_turn`, `times_attacked`, `move_last_turn_result`; `Side.total_fainted/fainted_this_turn`. 카운터·미러코트·메탈버스트(`beforeTurnMove`)·짓밟기·리벤지·복수·되갚기·레이지피스트·라스트리스펙트·위기회피 판정의 전제. 정규 출력에는 없으니 `Eq/Hash` 병합에만 영향 | `state.rs`, `diff.rs`, `battle.rs`(피해 시 기록), `residual.rs`(턴 종료 초기화) | 카운터 오라클 |
| **F14** | `Update` 이벤트 지점 | Showdown `eachEvent('Update')`가 불리는 9지점(행동 뒤, 타격 루프 안, 등장 뒤 등)에 훅을 두고 속도순 실행. 열매·트레이스·가면·열교환·마그마·부스트에너지의 전제 | `moves.rs`, `mod.rs`, `switching.rs` | 오랭열매 타이밍(타격 사이) 오라클 |
| **F15** | `DamagingHit` 이벤트 틀 | `spreadMoveHit` 뒤 피해 입은 대상들에 대해 속도순 실행, 공격자 반동 피해로 공격자 기절 가능, `SourceDamagingHit`(독수). 개별 특성은 Opus | `moves.rs` | 까칠한피부 반동으로 공격자 기절 |
| **F16** | 랭크 이벤트 틀 | `boost()`에 `TryBoost`(차단)·`ChangeBoost`(심술꾸러기·단순)·`AfterEachBoost`(경쟁심·오기: 출처가 상대일 때)·`AfterBoost`·`ModifyBoost`(천진: 능력치 계산 시). 위협 반응 특성의 전제 | `battle.rs`, `moves.rs`(데미지 계산의 ModifyBoost) | 위협 vs 경쟁심 |
| **F17** | 도구 억제 계층 | 매직룸·금제·서투름·클러치를 위한 `effective_item(slot)`(Showdown `ignoringItem`). **메가 자격·원시구슬은 예외**(DESIGN.md). 모든 도구 훅이 이 함수를 거치게 정리 | `battle.rs`, 모든 도구 참조 | 매직룸 + 생명의구슬 |
| **F18** | 정확 열거 성능 | `spread-damage`형 턴: 이미 행동한 대상의 풀죽음 분기 생략(분포 동치), 병합 전 정규화(턴 끝에 사라지는 차이), 결과 수 상한/근사 모드, 프런티어 메모리. 목표 PokaiEngine 자릿수 | `turn/mod.rs`, `branch.rs` | `spread-damage` 정확 열거 완주 |
| **F19** | 폼체인지·변신·일루전 | 원시회귀, 다루마모드, 쉴드다운, 아이스페이스, 디스가이즈, 제로투히어로, 꿀꺽미사일, 변신·임포스터, 일루전(정보 게임), 스탠스체인지. F1 위에서. 라이브러리 사용이 적어 후순위 | `turn/forme.rs` | 항목별 |

## 2. Opus 5.5 담당 — 중·하

"의존"이 비어 있으면 지금 시작할 수 있다. 각 행이 한 단위(한 커밋·한 시나리오)다.

### 2.1 기술

| ID | 기술 | 난이도 | 의존 | Showdown 콜백 | 구현 요점 |
|---|---|---|---|---|---|
| O1 | Expanding Force | 중 | — | `onBasePower`, `onModifyMove` | 사이코필드 + 접지: 위력 1.5, 대상 `allAdjacentFoes`로 변경(대상 변경 시 `spread`) |
| O2 | Grav Apple | 하 | — | `onBasePower` | 중력 아래 1.5 |
| O3 | Rising Voltage / Psyblade / Misty Explosion | 하 | — | `basePowerCallback`/`onBasePower` | 필드 조건 위력. 미스티익스플로전은 자폭(거부 유지 가능) |
| O4 | Weather Ball / Terrain Pulse | 중 | — | `onModifyMove`, `onModifyType` | 타입·위력 변경. `ActiveMove`에 타입 필드 도입(현재 `data.move_type` 직접 사용) |
| O5 | Blizzard / Hurricane / Thunder | 하 | — | `onModifyMove` | 날씨 필중·명중 50 |
| O6 | Ice Spinner / Steel Roller | 하 | — | `onAfterHit`, `onAfterSubDamage` | 타격 후 필드 제거(대타 피해 시에도). 스틸롤러는 필드 없으면 실패(`onTry`) |
| O7 | Helping Hand | 중 | F8 | `condition.onBasePower`, `onTryHit`, `onRestart` | 아군이 아직 행동 안 했을 때만(`willMove`), 위력 1.5 휘발 1턴 |
| O8 | Coaching / Life Dew / Decorate / Pollen Puff(아군 회복) | 하 | — | (없음) | 대상 종류 `AdjacentAlly`, `Allies` 처리(`get_move_targets`), 코칭은 `willMove`가 아니라 단순 랭크 |
| O9 | Foul Play / Body Press 확인 | 하 | — | (없음) | `override_offensive_pokemon_target`: 상대의 공격·랭크 사용 |
| O10 | Sucker Punch / Thunderclap / Upper Hand | 중 | F8 | `onTry` | 대상이 아직 행동 전이고 공격 기술을 선택했을 때만 |
| O11 | First Impression | 하 | — | `onTry`, `onDisableMove` | 속이기와 같은 `move_actions` 검사(풀죽음 없음) |
| O12 | Dire Claw / Tri Attack / Throat Chop 부가효과 | 하 | — | `secondary.onHit` | 세 상태 중 균등 분기(`rng.uniform(3)`) |
| O13 | Taunt | 중 | — | `condition.onBeforeMove`, `onDisableMove`, 지속 3(빠르면 4?) | 변화 기술 사용 불가·선택 불가, `check_turn` 반영, `DisableMove` |
| O14 | Disable | 중 | — | 같은 계열 + `onTryHit`(마지막 기술 필요) | 휘발에 기술 id 저장(`VolatileState.counter`에 `MoveId.0`) |
| O15 | Imprison / Heal Block / Torment | 중 | — | `onFoeBeforeMove` 등 | 선택 불가 처리 |
| O16 | Trick / Switcheroo | 하 | — | `onHit`, `onTryImmunity` | 도구 교환(`SetItem` 두 번), 메가스톤·메모리 예외(`item_can_be_taken`) |
| O17 | Knock Off 보정 확인 / Poltergeist / Acrobatics | 하 | — | `onTry`, `basePowerCallback` | 도구 유무 위력 |
| O18 | Roost | 중 | F1 | `condition.onStart`, `onType` | 비행 타입 제거 1턴(`SetTypes`), 순수 비행은 노말 |
| O19 | Perish Song | 중 | — | `onHitField`, `condition.onResidual` | 휘발 counter 3→0, 방음 면역, 턴 종료 순서 |
| O20 | Yawn | 하 | — | `condition.onStart/onEnd`, `onTryHit` | 다음 턴 종료 시 수면(필드·불면 면역) |
| O21 | Wide Guard / Quick Guard | 중 | — | `condition.onTryHit`, `onHitSide`, `onTry`(stall) | 진영 1턴 효과(`SideEffect` 있음), 스톨 카운터 공유 |
| O22 | Stealth Rock / Spikes / Toxic Spikes / Sticky Web + Defog / Rapid Spin / Court Change | 중 | F4 | `condition.onSwitchIn`, `onSideStart` | 층수(`Effect.value`), 등장 피해·랭크, 정규 출력 `layers` 추가 |
| O23 | Safeguard / Mist / Lucky Chant | 하 | — | 진영 조건 | `SideEffect` 있음. 정규 출력 id 추가 |
| O24 | Haze / Clear Smog / Topsy-Turvy / Power Swap류 | 하 | — | `onHit`, `onHitField` | 랭크 조작 |
| O25 | Morning Sun / Moonlight / Synthesis / Shore Up | 하 | — | `onHit` | 날씨별 회복량 |
| O26 | Icy Wind / Electroweb / Snarl 확인 | 하 | — | — | 부가 랭크 광역: 이미 지원돼야 함, 오라클로 확인만 |
| O27 | Endure | 중 | — | `condition.onDamage`, `onPrepareHit`(stall) | 1 HP 남김, 스톨 카운터 |
| O28 | Struggle (PP 0) | 하 | — | `struggleRecoil`, 타입 `???` | 현재 `Unsupported`. PP 전부 0일 때만 선택 가능 |
| O29 | Wish / Healing Wish / Lunar Dance | 중 | F12 | 슬롯 조건 | 다음 턴 종료 회복 / 교체 입장 회복 |
| O30 | Counter / Mirror Coat / Metal Burst / Comeuppance | 중 | F13, F8 | `beforeTurnCallback`, `damageCallback`, `onRedirectTarget` | 마지막 피해 기준 |
| O31 | Stomping Tantrum / Revenge / Avalanche / Payback / Assurance / Rage Fist / Last Respects | 하 | F13 | `basePowerCallback` | 이력 필드 읽기 |
| O32 | Glaive Rush | 중 | — | `condition.*` | 사용 후 피격 2배·필중 휘발 |
| O33 | Sparkling Aria / Double Shock / Electro Shot | 중 | (O32 무관) F9(전자포) | `onAfterMove`, `self.onHit`, `onTryMove` | 스파클링아리아 화상 치료, 더블쇼크 타입 상실(F1) |
| O34 | Freeze-Dry / Flying Press / Thousand Arrows | 하 | — | `onEffectiveness` | 상성 예외 |
| O35 | Sleep Talk / Snore | 중 | — | `sleepUsable`, `onHit`(무작위 기술) | 수면 중 사용, 다른 기술 호출(`use_move` 재진입) |
| O36 | Whirlwind / Roar / Dragon Tail / Circle Throw | 중 | F6 | `forceSwitch`, `DragOut` | 무작위 벤치(`getRandomSwitchable`) |
| O37 | Parting Shot / U-turn / Volt Switch / Flip Turn / Baton Pass / Shed Tail | 중 | F6 | `selfSwitch`, `onHit` | F6 위에서 각각 |
| O38 | Encore / Quash / After You / Instruct | 중 | F8, F9 | `onOverrideAction`, `onHit` | 큐 조작 |

### 2.2 특성

| ID | 특성 | 난이도 | 의존 | 콜백 | 요점 |
|---|---|---|---|---|---|
| O40 | Technician / Sharpness / Iron Fist / Strong Jaw / Mega Launcher / Reckless / Tough Claws / Punk Rock / Steely Spirit | 하 | — | `onBasePower` | 플래그·위력 조건 |
| O41 | Adaptability | 하 | — | `onModifySTAB` | STAB 2.0 |
| O42 | Blaze / Torrent / Overgrow / Swarm | 하 | — | `onModifyAtk/SpA` | HP ≤ 1/3 |
| O43 | Hustle | 하 | — | `onModifyAtk`, `onSourceModifyAccuracy` | 물리 명중 0.8 |
| O44 | Guts / Marvel Scale / Quick Feet | 하 | — | `onModifyAtk/Def/Spe` | 상태이상 조건, 근성은 화상 반감 무효 |
| O45 | Thick Fat / Heatproof / Water Bubble / Purifying Salt / Dry Skin(피해) | 하 | — | `onSourceModifyAtk/SpA` | 타입별 반감 |
| O46 | Solid Rock / Filter / Prism Armor / Multiscale / Shadow Shield / Fluffy / Ice Scales / Punk Rock(방어) / Aura Guard | 하 | — | `onSourceModifyDamage` | 최종 배율 |
| O47 | Friend Guard | 하 | — | `onAnyModifyDamage` | 아군 피해 0.75 |
| O48 | Good as Gold / Bulletproof / Volt Absorb / Water Absorb / Earth Eater / Well-Baked Body / Motor Drive / Sap Sipper / Flash Fire / Wonder Guard / Telepathy / Soundproof / Overcoat / Dazzling·Queenly Majesty·Armor Tail | 중 | — | `onTryHit`, `onFoeTryMove` | 면역·흡수(회복/랭크/휘발). 플래시파이어는 휘발 |
| O49 | Levitate 확인 / Air Lock·Cloud Nine | 중 | — | `suppressWeather` | 날씨 효과 억제(`effectiveWeather` 전 참조) |
| O50 | Speed Boost / Moody(제외) / Shed Skin / Hydration / Harvest(열매 뒤) | 하 | — | `onResidual` | 턴 종료 |
| O51 | Rain Dish / Ice Body / Solar Power / Dry Skin(회복) | 하 | — | `onWeather` | 날씨 이벤트 |
| O52 | Rock Head / Magic Guard | 하 | — | `onDamage` | 반동·간접 피해 무효 |
| O53 | Sturdy / Battle Armor 확인 | 하 | — | `onDamage`, `onTryHit` | 만피 1타 버팀, OHKO 면역 |
| O54 | Regenerator / Natural Cure | 중 | F6 또는 교체 경로 | `onSwitchOut` | 교체 시 회복/치료 |
| O55 | Rough Skin / Iron Barbs / Cursed Body / Stamina / Weak Armor / Static / Flame Body / Effect Spore / Poison Point / Cotton Down / Gooey·Tangling Hair / Seed Sower / Toxic Debris / Sand Spit / Electromorphosis / Steam Engine / Justified / Rattled / Thermal Exchange(피격) / Anger Shell / Berserk / Water Compaction | 하~중 | F15 | `onDamagingHit` | 각각 한 단위. 저주받은바디는 `disabled` 슬롯(O14와 같은 표현) |
| O56 | Poison Touch | 하 | F15 | `onSourceDamagingHit` | |
| O57 | Competitive / Defiant / Rattled(위협) / Guard Dog | 중 | F16 | `onAfterEachBoost`, `onTryBoost` | 상대 출처 랭크 하강 시 |
| O58 | Clear Body / White Smoke / Full Metal Body / Hyper Cutter / Keen Eye / Big Pecks / Mirror Armor / Inner Focus(위협) / Own Tempo / Oblivious / Scrappy(위협) | 하 | F16 | `onTryBoost` | |
| O59 | Contrary / Simple / Unaware | 중 | F16 | `onChangeBoost`, `onAnyModifyBoost` | 천진은 데미지 계산의 랭크 무시 |
| O60 | Inner Focus / Shield Dust / Covert Cloak(도구) / Serene Grace / Sheer Force | 중 | — | `onTryAddVolatile`, `onModifySecondaries`, `onModifyMove` | 부가효과 차단·확률 2배·제거 |
| O61 | Prankster 확인 / Gale Wings / Triage / Stall | 하 | — | `onModifyPriority` | 질풍날개 만피 조건 |
| O62 | Pressure | 하 | — | `onDeductPP` | 대상마다 PP 1 추가 |
| O63 | Unnerve | 하 | F14(열매) | `onFoeTryEatItem` | 열매 금지 |
| O64 | Unburden | 중 | F14 | `onAfterUseItem`, `condition.onModifySpe` | 도구 소모 후 속도 2배 휘발 |
| O65 | Thermal Exchange / Water Veil / Immunity / Insomnia / Vital Spirit / Limber / Magma Armor / Comatose / Purifying Salt / Leaf Guard / Flower Veil / Sweet Veil / Aroma Veil | 하 | — | `onSetStatus`, `onAllySetStatus`, `onTryAddVolatile` | 상태 면역 |
| O66 | Synchronize | 중 | — | `onAfterSetStatus` | 되받기 |
| O67 | Mold Breaker / Teravolt / Turboblaze | 중 | — | `onModifyMove`(`ignoreAbility`) | `breakable` 특성 무시 경로 전반 |
| O68 | Download / Intrepid Sword / Dauntless Shield / Frisk / Forewarn / Anticipation / Costar / Hospitality / Screen Cleaner / Supersweet Syrup / Embody Aspect / Unnerve(등장) / Pressure(등장) / Mold Breaker(등장) | 하 | F4 | `onStart` | 등장 효과 각각 |
| O69 | Protean / Libero | 중 | F1 | `onPrepareHit` | 등장 후 1회 타입 변경 |
| O70 | Pixilate / Aerilate / Refrigerate / Galvanize / Normalize / Liquid Voice | 하 | O4 | `onModifyType`, `onBasePower` | 노말→타입 1.2 |
| O71 | Trace(턴 중 등장) | 중 | F4 | `onStart`, `onUpdate` | 첫 등장 코드 재사용 |
| O72 | Protosynthesis / Quark Drive + Booster Energy | 중 | F14 | 조건 휘발 | 최고 능력치 1.3(속도 1.5), 날씨/필드 종료 시 해제 |
| O73 | Emergency Exit / Wimp Out | 중 | F6, F13 | `onEmergencyExit` | 반피 통과 시 교체 |
| O74 | Disguise / Ice Face | 중 | F1, F19 | `onDamage`, `onUpdate` | 폼체인지 |

### 2.3 도구

| ID | 도구 | 난이도 | 의존 | 콜백 | 요점 |
|---|---|---|---|---|---|
| O80 | Sitrus / Oran / Figy류(핀치 1/4)·Salac류(랭크) | 중 | F14 | `onUpdate`, `onEat`, `onTryEatItem` | 반피(1/4) 이하에서 즉시, 먹보(Gluttony) |
| O81 | Lum / Chesto / Cheri / Pecha / Rawst / Aspear / Persim / Mental Herb | 하 | F14 | `onUpdate`, `onAfterSetStatus` | 상태 즉시 치료 |
| O82 | 반감열매 17종 (Roseli, Colbur, Chople, Occa, Shuca, Passho, Yache, Kasib, Wacan, Rindo, Kebia, Coba, Payapa, Tanga, Charti, Haban, Chilan, Babiri) | 하 | — | `onSourceModifyDamage`, `onEat` | 효과 뛰어남 0.5 후 소모(`useItem`) |
| O83 | Choice Scarf / Band / Specs | 중 | — | `onModifySpe/Atk/SpA`, `onModifyMove`(`choicelock`), `onStart` | 잠금 휘발(기술 id 저장), `check_turn`·`DisableMove` 반영 |
| O84 | Expert Belt / Life Orb 확인 / Metronome | 하 | — | `onModifyDamage` | |
| O85 | Rocky Helmet | 하 | F15 | `onDamagingHit` | 접촉 시 1/6 |
| O86 | Weakness Policy / Absorb Bulb / Cell Battery / Luminous Moss / Snowball / Kee·Maranga Berry | 하 | F15 | `onDamagingHit` | 조건 랭크 + 소모 |
| O87 | Air Balloon | 중 | — | `onStart`, `onDamagingHit` | 비접지, 피격 시 파열(`useItem`이 아니라 제거) |
| O88 | Iron Ball / Lagging Tail·Full Incense(우선도 -0.1) / Quick Claw·Custap(확률 우선) | 중 | — | `onModifySpe`, `onEffectiveness`, `onFractionalPriority` | 분수 우선도는 정렬 키에 소수부 추가 |
| O89 | Assault Vest / Eviolite | 하 | — | `onModifySpD/Def`, `onDisableMove` | 돌격조끼는 변화 기술 선택 불가 |
| O90 | Covert Cloak / Clear Amulet / Safety Goggles / Utility Umbrella / Protective Pads / Heavy-Duty Boots | 하 | (O22 위험 방지용) | 각 훅 | |
| O91 | Wide Lens / Zoom Lens / Scope Lens / Razor Claw / Focus Band / King's Rock·Razor Fang | 하 | — | 명중·급소·확률 | |
| O92 | Psychic / Grassy / Electric / Misty Seed | 중 | F4, `TerrainChange` 이벤트 | `onStart`, `onTerrainChange` | 필드 발동 시 랭크 + 소모. `TerrainChange` 이벤트 지점 추가 |
| O93 | Black Sludge / Toxic Orb / Flame Orb / Sticky Barb / Shell Bell / Throat Spray | 하 | — | `onResidual`, `onAfterMoveSecondarySelf` | |
| O94 | Eject Button / Red Card / Eject Pack | 중 | F6 | `onAfterMoveSecondary`, `onAfterBoost` | 교체 트리거 |
| O95 | White Herb / Mirror Herb / Adrenaline Orb / Room Service | 중 | F16 | 랭크 이벤트 | |
| O96 | Loaded Dice | 하 | F10 | `onModifyMove` | |
| O97 | Power Herb | 하 | F9 | `onChargeMove` | |
| O98 | Booster Energy | 중 | O72 | | |

### 2.4 필드·기타

| ID | 항목 | 난이도 | 의존 | 요점 |
|---|---|---|---|---|
| O100 | Magic Room | 중 | F17 | 도구 효과 억제 5턴(메가 예외). `FieldEffect::MagicRoom` 자리 있음 |
| O101 | Wonder Room | 중 | — | 방어·특방 교환(`calculateStat`), 5턴 |
| O102 | `TerrainChange`/`WeatherChange` 이벤트 지점 | 중 | — | 필드·날씨 설정·종료 시 속도순 호출(시드·미미크리·프로토·쿼크 전제) |
| O103 | Utility Umbrella | 하 | O49 | 쾌청·비 무시 |
| O104 | VGC 4마리 선출 로더 (`gen9championsvgc2026regmc`) | 중 | — | `pickedTeamSize 4`, 팀 프리뷰 선택 → 파티 4 + 빈 슬롯. 시나리오 형식 확장. Showdown `chooseTeam` |
| O105 | 정규 출력 확장 | 하 | 각 항목 | `conditions.layers`, 새 휘발 id, `slotConditions`, `types`; `canonical.cjs`와 byte 일치 유지 |
| O106 | 오라클 시나리오 묶음 | 하 | — | 라이브러리 상위 파티(sand-owen, kickoff-wolfey 등)의 실제 1턴들을 시나리오화해 회귀 묶음으로. `Unsupported`가 나는 것은 그대로 목록에 남긴다 |

## 3. 우선순위 (중력 파티 기준)

1. F1 → F2 → F3 (메가진화). 이것이 없으면 라이브러리 팀의 대부분을 시작도 못 한다(메가스톤 사용 100+회).
2. F7 (날따름 유도) — 에써르 대책 평가.
3. F4 → F5 (기절 후 교체) — 두 턴 이상 이어 붙이기.
4. F14 → O80/O81/O82 (열매) — 도구 사용 1위.
5. F15/F16 → O55/O57/O58 — 피격·위협 반응 특성.
6. O83 (구애), O40~O46, O13, O1, O12 — 라이브러리 상위 미지원.
7. F6 → O37/O94/O73, F10, F9 → O38.
8. F18 (성능)은 F3·F7과 병행 가능(다른 파일).

## 4. 진행 체크리스트

완료한 단위는 여기 ID·커밋·시나리오 이름을 적는다. Opus 세션은 이 표로 전제 조건을 확인한다.

| ID | 상태 | 커밋 | 시나리오/테스트 | 비고 |
|---|---|---|---|---|
| F1 | 완료 2026-09-26 | `baca382` | `instruction.rs`·`diff.rs` 단위 테스트 | `Instruction::SetForme{old,new: Forme}`(종·타입·최대HP·능력치·특성·기본특성 일괄), `Instruction::SetTypes`. 정규 출력 `types`. 퇴장(교체·기절) 시 타입은 종 타입으로 복귀(`Battle::clear_volatile`) |
| F2 | 완료 2026-09-26 | `baca382` | 로더 fixture | `Pokemon { nature, stat_points }`, `stats::champions_stats_unchecked`, `Pokemon::forme_as(species)`. 사이드카 `MemberMeta`에서 성격·SP 제거 |
| F3 | 완료 2026-09-26 | `bc97223` | `mega-tyranitar`, `mega-tyranitar-sand` / `scenario/tests/mega.rs` | `turn/mega.rs`, `ActionKind::Mega`(order 104). 메가 대상 종의 특성이 필드·등장 모두 지원돼야 선택 가능(`mega_target`), 아니면 `Unsupported`. `AfterMega`는 훅 자리만. 첫 등장(`switch_in.rs`)에 날씨 4·필드 4·위협 추가(F4 통합 전 임시) |
| O2 | 완료 2026-09-26 (Opus) | `0f237c2` | `o2-grav-apple` / `tests/moves_basic.rs` | Grav Apple `onBasePower` |
| O3 | 완료 2026-09-26 (Opus) | `c8f73b3` | `o3-rising-voltage`, `o3-rising-voltage-ungrounded` | Rising Voltage `basePowerCallback`, Psyblade `onBasePower`. 미스티익스플로전 제외(자폭) |
| O5 | 완료 2026-09-26 (Opus) | `be8f2ce` | `o5-thunder-rain`, `o5-thunder-sun-gravity`, `o5-blizzard-snow` | 날씨 명중 `onModifyMove`. 폭풍은 혼란 휘발 때문에 여전히 거부. 우산은 날씨 읽는 곳에서 검사하되 도구 자체는 거부 |
| O34 | 완료 2026-09-26 (Opus) | `04ee9c7` | `o34-freeze-dry-flying-press`, `…-immune` | Freeze-Dry / Flying Press `onEffectiveness`. 사우전드애로는 `smackdown` 휘발 필요(미구현) |
| O17 | 완료 2026-09-26 (Opus) | `fdd07a5` | `o17-poltergeist-knock-off`, `o17-acrobatics-knock-off`, `o17-acrobatics-item` | Poltergeist `onTry`, Acrobatics `basePowerCallback`; 탁쳐서떨구기는 확인만 |
| O11 | 완료 2026-09-26 (Opus) | `55b3086` | `o11-first-impression`, `…-psychic-terrain` | `onTry`·`onDisableMove` |
| O12 | 완료 2026-09-26 (Opus) | `9827d00` | `o12-dire-claw`, `o12-tri-attack`, `o12-tri-attack-sun` | 3분기 상태. Champions 다이어클로 30%. 부수 수정: `status_immune`가 쾌청의 얼음 면역을 반영 |
| O25 | 완료 2026-09-26 (Opus) | `c72f340` | `o25-heal-sun/sand/snow-full/clear` | 날씨별 회복 `onHit` |
| O26 | 완료 2026-09-26 (Opus) | `643f140` | `o26-icy-wind`, `o26-electroweb`, `o26-snarl` | 코드 변경 없음, 오라클 확인 |
| O24 | 완료 2026-09-26 (Opus) | `50c4eed` | `o24-clear-smog`, `o24-haze`, `o24-boost-swaps`, `o24-topsy-turvy-fail` | Haze `onHitField`; Clear Smog·Topsy-Turvy·Power/Guard/Heart Swap `onHit`. 스피드스왑 제외 |
| F4 | 완료 2026-09-26 | `9dd7ea6` | `tie-start.initial.json`, 기존 `initial.rs` 10개 | 등장 처리 통합: `turn/switching.rs`의 `switch_in`(퇴장·fnt 해제·슬롯 배치)·`run_switch_in`(일괄 `runSwitch`: 저장 속도순, 동률 균등 무작위, 특성 바뀐 핸들러 건너뜀)·`start_ability`(날씨 4·필드 4·위협·트레이스·메시지만인 것)·`START_HANDLERS` 표. `scenario/switch_in.rs`는 검증 후 `turn::enumerate_start`에 위임 |
| F5 | 완료 2026-09-26 | `9dd7ea6` | `ko-replace`(setupTurns 1턴 + 교체 결정, 속도 동률 2결과) / `tests/replacement.rs` | `Slot.fainted_occupant`+`Instruction::SetFaintedOccupant`(기절 포켓몬이 자리를 지킴), `turn::enumerate_replacements`(instaswitch 기절자 속도순 → 일괄 runSwitch → endTurn), `enumerate_stages` 공용 열거. 로더: `setupTurns` 재생(`scenario_positions`), Showdown 파티 순서 추적(`advance_order`, `switch N`), `Decision::{Turn,Replacement}`, `lab-turn` 대응 |
| F9 | 완료 2026-09-26 (일부) | `522493d` | `outrage-lock`, `hyper-beam-recharge`, `encore` / `tests/locks.rs` | 휘발 `Confusion`(time 2~5, BeforeMove 33% 자해 `confusion_damage`), `LockedMove`(duration 2 + 숨은 `trueDuration` 2~3, onRestart·onResidual·onEnd→혼란), `MustRecharge`, `Encore`(duration 3/4, `move`; OverrideAction·DisableMove·PP 0 종료). `turn/lock.rs::locked_move`: 잠긴 포켓몬의 선택은 무엇이든 잠긴 기술(또는 `RECHARGE_INDEX` 재충전 의사 기술)로 정규화, 교체·기믹 거부, PP 미차감. `VolatileState`에 `time`·`mv`·`hidden` 추가, 정규 출력에 `time`·`move`·`trueDuration`(canonical.cjs에도 추가, schema 1 유지). `randomNormal` 대상·`recharge` 플래그·self 휘발 허용. **남은 것:** 2턴 기술(`twoturnmove`+반투명), 파워풀허브, 잠금 상태의 후보 생성(`Ruleset::joint_actions`) |
| F10 | 완료 2026-09-26 | `314d0e0` | `double-hit`(정확), `bullet-seed`·`loaded-dice`·`population-bomb`(Showdown `mc` 20,000 표본 vs 엔진 정확 분포, `tests/multihit.rs`) | 연속기는 **타격마다 한 단계**: `moves::MoveProgress`(사용자·기술 스냅샷·남은 대상·횟수·누적 피해)를 `Pending.in_progress`에 두고 `resume_move`로 이어서 타격하므로 타격 사이에 동일 상태가 병합된다(네즈미 10타가 25ms). `decide_hits`: 고정 횟수, 2~5(35/35/15/15), 스킬링크(최대), 로디드다이스(4~5, 10타는 4~10), `multiaccuracy` 재판정(스킬링크·로디드다이스는 생략). 타격마다 기합의띠·부가효과·Update. 트리플악셀류(`basePowerCallback`에 타격 번호)·스마트타겟(드래곤애로)은 미지원. `common::assert_mc_parity` 추가 |
| F8 | 완료 2026-09-26 | `d1ee8cb` | `sucker-punch`, `quash`, `after-you` / `tests/queue_moves.rs` | `turn/queue.rs`: 단계 실행 중 남은 큐가 `Battle::queue`에 있고 `will_act`·`will_move`·`queued_move`(기술·분류·우선도 1/10)·`prioritize_action`(order 3)·`quash_action`(order 201)을 제공. `Action.order` 덮어쓰기는 병합 키에 포함. 함께 구현: O10(기습·선더클랩·어퍼핸드 `onTry`), O38 일부(명령·애프터유 `onHit`). 도우미(O7)는 `adjacentAlly` 대상 처리(O8, Opus 진행 중) 뒤에 |
| F14 | 완료 2026-09-26 | `96b5eea` | `sitrus-lum` / `tests/berries.rs` | `turn/update.rs`: `update_event`를 Showdown의 `eachEvent('Update')` 지점(행동 뒤, 타격 피해 뒤, 날씨 잔여 뒤, 건강한 퇴장 전, 교체 등장 뒤, 턴 끝 `checkFainted` 뒤)에서 호출. 구현 리스너: 오랭·오렌·피지류 5·랭크 열매 5·럼(`onAfterSetStatus` 포함)·상태 열매 6·리피아, 먹보. 순서 무작위는 리스너끼리 간섭이 없어 소비하지 않음. 피지류가 혼란을 일으키는 성격은 거부(`berry_problem`). 특성 `onUpdate`는 여전히 `cured_on_update` 가드(치료 도달 불가) |
| F15 | 완료 2026-09-26 | `9f541aa` | `damaging-hit` / `tests/events.rs` | `moves::damaging_hit`: 피해 입은 대상들의 `onDamagingHit`를 `compareLeftToRightOrder`(order → 대상 index → 상태·특성·도구)로 실행. 기절한 보유자도 발동(Showdown과 같음). 구현: 얼음 해동, 까칠한피부·철가시(접촉, 방어패드 확인), 울퉁불퉁멧, 주눅(특성 절반). `onSourceDamagingHit`(독수 O56)·나머지 O55/O85/O86은 이 틀에 얹는다 |
| F16 | 완료 2026-09-26 | `9f541aa` | `boost-events`, `unaware-contrary` / `tests/events.rs` | `Battle::boost_by(target, boosts, source, BoostEffect)`: ChangeBoost(심술꾸러기·단순) → ±6 캡 → TryBoost(클리어바디·하얀연기·메탈프로텍트·괴력집게·부풀린가슴·미러아머(반사)·가드도그) → 랭크마다 AfterEachBoost(경쟁심·오기: 상대 출처 하강) → AfterBoost(주눅+위협). `boost_seen`: 천진의 ModifyBoost(데미지·명중 계산). 모든 랭크 변화 호출이 출처·원인을 넘긴다. 미구현: 이너포커스·마이페이스·둔감·배짱의 위협 차단(O58, 이너포커스는 O60 세션과 겹쳐 보류), 플라워베일(`try_set_status`에 출처 필요), 클리어아뮬렛(O90) |
| O48 | 완료 2026-09-26 (Opus) | `10e697d` | `o48-absorb-heal`, `o48-boost-absorb`, `o48-flash-fire-boost`, `o48-flash-fire-spread`, `o48-immunity`, `o48-wonder-guard-gold`, `o48-dazzling` / `tests/abilities_immunity.rs` | 흡수·면역 특성 `onTryHit`(`moves/ability_hooks.rs`), 플래시파이어 휘발, 오버코트 `onImmunity`, 여왕의위엄·아머테일·비비드바디 `onFoeTryMove`. 초식의 `onAllyTryHitSide`는 도달 불가 |
| O49 | 완료 2026-09-26 (Opus) | `c166d8b` | `o49-cloud-nine-sun`, `o49-air-lock-faint` | `Battle::effective_weather()`(억제), 날씨를 읽는 모든 자리 교체; `WeatherChange` 이벤트 자리(핸들러 없음) |
| O60 | 완료 2026-09-26 (Opus) | `236b3e7` | `o60-inner-focus`, `o60-shield-dust`, `o60-shield-dust-self-boost`, `o60-serene-grace`, `o60-sheer-force` | 이너포커스(풀죽음 차단; 위협 차단은 병합 시 `boost_by` TryBoost로 이동), 인분·하늘의은총(`secondary_chance_factor`)·우격다짐(`has_sheer_force`, 생명의구슬·해동 생략) |
| O66 | 완료 2026-09-26 (Opus) | `b7cd9d9` | `o66-synchronize` | `try_set_status_from(target, status, source)`, `after_set_status`(싱크로 → 병합 시 럼열매도 이 뒤에) |
| O67 | 완료 2026-09-26 (Opus) | `8a9b271` | `o67-mold-breaker`, `o67-turboblaze` | `ActiveMoveRef.ignore_ability`를 ModifyMove에서 설정; 특성 무시 기술이 `onUpdate` 치료 특성의 상태를 걸 수 있는 조합은 거부 |
| (병합) | 2026-09-26 | `8cbc007` | | Opus 브랜치 `worktree-agent-a241205e5cbe3f8a4` 병합. 충돌 4개 파일: 휘발 표 합침(FlashFire), 위협 이너포커스 → `boost_by`, 싱크로+럼 AfterSetStatus, `use_move` TryMove·우격다짐 꼬리 |
| O82 | 완료 2026-09-26 (Opus) | `386c565` | `o82-resist-berries`, `o82-chilan-roseli` / `tests/items.rs` | 반감열매 18종(`core/src/turn/items.rs` 신설), 서투름 보유자는 거부(F17 대기) |
| O83 | 완료 2026-09-26 (Opus) | `be626f7` | `o83-choice-scarf-band`, `o83-choice-lock`, `o83-choice-knocked-before-move` | 구애 3종, `choicelock` 휘발(`counter`에 기술 id, 정규 출력 `move`), 잠금 중 다른 기술 거부 |
| O84 | 완료 2026-09-26 (Opus) | `b2fdd28` | `o84-expert-belt`, `o84-life-orb` | 달인의띠. 메트로놈은 거부(F13 `moveLastTurnResult` 필요) |
| O89 | 완료 2026-09-26 (Opus) | `f86ee34` | `o89-assault-vest-eviolite`, `o89-av-psyshock` | 돌격조끼(변화 기술 선택 불가), 진화의휘석 |
| O91 | 완료 2026-09-26 (Opus) | `25e3026` | `o91-wide-zoom-lens`, `o91-zoom-lens-slower-target`, `o91-scope-lens-focus-band`, `o91-kings-rock` | 광각렌즈·줌렌즈(병합 후 `Battle::will_move` 큐 API 사용)·초점렌즈·예리한손톱·기합의머리띠·왕의징표석(`ActiveMove.added_secondary`) |
| O93 | 완료 2026-09-26 (Opus) | `01da75b` | `o93-residual-items`, `o93-orb-immunity-sticky-barb`, `o93-sticky-barb-shell-bell-throat-spray` | 검은진흙·화염구슬·맹독구슬·끈적끈적바늘·조개껍질방울(`ActiveMove.total_damage`)·목캔디 |
| O87·O88 | 완료 2026-09-26 (Opus) | `f724261` | `o87-o88-grounding`, `o88-lagging-tail-quick-claw` | 접지 순서(검은철구·풍선), 느림보꼬리·풀향로 −0.1, 선제공격손톱 1/5(첫 단계에서 추첨, `Pending.fractional_drawn`). 풍선 파열은 병합 시 F15 `damaging_hit`에 구현(`air-balloon-pop` 정확 일치). 커스탭열매 거부 |
| O90 | 완료 2026-09-26 (Opus) | `af789be` | `o90-safety-goggles`, `o90-covert-cloak`, `o90-covert-cloak-self-secondary` | 방진고글, 은폐망토(`keeps_secondary`), 방어패드·튼튼한부츠는 무동작 허용. 클리어아뮬렛(F16 뒤 미구현)·우산 거부 |
| (병합) | 2026-09-26 | `2cecf16` | | Opus 브랜치 `worktree-agent-a009b2b8c48836e66` 병합. 충돌 9개 파일: 휘발 표(ChoiceLock, 12종), 큐 비트셋 → `queue.rs`, 풍선 파열 → F15, 부가효과 루프(우격다짐/인분/은폐망토/왕의징표석 순서), `HitOutcome`에 총피해(조개껍질방울) |
| O1 | 완료 2026-09-26 (Opus) | `4ba5a2b` | `o1-expanding-force`, `o1-expanding-force-ungrounded` / `tests/moves_field.rs` | `ActiveMove.target`; 대상 종류가 바뀌면 새 대상 무작위 |
| O4 | 완료 2026-09-26 (Opus) | `ca85a64` | `o4-weather-ball-sun`, `o4-terrain-pulse-sand`, `o4-rain-ungrounded` | `ActiveMove.move_type`·`base_power`; 기술 타입을 읽는 모든 자리 교체(`on_modify_type`) |
| O6 | 완료 2026-09-26 (Opus) | `642ba59` | `o6-ice-spinner`, `o6-steel-roller` | `moves::clear_terrain`, `on_after_hit` |
| O8 | 완료 2026-09-26 (Opus) | `f9f45aa` | `o8-ally-support`, `o8-pollen-puff` | `AdjacentAlly`·`AdjacentAllyOrSelf`·`Allies` 대상 허용, `on_try_hit`(기술 자체 TryHit) |
| O9 | 완료 2026-09-26 (Opus) | `9384df3` | `o9-foul-play-body-press` | 탁쳐서떨구기... 아님: 속임수(상대 공격·랭크), 바디프레스 확인 |
| O16 | 완료 2026-09-26 (Opus) | `43c41f3` | `o16-trick`, `o16-trick-fail` | `on_try_immunity`(점착), 메가스톤 양방향 검사. Start/End/TakeItem 핸들러가 있는 도구(구애 포함) 이동은 거부 |
| O18·O69 | 완료 2026-09-26 (Opus) | `2dbf496` | `o18-roost`, `o69-protean`, `o69-protean-once` | 새 파일 `turn/conditions.rs`; 루스트(`SetTypes`, 이전 타입은 `counter`에 숨김, 잔여 순서 25), 변환자재·리베로(`ProteanUsed` 엔진 전용 휘발, 정규 출력 제외) |
| O20·O19 | 완료 2026-09-26 (Opus) | `7aae9b0` | `o20-yawn`, `o19-perish-song` | 하품(다음 턴 끝 수면, 일렉트릭필드 차단), 멸망의노래(`onHitField`, 방음·사이코필드의 `null` TryHit), `Battle::faint` |
| O27 | 완료 2026-09-26 (Opus) | `4ab5672` | `o27-endure`, `o27-endure-stall` | `Endure` 휘발, `Battle::damage` 순서(옹골참·기합의띠 앞), 방어 스톨 카운터 공유 |
| O21·O23 | 완료 2026-09-26 (Opus) | `8005c30` | `o21-wide-guard`, `o21-quick-guard`, `o23-safeguard-mist`, `o23-lucky-chant` | 진영 조건 5종(`SideEffect::LuckyChant` 추가), 신비의부적(`try_set_status_from` 출처), 흰안개(병합 시 `boost_by` TryBoost로 이동), 행운의축복(급소 차단) |
| (병합) | 2026-09-26 | `c0992da` | | Opus 브랜치 `worktree-agent-a2e6d0a74832d5666` 병합. 충돌 6개 파일: 휘발 17종으로 합침(`residual_order`에 앵콜 16 추가), `boost_by` 4인자 유지(흰안개 편입), 잔여 단계의 기절 처리 규칙(E)·잠긴 기술 onEnd(`conditions::volatile_end`)·`ActiveMove` 필드 합집합(`move_type`·`target`·`base_power`), 속임수+천진, 급소(초점렌즈+행운의축복), 도구·특성 보정 체인 합침 |
| O40 | 완료 2026-09-26 (Opus) | `ce5de4f` | `o40-*` 6개 / `tests/abilities_damage.rs` | `onBasePower` 8종, 펑크록 `onSourceModifyDamage`, 강철정신 `onAllyBasePower`. 새 파일 `core/src/turn/abilities.rs`: 보정 핸들러를 Showdown `speedSort` 순(우선도→subOrder→속도, 인수가 다를 때만 동률 분기)으로 연쇄하는 `Handler`/`chain` |
| O41 | 완료 2026-09-26 (Opus) | `5d403dd` | `o41-adaptability` | `onModifySTAB` |
| O42 | 완료 2026-09-26 (Opus) | `a5647fe` | `o42-blaze-torrent`, `o42-overgrow-swarm` | 1/3 정확 경계 |
| O43 | 완료 2026-09-26 (Opus) | `93762df` | `o43-hustle` | `onModifyAtk` 직접 곱, `onSourceModifyAccuracy` 3277(중력과 연쇄) |
| O44 | 완료 2026-09-26 (Opus) | `c208597` | `o44-guts-marvelscale`, `o44-quickfeet` | 근성 화상 반감 무효, 속보 마비 반감 무효 |
| O45 | 완료 2026-09-26 (Opus) | `0c833eb` | `o45-waterbubble`, `o45-thickfat-heatproof`, `o45-purifyingsalt`, `o45-dryskin-rain`, `o45-dryskin-sun` | 다섯 특성의 전 콜백. 수포 `onUpdate`(화상 치료)는 `check_state`가 화상 상태의 수포를 거부해 도달 불가 처리; 병합 시 `cured_on_update`에도 추가. 정화의소금 `onTryAddVolatile`는 하품 전용 |
| O46 | 완료 2026-09-26 (Opus) | `17b2658` | `o46-*` 4개 | `onSourceModifyDamage` 8종 |
| O47 | 완료 2026-09-26 (Opus) | `1f2e0ad` | `o47-friendguard`, `o47-friendguard-ally` | `onAnyModifyDamage`(아군 공격 포함) |
| (병합) | 2026-09-26 | `5654f3b` | | Opus 브랜치 `worktree-agent-aba64e625a6875c96` 병합. 충돌 6개 파일 수동 해소: 건조피부 중복(TryHit·onWeather는 O51 구현 유지, 위력 1.25는 `abilities.rs` 유지), 태양의힘 `onModifySpA`를 `attack_handlers`로 이동, 기술 자체 `onBasePower`(`handlers::on_base_power`)를 `Handler` 연쇄에 편입, `switch_in.rs`는 F4 버전 유지(수포 등장 표 추가), `super::abilities`는 `ability_events`로 별칭 |
| O65 | 완료 2026-09-26 (Opus) | `ff43a7c` | `o65-status-block`, `o65-comatose-sweet-veil`, `o65-leaf-guard-sun` / `tests/abilities_status.rs` | `onSetStatus`·`onAllySetStatus`·`onImmunity`(마그마의무장)·`onTryAddVolatile`. `onUpdate` 치료는 `cured_on_update`로 "도달 불가" 처리(그 상태로 필드에 있으면 거부). 클리어스모그(정화의소금) 고스트 반감은 O45, 열교환 공격 상승은 F15, 플라워베일은 F16 대기 |
| O61 | 완료 2026-09-26 (Opus) | `824fb04` | `o61-gale-wings-triage`, `o61-gale-wings-damaged`, `o61-stall` | 질풍날개·힐링시프트 `onModifyPriority`, 스톨 분수 우선도(`ActionKind::Move.fractional_tenths`, 큐 등록 시 고정) |
| O62 | 완료 2026-09-26 (Opus) | `2d22309` | `o62-pressure` | `onDeductPP`(`pressureTargets` 규칙) |
| O52 | 완료 2026-09-26 (Opus) | `9002719` | `o52-rock-head-magic-guard` | `onDamage`, `DamageSource::Recoil` |
| O53 | 완료 2026-09-26 (Opus) | `233fd5c` | `o53-sturdy`, `o53-shell-armor` | 옹골참 `onDamage`(기합의띠보다 먼저)·`onTryHit`; 특성 무시 기술(`ability_unless_broken`)이 조가비갑주·옹골참을 관통 |
| O50 | 완료 2026-09-26 (Opus) | `265fcc3` | `o50-residual-abilities` | 가속·탈피(33/100)·촉촉바디 `onResidual`. 가속은 `activeTurns` 대신 `move_actions > 0` 사용(현 메커닉에서 동치) |
| O51 | 완료 2026-09-26 (Opus) | `28dabdf` | `o51-rain`, `o51-sun`, `o51-snow` | 젖은접시·아이스바디·태양의힘·건조피부 `onWeather`(날씨 단계가 모든 날씨에 실행), 태양의힘 `onModifySpA`, 건조피부 `onTryHit`·`onSourceBasePower` |
| (병합) | 2026-09-26 | `04ec529` | | Opus 브랜치 `worktree-agent-af6b4f7b6acc3b6b1` 병합. 충돌 5개 파일 수동 해소: `status_immune`(쾌청+우산+마그마의무장 통합), `ActionKind::Move.fractional_tenths`+`Mega`, `try_hit`에 피뢰침·저수 흡수 통합(`ability_unless_broken`), `switch_in.rs` 표 |
| (병합) | 2026-09-26 | `2e0be94` | | Opus 브랜치 `worktree-agent-a334fdc160394f916` 병합(충돌 없음). 새 파일 `core/src/turn/moves/handlers.rs`(이벤트별 함수), `ActiveMove.accuracy` |
| F7 | 완료 2026-09-26 | `2e604ec` | `followme-hypnosis`, `ragepowder-grass`, `lightningrod-foe`, `lightningrod-ally` / `scenario/tests/redirect.rs` | `moves.rs::redirect_target`(`RedirectTarget` priority event: 우선도→속도, 첫 유효 대상), 휘발 `FollowMe`·`RagePowder`·`Spotlight`(1턴), 피뢰침·저수 `onTryHit`(흡수 + 특공 +1). 스포트라이트는 코드만(시나리오 없음). 같은 우선도·속도의 유효 유도자 둘(Showdown은 `effectOrder`)은 `Unsupported`. 스토커·프로펠러테일은 미지원(`onModifyMove`) |
| O55 | 완료 2026-09-26 (Opus) | `fabe40c`·`6a04019` | `o55-*` 11개 / `tests/abilities_contact.rs` | `onDamagingHit` 특성: 정전기·불꽃몸·독가시·포자(실제 분기 수면 11/마비 10/독 9)·지구력·약한갑옷·솜털·미끈미끈·컬리헤어·모래뿜기·씨뿌리기·전기로바꾸기·풍력발전·증기기관·정의의마음·열교환·꿀꺽... 아님: 물먹기(Water Compaction), 유폭·내용물분출(기절 시점 일치). 새 휘발 `Charge`(다음 전기 기술 위력 2배, 전기 기술 뒤 종료; 풍력발전은 순풍 시작에도), `AngerShellUnchecked`(분노의껍질·거괴의 `onDamage` 판정 대기, 숨김; 그동안 회복 열매를 `onTryEatItem`이 보류, `abilities::try_eat_item`). 우격다짐 타격 뒤 거괴가 오랭열매를 먹지 않는 Showdown 특성 재현. `damaging_hit`에 `Kind::Source`·`total_before`. 미지원: 저주받은바디·독치장·미라/의뭉함/떠도는영혼·멸망의바디·꿀꺽미사일·일루전·헤롱헤롱바디·매운뿜기·바람타기·하나가된 |
| O56 | 완료 2026-09-26 (Opus) | `fabe40c` | `o56-poison-touch`, `-blocked` | 독수·독조합 `onSourceDamagingHit`; 인분·은폐망토가 차단. 독수는 **피격 대상**의 방어패드가 막고 사용자의 것은 막지 않음(Showdown 그대로) |
| O58 | 완료 2026-09-26 (Opus) | `fabe40c` | `o58-update-cures`, `-own-tempo-intimidate`, `-scrappy-keen-eye` | 마이페이스·둔감·배짱·날카로운눈·발광·심안(`ActiveMove.ignore_evasion`·`scrappy`). **특성 `onUpdate` 치료가 실제로 실행**(`abilities::on_update`, `update_event`에서 도구보다 먼저) → `cured_on_update` 거부를 check_state·등장·트레이스·메가·`status_cure_bypassed`에서 모두 제거(거부 테스트 2개는 치료 테스트로). 둔감에 특성 무시 헤롱헤롱·유혹·도발은 거부(휘발 없음) |
| O68·O63 | 완료 2026-09-26 (Opus) | `d348829` | `o68-*` 6개 | 다운로드(원더룸 규칙 포함)·코스타·하숙·스크린클리너·기묘한약·파스텔베일(`onAnySwitchIn` 포함)·프리스크·예지몽·위험예지·몸으로하는표현 4종·긴장감(`onFoeTryEatItem`). 등장 핸들러는 `onSwitchInPriority` 순 뒤 속도. `enumerate_start` 끝에 Update. 불굴의검·불굴의방패·감미료는 배틀 시작에만(`Battle.battle_start`; 상태에 1회 플래그가 없어 이후 등장은 거부). 시나리오는 자시안·자마젠타 대신 엘레이드·한카리아스(`onBattleStart` 종은 거부) |
| O64 | 완료 2026-09-26 (Opus) | `71b5061` | `o64-unburden-berry`, `-take`, `-trick` | `Unburden` 휘발(도구 없을 때 속도 2배): `use_item`·풍선 파열·`take_item`(탁쳐서떨구기·끈적끈적바늘·트릭)에서 추가; 메가스톤을 못 뺏어도 추가(Showdown 그대로). 병합 시 `eat_item`의 `consume`에도 AfterUseItem 추가 |
| O54 | 완료 2026-09-26 (Opus) | `1c85bec` | `o54-regenerator-natural-cure` | 재생력·자연회복 `onSwitchOut`(건강한 퇴장자), 퇴장 전 Update(Showdown도 실행) |
| (병합) | 2026-09-26 | `ccedfc6` | | Opus 브랜치 `worktree-agent-a34814ec3cbea2406` 병합(충돌 없음; 병합 커밋 유지) |
| O86 | 완료 2026-09-26 (Opus) | `9a27a49` | `o86-weakness-policy`, `-absorb-bulb-moss`, `-cell-battery-snowball`, `-kee-maranga` / `tests/items_field.rs` | 약점보험·구근·축전지·빛이끼·스노볼은 피격 이벤트에서(`Battle.hit_type_mod`: `get_damage`가 대상별 상성 기록), 키열매·마랑고열매는 `hit_loop` 끝 AfterMoveSecondary |
| O80·O81 | 완료 2026-09-26 (Opus) | `abc74b6` | `o80-lansat-starf`, `o80-micle`(setupTurns), `o80-toxic-poison-user`, `o81-custap`, `o81-enigma-jaboca`, `o81-rowap-fainted` | 휘발 `FocusEnergy`(랜섬열매, 급소 +2)·`MicleBerry`(다음 명중 4915/4096, 턴 끝 섭취); 스타열매 무작위 랭크, 커스탭(큐 등록 시 섭취), 나조열매(효과 굿 뒤 회복), 야파·로플(0 HP 보유자도). 먹보 문턱 적용. **함께 고침:** 독 타입의 맹독은 필중, `hit_loop`의 반동 뒤 Update 누락(생명의구슬로 기절할 사용자가 오랭열매를 먼저 먹어야 하는 경우) |
| O92·O102 | 완료 2026-09-26 (Opus) | `96f8859` | `o92-seeds-psychic-electric`, `o92-seeds-grassy-misty` | 새 파일 `turn/field_events.rs`: `set_terrain`·`clear_terrain`·`set_weather`·턴 끝 만료에서 TerrainChange/WeatherChange 발화, 시드는 등장 시(특성 뒤)에도. 날씨·필드 변화에 반응하는 특성은 거부(테스트로 강제). 함정: 시드 보유자가 필드에 있을 때 필드 **패치**는 오라클만 이벤트를 발화해 불일치 |
| O95 | 완료 2026-09-26 (Opus) | `c0adf42` | `o95-white-herb`, `-intimidate-herb-orb`, `-room-service`, `-room-service-switch-in`, `-mirror-herb` | 하양허브·미러허브(등장·메가 뒤·기술 뒤·턴 끝), 아드레날린오브(위협에; 클리어바디가 하강을 막아도), 룸서비스(등장·트릭룸 시작). 미러허브는 단계 끝에 미사용 복사 랭크가 남으면 거부(`items::stage_end_check`; Showdown은 도구에 보관). 클러치·퍼시스턴트·하양허브 던지기 거부 |
| O101 | 완료 2026-09-26 (Opus) | `82eb7da` | `o101-wonder-room`, `o101-body-press` | 원더룸 5턴·재사용 시 종료, 데미지·혼란 자해에서 방어·특방 교환, 바디프레스는 특방 랭크. 정규 출력·패치 `wonderroom` |
| O103 | 완료 2026-09-26 (Opus) | `8aa38c6` | `o103-umbrella-sun`, `o103-umbrella-rain` | `Battle::weather_for(slot)`로 포켓몬별 날씨 읽기를 전부 교체(모래·눈 피해와 필드 전체 검사는 그대로) |
| O104 | 완료 2026-09-26 (Opus) | `3d807ef` | `o104-vgc-sand-owen`(+`.p1.json`) / `tests/vgc.rs` | VGC 팀 프리뷰 4마리(Showdown pick-and-fill). 레벨 50 조정 규칙을 로더·`enumerate.cjs`·`initial.cjs`에. 로더의 기본 레벨은 50, 오라클 배틀(validator 생략)은 100. 커스텀 게임은 속도 10000 상한이 없고 엔진·VGC는 있음 |
| O106 | 완료 2026-09-26 (Opus) | `3174b37` | `lab-library` → `reports/library-support-2026-09-26.md` | 병합 뒤 재생성: 28팀 모두 로드, 세 선출 모두 시작+방어 턴 실행 9팀(선출 60/84), 전 검사 통과 0팀. 상위 거부: 파팅샷 9, 라스트리스펙트 8, 목조르기·스카이스킨·페어리오라·플라워베일·노가드·유턴 6. 라이브러리 team.json은 대개 레벨 100 저장이라 VGC 규칙으로만 로드됨. `crown-ryukeivgc`는 `needs-review` |
| (병합) | 2026-09-26 | `79a9ba4` | | Opus 브랜치 `worktree-agent-a103cbba0d3c0a582` 병합. 충돌 6개 파일 합집합: 휘발 22종, `run_switch_in`에 특성 우선도(O68)와 도구 핸들러(O92·O95) 통합, `run_move_tail(b, user, mv)`에 Charge 종료 + 도구 `onAnyAfterMove`, `Battle { battle_start, hit_type_mod, mirror_herb }`, `enumerate_start` 끝 Update → 단계 끝 검사. **의미 충돌 2개**: `eat_item`의 새 `consume`이 AfterUseItem(언버든)을 빠뜨림(`o64-unburden-berry` 실패로 발견), AfterMoveSecondary를 대상별 한 루프에서 Showdown 하위 순서(해동 2 → 특성 7 → 도구 8)로 |
| O7 | 완료 2026-09-26 (Opus) | `95de0c0` | `o7-helping-hand`, `-fail`, `-ko` / `tests/moves_control.rs` | `HelpingHand` 휘발(1턴, `counter`에 적용 횟수, 정규 출력 제외), `onTryHit`(아군의 행동이 남았거나 `move_actions == 0`), 위력 1.5ⁿ(우선도 10) |
| O13 | 완료 2026-09-26 (Opus) | `83d4a46` | `o13-taunt`, `o13-taunt-ends` | 3턴(대상이 이미 행동했으면 4), BeforeMove 5와 선택 불가, `conditions::volatile_start` 훅(`add_volatile_from`) |
| O14 | 완료 2026-09-26 (Opus) | `16b4cec` | `o14-disable`, `-fail`, `-next` | 5턴(행동이 남았으면 4), `mv`에 기술, 잔여 순서 17, BeforeMove 7, 마지막 기술 필요(`onTryHit`) |
| O15 | 완료 2026-09-26 (Opus) | `65ed6e1` | `o15-torment-imprison`, `-next` | 트집(마지막 기술 선택 불가), 봉인(상대는 사용자의 기술을 선택 불가, BeforeMove 4). 회복봉인은 거부(`onTryHeal`이 모든 회복원을 막아야 함) |
| O22 | 완료 2026-09-26 (Opus) | `a0528e9` | `o22-*` 13개(`o22-entry`, `o22-replace-ko`, `o22-replace-toxic*`, `o22-defog-spin`, `o22-court-change` …) | 스텔스록·압정뿌리기 3층·독압정 2층·끈적끈적네트(`Effect.value`에 층수, 정규 출력 `layers`); `conditions::entry_hazards`가 등장 시 특성 앞(하위 순서 4), 부츠·접지 반영, 교체 등장 KO는 `check_fainted`로 다시 교체. 안개제거·고속스핀·코트체인지, 패치는 함정 `null`만. 거부: 함정 생성 순서(effectOrder)가 결과를 바꾸는 경우(`o22-hazard-order`, fixture 없음), 독압정+싱크로, 만천스핀·정리정돈 |
| O28 | 완료 2026-09-26 (Opus) | `7b9a23d` | `o28-struggle` | `turn::STRUGGLE_INDEX`, 파서 `move struggle`(쓸 기술이 없으면 `move 1`도 발버둥), 타입 없음(`Type::None`, STAB·변환자재 없음), 반동은 `Battle::direct_damage`(매직가드·돌머리 무시). `joint_actions`엔 미포함 |
| O32 | 완료 2026-09-26 (Opus) | `0ff190d` | `o32-glaive-rush`, `-slower`, `-faster` | 휘발: 피격 필중·2배, 다음 기술 시도 시 제거(BeforeMove 100) |
| O33 | 완료 2026-09-26 (Opus) | `09403ce` | `o33-sparkling-aria`, `-shield-dust`, `-sheer-force` | 휘발 + `handlers::on_after_move`, `ActiveMove.hit_targets`. 더블쇼크(`???` 타입 없음)·일렉트로샷(2턴 기술 틀) 거부 |
| O35 | 완료 2026-09-26 (Opus) | `fe78f78` | `o35-sleep-talk`, `o35-snore` | `sleepUsable`, `moves::call_move`(Showdown `useMove`), 프레셔 추가 PP는 `ActiveMove.source_effect`. 연속기나 `onAfterMove` 기술을 부를 수 있으면 거부(`o35-sleep-talk-multihit`) |
| O38 | 완료 2026-09-26 (Opus) | `3dd5b7b` | `o38-instruct`, `o38-instruct-spread` | 지시(order 3 새 행동; 명령·애프터유는 F8). 단일 대상 기술 반복은 거부(`lastMoveTargetLoc` 없음, `o38-instruct-target`) |
| (병합) | 2026-09-26 | `2fe0eb3` | | Opus 브랜치 `worktree-agent-aa232b8937c04f257` 병합. 충돌 4개 파일 합집합: 휘발 29종, `ActiveMove` 필드(`ignore_evasion`·`scrappy`·`hit_targets`·`source_effect`), 기술 자체 AfterMove → `run_move_tail`. **`run_switch_in` 재작성**: 하나의 SwitchIn 필드 이벤트(함정 4·특성 7·도구 8·파스텔베일 onAny)를 우선도 → **이벤트마다 한 번 뽑은 속도 순서**(Showdown `speedOrder`) → 하위 순서로 실행. Accuracy 이벤트: 글레이브러시·미클열매를 속도순(동률 50/50)으로, 비수치 명중엔 미클 휘발 유지. `Instruction::Switch.previous`를 `Box<Slot>`(휘발 29종으로 enum 크기 lint) |
| 수정 | 2026-09-26 | `70e7878` | `knock-off-fainted-user`, `lum-persim-confusion` / `tests/events.rs`, `tests/berries.rs` | 탁쳐서떨구기는 사용자가 울퉁불퉁멧으로 기절해도 도구를 제거(Champions `onAfterHit`에 `source.hp` 검사 없음; G 세션 발견, 오라클 확인), 럼·감귤열매가 혼란에도 발동하고 혼란을 치료(G 세션 발견) |
| F13 | 완료 2026-09-26 | `43bc715` | `history-avalanche-payback`, `history-tantrum-assurance`(setupTurns), `history-ragefist-metalburst`(setupTurns), `history-last-respects` / `scenario/tests/history.rs` | `Slot.history: SlotHistory { hurt_this_turn, last_damaged_by(DamagedBy{source,slot,damage}; 상대·이번 턴만), damaged_by_this_turn(공격자 비트), times_attacked, move_this_turn_result/move_last_turn_result(MoveResult: Undefined/Null/Failed/Succeeded), newly_switched(기본 true) }`, `Side.history: SideHistory { total_fainted(≤100), fainted_this_turn, fainted_last_turn }`; `Instruction::SetSlotHistory/SetSideHistory`(통째 교체), `diff.rs` 반영. 새 파일 `turn/history.rs`: `record_hurt`(`Battle::damage` 뒤, `directDamage` 제외), `record_attack`(`hit_loop` 끝, 마지막 타격의 대상들 `MoveProgress.last_hit`; 숫자 피해면 `timesAttacked += 타격 수`), `set_move_result`(BeforeMove 실패 false, 재충전은 null)·`finish_move_result`(`useMove` 끝; 불러낸 기술이 먼저 정하면 유지), `record_faint`(`faint_messages`), `end_turn_history`(`end_turn`의 턴 진행 시). 정규 출력에는 넣지 않음(setupTurns로 같은 이력을 재생). Showdown의 `attackedBy` 전체 목록 대신 소비자가 읽는 것만 보관(전부 `thisTurn` 필요). 소비자: 어시스트·보복·눈사태·분풀이·짜증폭발·분노의주먹·라스트리스펙트(`basePowerCallback`), 메탈버스트·카운터펀치(`onTry`·`onModifyTarget`·`damageCallback`; `MoveTarget::Scripted` 허용). 미구현: 카운터·미러코트(`beforeTurnCallback` 행동 필요, O30 나머지), 리벤지·리턴어택(`Past`), 셸트랩·기합펀치, 위기회피(F6) |
| O31 | 완료 2026-09-26 (일부, F13에서) | `43bc715` | 위 F13 시나리오 | 어시스트·보복·눈사태·분풀이·짜증폭발·분노의주먹·라스트리스펙트. 리벤지는 Champions `Past`라 제외 |
| O30 | 완료 2026-09-26 (일부, F13에서) | `43bc715` | `history-ragefist-metalburst` | 메탈버스트·카운터펀치. 카운터·미러코트는 `beforeTurnCallback`(턴 시작 휘발) 미구현 |
| F6 | 완료 2026-09-26 (일부: 자기 교체) | `fee8833` | `uturn-switch`, `uturn-no-bench`, `uturn-pause`, `uturn-both`, `parting-shot`, `volt-switch-immune`, `uturn-ko` / `scenario/tests/switching.rs` | **턴 중단·재개 API.** `Slot.switch_flag`(`Instruction::SetSwitchFlag`; 살아 있는 점유자에 켜져 있으면 정규 출력 `request`가 그 진영 `switch`, 상대는 `''`). 기술이 성공하고 사용자가 살아 있으면(`moveHit`) 플래그를 켜고, 행동 뒤(`after_action` → `request_switches`, Showdown `runAction` 꼬리) 벤치가 없으면 지우고 있으면 단계가 `StageEnd::Suspended`로 끝난다. `enumerate_stages`/`sample_stages`가 `Ending { end, pending, probability }`를 돌려주고 `Outcome.suspension: Option<Suspension>`(남은 큐·진행 중 기술을 감싼 불투명 값)으로 나간다. `turn::resume_turn(state, &suspension, [[Option<u8>; N]; 2])`: 교체 검증(`check_mid_turn_switches`: 플래그 슬롯마다 벤치 수만큼) → 첫 단계에서 `instaswitch`(나가는 포켓몬의 행동 속도순, 동률 균등, 사이 Update 없음) → 일괄 `runSwitch` → `after_action` → 남은 큐 계속(다시 중단 가능). 중단 상태로 `enumerate_turn`/`enumerate_replacements`를 부르면 `InvalidChoice`. `runMoveEffects`의 `selfSwitch` 절(벤치 있으면 성공, 없으면 실패 합산). 시나리오: 최상위 `midTurn: {p1: [..], p2: [..]}`와 `setupTurns` 항목 `[p1, p2, {p1, p2}]`(`SetupTurn`), `run_decision_mid_turn`(중단 결과마다 해당 진영의 다음 선택을 `parse_mid_turn`으로 읽어 `resume_turn`; 선택이 없으면 중단 상태가 결과), `side_must_switch`; `enumerate.cjs`도 같은 규칙(`applyMidTurn`: `requestState === 'switch' && queue.list.length`). **남은 것:** 강제 교체(`forceSwitch` 포효·날려버리기·드래곤테일·서클스로 → `dragIn` 무작위, `DragOut` 이벤트), 탈출버튼·레드카드·탈출팩(`onAfterMoveSecondary`/`onAfterBoost`), 위기회피·겁쟁이(`onEmergencyExit`, `runSwitch` 뒤 판정 포함), 배턴터치·꼬리자르기(`copyVolatileFrom`), 텔레포트(`Past`)·차가운반응. MC(`--mc`)는 중단 결과를 그대로 둔다 |
| O37 | 완료 2026-09-26 (일부, F6에서) | `fee8833` | 위 F6 시나리오 | 유턴·볼트체인지·플립턴(핸들러 없음), 파팅샷(`onHit`: 하강 실패 시 `delete move.selfSwitch`, 미러아머 예외). 배턴터치·꼬리자르기·텔레포트·차가운반응은 거부 |
| F6 (2부) | 완료 2026-09-26 (배턴터치·꼬리자르기 제외) | `b2f4aa2` | `roar-drag`, `roar-no-bench`, `dragon-tail`, `eject-button`, `eject-button-uturn`, `red-card`, `emergency-exit`, `emergency-exit-below`, `emergency-exit-residual`, `emergency-exit-hazard`(setupTurns) / `tests/switching.rs` | `Slot.switch_flag: SwitchFlag { None, Move, Effect }`(Showdown `false`/기술 id/`true`). **강제 교체:** `Battle.force_switch`(일시적 `forceSwitchFlag`)를 `spread_move_hit` 6단계(대상·사용자 생존, 대상 진영 벤치 있음, 흡반(`ability_unless_broken`)은 `null`→끌어내지도 실패하지도 않음)와 레드카드가 채우고, 기술 행동 직후 `drag_outs` → `switching::drag_in`(벤치 균등 무작위, 즉시 `runSwitch`). `runMoveEffects`의 `forceSwitch` 절(벤치 없으면 실패 합산). **탈출버튼**(Champions 판: 공격자의 자기 교체 플래그를 지우지 않음; 다른 활성에 `Effect` 플래그가 있으면 불발; `use_item` 실패 시 취소), **레드카드**(`useItem` 뒤 공격자 `DragOut`, 공격자 본인 흡반만 막음), 둘 다 `items::after_move_secondary(b, user, target, category)`. **위기회피·겁쟁이** `switching::emergency_exit`(벤치·끌려 나가는 중·이미 플래그면 불발, 모든 활성 플래그를 지우고 `Effect`): 타격 루프 끝(`(hurtThisTurn||0)+curDamage > max/2`, 단일 대상은 `totalDamage`), `runSwitch` 뒤(`after_action(newcomers)`: 선택 교체·턴 중 교체 신참; 기절 후 교체 신참은 `Unsupported`), 잔여 뒤(`Pending.residual_done`: 잔여 → checkFainted → Update → 위기회피 → 요청 → 재개 뒤 `end_turn`만). 오라클 `applyMidTurn`은 **살아 있는 활성에 대한 `forceSwitch` 요청**을 턴 중 요청으로 본다(잔여 뒤 포함; 큐 비었는지로 판단하지 않음). 탈출팩은 Champions `Past` |
| O36 | 완료 2026-09-26 (F6에서) | `b2f4aa2` | `roar-drag`, `roar-no-bench`, `dragon-tail` | 포효·날려버리기·드래곤테일·서클스로(`forceSwitch`; 핸들러 없음). 흡반(`onDragOut`) 지원 |
| O94 | 완료 2026-09-26 (F6에서; 탈출팩 제외) | `b2f4aa2` | `eject-button`, `eject-button-uturn`, `red-card` | 탈출버튼·레드카드 `onAfterMoveSecondary`. 탈출팩은 Champions `Past` |
| O73 | 완료 2026-09-26 (F6에서) | `b2f4aa2` | `emergency-exit`, `-below`, `-residual`, `-hazard` | 위기회피·겁쟁이 `onEmergencyExit` 3지점. 기절 후 교체로 들어온 신참의 발동은 거부 |
| O70 | 완료 2026-09-26 (Opus) | `cbd90d9` | `o70-pixilate-aerilate`, `o70-galvanize-refrigerate`, `o70-normalize-liquid-voice` / `tests/abilities_library.rs` | 픽시스킨·스카이스킨·프리즈스킨·일렉트릭스킨·드라고나이즈(?)·노말스킨·촉촉보이스 `onModifyType`(우선도 -1)·`onBasePower` 1.2. `ActiveMove.type_changer`(Eq/Hash). 기술 타입을 읽는 흡수·면역 `onTryHit`·불가사의부적·플래시파이어·반감열매가 dex 타입 대신 실제 기술 타입을 읽도록 수정, `items::modify_damage_handlers`는 기술 타입을 받음 |
| 오라 | 완료 2026-09-26 (Opus) | `a3d38fc` | `aura-fairy-dark`, `aura-break` | 페어리오라·다크오라 `onAnyBasePower` 5448/4096(오라브레이크 아래 3072), `onAnyTryPrimaryHit`, `onStart` 메시지 |
| 플라워베일 | 완료 2026-09-26 (Opus) | `5e59f64` | `flower-veil-status-boost`, `-yawn-mold-breaker`, `-mirror-armor`, `-mirror-armor-faster` | `onAllySetStatus`·`onAllyTryAddVolatile`(하품)·`onAllyTryBoost`; 미러아머와의 속도 순서 재현 |
| 노가드 | 완료 2026-09-26 (Opus) | `f73c3dd` | `no-guard`, `micle-accuracy-true`, `micle-glaive-rush` | `onAnyAccuracy`(일격기에도 적용, Showdown 코드 그대로); `onAnyInvulnerability`는 반투명 상태가 없어 발동 불가(테스트로 고정). `ability_hooks::accuracy_event`로 Accuracy 이벤트 재구성. **수정:** 미클열매 휘발은 Accuracy 이벤트가 돌면 항상 끝난다(비수치 명중·빠른 글레이브러시 포함; 오라클 확인, F6 병합 때의 추정 폐기) |
| 저주받은바디 | 완료 2026-09-26 (Opus) | `0d7e5bf` | `cursed-body`, `cursed-body-multihit`(MC) | `onDamagingHit` 30% → `Volatile::Disable` |
| 매직미러 | 완료 2026-09-26 (Opus) | `d253afe` | `magic-bounce`, `magic-bounce-side` | `onTryHit`·`onAllyTryHitSide`; `ActiveMove.has_bounced`, `moves::bounce_move`(원 우선도, 프랭크스터 없음, 프레셔 PP·구애 잠금 없음). `try_hit`가 `Result` 반환. 병합 시: 되돌아온 파팅샷은 반사자를 교체시킴(`self_switch` 복사) |
| 트랩 | 완료 2026-09-26 (Opus) | `59dcec6` | `trap-arena-magnet`, `trap-shadow-tag`, `trap-arena-levitate`, `trap-arena-gravity`(+ `expected/*.trapped.json`) | 섀도태그·개미지옥·자석의힘 `onFoeTrapPokemon`, 탈피껍질 `onTrapPokemon`. `turn::trapped(state, slot)`(Showdown `pokemon.trapped`), `Ruleset::validate_slot_action`이 `ActionError::Trapped`로 교체 선택 거부(`check_turn`·`joint_actions`; 기절 후 교체는 무관). 새 오라클 `oracle/trapped.cjs`(Showdown의 trapped 플래그 덤프). `onFoeMaybeTrapPokemon`은 표시용이라 무시 |
| O72·O98 | 완료 2026-09-26 (Opus) | `94792d5` | `o72-protosynthesis-sun`, `o72-best-stat-intimidate`, `o72-quark-drive-booster`, `o72-sun-ends`, `o98-booster-knock-off` | 고대활성·쿼크차지·부스트에너지 전 핸들러. 휘발 2종 추가(31종; `counter`=최고 능력치, `hidden`=부스트에너지 여부, 정규 출력 제외). 패러독스 종 목록은 dex `tags`가 Rust 표에 없어 하드코딩. 에어록·노천기 옆의 고대활성은 특성 End 이벤트가 없어 거부 |
| O71 | 완료 2026-09-26 (Opus) | `364a939` | `o71-trace-switch`, `o71-trace-switch-notrace` | 배틀 중 교체로 들어온 트레이스가 거부되던 것을 수정: 무작위 상대 특성 복사, 복사한 특성이 필드에서 미지원이면 거부 |
| 기타 특성 | 완료 2026-09-26 (Opus) | `ad8d828` | `toxic-debris`, `perish-body-mummy`, `lingering-aroma`, `wind-rider` | 독치장·멸망의바디·미라·의뭉함(`onDamagingHit`; `damaging_hit`·`on_damaging_hit`이 `Result` 반환), 바람타기(`onTryHit`·`onSideConditionStart`·`onStart`). 떠도는영혼(스킬스왑 End/Start 이벤트 필요)은 거부 |
| (병합) | 2026-09-26 | `b4a737a` | | Opus 브랜치 `worktree-agent-af9fc3012519157a0`(4차 특성) 병합. 충돌은 `moves.rs`의 `ActiveMove` 필드 6곳(합집합: `self_switch` + `type_changer`·`has_bounced`); 매직미러 복사 생성자에 `self_switch` 추가. 병합 뒤 전체 테스트 373개 통과. 세션 보고: `boost_by`에서 흰안개가 미러아머보다 먼저 적용되는데 Showdown은 진영 조건 핸들러 속도 0이라 빠른 미러아머가 먼저 반사할 수 있음(미확인·미수정); 특성 End 이벤트(기절·퇴장 시) 미구현 |
| F17 | 완료 2026-09-26 | `50c912c` | `magic-room`, `magic-room-end`(setupTurns), `magic-room-mega`(setupTurns), `klutz` / `scenario/tests/suppression.rs` | `items::ignoring_item(state, slot)`(Showdown `ignoringItem`: 매직룸, 또는 서투름 + `ignoreKlutz`가 아닌 도구; 금제·원시구슬은 미구현). `Battle::item(slot)`은 **효과가 적용되는 도구**(억제 중 `NONE`; `hasItem`·도구 핸들러가 읽는 곳 전부), `Battle::raw_item(slot)`은 `pokemon.item` 자체(탁쳐서떨구기·트릭·곡예·언버든·끈적끈적바늘의 `!source.item`·메가진화·구애 잠금의 `getItem().isChoice`). 바꾼 자리: 잔여 도구 핸들러 수집, 등장 도구 핸들러(`singleEvent('SwitchIn')`이 억제된 도구를 건너뜀), 피격 도구, 타입 강화 도구, 방어·데미지 보정, 기합의띠, 생명의구슬 반동, `onUpdate` 열매, 선제공격손톱·커스탭·상수 분수 우선도(`fractional_priority_tenths(state, slot)`), 돌격조끼·구애 잠금의 선택 제한(`disabled_move`: 억제 중엔 제한 없음, 잠금 자체는 유지). 서투름은 `START_HANDLERS`(`onStart`는 End 메시지만; `onSwitchInPriority` 1)에 올라 거부가 사라졌다(`held_item_problem` 삭제). 매직룸: `FieldEffect::MagicRoom` 5턴, 재사용 시 종료, 잔여 순서 27·6, 정규 출력·패치 `magicroom`. 메가진화는 억제되지 않음(오라클 확인: `magic-room-mega`) |
| O100 | 완료 2026-09-26 (F17에서) | `50c912c` | `magic-room`, `magic-room-end` | 매직룸 |
| 목조르기 | 완료 2026-09-26 (Opus) | `342c3a2` | `throat-chop`, `-next`, `-sleep-talk` / `tests/moves_library.rs` | `throatchop` 휘발(2턴, 잔여 22): 소리 기술 선택·BeforeMove(6)·잠꼬대 호출 차단 |
| 방어 변형 | 완료 2026-09-26 (Opus) | `6c32181` | 7개(`spiky-shield` 등) | 니들가드·트래핑쉘·킹실드·블록·실크트랩·버닝불워크(휘발 6종, 공용 TryHit: 사이코필드·와이드/패스트가드 뒤 우선도 4; 접촉 페널티), 페인트·하이퍼스페이스홀(`breaksProtect` 지원). 방어의 첫 턴 역린 잠금 해제도 공용 핸들러로 |
| 배북류 | 완료 2026-09-26 (Opus) | `56bb8bf` | | 배북·소울비트·필렛어웨이·배수의진(`noretreat` 휘발; `conditions::trapped`가 교체 선택 거부, 고스트 예외) |
| 회복·보조 `onHit` | 완료 2026-09-26 (Opus) | `5811bd0`·`61112ca`·`069204e` | 다수 | 힐벨·아로마테라피(`allyTeam` 대상 경로, 초식 아군 공격 상승 포함)·리프레시·정화·마음의눈·정글힐링·달의기도·꽃의치유·잠자기; 자기암시·스피드스왑·힘흡수·고통분담·원한·리플렉트타입·소크·죽기살기(스피드스왑 위해 `Battle::clear_volatile`이 퇴장 시 능력치 재계산); 벌레먹기·쪼아먹기·태우기·부식가스·리사이클(`update::eat_item` 분리 → `berry_on_eat`). 거부: 할로윈·나무의저주(추가 타입 필드 없음), 등장 핸들러 있는 도구의 리사이클, 스킬스왑·파워/가드셰어·변신·흉내내기·스케치·텍스처2 |
| 위력 콜백 | 완료 2026-09-26 (Opus) | `c2e47c6` | 9개 | 병상첨병·인페르노퍼레이드·트리플악셀·트리플킥(`hit` 번호가 데미지 계산에 전달)·물수리검·일렉트릭볼·자이로볼·분화·해수스파우팅·드래곤에너지·기사회생·저항·괴력집게·마지막수단?아님: 하드프레스·어시스트파워·파워트립·응보·트럼프카드·은혜갚기·화풀이(친밀도 255 가정)·전격부리·에라가니 |
| 씨뿌리기·조이기류 | 완료 2026-09-26 (Opus) | `4068062` | 6개 | 씨뿌리기(시더 슬롯의 점유자에게 회복), 부분 구속(5–6턴, 그립클로 8, 1/8, 바인드밴드 1/6; 트래퍼 퇴장 시 종료·교체 불가), 고속스핀이 둘도 제거, 기가톤해머·블러드문(연속 사용 불가). 그립클로·바인드밴드 지원 |
| 나머지 기술 | 완료 2026-09-26 (Opus) | `05586ff` | 13개 | 사이코팽·브릭브레이크·레이징불(벽 파괴), 무한다이노·스톤액스(타격 시 함정), 만천스핀, 무릎차기·점프킥·액스킥·슈퍼셀슬램(`onMoveFail` 자멸), 대폭발·자폭·미스티익스플로전·순간포착·목숨걸기, 일격기 4종, 길동무(기절 큐가 가해자를 기록, `faint_messages`에서 Faint 훅). 고속스핀은 사용자가 기절해도 함정 제거(4068062의 '생존 필요' 판정 철회) |
| (병합) | 2026-09-26 | `e9ecdd9` | | Opus 브랜치 `worktree-agent-a5868f587f56055f3`(4차 기술) 병합. 충돌 5개 파일: `damage()`(길동무 가해자 + F13 `record_hurt`), `support.rs`(일격기·자폭 지원, F6 게이트 유지, 대상 match 완전열거), 휘발 42종(`showdown_state`의 숨김 팔 통합), `handlers.rs`(`damage_callback` 통합: 죽기살기·목숨걸기·메탈버스트·카운터펀치; 위력 콜백은 K 구조 `power.max(1)`에 F13 팔 삽입, 곡예는 원시 도구), `moves.rs`(BeforeMove: 재충전 null + 길동무; TryHitSide: 매직미러 + 초식). `try_hit`의 `Result` 반환에 맞춤. 트랩 통합: `turn::trapped`가 `conditions::trapped`(배수의진·부분 구속)도 포함해 `joint_actions`가 그 교체를 만들지 않음 |
| F9 (2부) | 완료 2026-09-26 | (다음 커밋) | `solar-beam-charge`, `solar-beam-hit`(setupTurns), `solar-beam-sun`, `power-herb`, `fly-charge`, `fly-hit`(setupTurns), `dig-earthquake`, `meteor-beam`, `charge-abort`(setupTurns) / `scenario/tests/two_turn.rs` | **2턴 기술.** 휘발 `TwoTurnMove`(duration 2, `mv`, `counter`에 선택한 대상 위치 `targetLoc`(숨김); `onEnd`→기술 자체 휘발 제거, BeforeMove 실패 시 `onMoveAborted`로 즉시 제거)와 기술 자체 휘발 12종(`solarbeam`·`solarblade`·`meteorbeam`·`electroshot`·`skyattack`은 지속 없음; `fly`·`bounce`·`dig`·`dive`·`phantomforce`·`shadowforce`는 duration 2·반투명). 휘발 54종. `handlers::charge_try_move`(기술 자체 `onTryMove`, 특성 TryMove보다 먼저): 두 번째 턴이면 자체 휘발 제거 후 진행; 아니면 메테오빔·일렉트로샷 특공 +1 → 쾌청(솔라빔·솔라블레이드)/비(일렉트로샷; `weather_for`) 건너뜀 → 파워풀허브(`ChargeMove`, `use_item`) → `twoturnmove`+자체 휘발 시작 + PrepareHit(변환자재) → 중단. `lock::Locked::TwoTurn { id, target }`: 두 번째 턴 선택은 그 기술·저장된 대상 위치로 정규화(PP 미차감; Showdown 요청도 그 기술만 제공하므로 오라클 시나리오는 잠긴 기술을 그대로 적음). `handlers::invulnerable`(`hitStepInvulnerabilityEvent`, 0단계): 반투명 대상은 예외 기술(플라이·바운스: 돌풍·회오리·스카이어퍼·번개·폭풍·떨어뜨리기·사우전드애로; 구멍파기: 지진·매그니튜드; 다이빙: 파도타기·소용돌이)·노가드·독 타입의 맹독만 통과, 나머지는 실패; 예외 기술은 2배(`volatile_modify_damage`, 바운스는 `target_volatile_base_power`); 구멍파기·다이빙은 모래 피해 면역. 솔라빔·솔라블레이드 `onBasePower`(비·모래·눈 반감). `ActiveMove.target_loc`(선택한 대상 위치). 미구현: 스카이드롭·로케트박치기·바람일으키기·프리즈볼트·콜드플레어·지오컨트롤(Past 또는 미지원), 반투명 대상에 대한 `onAnyInvulnerability` 외 예외, 록온·마인드리더 |
| O97 | 완료 2026-09-26 (F9에서) | (다음 커밋) | `power-herb` | 파워풀허브 `onChargeMove` |
