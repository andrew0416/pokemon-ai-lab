# 턴 엔진 미구현 전수조사 (Showdown 대비)

작성 2026-09-25. 기준: `vendor/pokemon-showdown` 커밋 `9e317a6`(Champions 모드), 엔진 커밋 `40e78c3` 이후 작업 트리.
방법: (1) `sim/*.ts`에서 발생하는 모든 이벤트·행동 종류·상태 필드를 기계적으로 추출해 엔진 구현과 대조했다. (2) dex 전체(기술 938·특성 321·도구 583)를 엔진의 지원 검사(`core/src/turn/support.rs`)에 실제로 통과시켜 분류했다(`lab-coverage` → [COVERAGE.md](COVERAGE.md)). 수치는 전부 그 실행 결과다.

엔진이 "구현하지 않은" 것은 두 종류다. **거부**는 `support.rs`가 턴 실행 전에 `TurnError::Unsupported`로 막는 것이고, **무시**는 Showdown이 이벤트를 발생시키지만 엔진에는 해당 훅 자체가 없어 조용히 지나가는 것이다. 후자는 그 이벤트에 반응하는 기술·특성·도구가 모두 거부 목록에 있을 때만 안전하다. 아래에서 "무시(안전)"는 그 조건이 확인된 경우, "무시(위험)"는 거부 목록으로 완전히 막히지 않는 경우다.

> **2026-09-27 갱신:** 이 문서의 §3–§5는 2026-09-25 착수 시점의 전수조사 스냅숏이다. 그 뒤 12차례 병합으로 라이브러리 합법 범위는 100%가 되었고 (기술 788/938·특성 309/321·도구 497/583; 나머지 미지원은 Champions 모드에서 `Past`·`CAP`·G맥스·LGPE인 것과 딧토 변신/임포스터·조로아크 일루전 — 웨이브 12 EE 진행 중), 행동 종류는 테라·다이맥스(규칙셋 밖)만 남았다. **현재 상태는 자동 생성 `COVERAGE.md`와 `WORKPLAN.md` §4가 정본**이며, 아래 §1·§2 표만 2026-09-27 수치로 고쳤다. §3 이벤트별 상태는 고치지 않았다(항목별 완료는 WORKPLAN §4 F·O·B 행 참조).

## 1. 요약 수치

| 범주 | 전체 | 지원 | 비고 |
|---|---:|---:|---|
| 기술 | 938 | 788 (2026-09-27) | 라이브러리(팀 파일 74개·444세트) 사용 206종 전부 지원. 미지원 150 = Past 109·G맥스 33·LGPE 6·CAP 1·변신 1(EE1 진행) |
| 특성 | 321 | 309 (2026-09-27) | 라이브러리 사용 66종 전부 지원. 미지원 12 = CAP 3·Past 종에만 있는 것 6(멀티타입·RKS·원시 날씨 3·테라 2)·임포스터·일루전(EE 진행) |
| 도구 | 583 | 497 (2026-09-27) | 라이브러리 사용 60종 전부 지원. 미지원 86 = Past 85·CAP 1. 메가진화 행동 구현됨(F3) |
| 핵심 이벤트 | 119종 | (§3은 2026-09-25 스냅숏) | 항목별 완료는 WORKPLAN §4 |
| 행동 종류 | 15 | 12 (2026-09-27) | 남은 것: `terastallize`·`runDynamax`(규칙셋 밖), `shift`(트리플, 해당 없음) — §2 |

(2026-09-25 당시 라이브러리 상위 미지원 목록은 삭제 — 전부 구현됨. 이후 미지원 목록은 `COVERAGE.md` "나머지 미지원 (이유별)".)

## 2. 결정·행동 종류 (`battle-queue.ts` action choices)

| Showdown 행동 | 엔진 | 비고 |
|---|---|---|
| `team` (팀 프리뷰) | 로더가 순서 문자열로 처리 | VGC 4마리 선출 포함(O104, 2026-09-26) |
| `start` (첫 등장) | `turn/switching.rs`로 통합(F4) | 등장 특성·도구 전부, 동시 등장 속도순 |
| `move` | 있음 | |
| `switch` | 있음 | 등장 효과는 `switching.rs` 하나로 처리(F4) |
| `instaswitch` (기절 후 교체) | 있음(F5, 2026-09-26) | `enumerate_replacements`·`request: switch` 결정 단계 |
| `megaEvo` / `megaEvoX` / `megaEvoY` | 있음(F1–F3, 2026-09-26) | `turn/mega.rs`; 메가 자격은 F17 도구 억제와 무관하게 `getItem()` 직접 참조를 재현 |
| `terastallize` | 거부(규칙셋) | M-C 범위 밖. 구조만 유지 |
| `runDynamax` | 거부(규칙셋) | 같음 |
| `beforeTurnMove` (`beforeTurnCallback`: 카운터·미러코트·메탈버스트·속임수 등) | 있음 | 라이브러리 사용 메탈버스트 포함 |
| `priorityChargeMove` (`priorityChargeCallback`: 기합펀치·초점 기술) | 있음 | |
| `shift` (트리플) | 해당 없음 | |
| `revivalblessing` | 있음(F12 슬롯 조건) | 라이브러리 사용 3 |
| `pass` | 있음 | |
| `beforeTurn` (`BeforeTurn` 이벤트) | 있음 | 발동자는 `beforeTurnCallback` 기술 |
| `residual` | 있음(부분) | §3.6 |

## 3. 턴 진행 이벤트 전수 (`runEvent`/`singleEvent`/`priorityEvent`/`eachEvent`/`fieldEvent` 119종)

각 항목: **이벤트** — 엔진 상태 — 반응하는 데이터(dex `handlers` 기준, 괄호는 종수).

### 3.1 행동 시작·순서

- **OverrideAction** — 무시(안전). 앵콜(거부)만 사용.
- **ModifyPriority** — 부분: 그래스슬라이더, 짓궂은마음만. 미구현: 질풍날개(1), 트라이어지(1), 성냥불(마이 퍼스트)… 특성 3종·기술 1종.
- **FractionalPriority** — 무시(안전): 후발·스톨(특성)·매크로렌즈?… 실제로는 `fractional_priority_tenths != 0`인 도구·특성을 거부. 선제의손톱·커스탭열매(`onFractionalPriority`)도 거부.
- **LockMove / SemiLockMove** (`lockedmove`, 롤아웃, 아이스볼, 바이드, 대폭발계) — 없음. 해당 기술 거부.
- **BeforeMove** — 부분: 수면·얼음·풀죽음·중력·마비만. 미구현: 혼란, 헤롱헤롱, 도발(`condition.onBeforeMove`), 사슬묶기·봉인(`onFoeBeforeMove`), 게으름(Truant), 파괴광선 재충전(`mustrecharge`), 회상(Imprison), 목조르기, 굴레이브러시 등 12기술·2특성·6조건. 전부 거부.
- **MoveAborted** — 무시(안전). 두턴기술(거부)만.
- **DeductPP / 프레셔** — 없음. 프레셔 거부.
- **TryMove** (기술 자체 시도: 솔라빔·전자포 충전, 오로라베일 등) — 오로라베일 날씨 검사만 하드코딩. 22기술 거부.
- **UseMoveMessage** — 로그용. 해당 없음.

### 3.2 대상 결정

- **ModifyTarget** — 없음. (2기술, 거부)
- **RedirectTarget / FoeRedirectTarget / AnyRedirectTarget** — **없음**. 날따름·분노가루·스포트라이트(`condition.onFoeRedirectTarget`), 피뢰침·저수·노말스킨…(`onAnyRedirectTarget` 특성 2: 피뢰침·저수). 전부 거부. **중력 파티의 핵심 요구(날따름)가 여기 있다.**
- `getMoveTargets`의 `smartTarget`(드래곤애로우) — 거부.
- `tracksTarget`(스토커·프로펠러테일·Stalwart) — 거부.
- 광역 대상이 하나만 남을 때 `spreadHit=false`로 위력 0.75가 사라지는 규칙 — 구현됨.

### 3.3 명중 단계 (`trySpreadMoveHit` 7단계)

1. **Invulnerability** (하늘로 날기·구멍파기·다이빙·고스트다이브·스카이드롭 등 `condition.onInvulnerability`, 노가드 `onAnyInvulnerability`) — 없음. 해당 기술·특성 거부. `commanding`(시달림·닌자) 없음.
2. **TryHit** — 부분: 방어(volatile `protect`), 사이코필드만. 미구현: 판별 외 방어류(킹실드·니들가드·토치카·블로킹·스레드트랩·버닝바리케이드·맥스가드), 와이드가드·패스트가드·크래프티실드·매트블록(`condition.onTryHit` 진영), 마법의거울·매직코트(`onTryHit` 반사), 흡수 특성(축전·저수·마른피부·모터드라이브·피뢰침·타오르는불꽃·부풀리기·흙먹기·굳은빵·전기엔진·불가사의부적·방탄·텔레파시·상냥함(Good as Gold)·오버코트·방진…)(특성 `onTryHit` 22종), 도구 `onTryHit`(1). 기술 `onTryHit` 56종(트릭·하품·도발·뿔드릴 등) 전부 거부.
3. **타입 면역** (`runImmunity`) — 구현. **NegateImmunity**(작은 신비, 기적의눈 `condition.onNegateImmunity`, 뿌리박기 `onNegateImmunity`) — 없음, 거부. 검은철구 접지·자기부상·텔레키네시스·뿌리박기·스맥다운·부유 억제(`isGrounded` 전체 분기 중 중력·비행·부유만) — 부분.
4. **TryImmunity** (기술별 면역: 10종 `onTryImmunity`) — 가루·짓궂은마음 악만. 나머지 거부.
5. **Accuracy / ModifyAccuracy / SourceModifyAccuracy / AnyAccuracy** — 랭크·중력만. 미구현: 노가드, 복안·승리의별·의욕(`onSourceModifyAccuracy`), 눈숨기·모래숨기(`onModifyAccuracy`), 날씨 기반 필중(눈보라·폭풍·번개 `onModifyMove`), 광각렌즈·줌렌즈·마이크로렌즈(`onSourceAccuracy`), 정신력?… 특성 4+2종·도구 4종·기술 3종. 거부. **OHKO 명중 규칙** — 거부. 블런더폴리시 — 거부.
6. **BreakProtect** (페인트·섀도다이브·페이탈클로…) — 거부.
7. **StealBoosts** (스펙트럴시프) — 거부.

### 3.4 타격 루프 (`hitStepMoveHitLoop` → `spreadMoveHit`)

- **연속기** (`multihit` 31종, 로디드다이스, 스킬링크) — 전부 거부. 트리플킥/트리플악셀 `multiaccuracy` 거부.
- **TryPrimaryHit** (대타출동 `condition.onTryPrimaryHit`, 어미마음 `onAnyTryPrimaryHit`, 배리어/지느러미… 도구 `onSourceTryPrimaryHit` 18종=반감열매 계열이 아니라 열매? 실제로는 노멀젬 등) — 대타·대타 HP 없음. 거부.
- **getDamage**: `damageCallback`(11: 카운터·미러코트·메탈버스트·엔드에버·사이코웨이브·필살…) 거부. `basePowerCallback`(53: 잠재파워·짓밟기·리벤지·아크로바틱·헤비봄버·전기구슬·씨폭탄?…) — 로킥·풀묶기만 하드코딩. 나머지 거부.
- **ModifyCritRatio / CriticalHit / 급소** — 기술 `critRatio`, `willCrit`, `onCriticalHit: false` 특성만. 미구현: 기합모으기(`focusenergy`), 초점렌즈·예리한부리?·럭키펀치·긴파(`onModifyCritRatio` 도구 5·특성 2: 대운·스나이퍼는 `onModifyDamage`), 럭키챈트(`condition.onCriticalHit`), 아르마딜로(Merciless). 거부.
- **BasePower / SourceBasePower / AnyBasePower / AllyBasePower** — 타입 강화 도구(플레이트·메모리 제외), 필드, 탁쳐서떨구기만. 미구현: 테크니션·예리함·철주먹·강철정신·터프클로·무모… 특성 `onBasePower` 21종, 기술 `onBasePower` 27종(그라비애플·와이드포스·라이징볼트·솔라빔 등), 도구 플레이트류(`onBasePower`+`onTakeItem`), 도우미(`condition.onBasePower`), 배터리·파워스팟(`onAllyBasePower`), 오라계·다크오라(`onAnyBasePower`). 거부.
- **ModifyAtk/Def/SpA/SpD (+Source/Ally/Any)** — 모래 특방·설경 방어만. 미구현: 맹화·급류·심록·근성·의욕·순수한힘·천하장사·두꺼운지방·용의턱·트랜지스터·플라워기프트·마벨스케일·퍼코트·쓱쓱?… 특성 `onModifyAtk` 22·`onModifySpA` 19·`onModifyDef` 5·`onModifySpD` 2·`onSource*` 8·`onAlly*` 2·`onAny*` 4, 도구 구애머리띠·안경·돌격조끼·심해의이빨·전기구슬·굵은뼈·심안?(`onModifyAtk/SpA/Def/SpD` 11), 진화의휘석(`onModifyDef/SpD`). 거부. **원더룸**(방어·특방 교환) 없음.
- **ModifyBoost / AnyModifyBoost** (천진 Unaware, 단순 Simple, 심술꾸러기는 boost 단계) — 없음. 거부.
- **WeatherModifyDamage** — 쾌청·비만. 프라이멀 날씨(끝의대지·시작의바다·델타스트림) 없음(로더·패치가 거부).
- **STAB / ModifySTAB** (적응력) — 거부. 테라·스텔라 STAB — 범위 밖.
- **Effectiveness / AnyEffectiveness** (프리즈드라이·플라잉프레스·천벌?·검은철구·링타겟·화신 껍질·Tera Shell) — 없음. 해당 기술 4·특성 2·도구 1 거부. **`runEffectiveness`의 Stellar 분기** 범위 밖.
- 화상 물리 반감 — 구현(근성 예외는 특성 거부). 페이셰이드 예외 — 거부.
- **ModifyDamage / SourceModifyDamage / AnyModifyDamage** — 생명의구슬·리플렉터·빛의장막·오로라베일만. 미구현: 달인의띠·메트로놈·반감열매 계열(`onSourceModifyDamage` 도구 18종: 오카·이아열매…·로젤·콜버), 필터·하드록·프리즘아머·멀티스케일·섀도실드·아우라가드·아이스스케일·솜털(`onSourceModifyDamage` 특성 10), 스나이퍼·색안경(`onModifyDamage` 특성 3), 날카로운 눈?… 거부. `bypassProtect`(Z기술 방어 관통 0.25) — 범위 밖.
- **Damage** (피해 자체 개입: 기합의띠 구현) — 미구현: 옹골참·틀깨기?·매직가드·가면(Disguise)·아이스페이스·맞바람? 특성 `onDamage` 11(옹골참·매직가드·록헤드·디스가이즈·아이스페이스…), 기술 `onDamage` 4(버티기 `endure`), 도구 기합의머리띠(`onDamage`, 확률). 거부.
- **spreadDamage의 흡수/반동** — 구현. 액체오브·큰뿌리(`onTryHeal`/`onSourceTryHeal`) 거부.
- **runMoveEffects**: `boosts`·`heal`·`status`·`volatileStatus`(protect·flinch·stall만)·`sideCondition`(4종)·`slotCondition`(**없음**: 소원·치유소원·달의춤·리바이벌블레싱)·`weather`·`terrain`·`pseudoWeather`(중력·트릭룸만; 매직룸·원더룸·페어리록·플라즈마샤워·머드스포트·워터스포트 없음)·`forceSwitch`(**없음**)·`onHit`(178기술: 거부)·`onHitField`(6)·`onHitSide`(4)·`selfdestruct`(거부)·`selfSwitch`(거부).
- **SetStatus / AfterSetStatus / AllySetStatus / AnySetStatus** — 필드(일렉트릭·미스트)만. 미구현: 면역 특성(마이페이스·불면·둔감·수의베일·플라워베일·리프가드·순수한몸?·정화의소금·열교환·마그마의무장…) `onSetStatus` 12·`onAllySetStatus` 3, 신기루?, 럼열매·치료열매(`onAfterSetStatus`), 세이프가드(진영 조건, `onSetStatus`). 거부. **동기(Synchronize)** 없음.
- **TryAddVolatile / AllyTryAddVolatile** — 없음. 정신력(풀죽음 면제)·마이페이스(혼란)·플라워베일·아로마베일·불면(하품)·필드(하품·혼란) 거부.
- **TryBoost / AfterBoost / AfterEachBoost / ChangeBoost / FoeAfterBoost / AllyTryBoost** — 없음. 클리어바디·괴력집게·날카로운눈·미러아머·경쟁심·오기·천진?·하양허브·미러허브·거울갑옷·심술꾸러기(`onChangeBoost`)·단순(`onChangeBoost`)·잘난척(`onAfterBoost`)… 거부. `boost` 자체는 구현(±6 클램프).
- **selfDrops / secondaries / ModifySecondaries / SourceModifySecondaries** (하늘의은총·방탄조끼?·세이프고글?, 실드더스트·커버트클록) — 부가효과 확률 자체는 구현. 확률 변경·차단 특성·도구 거부. `secondary.onHit`(다이어클로·목조르기·트라이어택 상태 선택) 거부. 힘의 방출(Sheer Force) 거부.
- **DamagingHit / SourceDamagingHit** — 불꽃 기술의 얼음 해제만. 미구현: 접촉 특성(까칠한피부·철가시·정전기·불꽃몸·독가시·독수?·저주받은바디·미라·하늘의은총?·컬러체인지?·위크아머·스태미나·전기교환?·시드소워?·샌드스핏·토식데브리·베어내기?…) 32종, 울퉁불퉁멧·붉은실·에어벌룬 파열·잼?·약점보험·목표물…(`onDamagingHit` 도구 9). 거부. 방향성: `DamagingHit`은 이벤트 하나에 데이터 41종이 걸린 최대 공백이다.
- **AfterHit / AfterSubDamage** — 탁쳐서떨구기만. 아이스스피너·무모한칼날?·불필요?(`onAfterHit` 8, `onAfterSubDamage` 11) 거부.
- **AfterMoveSecondary / AfterMoveSecondarySelf / AnyAfterMove / AfterMove** — 얼음 해제·생명의구슬만. 미구현: 탈출버튼·붉은카드(`onAfterMoveSecondary`), 조개껍질의방울?(`onAfterMoveSecondarySelf` 도구 3), 스파클링아리아·전기충격… 기술 `onAfterMove` 6, 힘의 방출 무효화, `AnyAfterMove`(하양허브·초코?)… 거부.
- **EmergencyExit** (위기회피·도망태세, 9곳에서 발생) — 없음. 특성 거부.
- **Hit / HitField / HitSide** — `onHit` 178종 거부. 방어의 `onHit`(stall 추가)만 하드코딩.
- **MoveFail** (`onMoveFail`: 무릎차기·점프킥 반동 등 7) — 거부.
- **TryHeal / Heal** — `heal` 원시 연산은 구현. 힐블록(`healblock`)·액체오브 없음.
- **DragOut / AnyDragOut** (강제 교체, 흡반·뿌리박기·고스트 예외) — 없음. 거부.
- **AfterFaint / BeforeFaint / Faint / AllyFaint / AnyFaint / SourceAfterFaint** (자기암시?·독가스?·비스트부스트·소울하트·체스트?·리시브·가루?…: 특성 `onSourceAfterFaint` 8·`onAnyFaint` 1·`onAllyFaint` 2·`onFaint` 1, 기술 `onFaint` 3=길동무·원한·저주?) — 없음. 거부. 기절 처리 순서(`faintMessages`)·승패는 구현.

### 3.5 교체·등장

- **BeforeSwitchOut / SwitchOut** (자연회복·재생력·`onSwitchOut` 특성 3, 사슬묶기 `onFoeBeforeSwitchOut`) — 없음. 거부.
- **BeforeSwitchIn** (`onBeforeSwitchIn`: 일루전 1) — 없음. 거부.
- **SwitchIn / AnySwitchIn** (`fieldEvent`): 특성·도구 `onStart` 68+14종 중 날씨 4·필드 4·위협만. 미구현 등장: 다운로드·인티미데이트 계열 반응(고집·괴력집게·마이페이스·오기·경쟁심·거울갑옷·불굴의검·불굴의방패·코스타·크리어스모크?·프리스크·예지·긴장감·틈새?·프레셔·틀깨기·터보블레이즈·테라볼티지·에어록·노가드?…·엠바디아스펙트·헌팅?…·트레이스(턴 중)·하품?…), 도구 `onStart`(시드 4종·부스트에너지·룸서비스·에어벌룬·구애스카프 등 14), `onAnySwitchIn`(하양허브·트레이스? 3+3). 진영 조건 `onSwitchIn`(스텔스록·압정뿌리기·독압정·끈적끈적네트·치유소원·달의춤: `condition.onSwitchIn` 7) — **함정·소원류 전부 없음**. 상태 `tox.onSwitchIn`(맹독 스테이지 초기화) — 없음(맹독 스테이지는 있음). 특성 `onSwitchIn`(6: 위협 외 `onSwitchIn` 명시형)·도구(2) 거부.
- `runSwitch`의 등장 속도 정렬 — `scenario/switch_in.rs`에만. 턴 중 여러 교체가 같은 턴에 있을 때 등장 효과 순서(속도순, `speedOrder` 보정) — 턴 엔진은 교체를 한 번에 하나만 처리하므로 **동시 등장 순서 미구현**.
- **일루전·변신(`transformInto`)·폼체인지(`formeChange`, `setSpecies`)·원시회귀·배틀본드·다루마모드·쉴드다운·스쿨링·아이스페이스·디스가이즈·제로투히어로·꿀꺽미사일** — 종 변경 인프라 없음(`Pokemon.species` 변경 명령 없음, `types` 변경 명령 없음).
- **SetAbility / 특성 변경** (트레이스·스킬스왑·역할연기·개그·심플빔·벌레의알림·심술?: `SetAbility` 이벤트 3곳, `AfterSetAbility`) — 명령(`SetAbility`)은 있고 트레이스는 등장 코드에만. 기술은 거부.
- **TakeItem / AfterTakeItem / UseItem / AfterUseItem / Eat / TryEatItem / EatItem / FoeTryEatItem / AllyAfterUseItem** — `use_item`·`take_item` 원시 연산만(기합의띠·탁쳐서떨구기). **열매 전체**(`onEat` 57, `onUpdate` 35: 오랭·럼·치료·반감열매·핀치열매), 트릭·스위치·도둑질·플링·자연의선물·리사이클·배주밸리?·긴장감·부풀리기?·먹보?·수확·심퍼시(`onTryEatItem`)·쨈?(`onAfterUseItem`)·곡예(Unburden `onAfterUseItem`)·클러치?… 전부 없음. `lastItem`·`ateBerry`·`usedItemThisTurn`·`itemKnockedOff` 중 `last_item`만 있음.
- **Update** (`eachEvent('Update')` 9곳: 열매·트레이스·가면·열교환·마그마…·상태 치료 도구) — **없음**. 열매의 발동 타이밍이 전부 이 이벤트다.

### 3.6 턴 종료 (`residual` → `endTurn`)

- 구현: 날씨 지속·모래 피해(`Weather`), 필드·중력·트릭룸·순풍·벽 지속 감소, 그래스필드 회복(`condition.onResidual`), 먹다남은음식, 화상·독·맹독, 휘발 지속(방어·스톨·풀죽음), 정렬(order/priority/speed/subOrder, 동속 무작위).
- 미구현 잔여 효과: **Weather 이벤트** 중 설경 피해 없음(9세대 눈은 피해 없음 → 맞음), 싸라기눈(`hail`, 과거) 없음, 우천회복·아이스바디·마른피부·선파워(`onWeather` 특성 4) 거부. **Residual 핸들러**: 씨뿌리기·저주·악몽·소금절이·멸망의노래·소원·미래예지·파멸의소원·앵콜·도발·마법의방·원더룸·세이프가드·안개·미스트?·태그?·검은진흙·맹독구슬·화염구슬·끈적끈적?·플레임오브·독구슬·하양허브?·수확·픽업·가속(`onResidual` 기술 20·특성 16·도구 11·조건 5) — 거부. `slotCondition`(소원·치유소원·달의춤·리바이벌블레싱)의 `onResidual`/`onSwitchIn` — 없음. 잠꼬대·불면?… 
- **FieldResidual / SideResidual / onFieldEnd / onSideEnd / onEnd**의 종료 효과 — 지속 감소만 하고 `onEnd`류는 없음(현재 구현 효과에 종료 부수효과 없음이라 안전).
- **endTurn**: `moveThisTurn`·`newlySwitched`·`moveLastTurnResult`·`usedItemThisTurn`·`statsRaised/LoweredThisTurn`·`hurtThisTurn`·`attackedBy`·`activeTurns`·`faintedThisTurn/LastTurn` 초기화 — **필드 자체가 없음**(짓밟기·리벤지·복수·되갚기·리벤지·먹보?·어시스트?·앵콜 등의 전제). **DisableMove** 이벤트 — 속이기·중력만. 미구현: 도발·앵콜·트집·회상·힐블록·사슬묶기·구애 도구(`choicelock`)·어시스트볼트?·기가임팩트 재충전·블러드문/기가임팩트 `cantusetwice`·재충전(`mustrecharge`)·트로피컬킥?… 특성 `onDisableMove`(1: 개굴닌자?)·도구(1: 돌격조끼)·조건 `onDisableMove`(1). **TrapPokemon / MaybeTrapPokemon / FoeMaybeTrapPokemon** (개미지옥·그림자밟기·자력·검은눈빛·거미집·옥토락·조이기류) — 없음. 교체 가능 여부 판단에 함정이 반영되지 않음. 타입 공개(`knownType`) — 정보 게임 요소, 미구현. **Endless Battle Clause**·`tiebreak` — 없음.
- `turn` 증가는 교체 요청이 없을 때만(Showdown과 같음).

### 3.7 필드·진영 상태

- 날씨: 쾌청·비·모래·눈만. 프라이멀 3종·`suppressWeather`(에어록·노가드? 실제 `suppressWeather: true` 특성 2: 에어록·클라우드나인)·양산(`utilityumbrella`) 없음. **SetWeather / AnySetWeather** 이벤트(에어록?, 데스니?…) 없음. 날씨 바위 8턴 구현.
- 필드: 4종 구현(위력·수면/혼란 차단·사이코 우선기 차단·풀 회복·풀 위력·미스티 드래곤 반감). **TerrainChange**(시드 4종·미미키?·퓨어?) 없음. 아이스스피너·스플린터?의 필드 제거 없음(거부). 필드 확장기 8턴 구현.
- 유사 날씨: 중력(명중·비행 접지·비행 기술 사용 금지 구현), 트릭룸(속도 반전 구현). 매직룸·원더룸·페어리록·플라즈마샤워·머드스포트·워터스포트 없음(`FieldEffect` enum에는 룸 2종 자리만 있음).
- 진영 조건: 리플렉터·빛의장막·오로라베일·순풍 구현. 세이프가드·흰안개·행운의주문·와이드가드·패스트가드·크래프티실드·매트블록·스텔스록·압정·독압정·끈적끈적네트·소원(슬롯) 없음. `SideConditionStart`(꼬마돌?·스크린클리너?) 없음.

### 3.8 포켓몬 상태 필드 중 엔진에 없는 것 (`sim/pokemon.ts` 필드 대조)

`hpType/hpPower`(잠재파워, Champions에 없음), `showCure`, `baseStoredStats`/`storedStats` 재계산(메가·폼체인지·파워트릭·파워시프트·스피드스왑·가드스플릿), `trapped/maybeTrapped/maybeDisabled/maybeLocked`, `illusion`, `transformed`, `subFainted`, `formeRegression`, `addedType`(할로윈·숲의저주)/`types` 변경(변환·소크·리플렉트타입·프로틴)/`knownType/apparentType`, `switchFlag/forceSwitchFlag`(유턴·볼트체인지·바톤터치·쉐드테일·강제교체·탈출버튼·붉은카드), `draggedIn/newlySwitched/beingCalledBack`, `lastMoveEncore/lastMoveUsed/lastMoveTargetLoc/moveThisTurn`, `statsRaisedThisTurn/statsLoweredThisTurn`, `moveLastTurnResult/moveThisTurnResult`(짓밟기·게으름·`cantusetwice`), `hurtThisTurn`(위기회피 판정), `lastDamage/attackedBy/timesAttacked`(카운터·미러코트·메탈버스트·리벤지·복수·되갚기·화풀이·레이지피스트), `activeTurns`(속이기는 `activeMoveActions`로 처리됨·게으름·잠재?…), `truantTurn`, `bondTriggered/heroMessageDisplayed/swordBoost/shieldBoost/syrupTriggered`, `stellarBoostedTypes`, `ateBerry/usedItemThisTurn/itemKnockedOff`, `ppUps`(Champions에서 항상 0), `canMegaEvo/canUltraBurst/canTerastallize/teraType/terastallized`(규칙셋·자격 표로 대체), `staleness`(Endless Battle Clause), `modifiedStats/modifyStat`(Gen 1), `m`(기술별 임시 저장: 롤아웃 횟수·스톡파일 등).

### 3.9 특성·도구·기술의 콜백 우주 (dex `handlers` 집계)

기술 81종·특성 106종·도구 55종·조건 27종의 콜백 이름이 있다. 엔진이 하드코딩으로 흉내 내는 콜백: `onModifyPriority`(그래스슬라이더), `basePowerCallback`(로킥·풀묶기), `onTry`(속이기), `onDisableMove`(속이기), `onPrepareHit`/`onHit`/`condition.onTryHit`(방어·판별), `onBasePower`/`onAfterHit`(탁쳐서떨구기), 중력·트릭룸·순풍·벽·필드의 조건 콜백, `onModifySpe`(순풍·날씨 특성 4·마비), `onImmunity`(모래헤치기), `onStart`(날씨 4·필드 4·위협), `onResidual`(먹다남은음식·그래스필드), `onDamage`(기합의띠), `onModifyDamage`/`onAfterMoveSecondarySelf`(생명의구슬), `onBasePower`(타입 강화 도구 23종), `onModifySpD/Def`(모래·설경), `onWeather`(모래 피해), `onStallMove/onRestart`(스톨), `onModifyAccuracy`(중력), `onBeforeMove`(수면·얼음·마비·풀죽음·중력). 그 외 콜백이 하나라도 붙은 항목은 전부 거부된다. 이 목록은 `support.rs`의 표로 고정되어 있고, dex와 어긋나면 테스트가 실패한다.

## 4. 중력·최면술 파티(`teams/gravity-original.json`, `rillaboom-slot`)에 필요한 것부터

라이브러리 사용 빈도와 파티 필요를 합치면 순서는 다음과 같다.

1. **날따름·분노가루 유도**(`RedirectTarget`) — 에써르 대책 평가에 필수. `getMoveTargets`의 `priorityEvent('RedirectTarget')`(속도순, 동속은 `effectOrder`).
2. **메가진화**(`megaEvo` 행동·`runMegaEvo`·폼 변경·능력치 재계산·`AfterMega`) — 가디안·아쿠스타·화염레오 전부 메가. `Pokemon`에 성격·SP를 넣어야 재계산이 된다(현재 사이드카).
3. **기절 후 교체 결정 단계**(`instaswitch` + 상태 `''` 복원 + 등장 효과 + 다음 턴) — 이것이 없으면 두 턴 이상을 이을 수 없다.
4. **열매와 `Update` 이벤트**(오랭 51·로젤 22·콜버·럼·치료…) — 라이브러리 도구 1위.
5. **접촉·피격 특성**(`DamagingHit`: 까칠한피부 12·저주받은바디 8·스태미나 6·위크아머 3·울퉁불퉁멧 4) 과 **경쟁심/오기**(`AfterEachBoost`, 26+9: 위협에 반응).
6. **구애 도구**(`choicelock` 24)·**긴장감**(20)·**테크니션**(23)·**곡예**(22)·**열교환**(14)·**적응력**(12)·**피뢰침**(9, 유도).
7. **연속기**(씨기관총 22·네즈미 22·더블윙 9·스케일샷 4)·**교체기**(유턴 33·볼트체인지 5·바톤터치·유턴).
8. **앵콜 42·도발 16·명령 22·도우미 13·와이드포스 18·다이어클로 21·와이드가드 5·대타 4·트릭 4·멸망의노래 4**.
9. 등장 효과 일반화(`switch_in.rs`와 `switching.rs` 통합, 다운로드·긴장감·구애스카프 `onStart`·시드).
10. 함정(스텔스록 12)·소원류 슬롯 조건·회복 기술(리커버는 지원, 루스트 11은 타입 변경 때문에 거부).

## 5. 정확 열거가 끝나지 않는 턴

광역기 둘이 겹치는 턴(`spread-damage`)은 세 번째 단계에서 중간 상태 14,260개, 네 번째에서 수백만 개가 되어 정확 분포를 끝내지 못했다(표본 모드로만 검증). 이것은 미구현이 아니라 성능 문제이며 `CONTEXT.md` 6번에 후보를 적었다.
