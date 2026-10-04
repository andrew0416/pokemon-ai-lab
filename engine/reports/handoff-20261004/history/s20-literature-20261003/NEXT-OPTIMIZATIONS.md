# 다음 최적화 논문 조사 — 2026-10-03

1차 문헌10편을 현재 실험 코드와 대조했다. 다음 구현 순서는 **갱신 규칙 비교 → 이전 전략 재인증 → 결정 구조 압축**을 권한다. 본 조사는 구현·벤치마크가 아니며 실제 단축률은 아직 없다.

기준은 S19 `7f4ca4397e098d7877ba4f25b79e1164d21a080a`이다. 후보3 보류, 다중 턴 S17, 조건부 S18 병렬, cadence4라는 기존 판단을 바꾸지 않는다. 연구 자료와 원자 노트는 로컬 전용이다.

## 코드에서 확인한 출발점

- 현재는 교대 LCFR이다. signed regret과 평균전략에 선형 가중을 쓰며 DCFR/PCFR+가 아니다.
- cadence4에서도 다중 턴은5~7회의 풀이가 남는다. 매번 uniform/zero regret에서 시작한다.
- 최종32회 반복인 표본이 많다. 기본 check_every=32이므로 같은 간격만 비교하면 1~31회에서의 수렴 차이가 가려질 수 있다.
- sand의 고정 게임은8087노드/2정보집합/64전이/8013말단이다. 이는 오래된 추측이 아니라 S13 정적 census이며 S19는 완성 게임 일치가 확인된 누적 실험이다. 노드 수는 CPU 시간 비율이 아니다.
- chance-only value 캐시, 재사용 workspace, 소유권 전이, 증분 변환, bounded 병렬, 확장 묶음은 이미 존재한다. 이를 새 최적화로 다시 세지 않는다.

## 우선순위와 검증 단위

| 순위 | 작업 | 상태 | 논문 |
|---:|---|---|---|
| 1 | S26f-regret-rules: CFR 지역 갱신 규칙 교체 | proposed | DCFR-2019, PCFR-2021, SAPCFR-2026, HS-2026 |
| 2 | S26f-policy-reuse-gate: 이전 전략의 새 트리 인증 후 풀이 생략 | proposed | WARM-2016 |
| 3 | S26f-warm-start: 전략 기반 대체 후회 초기화 | proposed | WARM-2016 |
| 4 | S26f-sequence-kernel: 행동열·결정 구조 기반 CPU 풀이 커널 | proposed | SEQUENCE-1996, LITEEFG-2024, GPUCFR-2024 |
| 5 | S26f-lazy-cfr: 정보집합별 지연 갱신 | reserve-proposal | LAZY-2018 |
| 6 | S26f-regret-pruning: 복귀 시점이 정해진 후회 기반 가지치기 | reserve-proposal | RBP-2015, DCFR-2019 |
| 7 | S26f-gpu-cfr: 배치·대형 고정 게임의 GPU CFR | deferred-proposal | GPUCFR-2024 |

### S26f-regret-rules — CFR 지역 갱신 규칙 교체

현 코드는 signed regret에 t/T를 곱하는 교대 LCFR이다. DCFR, PCFR+, SAPCFR+, HS는 아직 구현되어 있지 않다.

고정된 완성 트리에서 LCFR 대 DCFR(1.5,0,2), 이어 PCFR+/SAPCFR+, 마지막 HS15/HS30을 독립 비교한다. tree walk·평가기는 공통으로 유지하고 지역 상태/갱신/평균 규칙을 모듈로 분리한다.

**채택 판단:** 같은 원본 Tree::assess gap 허용치에 도달하는 총시간, 노드방문 수, 반복 수, 비수렴 수를 비교한다. 그 후 S19 전체 성장 시간으로 재평가한다.

**주의할 계약:** 최종 정책 비트 일치를 요구하면 서로 다른 균형점도 버리게 된다. 게임·관측·평가·허용치는 고정한다. HS의 반복 horizon 및 교대/동시 갱신을 구분한다.

근거: [DCFR-2019](https://arxiv.org/abs/1809.04040), [PCFR-2021](https://ojs.aaai.org/index.php/AAAI/article/view/16676), [HS-2026](https://ojs.aaai.org/index.php/AAAI/article/view/38784), [SAPCFR-2026](https://ojs.aaai.org/index.php/AAAI/article/view/38766).

### S26f-policy-reuse-gate — 이전 전략의 새 트리 인증 후 풀이 생략

S19 cadence4에서도 다중 턴 탐색은 5~7번 다시 풀고, 매 풀이를 uniform/zero regret으로 시작한다.

이전 평균정책을 정보집합 의미키와 action label로 새 트리에 사상한다. 새 정보집합은 명시한 기본정책으로 채우고 새 트리에서 원래의 전체 BR 인증을 수행한다. 통과하면 CFR 없이 반환하고 실패하면 기존 초기화를 사용한다.

**채택 판단:** 인증 성공 빈도와 절약한 풀이 시간에서 추가 인증·매핑 비용을 뺀 전체 시간. 부분 예산/실패 시 마지막으로 풀이된 snapshot만 반환한다.

**주의할 계약:** 논문의 warm start 그 자체가 아니라 보수적인 별도 제안이다. 기존 정보집합의 정수 ID는 바뀔 수 있다. 부분 게임 인증을 미확장 게임의 인증으로 해석하지 않는다.

근거: [WARM-2016](https://ojs.aaai.org/index.php/AAAI/article/view/10056).

### S26f-warm-start — 전략 기반 대체 후회 초기화

이전 전략이 새 트리에서 허용 gap을 못 넘더라도 uniform보다 유용한 초기점일 가능성이 있다.

변경된 게임과 utility scale에서 논문 정리2의 대체 counterfactual values/후회 및 가상 반복 가중을 다시 구성한다. 먼저 논문의 CFR 설정을 재현하고 LCFR/DCFR 결합은 따로 검토한다.

**채택 판단:** 초기화 비용을 포함한 같은 gap까지의 시간. 나쁜 seed, 완전히 바뀐 leaf 평가, scale 감소, 신규 정보집합에 대한 독립 LP/BR 검증.

**주의할 계약:** 단순 regret 복사, 지역 행동확률의 무가중 평균, 새 행동의 임의 regret=0 삽입을 논문 보장으로 포장하지 않는다. 모든 스냅샷에서 이득이 난다는 근거는 없다.

근거: [WARM-2016](https://ojs.aaai.org/index.php/AAAI/article/view/10056).

### S26f-sequence-kernel — 행동열·결정 구조 기반 CPU 풀이 커널

sand는 8087노드/2정보집합/64공동행동이다. CachedValues는 이미 chance-only 가치를 캐시하지만 reach와 최종 BR은 물리 트리를 순회한다.

먼저 한 세계·양측 각각 한 정보집합·후속 결정 없음이 확인된 트리만 기존 행렬 표현으로 내린다. 일반형은 양측 행동열과 realization 제약을 유지하는 희소 CPU 커널로 분리한다. 원본 트리는 관측·성장·최종 인증에 보존한다.

**채택 판단:** 압축/행렬 준비를 포함한 시간과 RSS를 측정한다. 최종 인증은 압축 전 원본에서 수행한다. 정보집합 공유, zero prior, 숨은 약속, switch closure는 자동 fallback 조건을 갖춘다.

**주의할 계약:** 8087/64는 가속 배수가 아니다. 기존 Bayesian 행렬 solver는 공통 우리 행동×상대 world 구조로 제한되어 임의의 다중 턴 게임을 바로 넣을 수 없다. 합산순서/scale/양의 확률 underflow 오류 계약도 보존해야 한다.

근거: [SEQUENCE-1996](https://ai.stanford.edu/~koller/Papers/Koller%2Bal%3AGEB96.pdf), [LITEEFG-2024](https://arxiv.org/abs/2407.20351), [GPUCFR-2024](https://arxiv.org/abs/2408.14778).

### S26f-lazy-cfr — 정보집합별 지연 갱신

더 큰 다중 턴 트리에서는 낮은 도달질량 정보집합의 반복 갱신을 줄일 여지가 있다.

누적 counterfactual reach와 지연 구간의 후회/평균 전략 기여를 관리한다. 중간 갱신만 미루고 전체 원본 BR로 인증한다.

**채택 판단:** 정보집합 갱신 감소량뿐 아니라 같은 gap까지의 총시간. 현재 짧은32회 풀이와 정보집합2개 sand는 낮은 우선순위다.

**주의할 계약:** 누적 기여를 버리면 알고리즘이 달라진다. 논문의 균형적인 트리 가정을 현재 트리가 만족한다는 주장은 하지 않는다.

근거: [LAZY-2018](https://arxiv.org/abs/1810.04433).

### S26f-regret-pruning — 복귀 시점이 정해진 후회 기반 가지치기

장시간 풀이에서 확실히 불리한 행동 아래의 반복 순회를 줄일 수 있다.

채택할 후회 갱신 규칙을 먼저 고정하고 보상범위·상대 도달질량으로 안전한 skip 기간을 유도한다. 만료·후회 복원·평균전략·독립 BR을 검증한다.

**채택 판단:** 실제로 건너뛴 방문 수와 재검사 비용, 동일 gap까지의 시간. 고정32회 풀이보다 큰 게임에서 먼저 계측한다.

**주의할 계약:** LCFR/DCFR/PCFR+에 vanilla RBP 경계를 그대로 적용하지 않는다. 작은 chance 결과의 영구 삭제나 휴리스틱 빔 축소는 이 후보가 아니다.

근거: [DCFR-2019](https://arxiv.org/abs/1809.04040), [RBP-2015](https://proceedings.neurips.cc/paper/2015/hash/c54e7837e0cd0ced286cb5995327d1ab-Abstract.html).

### S26f-gpu-cfr — 배치·대형 고정 게임의 GPU CFR

많은 독립 고정 게임 또는 더 큰 트리를 오래 풀 때 행렬 연산을 장치에 상주시킬 수 있다.

CPU 희소 커널이 준비된 뒤 GPU가 있는 별도 러너에서 준비/전송/동기화/최종 인증을 포함해 비교한다.

**채택 판단:** 전체시간 및 메모리와 CPU fallback 검증. GPU 없는 장치와 AVX2 없는 CPU도 기존 경로로 실행 가능해야 한다.

**주의할 계약:** 현재6표본만으로 착수 우선순위를 높일 근거는 부족하다. 일반 GitHub CPU 러너에서 CUDA 속도를 검증했다고 하지 않는다.

근거: [GPUCFR-2024](https://arxiv.org/abs/2408.14778).

## 첫 실험의 비교 설계

1. **고정 게임부터 비교한다.** 기존6개 엔진 국면의 완성 트리를 한 번 생성하여 모든 solver에 동일하게 전달한다. 별도로 숨은 세계·정보집합 공유·강제 교체·불균형 보상·드문 양의 chance·scale 감소가 있는 새 생성 문제를 준비한다. 기존 LP49+96과24개 문제의168인증을 새 데이터라고 세지 않는다.
2. **동일한 오차까지의 시간을 잰다.** 현재 허용치0.1과 더 엄격한0.01/0.001을 분리한다. 모두 같은 check schedule(예:1,2,4,8,16,32,64…)과 최대 반복 예산을 쓴다. 첫 인증 시점만 관측하므로 그 사이 더 일찍 수렴했는지는 주장하지 않는다. 스케줄 변경 자체의 이득은 기존 LCFR도 같은 스케줄로 돌려 분리한다.
3. **완성 결과의 품질 기준은 원본 정보제약 BR의 gap이다.** 독립 LP oracle로 하한/상한과 실현확률을 점검한다. 여러 균형점이 있을 수 있으므로 알고리즘 교체에 정책 비트 동일성을 요구하지 않는다. 게임·합법행동·정보집합·평가·허용치가 바뀌어서는 안 된다.
4. **시간에 모든 비용을 넣는다.** 초기화, compile/compress, 원본 인증, warm-start 실패 비용, 비수렴 및 RSS를 함께 기록한다. 고정 게임 풀이와 전체 성장 탐색을 별도 측정하고 둘 다 GitHub의 동일 러너에서 짝 비교한다. 기존 fixture 설정을 사후 유리하게 바꾸지 않는다.
5. **조합은 독립 효과 확인 뒤 한다.** 첫 DCFR 실험은 warm start/압축 없이, 첫 압축 실험은 기존 LCFR로 한다. 변경 후 선택 궤적이 달라지는 growing은 완료 horizon과 미완료 예산을 분리하고 미풀이 snapshot을 내보내지 않는다.
6. **범용 CPU 경로를 유지한다.** 기본 OFF·원래 scalar/serial fallback. GPU/AVX2는 전제하지 않는다. 승률 비교는 이번 목표에 없다.

## 1차 문헌과 확인 범위

### DCFR-2019

[Solving Imperfect-Information Games via Discounted Regret Minimization](https://arxiv.org/abs/1809.04040) — Noam Brown, Tuomas Sandholm; AAAI / arXiv v3, 2019.
수집일 2026-10-03; 게시일 연도만 확인; 확인한 판본 날짜 2019-02-21.

양·음 누적 후회와 평균정책을 서로 다른 비율로 할인하여 초기 반복의 영향을 줄인다.

논문 권장 DCFR(1.5,0,2)을 첫 기준으로 삼는다. RM+처럼 음수 후회를 무조건 0으로 만드는 방법과 다르며, pruning에 적합한 설정도 별도다.

확인 범위: 초록, 할인 갱신식, DCFR 변형 비교와 pruning 관련 절. [본문](https://arxiv.org/pdf/1809.04040).

### PCFR-2021

[Faster Game Solving via Predictive Blackwell Approachability: Connecting Regret Matching and Mirror Descent](https://ojs.aaai.org/index.php/AAAI/article/view/16676) — Gabriele Farina, Christian Kroer, Tuomas Sandholm; AAAI, 2021.
수집일 2026-10-03; 게시일 2021-05-18; 확인한 판본 날짜 별도 특정하지 않음.

다음 손실/후회 방향의 예측을 지역 regret minimizer에 반영한다.

논문에서도 모든 포커 표본에서 우월하지 않았다. 현재 signed LCFR에 직전 regret을 한 번 더 더하는 것만으로 PCFR+ 구현이라고 부를 수 없다.

확인 범위: 공식 서지, 초록, predictive RM/RM+ 정의와 이론·실험 설명. [본문](https://ojs.aaai.org/index.php/AAAI/article/view/16676/16483).

### HS-2026

[Faster Game Solving via Hyperparameter Schedules](https://ojs.aaai.org/index.php/AAAI/article/view/38784) — Naifeng Zhang, Stephen Marcus McAleer, Tuomas Sandholm; AAAI, 2026.
수집일 2026-10-03; 게시일 2026-03-14; 확인한 판본 날짜 별도 특정하지 않음.

학습 없이 반복 진행도에 따라 DCFR/PCFR+ 할인 강도를 조정한다.

식4는 사전에 정한 총 반복수 n을 사용한다. 정리의 동시 갱신 전제를 현재 교대 갱신에 그대로 옮기지 않는다. 매 스냅샷의 짧은 풀이에서도 빠를지는 미측정이다.

확인 범위: 공식 서지, 3.2 식4, 3.3 정리 전제, 실험·ablation. [본문](https://ojs.aaai.org/index.php/AAAI/article/view/38784/42746).

### SAPCFR-2026

[Faster Game Solving via Asymmetry of Step Sizes](https://ojs.aaai.org/index.php/AAAI/article/view/38766) — Linjian Meng, Tianpei Yang, Youzhi Zhang, Zhenxing Ge, Yang Gao; AAAI, 2026.
수집일 2026-10-03; 게시일 2026-03-14; 확인한 판본 날짜 별도 특정하지 않음.

예측과 실제 누적 후회 갱신의 보폭을 다르게 하여 잘못된 예측의 영향을 줄인다.

SAPCFR+는 예측 항에 1/3을 쓰는 고정 비대칭 변형이다. PCFR+가 먼저 필요하며 모든 시험 게임에서 DCFR보다 우세한 것은 아니다.

확인 범위: 공식 서지, 4.2 SAPCFR+ 갱신식, 실험 설정 및 DCFR 비교. [본문](https://ojs.aaai.org/index.php/AAAI/article/view/38766/42728).

### WARM-2016

[Strategy-Based Warm Starting for Regret Minimization in Games](https://ojs.aaai.org/index.php/AAAI/article/view/10056) — Noam Brown, Tuomas Sandholm; AAAI, 2016.
수집일 2026-10-03; 게시일 연도만 확인; 확인한 판본 날짜 별도 특정하지 않음.

주어진 양측 전략으로부터 조건을 만족하는 대체 후회를 구성하여 CFR을 초기화한다.

가상 반복수와 대체 counterfactual value의 조건을 만족해야 한다. 다른 게임의 regret 복사와 다르다. 현재 LCFR/DCFR에의 확장은 별도 정당화가 필요하다.

확인 범위: 공식 서지, 저자 수정본 초록, 정리2와 warm-start 반복수 선택 절. [본문](https://www.cs.cmu.edu/~sandholm/warmStart.aaai16.withAppendixAndTypoFix.pdf).

### SEQUENCE-1996

[Efficient Computation of Equilibria for Extensive Two-Person Games](https://ai.stanford.edu/~koller/Papers/Koller%2Bal%3AGEB96.pdf) — Daphne Koller, Nimrod Megiddo, Bernhard von Stengel; Games and Economic Behavior, 1996.
수집일 2026-10-03; 게시일 연도만 확인; 확인한 판본 날짜 별도 특정하지 않음.

완전 회상 게임에서 행동열의 실현확률과 희소 보상행렬로 전략을 표현한다. 동일한 양측 행동열에 해당하는 말단 보상은 chance 가중으로 합산할 수 있다.

상태만 같은 이력을 병합하는 근거가 아니다. 자기 행동 이력과 정보집합 제약을 유지해야 한다. 수학적 동등성과 부동소수점 비트 동등성은 다르다.

확인 범위: 저자 공개본, sequence form의 realization 제약과 보상행렬 구성 절. [본문](https://ai.stanford.edu/~koller/Papers/Koller%2Bal%3AGEB96.pdf).

### LITEEFG-2024

[LiteEFG: An Efficient Python Library for Solving Extensive-form Games](https://arxiv.org/abs/2407.20351) — Mingyang Liu, Gabriele Farina, Asuman Ozdaglar; arXiv preprint v1, 2024.
수집일 2026-10-03; 게시일 2024-07-29; 확인한 판본 날짜 2024-07-29.

정보집합의 지역 갱신식을 계산 그래프로 정의하고 정적 계산과 반복 계산을 분리한다.

기존 S10 CachedValues가 정적 가치 계산을 이미 분리한다. 라이브러리 교체나 논문의 비교 배수를 현재 Rust 코드의 예상 가속률로 제시하지 않는다.

확인 범위: 초록, computation graph, static/dynamic forward/backward, benchmark 설명. [본문](https://arxiv.org/html/2407.20351v1).

### LAZY-2018

[Lazy-CFR: fast and near-optimal regret minimization for extensive games with imperfect information](https://arxiv.org/abs/1810.04433) — Yichi Zhou, Tongzheng Ren, Dong Yan, Jialian Li, Jun Zhu; arXiv preprint v3, 2018.
수집일 2026-10-03; 게시일 2018-10-10; 확인한 판본 날짜 2018-12-25.

정보집합별 누적 도달질량에 따라 갱신 시점을 다르게 하여 매 반복 전체 갱신을 피한다.

O(sqrt(|I|)) 형태의 단순 비용 결과는 정보집합 트리 구조·이력 수에 관한 가정을 사용한다. 불규칙 교체/연속 의사결정이 있는 포켓몬 트리에 그대로 보장할 수 없다.

확인 범위: 초록, lazy update 설명, Assumptions 1/2 및 Algorithm 1 복잡도 절. [본문](https://arxiv.org/pdf/1810.04433).

### RBP-2015

[Regret-Based Pruning in Extensive-Form Games](https://proceedings.neurips.cc/paper/2015/hash/c54e7837e0cd0ced286cb5995327d1ab-Abstract.html) — Noam Brown, Tuomas Sandholm; NIPS, 2015.
수집일 2026-10-03; 게시일 연도만 확인; 확인한 판본 날짜 별도 특정하지 않음.

음수 후회가 다시 양수가 될 수 있는 가장 이른 시점을 경계로 특정 행동 이하의 순회를 잠시 미룬다.

음수이면 영구 삭제하거나 낮은 chance 확률을 잘라내는 방법이 아니다. 할인/선형 가중을 쓰면 원래의 스킵 경계를 재유도해야 한다.

확인 범위: 공식 초록, 4절 RBP 및 재방문/overhead 설명. [본문](https://proceedings.neurips.cc/paper/2015/file/c54e7837e0cd0ced286cb5995327d1ab-Paper.pdf).

### GPUCFR-2024

[GPU-Accelerated Counterfactual Regret Minimization](https://arxiv.org/abs/2408.14778) — Juho Kim; arXiv preprint (reviewed HTML v3), 2024.
수집일 2026-10-03; 게시일 2024-08-27; 확인한 판본 날짜 별도 특정하지 않음.

CFR 순회를 희소·밀집 행렬/벡터 연산으로 재구성해 GPU 병렬 처리가 가능하게 한다.

논문 자체에서도 작은 게임은 GPU/Python 비용 때문에 불리하다. 현재 수천 노드·짧은 반복에 이득을 보장할 수 없으며 GPU 준비/전송/메모리 비용까지 측정해야 한다.

확인 범위: 초록, 행렬·벡터 구성, 4.1 작은/중간/큰 게임 및 메모리 비교. [본문](https://arxiv.org/html/2408.14778v3).

## 코드와 측정 출처

- [cached.rs:62](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian/tree/workspace/cached.rs:62>) — 새 풀이마다 uniform/zero regret 초기화; SHA-256 `0c227887a67eed2f529a0fc2aa4a42774f3e05983f8e024cea2dfde5a0a4d787`.
- [cached.rs:69](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian/tree/workspace/cached.rs:69>) — 교대 signed linear CFR; 매 반복 전체 information 순회; SHA-256 `0c227887a67eed2f529a0fc2aa4a42774f3e05983f8e024cea2dfde5a0a4d787`.
- [cached.rs:12](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian/tree/workspace/cached.rs:12>) — 정책 독립 chance-only value 캐시가 이미 존재; SHA-256 `0c227887a67eed2f529a0fc2aa4a42774f3e05983f8e024cea2dfde5a0a4d787`.
- [workspace.rs:20](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian/tree/workspace.rs:20>) — reach는 물리 노드·간선을 전체 순회; SHA-256 `0dfa89d2ccc8479eec42392223210a2f7ab7925b3757006b8fce24981edd26a1`.
- [tree.rs:285](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian/tree.rs:285>) — 원본 전체 트리의 정보제약 best response 기반 인증; SHA-256 `4afbb52891ff2ec19075a854ff32c33b9dc80e340a17ca5ca04170b61ed4a3f5`.
- [cadence.rs:229](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian/tree/builder/growing/reuse/cadence.rs:229>) — 새 게임 컴파일 후 새 풀이; 기존 regret 전달 없음; SHA-256 `9d5c42fa52f6f1f52fe0eba089f60573895f8f337b73b613b3dee75bff3fdc11`.
- [bayesian.rs:249](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian.rs:249>) — 양의 regret으로 정책을 정하되 누적 음수 자체는 보존; SHA-256 `e188f10d17e3fe421fe363b018648ae70c1306833d0d6d77c7633f6ab8d7af42`.
- [bayesian.rs:31](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/source/engine/search/src/bayesian.rs:31>) — 기존 별도 Bayesian 행렬 계층; SHA-256 `e188f10d17e3fe421fe363b018648ae70c1306833d0d6d77c7633f6ab8d7af42`.
- [ADDITIONAL-EVIDENCE.json](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/ADDITIONAL-EVIDENCE.json>) — SHA-256 `80139a055f0133b0e857e194a380c4abebed5263d2db7ceb913f1522b6361ad7`.
- [COMPLETION.json](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s19-growth-cadence-20261003/COMPLETION.json>) — SHA-256 `9739fd77ba91ae48c99ad735ef025fada54ae8b274da6fd1eb5b92f1a4d6cd14`.
- [findings.json](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s13-performance-candidates-20261003/findings.json>) — SHA-256 `08b238d7e8c57eca2d6112b328e1614d7bad1c9b4b657af298c5bdcd92104d46`.

이번 조사에서 엔진 코드 변경·엔진 실행·시간 측정·GitHub dispatch는0회다. 문헌의 성능 배수를 현재 엔진의 예상 개선율로 옮기지 않았다. 모든 후보는 미구현 상태로 기록한다.
