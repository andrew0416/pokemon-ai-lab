# S26a 결과

2026-10-02 · source `90901a6bd430a560b240379d985ba3b1c8e2df09` ·
parent S25d `0501eeb7220bcc3f5e2f00145c970fa350666031`.
Branch `codex/s26a-bayesian-cfr-20261002`.
[GitHub run 37008543602](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37008543602):
success, 77초. 이 시간은 CI 전체 소요시간이며 엔진 성능 수치가 아니다.

## 구현

하나의 우리 전략과 세계별 상대 전략을 함께 계산하는 Bayesian 행렬 게임 라이브러리를 추가했다.
상대는 자기 세계를 알고, 우리 설정도 안다고 가정한다. 상대 행동 목록은 세계마다 달라도 된다.
Regret와 평균 전략 모두 선형 가중하며 우리 쪽, 상대 쪽 순서로 갱신하는 alternating linear CFR이다.
기존 RM+를 세계별로 풀어서 평균 내는 방식이 아니다.

고정 행렬에 대한 최선 응수 하한·상한과 gap, 반복 횟수, 수렴 여부를 별도로 반환한다.
세계 ID별 관측 가능도에 따른 Bayesian posterior 갱신은 불변 입력에서 새 게임을 만들며,
모든 후보가 반박된 경우나 양수 확률의 underflow를 오류로 처리한다.

`lab-bayes` CLI에는 직접 행렬 입력과 실제 엔진의 한 턴 입력이 있다. 엔진 연결은 Full 난수,
모든 합법 타깃, 교체 가능한 기존 Evaluator를 사용한다. 우리 측 상태와 명시하지 않은 정보는
세계 간 동일해야 한다. 숨은 비HP 능력치 및 첫 턴의 미공개 벤치 후보를 명시적으로 허용할 수 있다.
중간 교체·기절 후 교체가 나오면 전체 계산을 오류로 반환한다. 숨은 세계를 미리 안 상태에서
그 후속 선택을 최적화해 정보가 새는 전략을 만들지 않는다.

## 검증

| 항목 | 결과 |
|---|---|
| Rust 새 기능 + 기존 선택 탐색 | 53 passed |
| CLI 단위 검사 | 1 passed |
| 기능 기본 OFF 기존 검사 | 17 passed |
| JSON/CLI 없는 라이브러리 빌드 | passed |
| clippy `-D warnings` | passed |
| 서로 다른 생성 게임의 전개형 normal-form RM+ 대조 | 24 cases passed |
| 별도 SciPy/HiGHS Bayesian LP 기준값 대조 | 72/72 passed |
| LP 대조 수렴 | 72/72, 최대 gap 0.01968748357698491 ≤ 요청 0.02 |
| 원시 CLI 확률·기대값·세계별 값·상하한·gap 재검산 | 72/72 passed |
| 실제 엔진 CLI, 상대 능력치 후보 2개 | 공통 16개 행동, 각 상대 16개 행동, gap 0.05927568162955765 ≤ 요청 0.1 |

Rust의 24개 사례는 상대의 모든 세계별 조건부 행동 조합을 펼친 작은 normal form을 기존
RM+ 솔버로 풀었다. 별도 72개 사례는 행 수 2–5, 세계 수 1–6, 세계별 열 수 1–5와 다양한
payoff·prior·posterior·0 prior를 사용하고 독립 LP로 확인했다. 동일 입력 반복 일치도가 아니다.
숨은 세계에 따라 우리 전략을 바꿔 부당하게 값을 높이는 반례와, 세계별 상대 전략을 하나로
평균내서 위험을 놓치는 반례가 별도 테스트로 고정되어 있다.

전체 artifact의 SHA-256과 소스 커밋을 확인했다. 검산 결과는 `AUDIT-37008543602.json`,
원시 자료는 `publication/run-37008543602/`, 로컬 로그는 `validation/r1/`,
실제 엔진 CLI 요청·후보·응답은 `validation/cli-engine/`에 보존했다.

## 적용 상태와 다음 경계

`experiment-bayesian-cfr`는 기본 OFF이고 기존 생산 엔진/R1/S25 선택적 탐색에 자동 연결하지 않았다.
기존 4,099개 파일은 변경하지 않았으며 기존 수정은 Cargo의 opt-in 항목과 모듈 export 2곳이다.
공유 Git checkout의 HEAD, 상태, 인덱스 SHA-256은 보존했다. AVX2 없이 generic x86-64에서 검증했다.

이번 범위는 고정 Bayesian 의사결정 게임과 한 턴 엔진 연결이다. 다중 턴 PBS 관측 이력,
전략에 의해 유도되는 트리 내부 posterior, 숨은 예약 행동을 포함한 pivot 정보집합,
그 믿음에 맞춘 counterfactual leaf value, GT-CFR 확장과 신경망 학습은 아직 없다.
고정 행렬의 작은 gap을 전체 포켓몬 게임의 exploitability 보장으로 해석하면 안 된다.
속도·실전 승률·PokaiTrainer 우열은 이번 검사에서 측정하지 않았다.

API/입력 계약: `source/engine/benchmarks/s26_bayesian/CONTRACT.md`.
원문: [PokaiTrainer v2 §4.1–4.2](https://arxiv.org/html/2608.29197v2#S4).
