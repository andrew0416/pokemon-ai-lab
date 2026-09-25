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
| F7 | 완료 2026-09-26 | `2e604ec` | `followme-hypnosis`, `ragepowder-grass`, `lightningrod-foe`, `lightningrod-ally` / `scenario/tests/redirect.rs` | `moves.rs::redirect_target`(`RedirectTarget` priority event: 우선도→속도, 첫 유효 대상), 휘발 `FollowMe`·`RagePowder`·`Spotlight`(1턴), 피뢰침·저수 `onTryHit`(흡수 + 특공 +1). 스포트라이트는 코드만(시나리오 없음). 같은 우선도·속도의 유효 유도자 둘(Showdown은 `effectOrder`)은 `Unsupported`. 스토커·프로펠러테일은 미지원(`onModifyMove`) |
