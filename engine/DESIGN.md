# lab-engine 설계

더블(VGC)을 기본으로 하되 싱글도 손해 없이 쓰는 탐색용 배틀 엔진. 현재는 골격과 정적 Dex, Champions SP 능력치 및 저수준 데미지 원시 연산이 있다. 턴 진행과 기술 효과 연결은 아직 없다.

## 목표와 범위 (2026-09-25 사용자 지시)

- **1차 목표: PokaiEngine급 엔진.** 한 행동 조합(joint action)의 결과 분포 전체를 확률과 함께 한 번에 열거하고, Showdown과의 결과 일치율 99% 이상을 목표로 한다. 속도 목표는 PokaiEngine 보고치(턴당 약 0.08ms)와 같은 자릿수.
- **대응 범위는 포켓몬 챔피언스뿐.** 규칙·메커니즘은 Champions를 따른다(SP 능력치 공식, PP 상한 20, Champions 기술·도구·특성 변경, 메가진화, 레벨 50). Champions M-C 규칙셋은 메가진화만 허용한다. 울트라버스트·Z기술·다이맥스(거다이맥스 포함)·테라스탈은 **규칙셋에서 꺼 둔 것이지 엔진에서 제거한 것이 아니다.** 행동 인코딩과 상태에는 남겨 두고, 동작(메커니즘)은 해당 규칙셋이 필요해질 때 구현한다. 아래 "기믹과 규칙셋" 참고.
- **규칙이 범위이고 로스터는 제한하지 않는다.** 현 레귤레이션에 없는 포켓몬·기술·도구도 Champions 메커니즘 안에서라면 쓸 수 있어야 한다. 엔진에 합법성 목록을 넣지 않고 `isNonstandard` 태그는 데이터로만 둔다. 합법성은 별도 validator 단계. Showdown 정답은 `gen9championsdoublescustomgame`(Champions 모드, 로스터 제한 없음)으로 만든다.
- **형식 우선순위:** M-C 더블 정합성을 먼저 맞춘다. 싱글(BSS)은 `N=1`로 컴파일되는 구조만 유지하고 검증은 나중에 한다.

## 탐색의 용도와 정보 모델 (2026-09-25 사용자 지시)

**용도는 일반적으로 강한 AI가 아니라, 알려진 특정 파티를 이기는 플랜을 찾는 오프라인 공략 도구다.** 상대 파티는 `teams/library`의 공개 팀시트로 확정하고, 우리 파티도 확정한다. 그래서 상대 세팅 추정(사용률 사전분포, 결정화 샘플링, 전략 융합 문제)은 후순위다. 다만 아래 ②의 이유로 "상대가 우리를 어떻게 믿는가"는 처음부터 필요하다.

**전제:** Champions 대전에서 배분(SP·성격)과 도구는 비공개다(사용자 확인). 기술·특성 공개 범위는 공식 규칙으로 다시 확인한다.

### 상대 모델 세 단계

같은 플랜이라도 상대가 우리 세팅을 아느냐에 따라 성립 여부가 바뀐다. 예: "메가갸라도스의 +1 폭포오르기를 버티는 배분으로 되받아치는 플랜"은 상대가 우리 내구를 모른다는 것 자체가 성립 조건이다. 완전정보 상대는 그 배분 앞에서 용춤을 추지 않는다. 반면 "1용춤에 죽는 배분을 전제로 한 플랜"은 읽혀도 성립 여부가 바뀌지 않는다. 그래서 탐색은 상대 모델을 명시하고, 플랜마다 어느 모델에서 성립하는지 출력한다.

| 모델 | 상대가 아는 우리 정보 | 얻는 답 |
|---|---|---|
| ① 완전정보 | 실제 배분·도구까지 전부 | 읽혀도 성립하는 플랜. 보수적 하한. "결정론적 공략"에 가장 가까움 |
| ② 표준 믿음 + 관측 갱신 | 공개된 표준형 또는 후보 집합. 경기 중 관측(받은 데미지, 행동 순서, 드러난 도구)으로 후보를 좁힘 | 실전에 가장 가까움. 기습이 한 번 통하고 이후엔 읽힘 |
| ③ 표준 믿음 고정 | ②와 같지만 갱신 없음 | 낙관적 상한. 참고용 |

- ②③에서 상대 정책은 **상대가 믿는 우리 팀**으로 계산하고, 결과 판정은 **우리 실제 팀**으로 한다. 상대 정책을 먼저 고정하고 그 정책을 상대로 우리 쪽만 최적화하므로, 불완전정보 게임을 통째로 푸는 것보다 훨씬 싸다.
- ②의 관측 갱신은 최소한 "공개된 사실과 모순되는 후보 배분 제거"여야 한다. 갱신이 없으면(③) 한 번 버틴 뒤에도 상대가 계속 속는 것으로 계산되어 과대평가된다.
- 우리 세팅 후보 집합은 사용자가 지정한다(표준형 + 검토 중인 조정안). 균등분포는 쓰지 않는다.
- **표준형의 정의(2026-09-25 사용자):** 통계상 보통 두 능력치에 32씩 준다(총합 66이므로 나머지 2). 그래서 믿음의 기본 후보는 포켓몬별로 그럴듯한 능력치 쌍의 `32/32/2` 배분이고, 조정은 이 기본형에서 SP를 옮긴 것으로 표현한다. **유명한 조정은 별도로 수집해 후보에 넣는다**(출처·수집일과 함께, 예: `teams/library` 또는 전용 조정 목록 파일). "보통 두 능력치에 32"라는 통계 자체도 나중에 출처를 붙인다.
- **출처 확인(2026-09-25 수집, OP.GG 인게임 랭크배틀 통계, 시즌 M-6, 싱글, 갱신 2026-09-25 19:36):** 상위 배분이 모두 `32/32/나머지 2` 변형이었다. 보만다 `H1 A32 B1 S32` 고집 14.0%, `H2 A32 S32` 명랑 12.2%; 고릴타 `H32 A32 S2` 13.6%, `H2 A32 S32` 4.5%; 인크레이 `H32 B32 D2` 무사태평 13.9%, `H32 D32 B2` 신중 4.7%. 출처: `https://op.gg/pokemon-champions/pokedex/{salamence,rillaboom,incineroar}`. 페이지에 더블 전환 탭이 있으나 이번엔 싱글만 읽었다. **더블 배분은 별도 수집 필요.**
- 통계에서 읽히는 두 가지: (1) 최빈 배분이라도 점유율은 10%대라 꼬리가 길다. 그래서 믿음은 최빈 하나가 아니라 상위 몇 개 + "그 외"로 둔다. (2) 나머지 2의 배치(`H2` vs `H1 B1`)처럼 능력치 차이가 1인 변형이 많다. 이런 변형은 우리 기술의 KO 경계를 넘지 않는 한 같은 동치류로 합친다.

### 가정 없는 검증 (2026-09-25 사용자 질의로 추가)

믿음 없이 모든 합법 배분을 보는 기능은 **별도 탐색 모드가 아니라 플랜 검증**으로 둔다. 플랜을 고정한 뒤 격자 전체에서 결과를 계산해 (1) 어떤 배분에도 성립하는지, (2) 아니면 어느 구간이 깨뜨리는지를 낸다.

비용(2026-09-25 추정):
- 표 만들기(배분 × 도구 × 특성 × 기술 × 난수): 우리 기술 24 × 상대 6 × 격자 3,300 × 도구 약 3 × 특성 1–3 × 난수 16 ≈ 5×10⁷ 데미지 계산. Rust로 수 초.
- 고정 라인 검증: 격자 전체의 부분합 합성곱이라 밀리초.
- **분기하는 정책**의 전 세팅 검증이 비싸다. 상대 4마리 × 마리당 동치류 20이면 20⁴ = 1.6×10⁵ 번의 게임 풀이. 지배 관계(더 잘 버티고 더 빠른 세팅이 있으면 나머지는 불필요, 총합 66 제약으로 파레토 경계가 작음)와 분할 필요 시 분할로 10³–10⁴ 수준까지 줄인다. 6마리 조합을 곱해 처음부터 다시 푸는 방식은 쓰지 않는다.
- 배분보다 큰 미지수는 **기술 4개**(실전 후보 40개 중 4개 ≈ 9×10⁴)다. 그래서 "배분 무관"은 **배분·도구에 무관**으로 정의하고 기술은 사용률 상위 후보로 제한한다. 기술까지 무관하게 보려면 "이 기술이 있으면 깨진다"식 조건부 출력으로 돌아간다. 배분 미공개 자료(`source-complete-sp-unknown`)는 이 검증으로만 다룬다. 결과는 플랜 등급 "배분 무관"으로 표시하며, 관측 갱신과 우리 조정 외부 루프의 기준점이 된다.

### 플랜 출력 형식

플랜마다 성립 조건을 붙인다. 등급: **배분 무관**(어떤 합법 배분·①에서도 성립) > **①**(믿음의 배분들에 대해 완전정보에서 성립) > **②**(상대가 표준형으로 믿을 때만 성립). 예:
- "1용춤 허용 후 하이퍼보이스 집중 — 성립 조건: 없음(①에서 성립)"
- "+1 폭포 버티고 최면술 — 성립 조건: 상대가 밀로틱을 표준 배분으로 믿을 것. 버틴 다음 턴부터 읽힘"

### 난수 처리 두 모드

"결정론적"은 난수(데미지 16단계, 급소, 명중, 추가효과, 수면 턴 수) 때문에 문자 그대로는 불가능하다. 대신:
- **최악 난수 보장:** 난수 분기를 상대의 선택으로 취급(minimax)해 어떤 난수에도 이기는 라인을 찾는다. 존재하지 않을 수 있다. 사용자가 이미 쓰는 "최저난수 KO 보장" 기준과 같다.
- **승률 최대화:** 난수를 기대값으로 처리. 보장 라인이 없을 때의 차선.

### 배분 공간의 사전 계산 (동치류)

데미지·순서 계산에 쓰이는 것은 최종 능력치뿐이다. Champions는 한 능력치 32·총합 66이라 EV 체계보다 공간이 작다. 총합 66 배분은 약 1.0×10⁷개지만 (HP, 방어 또는 특방) 최종값 격자는 성격 포함 약 3,300점이다.

- **표는 압축하지 않는다.** 선출 화면(엔트리 공개) 시점에 우리 기술 × 상대 개체 × 격자 3,300 × 난수 16의 정확한 데미지 값을 그대로 둔다(약 15 MB). 반대 방향(상대 공격 → 우리)과 스피드 대소표(순풍·트릭룸·스카프·마비·랭크별)도 같다. 탐색 노드 비용은 표 찾기 수준이 된다.
- **"확정 1타/2타" 같은 기술별 KO 등급으로 압축하지 않는다(2026-09-25 사용자 지적).** 필요한 양은 KO 여부가 아니라 **맞은 뒤 남은 HP**이고, 남은 HP는 후속 상호작용(불릿펀치 등 선공기, 모래·생구 반동·열매 회복, 아군 광역기 두 번째 타격)에서 다시 경계를 만든다. 경계는 기술 하나가 아니라 기술 조합·순서마다 생긴다.
- **동치류는 플랜(또는 탐색 노드)에서 도출한다.** 어떤 라인의 결과는 상대 격자점의 계단 함수이고, "부분합(난수 분포의 합성곱 + 고정 피해/회복)이 HP를 넘는 지점"에서만 값이 바뀐다. 같은 라인에서 결과가 같은 격자점 집합이 그 라인의 동치류다. 게임을 풀 때는 상대 한 마리를 한 덩어리로 시작해 트리 안에서 두 격자점의 결과가 갈릴 때만 쪼갠다(분할 필요 시 분할). 남은 HP 분포는 타격 수에 지수적이지 않고 HP 값 범위(≤ 약 250)의 히스토그램으로 묶인다.
- 관측 갱신도 정확한 표가 필요하다. 경기에서 보이는 것은 HP 바 %이므로 "하이퍼보이스에 43% → 특방 최종값 구간"의 역산은 KO 등급으로는 불가능하다.
- 배분이 미공개인 라이브러리 자료(`source-complete-sp-unknown`)는 격자 전체 위의 믿음으로 다룬다.

### 외부 루프: 우리 조정 탐색

우리 배분·도구·슬롯 일부를 탐색 변수로 두면 "이 파티를 이기려면 어떤 조정이 필요한가"까지 찾을 수 있다(현재 수동으로 하는 가디안·아쿠스타·클레스퍼트라 비교의 자동화). 후보 공간은 사용자가 지정한 슬롯과 SP 범위로 제한한다. 결과 조정안은 `AGENTS.md` 규칙대로 별도 파생 파일에 근거와 함께 남긴다.

### 용어

이 문서의 `oracle/`(정답 분포)은 Showdown을 기준으로 엔진 규칙 구현을 검증하는 도구다. 위의 "완전정보 모델"과는 다른 개념이므로 코드·문서에서는 `PerfectInfo` / `BeliefModel`처럼 별도 이름을 쓴다.

### 선행 사례 (검색 미수행, 기억 기반 — 확인 필요)

Foul Play(사용률 후보 샘플링·결정화), Technical Machine(관측으로 상대 EV 추정), ISMCTS 포켓몬 적용 연구(정보 집합 단위 통계), PokéChamp(LLM 국면 평가 + minimax), Metamon(오프라인 RL). 대부분 싱글이고 "상대 세팅 추정 + 결정화" 구조다. 위의 "특정 파티 공략 + 상대 모델 ①②③ + 동치류 사전 계산" 조합을 공개한 사례는 기억에 없다. 인용 시 출처 URL·수집일을 붙여 확인한 뒤 쓴다.

## 왜 직접 만드나 (2026-09-25 조사)

| 후보 | 탈락 사유 |
|---|---|
| PokaiEngine (arXiv 2608.29197) | 더블·M-B·턴당 0.08ms로 가장 가깝지만 **미공개**(Showdown 운영진과 공개 조율 중), 싱글 미지원 |
| poke-engine (pmariglia) | 빠르지만 싱글 전용. 랭크·volatile이 Side에 붙어 있고 행동자 2명 전제, `MoveTarget`은 User/Opponent뿐 |
| Showdown | M-C 정확, 싱글·더블 공통. 턴당 약 2ms로 탐색에 느림 → **정답(oracle)으로 사용** |
| battler, pokemon-showdown-rs, pkmn/engine | Champions 미지원, 탐색용 make/unmake 없음, 또는 Gen 1–2만 |

전제: 탐색 예산이 늘수록 실력이 오른다. 그래서 엔진 속도가 병목이다.

## 핵심 원칙

1. **슬롯 수 = const generic `N`.** `State<1>` 싱글, `State<2>` 더블. 형식별로 따로 컴파일되므로 싱글이 더블 지원 비용을 내지 않는다. 광역 0.75배처럼 "실행 시점 대상 수"로 정의된 규칙은 N=1에서 저절로 싱글 규칙이 된다. 저절로 맞지 않는 예외만 형식 분기로 명시한다.
2. **make/unmake.** 턴 결과는 `Vec<Outcome { probability, instructions }>`. 각 `Instruction`은 되돌릴 정보를 담는다. 탐색은 상태를 복사하지 않는다(poke-engine 방식 계승).
3. **영구 상태와 슬롯 상태 분리.** HP·상태이상·도구·PP는 `Pokemon`(파티), 랭크·volatile·대타는 `Slot`.
4. **필드 효과는 테이블.** `field[FieldEffect]`, `side.effects[SideEffect]`에 `{value, turns}`. 중력 같은 효과를 추가할 때 enum variant만 늘린다.
5. **행동은 슬롯 단위.** `SlotAction`(기술+대상/교체), 진영 결정은 `[SlotAction; N]`. 탐색이 슬롯별 통계(factored DUCT)와 가지치기를 쓸 수 있게 한다. N=1이면 기존 DUCT와 같다.
6. **평가자는 trait.** `Evaluator<N>`로 휴리스틱·선형·신경망 평가자를 교체할 수 있다.
7. **대상 표기는 Showdown과 같다.** 양수=상대 위치, 음수=아군 위치, 0=지정 없음.

## 계층 (예정 포함)

```
core/
  state.rs        State<N> / Side<N> / Slot / Pokemon           [골격 있음]
  instruction.rs  Instruction, apply/reverse, Outcome            [골격 있음]
  field.rs        FieldEffect / SideEffect / Effect 테이블       [골격 있음]
  action.rs       SlotAction, JointAction<N>                     [골격 있음]
  gimmick.rs      Gimmick(활성화 방식), GimmickSet(1바이트), DynamaxState  [있음]
  rules.rs        Ruleset(규칙셋 능력), 행동 검증·기믹 포함 합법 행동 생성  [있음]
  eval.rs         Evaluator<N>, Material                         [골격 있음]
  stats.rs        Champions SP 검증·레벨 50 능력치                [있음]
  damage.rs       4096 고정소수점·16롤 데미지 원시 연산            [있음]
  dex/            종·기술·특성·도구·타입·성격·조건 정적 테이블         [있음]
                  (mod.rs 수기 타입/API, generated.rs는 data/gen-rust.cjs 생성물)
  turn/           행동 큐(우선도→스피드, 동속 확률 분기, 행동마다 재정렬),
                  대상 해석(생존 슬롯, 대상 유도, 광역 여부), 훅, 턴 종료  [예정]
  search/         슬롯 분리 DUCT, PUCT 사전확률, 결정화 샘플 병렬       [예정]
scenario/         lab-scenario: 시나리오·팀 JSON → State<N> + 사이드카   [있음, 검증 완료]
                  (serde는 여기에만. 탐색 경로 밖)
py/               pyo3 바인딩 (abi3, Python 3.12+)
```

## 시나리오 로더 — `scenario/` (`lab-scenario`)

- **분리 이유:** JSON 파싱은 시나리오당 한 번이고 탐색 루프와 무관하다. serde/serde_json을 별도 크레이트에 두어 `lab-engine`은 외부 의존성 없이 유지한다. `State`에는 문자열·JSON 값이 들어가지 않는다.
- **입력:** oracle 시나리오(`format`, `p1`/`p2`의 `team`(경로 또는 인라인 배열)·`order`, `turn`, 선택적 `description`·`seed`)와 Showdown JSON 팀 세트. 모르는 필드는 serde 단계에서 거부한다. 필드를 버리면 국면이 조용히 바뀔 수 있기 때문이다. `seed`는 받되 무시한다(열거 엔진에는 의미가 없음). `turn`의 선택 문자열은 해석하지 않고 그대로 보관한다.
- **거부하는 것:** 비어 있지 않은 `setupTurns`(턴 엔진 필요), 비어 있지 않은 `patch`(아직 적용 코드 없음), `gen9championsdoublescustomgame` 외 형식(VGC 4마리 선출 미구현), 레벨 50 외(`stats.rs`는 레벨 50 공식), 모르는 종·기술·도구·특성·성격·성별·테라 타입, 특성·성격·기술 누락, 기술 5개 이상·중복, SP 제한 위반(`stats::champions_stats`), IV 31 초과, 편 안 이름 중복. 오류는 편·팀 위치·표시 이름을 붙인다.
- **채우는 것:** 레벨, 종 타입, HP = 최대 HP, 5능력치, 상태 없음, 도구, 특성, 기술 4칸과 Champions PP(`state::champions_max_pp`: `(pp/5+1)*4`, 최대 20), 메가 자격(`gimmick::structural_gimmicks`). 다른 기믹 자격은 도출하지 않으며 `Ruleset::CHAMPIONS_MC`가 어차피 막는다. 기믹 variant는 그대로 둔다.
- **선두:** Showdown `Side.chooseTeam`처럼 순서 문자열(쉼표 또는 글자 단위)을 팀 크기로 자르고, 빠진 멤버는 원래 순서로 뒤에 붙인다. 파티 순서 = 이 순서이고, 앞 `N`마리가 슬롯 0..N에 선다. 같은 코드가 `State<1>`도 만든다.
- **사이드카:** `ScenarioMeta { format, description, sides: [SideMeta; 2], turn }`, `SideMeta.members[party_index] = MemberMeta { name, team_index, nature, stat_points, gender, tera_type }`. 정규 상태는 포켓몬을 이름으로 식별·정렬하므로 이름(Showdown처럼 20자로 자름, 편마다 유일)만 있으면 된다. 나머지 필드(`species` 이름, `item`/`ability`/PP 키의 Showdown id, `slot`)는 `State`와 dex에서 계산한다. `SideMeta::canonical_order`가 `canonical.cjs`의 이름 정렬 순서를 준다.
- **범위:** 로더 상태는 선두 배치 직후, **등장 효과 전**이다. 등장 효과는 아래 `switch_in`이 따로 펼친다.
- **첫 등장 펼치기 — `switch_in.rs` (2026-09-25):** `initial_outcomes(&LoadedScenario)` / `expand_switch_ins(&State<N>)`가 선두의 시작 효과를 적용한 가중 상태 목록(`InitialOutcome { probability, state }`, 확률 합 1, 같은 상태는 합침)을 돌려준다. 탐색 경로와 `State` 밖에 있고 `lab-engine`에는 코드를 넣지 않았다. Showdown `runSwitch`를 따른다: 선두 전체를 한 번 스피드 정렬(저장된 S 실수치, 동속은 k!개 순서 균등 분기), `fieldEvent('SwitchIn')`에서 그 순서로 특성 `onStart` 실행, 실행 전에 특성이 바뀐 핸들러는 건너뜀. 트레이스는 인접한 상대 중 `NOTRACE` 플래그가 없는 특성을 균등 분기로 복사하고, 복사한 특성의 `onStart`를 바로 실행한다. 모래날림은 모래바람(이미 모래면 실패), 그래스메이커는 그래스필드(이미 같으면 실패). 지속은 5턴, 발동자가 보송보송바위/그라운드코트를 들면 8턴. `Effect.turns` = Showdown 남은 `duration`.
- **구현한 특성 경계:** 동작 구현 = 트레이스·모래날림·그래스메이커. 시작 시 무동작으로 검증 = 모래헤치기(`onModifySpe`는 모래에서만, 순서는 핸들러 실행 전에 고정). 그 밖에는 dex `handlers`에 시작 구간 이벤트(`Start`, `SwitchIn`, `BeforeSwitchIn`, `BattleStart`, `Update`, `SetAbility`, `SetWeather`, `WeatherChange`, `TerrainChange`, `ModifySpe`; `Ally/Foe/Any/Source` 접두 포함)가 없고 `suppressWeather`가 아닌 특성만 무동작으로 받는다. 도구·종도 같은 이벤트가 있으면 거부한다(구애스카프, 에어록 등). 구현한 특성의 dex `handlers` 목록이 바뀌면 `HandlersChanged`로 거부한다. 트레이스 대상이 없을 때(계속 탐색하는 숨은 상태), 옆에 `No Ability`, 복사할 특성이 `CANTSUPPRESS`일 때, 3슬롯 이상, 이미 시작된 상태(필드·진영 효과, 랭크, 휘발 상태, 상태이상, HP 감소, 기믹 사용)도 거부한다. 형식·규칙의 핸들러(`onBegin` 등)는 dex에 없으므로 oracle fixture로만 확인한다.
- **정규 상태 출력 — `canonical.rs` (2026-09-25):** `canonical_json(&State<N>, &ScenarioMeta)`가 `canonicalKey(canonical(battle))`와 같은 바이트열(키 순서 포함)을, `canonical_value`가 `serde_json::Value`를 낸다. 이름은 사이드카, 종 이름·도구/특성/기술 id는 dex에서, 정렬은 JS와 같은 UTF-16 코드 단위 순서(`SideMeta::canonical_order`). `request="move"`, `ended=false`, `winner=""`, 빈 `conditions`/`slotConditions`/`volatiles`/`pseudoWeather`, 0이 아닌 랭크만, `canMega`는 개체 메가 자격 ∩ 형식 규칙셋(`format_ruleset`: 현재 custom game → `CHAMPIONS_MC`) − 진영 메가 사용. 아직 정규 형태를 정하지 않은 상태는 `CanonicalError::Unrepresentable`로 거부한다: 휘발 상태 비트, 대타·다이맥스, 진영 효과, 중력 등 유사 날씨, 원시 날씨·영구 지속, 수면·맹독(카운터 의미 미정)·`status_turns`, 타입 변화, 사슬묶기, 기절, 빈 슬롯. `lastMove`·`lastItem`은 `State`에 없으므로 쓰지 않는다.

## 기믹과 규칙셋 — `gimmick.rs`, `rules.rs`

- **활성화 방식은 모두 남긴다.** `Gimmick`은 `None / Mega / UltraBurst / ZMove / Dynamax / Tera`이고 `SlotAction::Move`마다 하나를 붙인다. 형식이 허용하지 않는 방식도 enum에서 빼지 않는다. 그래야 행동 인코딩, 탐색 통계, 직렬화 형식이 규칙셋에 따라 바뀌지 않는다. `SlotAction`은 4바이트 이하다.
- **거다이맥스는 별도 선택지가 아니다.** `Gimmick::Dynamax`를 고른 포켓몬에게 `Pokemon.gigantamax_factor`가 있으면 슬롯의 `DynamaxState::Gigantamax`가 된다. 다이맥스 예산을 공유하고 교체 시 슬롯과 함께 초기화된다.
- **원시회귀는 기믹이 아니다.** 등장 시 자동으로 일어나는 폼체인지이고 선택하지 않으며 예산도 없다. 턴 엔진의 폼체인지 처리에 둔다.
- **규칙셋 능력 계층:** `Ruleset { gimmicks: GimmickSet }`. `Ruleset::CHAMPIONS_MC`는 `MEGA`만 켠다(더블 `gen9championsvgc2026regmc`, 싱글 `gen9championsbssregmc`). 앞으로 허용할 방식은 `Ruleset::CHAMPIONS_MC.enabling(Gimmick::Tera)`처럼 규칙셋만 바꿔서 켠다. 규칙셋은 형식별 상수라서 `State`에 넣지 않고 인자로 넘긴다.
- **진영별 1회 사용:** `Side.gimmicks_used: GimmickSet`(1바이트). 방식마다 예산이 따로 있다(Showdown처럼 메가와 울트라버스트도 별개). 사용 기록은 되돌릴 수 있는 `Instruction::UseGimmick`으로 남긴다. 한 턴에 두 슬롯이 같은 방식을 요청하면 거부한다. 서로 다른 방식은 같은 턴에 쓸 수 있다.
- **개체 자격:** `Pokemon.gimmicks: GimmickSet`(메가스톤·Z크리스탈·테라 타입 등). 데이터 로더가 `gimmick::structural_gimmicks(species, item)`로 채운다. 현재는 메가만 도출한다(Champions `canMegaEvo`와 같이 스톤의 `mega_stone` 표에서 정확한 종 일치). 자격은 허가가 아니므로 나중에 다른 방식을 도출해도 M-C에서는 규칙셋이 막는다. 쓸 수 있는 방식 = 규칙셋 ∩ 개체 자격 − 진영 사용분(`Ruleset::available_gimmicks`).
- **이중 차단:** 생성(`Ruleset::joint_actions`)은 기믹이 없는 슬롯별 후보를 받아 허용된 방식만 붙인다. 후보에 이미 붙어 있던 기믹은 무시한다. 외부에서 들어온 행동은 `Ruleset::validate_slot_action`, `validate_joint_action`으로 검증한다. 둘 다 같은 검사를 쓰므로 생성한 행동은 항상 검증을 통과한다. M-C에서 메가 외 방식은 `ActionError::GimmickDisabled`가 된다.
- **아직 없는 것:** 기믹의 실제 효과(메가 폼·특성 변경, Z기술·다이맥스기 변환, 다이맥스 HP·턴, 테라 타입·STAB)와 기술별 검사(PP, 사슬묶기, 기술별 대상 규칙, 교체 대상의 생존 여부). 이것들은 턴 엔진과 데이터 테이블이 생긴 뒤에 구현한다.

## 정답 분포 (oracle) — `oracle/`

- `oracle/enumerate.cjs <scenario.json>`: Showdown의 PRNG를 스크립트 PRNG로 바꿔 한 턴의 모든 난수 분기를 깊이 우선으로 열거한다. 분기마다 직렬화 스냅샷에서 턴을 다시 실행하고, 같은 최종 상태는 `oracle/canonical.cjs`의 정규 상태로 합친다. 모드: `full`(정확 확률), `extremes`(데미지 난수 최소·최대만, 확률 부정확), `mc`(자연 PRNG N회 표본).
- Showdown이 더블 광역기의 명목 대상을 고르며 소모하는 난수는 결과와 무관하므로 `full`에서 접는다(`--keep-nominal-draws`로 해제). `mc`는 접지 않으므로 `oracle/compare.cjs`로 `full`과 `mc`를 비교해 이 가정을 검사한다.
- 정규 상태(`canonical.cjs`, schema 1)가 엔진과 oracle의 비교 형식이다. lab-engine은 같은 JSON을 내야 한다. 포켓몬은 이름으로 식별한다(Showdown이 교체마다 `side.pokemon` 순서를 바꾸므로).
- 시나리오: 팀, 선출 순서, 선택적 준비 턴(`setupTurns`), 상태 패치(HP·상태이상·수면 턴·랭크·날씨·필드·중력 등의 남은 턴), 검사할 한 턴의 선택. 대상 번호는 Showdown 규칙(양수=상대, 음수=아군).
- 비용: Showdown 재실행은 분기당 약 3.5ms. 단일 공격 턴은 `full`(약 2천 분기, 7초)이 가능하지만, 광역기와 공격이 여럿인 턴은 분기가 지수적으로 늘어 `full`이 수십 분을 넘는다. 그런 턴은 `mc`로 엔진의 정확 분포를 통계 검사한다.
- 2026-09-25 확인: `single-hit`에서 `full`과 `mc` 4000회의 TV 거리 0.088(잡음 추정 약 0.10). `hypnosis-gravity`는 명목 대상 접기 전후 결과가 같았다(768→6분기, 2결과).
- `oracle/initial.cjs <scenario.json> [--out file]` (2026-09-25 추가): 팀 프리뷰 선택부터 첫 결정까지를 스크립트 PRNG로 열거해 초기 상태 분포(트레이스 대상, 동속 등)와 고정 시드의 `before`를 함께 낸다. 기대값은 `oracle/expected/<scenario>.initial.json`에 둔다. `single-hit.initial.json`은 Showdown 고정 커밋에서 재생성했으며 2분기·2결과(각 1/2)다. Rust 결과의 상태·확률과 고정 시드 `before` 일치를 테스트한다.

## 데이터 — `data/`

- `data/export.cjs` → `data/champions.json`: Champions 모드 dex 전체(종 1518, 기술 938, 도구 583, 특성 321, 타입 상성, 성격, 주요 상태). 비표준 항목도 `isNonstandard` 태그와 함께 포함한다. Showdown에서 콜백으로 구현된 동작은 `handlers`에 이름만 남긴다. 이것이 손으로 구현할 목록이자 커버리지 점검표다.

- `data/gen-rust.cjs` → `core/src/dex/generated.rs`: 위 JSON을 Rust 정적 테이블로 바꾼다. 테이블은 id 순 정렬(조회는 이진 탐색), 인덱스 0은 "없음" 항목, 기본값과 같은 필드는 `..MoveData::NONE`로 생략, 참조는 생성 상수 이름으로 쓴다. 모르는 필드는 생성 실패로 처리한다(조용한 데이터 손실 방지). 제외는 `EXCLUDED_SPECIES`에 이유와 함께 둔다(현재 MissingNo.: Bird 타입이 Champions 상성표에 없음). 외부 크레이트 의존 없이 컴파일된다. CI가 `--check`로 최신 여부를 검사한다.

## 검증 계획

- **정확성:** Showdown(`vendor/pokemon-showdown`, M-C)과 차분 테스트. 같은 상태·행동에서 결과 분포(데미지, 순서, 필드 변화)를 `oracle/`의 정규 상태로 비교한다. 목표는 시나리오 모음 전체에서 정확 일치(TV 0) 비율 99% 이상.
- **속도:** poke-engine `data/benchmark.rs`와 같은 상태로 초당 노드 수 비교. 싱글 모드 ±5% 이내 목표.
- **실력:** 싱글에서 새 엔진 foul-play와 원본 foul-play의 쌍 비교 대전.
- 기술 이식 순서는 `teams/library`의 사용 빈도 × 미구현 여부(커버리지 감사)로 정한다.

## 빌드

로컬에는 MSVC 링커가 없지만 GNU 툴체인은 동작한다. 대상 디렉터리는 저장소 밖(`%TEMP%`)에 둔다. serde 프로시저 매크로의 빌드 스크립트 때문에 Clippy도 GNU 툴체인으로 실행하고, rustfmt만 같은 rustc 1.98.1의 MSVC 툴체인을 쓴다. 명령은 `CONTEXT.md`를 참고한다. CI는 `.github/workflows/engine.yml`이 GitHub Actions에서 수행한다.

- `core`: fmt, clippy, `cargo test -p lab-engine -p lab-scenario` (Ubuntu, Windows)
- `wheels`: lab-engine wheel (maturin, abi3)
- `poke-engine-baseline`: poke-engine `f4e224c`(0.0.48, foul-play 고정 버전)의 `terastallization`(Gen 9 싱글)·`bss`(Champions BSS) wheel. 싱글 기준선용.

결과 wheel은 Actions artifact에서 받는다.

## 로드맵

1. [완료] oracle(정확 열거·MC·비교기), Champions 데이터 추출.
2. [테이블·초기 로더·초기 분포 완료] 데이터 → Rust 정적 테이블 생성(`core/src/dex/`), 초기 상태 시나리오 로더, 정규 상태 출력, 첫 등장 펼치기(트레이스·모래날림·그래스메이커). `single-hit` 초기 분포는 Showdown oracle과 정확 일치. 남은 것: 다른 등장 특성·도구, `patch` 적용, VGC 4마리 선출.
3. [원시 연산 완료] Champions 능력치 공식(SP), 데미지 공식(난수 16단계·급소·STAB·상성·광역 0.75·날씨·필드·아이템 보정). 남은 일은 타입/필드/아이템/특성 훅을 상태와 연결하고 `@smogon/calc` generation 0 및 oracle 시나리오를 늘리는 것이다.
4. 턴 진행: 행동 큐(우선도→스피드→동속 분기, 행동마다 재정렬), 대상 해석(생존 슬롯, 날따름·분노가루 유도), 방어·속이기·교체·기절 후 교체 요청, 턴 종료 처리 순서.
5. 기술·특성·도구 이식. 순서는 `teams/library` 더블 팀의 사용 빈도 × `handlers` 미구현 여부. 중력·최면술·수면·날따름·트릭룸·날씨·필드·위협·메가진화가 선두.
6. 분포 열거의 성능: 같은 결과 병합, 무관한 난수 생략, make/unmake로 복사 없는 탐색. poke-engine 벤치와 초당 노드 수 비교.
7. 탐색은 엔진 정합성 99% 이후. 순서: 엔트리 공개 시 동치류 사전 계산표 → 상대 모델 ①(완전정보, 최악 난수 보장/승률 두 모드) → ②(표준 믿음 + 관측 갱신) → 플랜별 성립 조건 출력 → 우리 조정 외부 루프. 일반 대전용 상대 세팅 추정(사용률 사전분포·결정화)은 그 뒤.
