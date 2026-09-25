# lab-engine 작업 인계 (2026-09-25)

다음 세션이 이 문서만 읽고 이어서 작업할 수 있도록 정리했다. 설계 원칙과 로드맵 전체는 `DESIGN.md`에 있다.

## 사용자 지시 (우선)

- 1차 목표는 **PokaiEngine급 엔진**이다(arXiv 2608.29197, Max Yu). 이 엔진은 행동 조합 하나의 결과 분포 전체를 한 번에 열거하고, Showdown과 약 99% 일치한다고 보고됐다. 코드는 공개되지 않았다.
- 대응 범위는 **포켓몬 챔피언스뿐**이다. 규칙과 메커니즘은 Champions를 따른다.
- **기믹 (2026-09-25 사용자 명확화):** Champions M-C 규칙셋은 메가진화만 허용하고 나머지 활성화 방식은 거부한다. 울트라버스트·Z기술·다이맥스(거다이맥스는 다이맥스 상태로 표현)·테라스탈은 **꺼 둔 것이지 제거한 것이 아니다.** 행동·상태 구조에 남겨 두고 규칙셋으로 켤 수 있게 유지한다. 원시회귀는 선택하는 기믹이 아니고 자동 폼체인지로 다룬다.
- **규칙에서 벗어나지 않으면 레귤레이션에 없는 포켓몬·기술·도구도 쓸 수 있다.** 엔진에 합법성 목록을 넣지 않는다.
- 형식은 **더블(M-C)을 먼저** 맞춘다. 싱글은 `State<1>`로 컴파일되는 구조만 유지한다.
- 기존 골격(다른 세션 "Stockfish 포켓몬 AI"가 커밋 `e38207f`로 만든 `engine/core`)을 이어서 쓴다.

## 현재 상태

| 항목 | 상태 |
|---|---|
| `core/` Rust 골격 | `State<N>`, 되돌릴 수 있는 `Instruction`(apply/reverse), 필드·진영 효과 테이블, `SlotAction`, `Evaluator`. 2026-09-25 확장: `Slot`에 `Volatiles`(`volatile.rs`, 종류별 `{active, duration, counter}` 표)·`last_move`·`move_actions`, `Pokemon`에 `base_ability`·`last_item`, `Status::Fainted`(fnt), `State.result`(`BattleResult`), 파티 주소 `PokemonRef`. 명령은 상태·도구·특성·PP·휘발·턴·결과까지 되돌릴 수 있다. `State`는 `Eq + Hash`(결과 병합 키) |
| `core/src/turn/` 턴 엔진 | **2026-09-25 착수, oracle 정확 일치 확인.** `enumerate_turn(state, ruleset, [JointAction;2]) → Vec<Outcome{probability: f64, instructions}>`(정확 분포), `sample_turn(…, samples, seed)`(몬테카를로). 행동 하나 또는 턴 종료를 한 단계로 보고, 단계 안의 난수는 재실행 열거(`branch.rs`), 단계마다 같은 (상태, 남은 행동)을 병합한다. 결과 명령은 시작·끝 상태의 차이(`diff.rs`). 구현: 행동 순서(우선도·스피드·동속 무작위, 행동마다 재정렬, 쓰러진 포켓몬의 대기 행동 포함), `runMove`(수면·얼음·풀죽음·중력·마비 순의 BeforeMove, PP, lastMove), 대상 해석(재지정 포함, 유도 없음), 방어/판별(연속 사용 카운터, willAct), 속이기, 사이코필드·타입 면역·가루·짓궂은마음 악 면역, 명중(중력 6840/4096, 랭크), 데미지(급소 1/24 등, 타입 강화 도구, 필드 보정, 모래 특방·설경 방어, 날씨, STAB, 상성, 화상, 생명의구슬, 벽), 기합의띠, 흡수·반동, 상태이상·휘발·랭크 효과, 부가효과, 탁쳐서떨구기, 날씨·필드·중력·트릭룸·순풍·벽 설정, 교체(날씨·필드 특성·위협), 기절 처리·승패(`checkWin`), 턴 종료(`fieldEvent('Residual')` 정렬: 날씨 피해·지속 감소, 그래스필드 회복, 먹다남은음식, 화상·독·맹독, 휘발 지속), `checkFainted`(fnt)·교체 요청 대기. **지원 검사(`support.rs`)**: 선택한 기술, 필드의 특성·도구·종·휘발·필드/진영 효과 중 구현 안 된 것은 `TurnError::Unsupported`로 거부한다(콜백 목록을 표로 고정하고 dex와 다르면 테스트 실패) |
| `core/` 능력치·데미지 원시 연산 | `stats.rs`에 Champions SP 제한/능력치 공식, `damage.rs`에 4096 고정소수점 보정·16개 정수 데미지 롤. Rillaboom의 Grassy Glide → Tyranitar 오라클 롤 `[146..174]`과 정확 일치. 아직 기술·특성·도구 훅 및 턴 진행에는 연결하지 않았다. |
| `core/` 기믹·규칙셋 | `gimmick.rs`(`Gimmick` 6종, 1바이트 `GimmickSet`, `DynamaxState`), `rules.rs`(`Ruleset::CHAMPIONS_MC` = 메가만, 슬롯·진영 행동 검증 `ActionError`, 기믹을 붙인 합법 행동 생성 `joint_actions`), `Side.gimmicks_used`, 되돌릴 수 있는 `Instruction::UseGimmick`. 기믹의 실제 효과는 미구현. 테스트 11개 통과(GNU, 2026-09-25) |
| `oracle/enumerate.cjs` | Showdown 정답 분포 추출기. `full` / `extremes` / `mc` 모드. 동작 확인함 |
| `oracle/canonical.cjs` | 엔진과 oracle이 공유하는 정규 상태 JSON(schema 1). 포켓몬은 이름으로 식별 |
| `oracle/compare.cjs` | 두 분포의 TV 거리와 차이가 큰 결과를 출력. MC가 섞이면 잡음 기준으로 종료 코드를 정한다 |
| `oracle/scenarios/` | `hypnosis-gravity`, `single-hit`, `spread-damage`, 공용 팀 `hypnosis-gravity.p1/p2.json` |
| `data/export.cjs` → `data/champions.json` | Champions 모드 dex 전체(종 1518, 기술 938, 도구 583, 특성 321, 타입, 성격, 주요 상태). `isNonstandard`는 태그로만 남기고, 콜백으로 구현된 동작은 `handlers`에 이름만 남긴다(= 직접 구현할 목록) |
| `data/gen-rust.cjs` → `core/src/dex/generated.rs` | **Rust 정적 테이블(2026-09-25).** 종 1517(MissingNo. 제외: Bird 타입이 상성표에 없음), 기술 938, 도구 583, 특성 321, 타입 19+상성표·타입 면역, 성격 25, 이름으로 참조되는 조건 110. 모르는 필드·값 형태·이름 참조가 나오면 생성이 실패한다. 생성 파일은 직접 고치지 않는다 |
| `core/src/dex/mod.rs` | 테이블 타입과 조회 API. `SpeciesId`·`MoveId`·`ItemId`·`AbilityId`·`ConditionId`(인덱스 0 = 없음), `from_id`(이진 탐색)·`from_name`, 항목별 상수(`species::GARDEVOIR_MEGA`, `moves::HYPNOSIS`, `conditions::GRAVITY`). `Pokemon`의 종·도구·특성·타입·기술 필드도 이 ID 타입으로 바꿨다. 테스트 21개 통과(GNU), fmt·clippy 통과 |
| `scenario/` (`lab-scenario`) | **시나리오·팀 JSON → `State<2>` 로더(2026-09-25, 검증 완료).** serde/serde_json은 이 크레이트에만 있고 `lab-engine`은 의존성 없음 그대로다. 종·기술·도구·특성·성격은 dex API로 찾고 모르는 이름은 편·팀 위치·이름을 붙인 오류로 거부한다. 능력치는 `stats::champions_stats`(SP 검증 포함). 레벨·타입·HP·5능력치·상태·도구·특성·기술 4칸·PP를 채운다. 파티 순서 = 팀 프리뷰 순서(Showdown `chooseTeam`처럼 순서 문자열을 팀 크기로 자르고 빠진 멤버는 원래 순서로 뒤에 붙임), 앞 N마리가 선두. 표시 이름·팀 위치·성격·SP·성별·테라 타입은 `State` 밖 사이드카(`ScenarioMeta`/`SideMeta`)에 둔다. `setupTurns`·custom game 외 형식·레벨 50 외·모르는 JSON 필드는 거부한다. 2026-09-25: `patch`는 `decision.rs`가 `enumerate.cjs` `applyPatch` 의미로 적용한다(`scenario_states` = 등장 펼치기 + 패치; 수면 패치는 `statusTime` 필수, 날씨·필드 패치는 지속 턴 필수). `parse_choice`/`scenario_choices`가 Showdown 선택 문자열을 `JointAction`으로 바꾼다. 정규 출력은 휘발·lastMove·lastItem·statusTime/Stage·중력/트릭룸·순풍/벽·기절(fnt, slot null)·request(move/switch/"")·ended/winner를 쓴다 |
| `scenario/src/bin/lab-turn.rs` | 시나리오를 엔진으로 돌려 oracle과 같은 형식의 보고서를 쓴다(`--before <oracle 보고서>`로 시작 상태 선택, `--mc N --seed S`로 표본 모드). `oracle/compare.cjs`(결합 분포 TV)·`oracle/marginals.cjs`(특징별 주변분포, 큰 분포용)로 비교 |
| `core/` 로더 지원 | `state::champions_max_pp`/`MoveSlot::full`(Champions PP: `(pp/5+1)*4`, PP 증가 불가 기술은 기본값), `gimmick::mega_evolution`/`structural_gimmicks`(메가스톤의 `mega_stone` 표에서 정확한 종 일치로 메가 자격만 도출). 다른 기믹 자격은 도출하지 않고, 규칙셋도 M-C에서 막는다 |
| `scenario/src/switch_in.rs` | **첫 등장 펼치기(2026-09-25, 검증 완료).** `initial_outcomes(&LoadedScenario)`/`expand_switch_ins(&State<N>)` → `Vec<InitialOutcome { probability, state }>`. Showdown `runSwitch` 순서(저장 S 내림차순, 동속 균등 분기), 트레이스 균등 대상 분기(`NOTRACE`), 복사 특성 즉시 시작, 모래날림·그래스메이커(5턴, 보송보송바위/그라운드코트 8턴). 구현 목록 밖의 시작 핸들러는 오류로 거부. `State`와 `lab-engine`은 바꾸지 않았다 |
| `scenario/src/canonical.rs` | **정규 상태 schema 1 출력(2026-09-25, 검증 완료).** `canonical_json`(canonicalKey와 같은 바이트열)·`canonical_value`. 표현 못 하는 상태는 `CanonicalError::Unrepresentable` |
| `oracle/initial.cjs` → `oracle/expected/single-hit.initial.json` | 초기 분포 oracle(팀 프리뷰→첫 결정 열거 + 고정 시드 `before`). Showdown 고정 커밋에서 재생성 완료: 2분기·2결과, 각 1/2 |
| `DESIGN.md` | 범위, oracle, 데이터, 빌드, 로더, 등장 펼치기·정규 출력, 로드맵 갱신 |

**커밋:** 2026-09-25 사용자 지시로 브랜치 `lab-engine`에 커밋한다(원격 없음, `main`은 `e38207f`). 이후 커밋도 사용자 지시가 있을 때만 한다.

**병렬 작업 체계 (2026-09-26):** 사용자 지시("진행 가능한 것부터, 동시에 가능한 것은 동시에")로 Fable 세션이 본 트리에서 F 단위를, Opus 세션들이 각자 **git worktree**(별도 브랜치)에서 O 단위를 맡는다. worktree에는 `vendor/`가 없으므로 oracle 스크립트(`enumerate.cjs`·`initial.cjs`·`export.cjs`)는 환경변수 `LAB_ROOT=D:/pokemon-ai-lab`로 본 저장소의 Showdown을 찾는다. 세션마다 `CARGO_TARGET_DIR`을 따로 둔다(`D:/cargo-target-opus-a` 등). 정확 비교 테스트 헬퍼는 `scenario/tests/common/mod.rs`에 있고 단위마다 자기 테스트 파일을 만든다(`tests/mega.rs`, `tests/abilities_damage.rs`, …). `WORKPLAN.md` §4·`COVERAGE.md`·이 문서는 병합하는 세션(Fable)만 고친다. Opus 브랜치는 Fable 세션이 `lab-engine`에 merge 한다. 첫 병합(2026-09-26, `2e0be94`): 기술 핸들러 10단위(O2·O3·O5·O34·O17·O11·O12·O25·O26·O24), `core/src/turn/moves/handlers.rs` 신설. **시나리오 설계 요령(Opus 보고):** 활성 포켓몬 사이에 속도 동률이 있으면 Showdown이 `eachEvent('Update')`마다 셔플해 분기가 폭증하고, 부가효과 확률 판정 하나가 100분기라 그런 대상은 한 마리만 둔다. 시작 시 속도 동률은 오라클로 확인한 대로 **균등 무작위**다(`tie-start.initial.json`, 2026-09-26). 둘째 병합(`04ec529`): 상태 면역·우선도·잔여·날씨 특성 7단위(O65·O61·O62·O52·O53·O50·O51). 새 도우미: `Battle::active_move`, `ability_unless_broken`(특성 무시 기술), `set_status_blocked`, `cured_on_update`(`Update` 이벤트가 없어 "치료될 상태로 필드에 있는 것"을 거부; 메가진화·트레이스도 검사), `DamageSource::Recoil`. 셋째 병합(`5654f3b`): 데미지 보정 특성 8단위(O40~O47), 새 파일 `core/src/turn/abilities.rs`(모듈은 dex `abilities`와 이름이 겹쳐 `moves.rs`·`residual.rs`에서 `ability_events`로 별칭). 데미지 계산의 모든 보정(BasePower·ModifyAtk/SpA·ModifyDef/SpD·ModifyDamage·정확도)은 `Handler` 목록을 `ability_events::chain`으로 Showdown 순서대로 연쇄한다. 기술 고유 콜백은 `moves/handlers.rs`(이벤트별 함수)에 둔다. 새 기술·특성·도구 보정을 넣을 때는 이 두 틀에 얹는다.

**메가진화 (2026-09-26, WORKPLAN F1~F3 완료):** `Instruction::SetForme`/`SetTypes`(F1), `Pokemon { nature, stat_points }`(F2), `turn/mega.rs`의 `megaEvo` 행동(F3). 시나리오 `mega-tyranitar`(메가 마기라스의 모래날림이 눈을 덮음, 속도 113→123으로 갸라도스 118을 추월)·`mega-tyranitar-sand`(자기 모래 아래 메가: 같은 날씨라 지속 3이 유지)가 Showdown `full`과 정확 일치(`scenario/tests/mega.rs`). 메가 대상 종의 특성이 미지원이면 `TurnError::Unsupported`(예: 메가가디안 픽시레이트, 메가보만다 스카이스킨). 첫 등장 펼치기(`scenario/switch_in.rs`)는 이제 날씨 4종·필드 4종·위협도 처리한다(F4에서 `turn/switching.rs`와 통합 예정).

**등장 통합·기절 후 교체 (2026-09-26, WORKPLAN F4·F5 완료):** 등장 처리는 `core/src/turn/switching.rs` 하나다: `switch_in`(Showdown `switchIn`: 퇴장자 특성·타입 복귀, 기절 점유자 `fnt` 해제, 슬롯 배치), `run_switch_in`(일괄 `runSwitch`: 등장자들의 시작 핸들러를 **저장 속도(원시 스탯)** 내림차순, 동률은 균등 무작위, 특성이 바뀐 핸들러는 건너뜀), `start_ability`(날씨·필드·위협·트레이스). 첫 등장(`scenario/switch_in.rs`)은 상태 검증 뒤 `turn::enumerate_start`에 위임한다. 기절한 포켓몬은 `Slot::fainted_occupant`로 자리를 지키고(`checkFainted`가 `fnt`를 찍는 근거), 교체 결정은 `turn::enumerate_replacements(state, [[Option<party>; N]; 2])`: instaswitch(기절자 속도순, 동률 무작위) → 일괄 runSwitch → `endTurn`(턴 증가). 로더는 `setupTurns`를 엔진으로 재생하고(`scenario_positions`: 등장 → 설정 턴들 → 패치), Showdown의 `side.pokemon` 순서를 교체마다 갱신해 `switch N`을 해석한다(`advance_order`). `scenario_decision`이 `Decision::Turn`/`Replacement`를 고르고 `run_decision`이 실행한다. 시나리오 `ko-replace`(더블 KO → 양쪽 교체, 등장자 속도 동률로 2결과)가 정확 일치(`tests/replacement.rs`).

**넷째·다섯째 병합 (2026-09-26, `8cbc007`·`2cecf16`):** 면역·흡수·날씨 억제·부가효과 특성(O48·O49·O60·O66·O67; 새 파일 `moves/ability_hooks.rs`, `Battle::effective_weather`, `try_set_status_from`)과 도구(O82~O93; 새 파일 `turn/items.rs`, 구애 잠금 `choicelock`). 병합에서 정한 것: 위협 반응은 전부 `boost_by`(이너포커스 포함); 남은 큐는 `Battle::queue`만(줌렌즈도 이것을 읽음); 풍선 파열은 `damaging_hit`; 부가효과는 우격다짐/인분(`ability_hooks::secondaries`) → 하늘의은총 배수 → 은폐망토 → 왕의징표석 추가 풀죽음(배수 없음, 우격다짐도 통과) 순. 남은 거부: 메트로놈(F13), 커스탭(분수 우선도 열매), 클리어아뮬렛(`boost_by` TryBoost에 넣으면 됨), 우산(`effective_weather`에 보유자 조건 추가), 서투름(F17).

**잠금·혼란·앵콜 (2026-09-26, WORKPLAN F9 일부 완료):** 휘발 `Confusion`·`LockedMove`·`MustRecharge`·`Encore`를 구현했다(`VolatileState { time, mv, hidden }` 추가; 정규 출력은 `time`·`move`와 잠긴 기술의 `trueDuration`(숨은 실제 길이; `oracle/canonical.cjs`의 `EFFECT_FIELDS`에도 추가)을 쓴다). 잠긴 포켓몬은 `turn::locked_move`가 판정하고 `check_turn`이 선택을 잠긴 기술로 정규화한다(재충전 턴은 `RECHARGE_INDEX`, 시나리오 문자열은 `move recharge 1`; Showdown 파서가 더블에서는 대상을 요구하므로 잠긴 기술에도 대상을 적는다). 앵콜은 같은 턴에 이미 고른 기술을 덮어쓰고(`OverrideAction`: 원래 기술의 우선도 유지, 대상은 새로 무작위) 다음 턴부터 다른 기술을 막는다(`disabled`). 2턴 기술(솔라빔·구멍파기: `twoturnmove`·반투명)은 아직 없다.

**연속기 (2026-09-26, WORKPLAN F10 완료):** 연속기는 타격마다 한 단계로 실행된다. `moves::run_move`가 첫 타격 뒤 `MoveStep::Suspended(MoveProgress)`를 돌려주면 `Pending.in_progress`에 넣고 다음 단계에서 `resume_move`가 이어 간다(기술 꼬리 `use_move_tail`·`run_move_tail`은 마지막 타격 뒤). 그래서 5타짜리 기술도 타격 사이에 상태가 병합돼 열거가 폭발하지 않는다. Showdown은 이런 턴을 정확 열거하지 못하므로 `--mode mc --samples 20000` 보고서를 `oracle/expected/<이름>.mc.json`으로 두고 `common::assert_mc_parity`(TV < 3·잡음)로 비교한다. `ActiveMove`는 `MoveProgress`에 들어가므로 `PartialEq`/`Hash`를 수동 구현했다—필드를 더하면 거기도 더한다.

**행동 큐 API (2026-09-26, WORKPLAN F8 완료):** 단계가 실행되는 동안 남은 행동들이 `Battle::queue`(`core/src/turn/queue.rs`)에 있다. `will_move(slot)`·`queued_move(slot)`로 대상의 남은 기술을 읽고(기습·선더클랩·어퍼핸드), `prioritize_action`(애프터유, order 3)·`quash_action`(명령, order 201)으로 순서를 바꾼다(`Action.order`). 앵콜의 `OverrideAction`(F9)과 도우미(O7)는 이 위에 얹는다.

**`Update` 이벤트·열매 (2026-09-26, WORKPLAN F14 완료):** `core/src/turn/update.rs`의 `update_event`가 Showdown `eachEvent('Update')` 지점마다 실행된다(행동 뒤·타격 피해 뒤·날씨 잔여 뒤·건강한 퇴장 전·교체 등장 뒤·턴 끝). 열매(오랭·오렌·피지류·랭크 열매·럼·상태 열매·리피아)와 먹보를 구현했고 `eat_item`이 Showdown `eatItem`이다(`lastItem` 기록). 특성 `onUpdate`(상태 치료류)는 아직 `cured_on_update` 가드로 도달 불가 처리한다—이제 `update_event`에 팔을 더하고 가드를 풀 수 있는 자리가 생겼다. 새 Update 리스너가 다른 포켓몬에 영향을 주면 순서(속도, 동률 무작위)를 실제로 분기시켜야 한다.

**랭크 이벤트·피격 이벤트 (2026-09-26, WORKPLAN F15·F16 완료):** 랭크 변화는 모두 `Battle::boost_by(target, boosts, source, BoostEffect::{Move,Ability,Item})`를 거친다(Showdown `boost()`의 ChangeBoost → 캡 → TryBoost → 랭크별 AfterEachBoost → AfterBoost). 위협 반응(경쟁심·오기·미러아머·가드도그·주눅), 클리어바디류, 심술꾸러기·단순·천진(`boost_seen`)이 구현됐다. 피해 입힌 뒤 `moves::damaging_hit`가 `onDamagingHit`를 Showdown 순서로 돌린다(까칠한피부·철가시·울퉁불퉁멧·주눅). 새 특성·도구는 두 함수의 `match`에 팔을 더하고 `support.rs` 표에 콜백을 고정한다. 시나리오 `boost-events`(위협 vs 경쟁심·오기·미러아머), `damaging-hit`, `unaware-contrary`(setupTurns 사용) 정확 일치.

**대상 유도 (2026-09-26, WORKPLAN F7 완료):** `moves.rs::redirect_target`가 Showdown `priorityEvent('RedirectTarget')`를 재현한다. 후보는 상대의 날따름·분노가루·스포트라이트 휘발(`onFoeRedirectTarget`, 우선도 1·1·2)과 아군·상대의 피뢰침·저수(`onAnyRedirectTarget`, 우선도 0; 전기/물 기술만). 우선도 내림차순 → 보유자 속도(`action_speed`, 트릭룸이면 음수) 내림차순으로 정렬해 기술의 대상 종류에 유효한 첫 보유자로 바꾼다(분노가루는 공격자가 가루 면역이면 건너뜀). 피뢰침·저수의 `onTryHit`는 방어·사이코필드 뒤에 판정해 기술을 무효화하고 특공을 1 올린다. 같은 우선도·속도의 유효 후보가 둘이면 Showdown은 `effectOrder`(등장·특성 변경 순서)로 가르는데 상태에 없으므로 `TurnError::Unsupported`. 시나리오 4개(`followme-hypnosis`, `ragepowder-grass`, `lightningrod-foe`, `lightningrod-ally`)가 정확 일치(`scenario/tests/redirect.rs`). 로더의 기본 표시 이름은 Showdown처럼 **기본종 이름**이다(`Indeedee-F` → `Indeedee`; 닉네임이 종 문자열과 같아도 기본종).

**턴 엔진 검증(2026-09-25):** `single-hit` 정확 분포 271개 결과가 Showdown `full`과 TV 0으로 일치(엔진 약 10ms, Showdown 7.8초). `hypnosis-gravity`(중력 패치) 2개 결과 정확 일치. 두 fixture는 `oracle/expected/*.turn.json`(`strip-report.cjs`로 생성)이고 `scenario/tests/turn.rs`가 정확 비교한다. `spread-damage`는 정확 분포가 너무 커서(세 번째 단계에 중간 상태 14,260개, 네 번째 단계는 수백만) 끝까지 열거하지 못했다. 대신 엔진 표본 200,000회(2.4초)와 Showdown 표본 20,000회(90초)의 주변분포 69개가 모두 잡음(4σ 기준) 안에 있음을 확인했다(`marginals.cjs`). 테스트: `lab-engine` 36개, `lab-scenario` 18개, fmt·clippy(-D warnings)·dex 최신 검사 통과(MSVC).

**로더 검증(2026-09-25):** serde 의존성과 `Cargo.lock`을 갱신했고, GNU에서 `lab-engine` 28개 + `lab-scenario` fixture 6개 테스트가 통과했다. rustfmt, GNU Clippy(`-D warnings`), dex 생성 최신 검사도 통과했다.

**등장 펼치기·정규 출력 검증(2026-09-25):** `oracle/initial.cjs`를 실제 Showdown에서 실행해 fixture를 재생성했다. Rust의 두 초기 결과가 oracle과 상태·확률 모두 정확히 일치하고, 고정 시드 `before`가 그중 하나와 일치한다. `lab-engine` 28개 + 기존 loader fixture 6개 + 초기/canonical 9개 테스트가 통과했다. `speed_ties_branch_and_smooth_rock_extends_sand`의 기대 분포(1/4·1/4·1/2)와 `canMega` 출력은 Showdown 코드를 읽고 유도했으며 별도 oracle 시나리오는 아직 없다.

## 검증 결과 (2026-09-25)

- `hypnosis-gravity`(중력 아래 최면술, 속이기는 방어에 막힘): 결과 2가지, 수면 2턴 2/3, 3턴 1/3. 명목 대상 난수를 접으면 분기가 768개에서 6개로 줄고 결과는 같다.
- `single-hit`(그래스슬라이더 1회): `full` 2112분기 → 결과 271가지, 7.3초. `mc` 4000회와 비교하면 TV 0.088이고 잡음 추정치는 약 0.10이라 일치한다.
- `spread-damage`(광역기 2개 + 단일기 2개): `extremes` 모드도 10분을 넘겨 중단했다. `mc` 3000회는 8.3초, 결과 167가지. 이런 턴은 `mc`로 통계 검사한다.
- Showdown 재실행 비용은 분기당 약 3.5ms다.
- `damage_rolls` 릴리스 마이크로벤치(20,000,000회 × 5): 중앙값 25.40ns/16롤 분포, 초당 약 3,938만 분포. 이는 저수준 데미지 원시 연산 수치이며 전체 턴 열거 속도는 아니다.

## 실행 방법

```bash
node engine/oracle/enumerate.cjs engine/oracle/scenarios/single-hit.json --out "$TEMP/full.json"
node engine/oracle/enumerate.cjs engine/oracle/scenarios/single-hit.json --mode mc --samples 4000 --out "$TEMP/mc.json"
node engine/oracle/compare.cjs "$TEMP/full.json" "$TEMP/mc.json"
node engine/data/export.cjs
node engine/data/gen-rust.cjs            # champions.json → core/src/dex/generated.rs (--check: CI용 최신 여부 검사)
# 턴 엔진 대 Showdown (엔진 쪽은 cargo build -p lab-scenario --release 후)
D:/cargo-target/release/lab-turn.exe engine/oracle/scenarios/single-hit.json --before "$TEMP/full.json" --out "$TEMP/engine.json"
node engine/oracle/compare.cjs "$TEMP/full.json" "$TEMP/engine.json"          # 정확 분포: TV 0이어야 함
D:/cargo-target/release/lab-turn.exe <scenario> --before <mc 보고서> --mc 200000 --out "$TEMP/engine-mc.json"
node engine/oracle/marginals.cjs <showdown mc 보고서> "$TEMP/engine-mc.json"   # 큰 분포: 주변분포 비교
node engine/oracle/strip-report.cjs "$TEMP/full.json" engine/oracle/expected/<이름>.turn.json   # fixture 갱신
LAB_ENGINE_STATS=1 …                                                       # 단계별 프런티어 크기 출력
# 로컬 빌드 (2026-09-25 이후: 기본 MSVC 툴체인, 산출물은 D:\cargo-target)
cd engine && cargo fmt --all --check
cd engine && cargo clippy --workspace --all-targets -- -D warnings
cd engine && cargo test --workspace --exclude lab-engine-py     # py 크레이트는 extension-module이라 cargo test 불가(CI도 제외)
cd engine && cargo bench -p lab-engine --bench damage
cd engine/py && ../../.venv-doubles/Scripts/maturin.exe build --release -i ../../.venv-doubles/Scripts/python.exe --out D:/cargo-target/wheels
../.venv-doubles/Scripts/python.exe -m pip install --force-reinstall --no-deps D:/cargo-target/wheels/lab_engine-0.1.0-cp312-abi3-win_amd64.whl
```

- **로컬 빌드 체계 (2026-09-25 사용자 지시로 전환. 이전 "로컬 빌드 금지" 방침은 폐기.)**
  - **MSVC(기본):** VS Build Tools 2022 17.14 C++ 워크로드를 `D:\VS\BuildTools`에 설치(MSVC 14.44.35207, Windows SDK 26100). vswhere가 찾으므로 rustc가 `link.exe`를 자동 인식한다. 설치 스크립트·로그: `D:\VS\install-buildtools.ps1`, `D:\VS\install-buildtools.log`. 실제 폴더가 삭제된 채 남아 있던 옛 인스턴스 메타데이터(Build Tools 2022, VS 2019 Community)는 `D:\VS\stale-instances-backup`으로 옮겨 두었다. rustup override는 없으므로 기본 MSVC 툴체인이 쓰이고, CI Windows 잡과 같다. fmt·clippy·test·maturin 모두 MSVC에서 통과 확인. py 크레이트 빌드 시 `linker stdout: ... .lib ... .exp 개체를 생성` 경고는 MSVC 링커 안내 메시지(`linker_messages`)로 무해하다.
  - **GNU(대안):** `cargo +stable-x86_64-pc-windows-gnu ...`. GNU 툴체인에 rustfmt·clippy 설치됨. rustup 자체 포함 MinGW에는 `as.exe`가 없어 pyo3(raw-dylib) 링크 시 dlltool이 실패하므로, 전역 설정이 GNU 타깃 링커·dlltool을 기존에 있던 `D:\winlibs`(GCC 16.2.0 UCRT)로 잡는다. GNU에서도 전 과정 통과 확인.
  - `%USERPROFILE%\.cargo\config.toml`(전역, 저장소 밖): `target-dir = D:/cargo-target` 및 위 GNU 링커 설정.
  - **CUDA 13.4.1** 툴킷을 `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.4`에 설치(2026-09-25, winget `Nvidia.CUDA`, 약 3.2 GB, `CUDA_PATH` 설정됨). **디스플레이 드라이버는 제외**했다(드라이버 591.86 유지, 지원 CUDA 13.1이지만 13.x 마이너 버전 호환으로 동작). 설치 구성 요소: nvcc, crt, cudart, nvvm, nvptxcompiler, cupti, nvrtc(+dev), nvjitlink, nvfatbin, nvml_dev, nvtx, thrust(cccl), cublas(+dev), curand(+dev), sanitizer, visual_studio_integration, cuda_profiler_api, cuobjdump, nvprune, cuxxfilt, nvdisasm, occupancy_calculator. cufft·cusolver·cusparse·npp·nvjpeg·nsight는 미설치이며 필요 시 `winget install --id Nvidia.CUDA --force --override "-s -n <구성요소_13.4 ...>"`로 추가한다. 첫 설치 때 `crt_13.4`·`nvvm_13.4`·`nvptxcompiler_13.4`를 빠뜨려 `crt/host_config.h` 없음 오류가 났고 추가 설치로 해결했다. 구성 요소 이름은 NVIDIA Windows 설치 가이드 표 참고.
  - 검증: 벡터 덧셈 커널(`nvcc -ccbin <D:\VS\BuildTools ...\cl.exe> -arch=sm_75`)이 GTX 1660 SUPER(sm_75)에서 1,048,576/1,048,576 정답. MSVC 헤더 코드페이지 경고 C4819는 무해(`-Xcompiler /utf-8`로 억제 가능). GPU 실험은 `wgpu`(Vulkan)로도 가능. 장비: Ryzen 5 3500X 6코어, 32 GB, GTX 1660 SUPER 6 GB.
  - `.venv-doubles`에 pip(ensurepip)·maturin 1.15.0 설치. wheel `lab_engine-0.1.0-cp312-abi3-win_amd64.whl` 빌드·설치·`slots("doubles")==2` 확인.
  - `py/src/lib.rs`는 `::lab_engine::`로 참조한다. `#[pymodule] fn lab_engine`이 같은 이름의 모듈을 만들어 의존 크레이트를 가렸고, 이전에는 CI maturin 단계가 이 이유로 실패했을 것이다.
- 커밋 `e38207f`의 골격은 rustfmt 기준에 맞지 않았다. 2026-09-25에 `cargo fmt`로 `instruction.rs`, `state.rs`의 기존 코드도 정렬되었다(동작 변화 없음).
- 정답 형식은 `gen9championsdoublescustomgame`이다. Champions 모드이고, 로스터 제한과 4마리 선출이 없으며, 레벨은 기본 50이다.

## 주의점과 함정

- **대상 번호는 양수=상대, 음수=아군이다.** `spread-damage`를 처음 만들 때 `highhorsepower -1`로 써서 아군 마기라스를 공격하는 실수가 있었고 `2`로 고쳤다. 새 시나리오는 로그로 대상을 확인한다.
- 필드·날씨·진영 효과 패치에는 source 포켓몬이 필요하다. `enumerate.cjs`는 p1 선두를 쓰고 남은 턴은 패치 값으로 덮어쓴다.
- Showdown은 더블 광역기(`allAdjacentFoes`, `allAdjacent`, `foeSide`)의 명목 대상을 무작위로 뽑는다. 결과와 무관한 난수라서 `full`에서는 접는다. 이 가정이 틀리지 않는지 `mc` 비교로 계속 확인한다. `randomNormal`(역린 등)은 접지 않는다.
- Champions 차이점:
  - 수면은 `sample([2,3,3])`이다.
  - 마비로 못 움직일 확률은 1/8이다.
  - 얼음은 최대 3턴이고 매 턴 1/4 확률로 녹는다.
  - PP 상한은 20이다.
  - 능력치는 HP = 종족값 + SP + 75, 나머지는 (종족값 + SP + 20) × 성격 보정이다(레벨 50, IV 반영 없음).
  - 자세한 구현은 `vendor/pokemon-showdown/data/mods/champions/{scripts,conditions}.ts`에 있다.
- 기믹은 `Ruleset`을 거쳐서만 허용한다. `Pokemon.gimmicks`(개체 자격)는 기본값이 비어 있다. `lab-scenario` 로더가 `gimmick::structural_gimmicks`로 메가 자격만 채운다. 손으로 만든 상태는 채우지 않으면 메가 행동도 `GimmickUnavailable`로 거부된다. 새 기믹을 켤 때는 enum을 바꾸지 말고 규칙셋에 추가한다.
- 정규 상태의 effect 필드는 허용 목록(`EFFECT_FIELDS`)이다. 불일치가 나오면 필드를 추가하고 `SCHEMA`를 올린다.
- `data/champions.json`(2.5MB)은 고정된 Showdown 커밋 `9e317a6`에서 생성했다. vendor를 갱신하면 `export.cjs` → `gen-rust.cjs` 순서로 다시 생성한다. CI에는 vendor가 없으므로(gitignore) CI는 `gen-rust.cjs --check`만 한다.
- dex 인덱스는 export가 바뀌면 달라진다. 규칙 코드는 숫자 대신 생성 상수(`moves::GRAVITY`)를 쓴다. 항목이 export에서 빠지면 컴파일 오류로 드러난다.
- 생성기가 일부러 버리는 필드(진화·알 그룹·색·태그·`requiredItem` 등 합법성/표시용)는 `gen-rust.cjs`의 `*_DROPPED`에 있다. 콜백 대신 상수 `false`인 이벤트(`onTakeItem: false` 등)는 bool 필드가 된다. 이벤트 순서 상수(`on*Priority/Order/SubOrder`)는 `event_orders`에 남긴다.
- `MoveTarget::User`는 Showdown의 `self` 대상이다(Rust 예약어 회피).
- **로더 상태는 선두 배치 직후, 등장 효과 전이다.** 등장 효과는 `switch_in::initial_outcomes`가 따로 펼친다. oracle `before`는 고정 시드 한 경로이므로 펼친 결과 **중 하나**와 같아야 한다(`single-hit`: 트레이스가 마기라스/몰드류 중 무엇을 복사했는지는 시드가 정한다). `state.turn`은 Showdown의 첫 결정 시점 값인 1로 둔다.
- **등장 펼치기의 특성 경계:** 동작 구현 = 트레이스(Trace), 날씨 4종(가뭄·잔비·모래날림·눈퍼뜨리기; 바위 도구로 8턴), 필드 4종(일렉트릭·그래스·미스트·사이코메이커; 필드확장 8턴), 위협(인접 상대 공격 -1, 랭크 이벤트 없음). 시작 시 무동작 확인 = 모래헤치기(Sand Rush). 나머지는 dex `handlers`에 시작 구간 이벤트가 없을 때만 무동작으로 받고, 있으면 `SwitchInError::UnsupportedAbility/Item/Species`로 거부한다(에어록, 쓱쓱(`onModifySpe`)·구애스카프, 씨앗류·부스트에너지·풍선처럼 시작 시 발동하는 도구 등).
- `Effect.turns`는 Showdown의 남은 `duration`과 같은 값으로 정했다(정규 출력 `weatherDuration` 등). `Effect::PERMANENT`는 정규 출력에서 아직 거부한다.
- `can_mega`: 정규 출력의 `canMega`는 개체 메가 자격 ∩ `format_ruleset(meta.format)` − 진영 메가 사용이다. M-C 메가 전용 잠금과 꺼 둔 기믹 구조는 바꾸지 않았다.
- 패치가 있는 시나리오(`hypnosis-gravity.json` 등)는 `scenario_positions`가 등장·설정 턴 뒤에 패치를 적용한다(`enumerate.cjs`와 같은 순서). 설정 턴은 결과가 하나로 수렴하도록(확정 KO 등) 설계한다. 여러 위치가 나오면 fixture의 `before`가 그중 하나를 고른다.
- Champions PP는 단순 상한이 아니다. 방어는 Champions 기본 PP가 5라서 최대 8이다(10이 아님). 기본 PP가 5의 배수가 아닌 기술은 모두 PP 증가 불가(Z/다이맥스기)이고, 이를 `state.rs` 테스트가 확인한다.
- 메가 자격은 Champions `canMegaEvo`처럼 `item.megaStone[species.name]`, 즉 정확한 종 일치로만 준다. `Gardevoir-Mega`가 가디안나이트를 들어도 자격이 없다. 레쿠쟈의 화룡점정 경로는 past/future 태그 규칙이 필요해서 모델링하지 않았다. `teraType`은 사이드카에만 보관하고 자격으로 주지 않는다(`Pokemon`에 테라 필드가 아직 없음).
- 로더가 받는 형식은 `gen9championsdoublescustomgame`뿐이다(모든 멤버 선출). `gen9championsvgc2026regmc`의 4마리 선출은 아직 없어서 거부한다. IV는 0–31 범위만 검사하고 버린다(Champions 공식에 IV 항이 없음). 이름은 Showdown처럼 20자로 자르고 편마다 유일해야 한다.

## 미구현 전수조사 (2026-09-25)

- [`SHOWDOWN-GAPS.md`](SHOWDOWN-GAPS.md): Showdown `sim/*.ts`의 행동 종류 15·이벤트 119·상태 필드를 엔진과 대조한 목록과, 중력 파티 기준 우선순위. [`COVERAGE.md`](COVERAGE.md): dex 전체를 `support.rs`에 통과시킨 자동 생성 표(기술 399/938, 특성 21/321, 도구 370/583 지원; 라이브러리 사용 기준 57%/17%/63%). 생성: `cargo run -p lab-scenario --release --bin lab-coverage -- --out engine/COVERAGE.md`. `core/src/turn/coverage.rs`가 지원 검사의 공개 API다.
- 가장 큰 공백은 순서대로 날따름·분노가루 유도, 메가진화, 기절 후 교체 결정 단계, 열매·`Update` 이벤트, 피격 특성(`DamagingHit`)·경쟁심/오기, 구애 도구, 연속기·교체기, 앵콜·도발·명령·도우미다. (2026-09-26: 유도와 메가진화는 완료.)
- [`reports/seed-champions-check-2026-09-25.md`](reports/seed-champions-check-2026-09-25.md): `D:\poke-teambuilder-seed`의 Champions 자료를 vendor와 대조한 결과(M-C 전수 일치, 메가루카리오Z 특성 칸 오류 1건, npm 0.11.11 기반 M-B 바인딩 stale). seed 디렉터리는 읽기 전용으로만 열었다.

## 작업 배분 (2026-09-25 사용자 지시)

미구현 항목을 원소 단위로 쪼개 [`WORKPLAN.md`](WORKPLAN.md)에 정리했다. 난이도 **상**(상태 모델·명령·열거 구조·행동 큐·결정 단계·이벤트 순서 재현: F1~F19)은 **Fable 5.1**, **중·하**(기존 훅에 핸들러 추가: O1~O106)는 **Opus 5.5**가 맡는다. 각 단위의 완료 조건(지원 표 갱신·오라클 시나리오·fixture·테스트·fmt/clippy/coverage 재생성)과 전제 조건은 그 문서 §0·§4를 따른다. Opus 세션은 "의존" 열이 §4에서 완료된 단위만 시작한다.

## 다음 할 일 (순서대로)

1. [완료 2026-09-25] `data/champions.json` → Rust 정적 테이블 (`core/src/dex/`).
2. [로더·초기 분포·canonical·patch 완료 2026-09-25, 등장 통합·setupTurns 재생 완료 2026-09-26] 남은 것: VGC 4마리 선출(O104).
3. [원시 연산·턴 연결 완료 2026-09-25] 데미지 코어가 턴 엔진에 연결됨.
4. [턴 엔진 골격 완료 2026-09-25, 메가진화·유도·등장 통합·기절 후 교체 완료 2026-09-26] 남은 것: 연속기(multihit), 교체기(유턴 등), 혼란·도발·앵콜 등 휘발, 구애 도구, 열매(오봉 등 `onUpdate`), 급소 랭크 보정 도구/특성. 진행 상황은 `WORKPLAN.md` §4.
5. 기술 이식: 날따름, 분노가루, 위협 이외 등장 특성, 메가진화를 먼저 한다. 우선순위는 `teams/library` 더블 사용 빈도 × `support.rs` 미지원 여부로 정한다. 추가할 때마다 oracle 시나리오와 `*.turn.json` fixture를 만든다.
6. 분포 열거 성능: 광역기 두 개가 겹치는 턴은 정확 분포가 수백만 결과다. 후보: 이미 행동한 대상에 대한 풀죽음 분기 생략(분포 동치), 턴 종료에 사라지는 중간 상태 차이(풀죽음·lastMove 없는 교환 등)를 병합 전에 정규화, HP 구간 대신 정확 값 유지하되 결과 수 상한/근사 모드 도입. PokaiEngine 목표(턴당 약 0.08ms)와 같은 자릿수.
7. 목표 지표: 시나리오 모음에서 정확 일치 99% 이상.

## 참고 자료

- PokaiTrainer 논문 요약:
  - 규칙은 Reg M-B, 공개 팀시트 전제.
  - 선출은 90×90 행렬 게임으로 푼다.
  - 배분은 엔진 재분기 기반의 베이즈 갱신으로 추정한다.
  - 사람 래더 150세트에서 약 59% 승률.
  - 판단 시간 중앙값은 2.7초이고, 탐색 규모를 키우면 23초.
  - 한계: 초반 형세를 낙관적으로 평가하는 문제가 남아 있고, 비공개 팀시트 환경은 다루지 않는다.
- 요약 도구로 읽은 내용이므로 수치를 인용하기 전에 원문을 확인한다.
