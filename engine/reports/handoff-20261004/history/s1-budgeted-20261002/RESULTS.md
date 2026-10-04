# S25a 검증 결과 — 2026-10-02

완전정보 선택적 탐색의 프로토타입과 실행용 CLI를 별도 실험 폴더에 구현했다. 기본 OFF이며 기존 평가기 인터페이스와 RM+를 사용한다. 동시행동의 혼합 전략, lazy 행렬 셀 평가, 확률 결과 보관, 선택적 깊이 확장, 조상 backup, 엄격한 전이 비용 예산과 마지막 완결 전략 반환을 지원한다.

## 최종 검증

| 구분 | 결과 |
|---|---|
| feature ON library | 34/34 통과: 신규17 + 기존17 |
| feature OFF library | 기존17/17 통과 |
| clippy | 새 library/CLI `-D warnings` 통과 |
| library 분리 | `--no-default-features --features experiment-budgeted-search` 통과 |
| CLI | 도움말, 예산0 미완성, p1, p2 네 조건 통과 |
| 확률·관점 | CLI 양쪽 분포 합1, 비음수; p1/p2 값은 +7.73945/-7.73945 |
| 예산 | CLI 완료 사례 turn15회×10 + switch24회×1 =174; 허용1000 이하. 합성 검사는 0 경계와 도중 중단 포함 |
| 소스 범위 | 기존2파일의 feature/모듈 등록 변경 + 새Rust5파일. 기존4,081파일 바이트 유지 |

프로파일은 Windows MSVC opt0, generic x86-64, 빌드 동시성2, 테스트 스레드1이다. 시간 로그는 검증 기록이며 성능 벤치마크로 해석하지 않는다. 기존 source의 P1e/P7d/P7h·KEEP-R1 runtime feature 결합 검증은 이 결과에 포함하지 않는다.

실제 엔진 대조는 원본 eject-button-uturn의 Full/Median과 ko-replace의 좁은 파생 상태다. ko-replace는 setup을 수행한 뒤 실존 party의 기술을 Harden 한 개로 제한했다. KO 상태, 강제교체, 등장 순서와 날씨는 유지한다. 독립 재귀가 전체 행동과 확률을 따라 계산한 값에 대해 허용오차0.02 이내를 확인했다.

## 별도 기준 행렬에서의 품질 검사

8개 합성 동시행동 게임, 깊이2, 5예산, 3seed의 **120조건**이다. 각 게임은 잘못된 shallow heuristic을 허용하고, 독립 all-action 재귀로 공통 reference root 행렬을 만든다. RM+는 양쪽에서 사용하는 수치 solver이므로 수학적 exact Nash oracle이라고 부르지 않는다.

| 전이 비용 예산 | 조건 수 | reference root BR gap 평균 | 중앙값 | 최댓값 |
|---|---:|---:|---:|---:|
| 4 | 24 | 4.054493 | 3.319216 | 8.574658 |
| 8 | 24 | 2.520020 | 0.759131 | 7.574885 |
| 16 | 24 | 1.046997 | 0.759102 | 2.993418 |
| 24 | 24 | 0.776988 | 0.000148 | 3.680837 |
| 36 | 24 | 0.000150 | 0.000093 | 0.000642 |

이 수치는 합성 게임의 임의 효용 단위다. Pokémon 승률·실력·속도 수치가 아니다. 예산36은 이 작은 트리를 완전히 확장하며, 모든 seed에서 기준값 및 root BR gap 허용오차0.02를 통과했다. 예산 증가에 따른 개별 정책 품질의 단조 개선은 주장하지 않는다.

`local_leaf_matrix_gap`은 후보가 현재 만든 임시 행렬의 편차 gap이다. 위 표는 그와 다른 `reference_root_br_gap`이다. 별도 회귀검사는 local gap이 거의0인데도 reference gap이3을 넘는 잘못된 shallow 선택을 재현한다. 또한 동시행동 matching pennies에서 50/50 혼합 전략을 확인하고, 12×12 지배 행렬에서는 DO가144셀 대신23셀만 평가하면서 전체 행렬의 BR gap0임을 확인했다. 이것도 실제 엔진 속도 향상의 증거로 사용하지 않는다.

## 한계와 다음 평가

- PBS·세계별 CFR·학습 가치망·정보집합 공유는 미구현이다. 완전정보 adapter가 공개되지 않은 commitment를 읽을 수 있어 실전 불완전정보 AI로 사용할 수 있다는 주장은 하지 않는다.
- 비용 예산은 엔진 호출 단위 proxy다. 1회 열거와 RM+ 호출 중간을 선점하지 않는다. 실제 비용 기반 우선순위 및 wall deadline은 후속이다.
- tree의 각 outcome 상태를 저장하며 state-merged DAG/TT, 다른 턴의 session 재사용, worker 병렬화는 없다. 큰 Full 분포의 peak memory 검증이 필요하다.
- `FrontierExhausted`와 RM+ tolerance 충족은 별개다. CLI smoke의 현재 local gap0.0180469는 설정0.01을 넘어서 `local_matrix_tolerance_met=false`로 기록된다.
- 생산 채택과 GitHub 성능 실행은 하지 않았다. S25b에서 동일 horizon·평가기·정보·시간 조건의 기존 탐색과 비교해야 한다. 기존 solver의 빠른 승리 depth bonus도 이 실험의 고정 ±WIN과 맞춰야 한다.

## 기록

- 최종 로그/영수증: `validation/r4-*`, 합성 원자료 `validation/quality-grid-r4.json`.
- 최초 compile에서 테스트가 private helper를 호출한 오류를 수정했다. r1은 큰 팀 fixture의 모든 행동을 Full로 펼쳐 bounded logic 범위를 벗어나 수동 중단했다. r2는 좁은 파생 fixture가 빈 행동 목록을 만들어 실패했고, 실존 party에 한해 지원되는 Harden을 배치하여 수정했다. 당시 로그는 보존했다. 이 실패들을 성공 표본에 합산하지 않는다.
- 기준 소스와 파일 해시는 `baseline-manifest.json`; 최종 변경분과 검증 증거 해시는 `completion-receipt.json`에 기록한다.
- 방법과 한계의 근거는 [심층 논문 조사](SEARCH-LITERATURE.md)에 연결했다.
