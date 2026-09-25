# poke-teambuilder-seed의 Champions 자료 정확성 검사 (2026-09-25)

대상: `D:\poke-teambuilder-seed`의 원장 `tools/seed-pipeline/out/historical-seed/2026-09-19-ledger-v1/run1/ledger.sqlite`(최신 step `fn117-crystal-egg-table-observations`, 46단계). 읽기 전용(`mode=ro`)으로 열었고 seed 쪽 파일은 바꾸지 않았다.
기준: `vendor/pokemon-showdown` 커밋 `9e317a6`(2026-09-19)의 `champions`(M-C)·`championsregmb`(M-B) 모드, 그리고 그것으로 만든 `engine/data/champions.json`.
스크립트: [`seed-check/`](seed-check/) (`seed_check.py` M-C, `seed_check_mb.py` M-B·M-A·명단). 실행 순서는 각 파일 머리말 참고.

## 결론

- **M-C 메커니즘 값은 엔진과 전수 일치한다.** seed가 M-C로 기록한 Showdown 커밋 `aa17ca0`(9월 11일경)과 우리 기준 `9e317a6` 사이에 Champions 데이터 파일은 CRLF만 다르고 내용이 같다(예외: 아래 3항). 기술 515종의 타입·분류·위력·명중·PP(기본값과 Champions 시작 PP 모두)·우선도·대상, 종·폼 392개의 타입·종족값·폼 관계, 특성 슬롯 1,176칸, 학습표 15,764쌍(전부 `9M`), 도구 166개의 메가스톤 대응·사용자 제한, 타입 상성 324쌍이 모두 같다. 이 값들은 최신 원장 step에서도 그대로 선택돼 있다(24,566칸 = fn15 23,591 + fn105 스프라이트 975).
- **오류 1건: 메가루카리오Z의 특성 슬롯 0이 `explicitly_absent`로 기록됐다.** 엔진은 두 커밋 모두 `Aura Guard`다. 원인은 seed 작성 도구(`fn15-mc-engine/author.ts` 157행)가 `isNonstandard`가 붙은 특성을 주제 목록에서 빼는데, 아우라가드는 `isNonstandard: "Future"`다. 그 뒤 187행이 "주제 목록에 없는 특성"을 "엔진이 이 슬롯에 특성을 선언하지 않음"(`explicitly_absent`)으로 적었다. 이 `closedScope`의 근거 문장(`explicitly_absent means the engine declares no ability in that slot`)이 이 칸에서는 거짓이다. 영향 범위: M-C 명단 종 중 아우라가드 사용자는 메가루카리오Z 하나. `Mountaineer`·`Persistent`·`Rebound`(CAP)·`No Ability`도 같은 이유로 주제가 없지만 명단 종이 쓰지 않는다. 고치는 방법은 seed 절차대로 새 step에서 경쟁 assertion을 쓰고 기존 칸을 reject하는 것이다(여기서 하지 않았다).
- **stale 1건: npm `pokemon-showdown@0.11.11`로 만든 M-B 바인딩(`binding:champions:mechanics:observed`)이 현재 M-B 모드와 다르다.** 기술 15종(`courtchange, doubleshock, drumbeating, glaiverush, jawlock, meteorassault, milkdrink, octolock, overdrive, pyroball, revivalblessing, shiftgear, slash, snipeshot, zingzap`)이 없고, 학습표에 `slash` 28쌍이 없으며 `politoed/pound`가 남아 있다. 소원·힘흡수 PP도 10(옛 값)이다. 같은 M-B 활성화에 `aa17ca0`으로 만든 두 번째 바인딩(`binding:championsregmb:ps-git-aa17ca0…`)은 현재 엔진과 전수 일치하므로, 읽는 쪽이 어느 바인딩을 쓰는지에 따라 답이 달라진다. seed 기록(`delta-check.json`의 "M-B engine observation not silently corrected")은 이 차이를 알고 보존한 것이다.
- **명단(공식 페이지 fn6)은 종 번호 수준에서 엔진과 일치한다.** M-C 262항목(231종), M-B 235항목(208종). 엔진에만 있는 번호는 전투 중 폼(메로엣타 스텝, 메테노 코어, 가마팔이 폼, 얼음귀신 노아이스, 오거폰 테라 폼, 테라파고스 테라스탈)뿐이며 이는 기본 폼이 금지 태그라 `isNonstandard` 표기만으로는 합법성을 말할 수 없는 경우다. 공식 페이지의 폼 index(예 `0666-018` 비비용)는 seed가 폼 주제에 귀속시키지 않았다(`V30-F3`, 기록만).

## 자세한 결과

### 1. 두 Showdown 커밋의 차이 (`aa17ca0` → `9e317a6`)

| 파일 | 차이 |
|---|---|
| `data/mods/champions/items.ts` | 탈출버튼 `onAfterMoveSecondary` 핸들러 추가(16줄). seed의 `item_effect.conditions/mechanics`(opaque callback 해시)는 이 항목에서 달라진다. 값 필드는 영향 없음 |
| `data/mods/champions/formats-data.ts` | Smogon tier 표기 4곳(`UUBL/OU` → `Uber`). 게임 명단과 무관 |
| `config/formats.ts` | Champions 외 포맷 26줄. 무관 |
| 나머지 (`champions`·`championsregmb` 모드 전체, `pokedex/moves/items/abilities/learnsets/typechart/conditions`, `sim/*`) | 동일 |

### 2. M-C 값 대조 (`seed_check.py`)

| 차원·필드 | 검사 | 불일치 |
|---|---:|---:|
| move_core type/category/basePower/accuracy/priority/target | 515 × 6 | 0 |
| move_core pp: `resolved`(=dex 기본 PP), `initial`(=Champions 시작 PP `(pp/5+1)*4`, `noPPBoosts`면 기본값) | 515 × 2 | 0 |
| species_stats types/baseStats/formRelation(+isBaseForm) | 392 × 4 | 0 |
| ability_slot 0/1/H | 1,176 | **1** (lucariomegaz/0) |
| learn_route (종, 기술) → 코드 | 15,764 | 0 (양쪽 집합도 동일) |
| item_effect availability/formRequirement/transformationTarget | 166 × 3 | 0 |
| type_chart (주제=공격 타입, 필드=방어 타입) | 324 | 0 |
| 집합: 종 392, 기술 515, 도구 166 | — | 양쪽 동일 |

표현 차이(오류 아님): `accuracy: true`는 `{"kind":"always"}`; `forme: ""`는 `null`; `formRelation.otherFormes/cosmeticFormes`는 seed에 있으나 우리 export가 버린 필드라 비교 생략. `move_core.effect`·`item_effect.conditions/mechanics`는 콜백 해시라 비교 생략.

### 3. M-B·M-A (`seed_check_mb.py`)

| 비교 | 결과 |
|---|---|
| seed `championsregmb@aa17ca0` vs 엔진 `championsregmb` | 종 357·기술 515·도구 148·학습 14,219쌍 전수 일치. 특성 슬롯 1,071 일치 |
| seed `champions@npm 0.11.11`(M-B 표기) vs 엔진 `championsregmb` | 기술 500/515(15종 없음), 학습 `slash` 28쌍 없음·`politoed/pound` 잉여, 나머지 값 일치 |
| seed `champions@npm 0.11.11` vs 엔진 `champions`(M-C) | 위에 더해 종 35·도구 18 부족(M-C 신규), 소원·힘흡수 PP 10 vs 5 |
| M-A (`championsregma@npm 0.11.11`) | 우리 vendor에 `championsregma` 모드가 없어 값 대조 불가. 공식 M-A 명단 213항목, seed 종·폼 319개(수치만) |

### 4. 확인하지 않은 것

- 공식 게임 자체와의 대조. 이 검사는 "seed = 그 시점 Showdown"인지를 본 것이고, Showdown이 게임과 같은지는 별개다(seed 문서도 엔진 값을 비공식 원천으로 분류한다).
- opaque callback(`effect`·`conditions`·`mechanics`)의 해시가 어떤 동작을 가리키는지.
- 폼 index → 폼 주제 대응, 도구 목록 페이지(403), 규칙 텍스트(`format_rule`) 칸의 값.
- `era-v32` 이전 JSON 체인의 값(원장 projection의 선택값만 봤다).
