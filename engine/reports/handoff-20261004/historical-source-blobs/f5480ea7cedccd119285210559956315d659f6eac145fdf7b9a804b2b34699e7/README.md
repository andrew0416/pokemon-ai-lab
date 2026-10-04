# P14 구현: 정상 행렬 재구성과 Lazy full 선택지 복사 생략

기준은 immutable `048e4748e8854dac5eb511a358cc2f8c38c9dc29`다. `source/`는 공유 저장소의 `git -c core.autocrlf=false archive`로 새로 추출했다. 기존 DESIGN.md, review.json, review-followup.json은 그대로 보존했다. P11/P12/P13 구현을 합치지 않았고 production이나 공유 checkout을 수정하지 않았다.

## 구현 계약

`lab-search/experiment-matrix-pass-through=[]`는 기본 OFF다. 별도 `experiment-matrix-pass-through-observer=[]`는 runtime을 암묵적으로 켜지 않으며 timing 빌드에는 넣지 않는다. core/scenario feature나 production API는 바꾸지 않았다.

- **P14a:** `drop_unevaluable`에서 `rows.checked_mul(cols)==Some(values.len())`이고 모든 값이 `!is_nan()`이면 기존 소유 `(ours,theirs,values)`를 그대로 돌려준다. omitted는 둘 다0이고 각 Vec의 포인터·capacity·순서·값 bits를 유지한다. NaN 또는 잘못된 차원은 원래 filter 본문으로 간다. ±infinity는 그대로 허용하며 `is_finite`를 쓰지 않는다.
- **P14b:** `LazyGame::step`의 full 경로는 원래대로 모든 known cell에 expect를 적용하여 values를 먼저 만든다. 그 뒤 `full_matrix_parts`의 같은 guard가 맞으면 원래 선택지 길이를 직접 사용하고 choice Vec clone 둘을 생략한다. fallback은 기존 clone→filter→길이 순서다. 빈 축 검사, restricted NaN의 Full 전환, dominance/Nash 호출과 반환 순서는 그대로다.

P14b helper 추출은 feature OFF 소스에도 있으므로 이번 비교는 **동일한 P14 소스에서 feature OFF/ON**이다. source048 원본과 전체 코드 생성이 같다는 주장은 하지 않는다. 원래 fallback 본문 자체는 source048과 바이트 비교한다.

## 완료된 로컬 논리 검증

`local-validation/run.ps1`/`verify_logic.py`가 명령·실제 compiler artifacts/features·바이너리 해시·로그·JSONL 해시를 보존한다. 오프라인/locked, Cargo jobs1, generic x86-64, debug opt0, debug정보0, 16MiB Rust test stack이다. 검색 crate는 `--no-default-features`로 불필요한 CLI 빌드를 제외했다. core/scenario/search 전체 회귀 및 default-CLI CI 빌드는 이 로컬 범위에 포함하지 않는다.

6개 구성을 실행했다: dense/compact × runtime OFF/ON의 observer 빌드4개, compact × runtime OFF/ON의 비계측 빌드2개. compact 기준선은 old4+D이며 dense 축은 P10만 끈다. P8e/P8f/P11/P12/P13은 모두 OFF다.

- 일반 lib/public test **106 실행·통과**, 별도 allocator child **6 실행·통과**. 실패·무시0. allocator child는 자기 단일 test만 실행하므로 관련 없는 test 필터 수가 로그에 나타난다.
- source048 원본 filter 참조와 0..3×0..3 모든 NaN mask를 N1/2 양쪽에서 대조한 **1,378개** 행렬 record. ±0·±infinity·정상/서브노멀 값·NaN payload3종, choice 순서, omitted 수를 bits로 비교했다. 추가 malformed 짧은 입력의 short-circuit 성공/인덱스 panic, trailing 값 무시, checked_mul overflow, 빈 축을 검증했다.
- source048 원본 LazyGame::step 참조와 full/restricted, NaN, None, 잘못된 차원, 빈 축, dominance OFF/ON의 결과·전략·exploitability·iterations bits와 panic 내용을 대조했다. full+빈축+None에서도 원래 known-cell expect가 먼저 실행된다.
- 공개 검색 **51개** records: toy32(슬롯1/2·Expect/Worst·1/2threads·dominance OFF/ON·lazy OFF/ON), depth3 2, terminal2, unsupported2, 성공 resume1/replacement1, NaN evaluator4, 기존 turn fixture7. 전체 State Debug/hash·복구, instructions/suspension/probability bits, matrix/전략/값 bits, 7개 정수 통계와 시간 필드0의 전체 analysis를 기록했다.
- 공개51 및 unit1378 JSONL은 **6개 구성 모두 바이트 동일**했다. 환경 변수는 `LAB_P14_RECORDS`와 `LAB_P14_UNIT_RECORDS`이며 파일은 create_new로만 만든다. 이전 결과를 덮어쓰지 않는다.
- observer 공개 corpus에서 OFF `(a_calls,b_calls)=(182,108)`과 pass0, ON `a_calls=80,a_pass=72,a_fallback=8,b_calls=108,b_pass=102,b_fallback=6`을 확인했다. 따라서 b 호출 수와 `a_calls+b_pass`가 보존되고 두 shortcut 및 두 fallback이 실제 호출됐다. 이는 위 작은 corpus의 work 증거이며 coaching/sand timing 국면 증거는 CI에서 별도로 확인해야 한다.

## 할당 증거와 한계

격리된 test subprocess가 이미 구성된 3×4 행렬 입력을 사용한다. 할당·재할당 호출만 세며 시간은 측정하지 않는다.

| 구간 | OFF reference / actual | ON reference / actual |
|---|---:|---:|
| P14a 소유 행렬 filter | 5 / 5 | 5 / 0 |
| P14b 선택지 clone 이후 정리 구간 | 7 / 7 | 2 / 0 |

P14b reference는 **그 빌드의 실제 P14a**를 호출한다. 따라서 ON reference2는 choice clone 두 개만 나타내며 P14a 효과와 중복 계산하지 않는다. 이 구간의 values Vec는 이미 만들어진 입력이다. **LazyGame::step의 values materialization과 Nash 내부 할당은 남아 있다.** 전체 step·탐색이 allocation-free라는 뜻이 아니다. 기능별 할당 제거가 전체 속도 개선을 뜻하지 않으며 local timing은 실행하지 않았다.

허용 source diff는 search Cargo.toml, solve.rs, 새 solve/matrix_pass_through_observer.rs·matrix_pass_through_tests.rs, 새 tests/matrix_pass_through.rs·p14_support.rs 여섯 파일이다. 수정한 Rust 파일만 rustfmt(skip_children=true)를 실행했다. source-receipt.json은 최종 SHA·허용 diff·원본 fallback hash·공유 HEAD/index 보존을 고정한다. local-validation/logic-receipt.json은 실제 검사 범위와 미실행 항목을 고정한다.

미완료: CI 전체 회귀, fresh6,184 정확성, 실제 timing workload의 activation, 동일VM observer-OFF CPU/wall 및 별도 RSS. 채택과 속도 평가는 그 결과 뒤에 판단한다.

최종 source commit은 `bc4fb5ef7d1aaa2400edd34931071ec6f0b64781`이며 ref는 `codex/p14-matrix-pass-through-source-20260930`이다. private index로 parent048 위 여섯 파일만 반영했다. Git diff --check를 통과했고 기존 원본 파일 3,560개가 그대로임을 확인했다. source commit 이후 엔진 파일을 수정하지 않는다.


## 실제 GitHub 실행 — 2026-09-30

검토 완료한 r2 controller `46c600278aec8360bb02611fe0ed862e6668c447`와 source `bc4fb5ef7d1aaa2400edd34931071ec6f0b64781`를 새 실험 브랜치에 게시했다. [Run 36730877673](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/36730877673)은 **23:40:20 KST 실행 접수**, 계획 job은 23:40:26–23:40:38 성공, 정확성 base/P14 runner는 각각 23:40:42/23:40:41에 시작했다. 23:41:18 확인 시 in_progress, 실패 단계 없음이었다.

실행 기준은 같은 source에서 old4+D 대 old4+D+P14이다. P12/P13은 포함하지 않는다. fresh 6,184건과 전체 회귀·실제 기능 활성화 검증 뒤 동일 VM 1스레드 10쌍 AB/BA와 별도 RSS 2쌍을 측정한다. 아직 정확성·성능 완료 결과를 뜻하지 않으며 생산 적용은 하지 않았다. 실행 영수증은 CI 실험 폴더 `launch.json`, 최종 게시 package/root review는 `revisions/r2/`에 있다.
