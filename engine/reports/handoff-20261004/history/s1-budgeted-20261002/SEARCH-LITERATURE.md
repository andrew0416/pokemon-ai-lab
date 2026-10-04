# 선택적 탐색 참고 논문 — 2026-10-02

사용자 요청으로 서브에이전트가 10편의 논문 원문과 일부 저자 코드를 조사했다. 아래는 우리 코드에 적용하기 위한 검토이며, 논문의 구현을 재현하거나 Pokai보다 우수함을 입증한 결과가 아니다. 코드와 실행 증거는 이 실험의 README와 validation을 기준으로 한다.

## 우선순위

1. **Student of Games**: 트리 확장과 전략 풀이를 분리한다. PUCT와 평균 정책을 섞어 확장할 위치를 선택한다. 현재 CPU 프로토타입에서 구조를 시험할 수 있지만, 논문의 PBS CFR와 학습 가치망을 구현한 것은 아니다.
2. **Small Nash Equilibrium Certificates**: 미계산 부분의 실제 하한·상한을 보존하여 오차와 중단 판단을 검증하는 방향. 휴리스틱 점수는 유효한 bound가 아니다.
3. **Toward Optimal Policy Population Growth / ADO**: 중간 결과를 별도 검증해야 한다. 일반 Double Oracle은 확장마다 원게임 exploitability가 단조 감소하지 않을 수 있다. ADO의 보장은 고정 게임과 정확한 하위 solve/BR 조건에 의존한다.

## 논문 비교

| 논문·원문 | 가져올 부분 | 적용 조건·한계 |
|---|---|---|
| [Student of Games: A unified learning algorithm for both perfect and imperfect information games](https://arxiv.org/html/2112.03178v2) — 2023, 초판 2021 | GT-CFR의 성장 트리, 확장/후회 갱신 교대, PUCT+평균 정책 경로 | public tree와 frontier 값의 조건이 필요하다. 노드를 계속 제거하는 beam이나 개별 상태 RM+에 정리가 자동 적용되지 않는다. 전략 갱신 자체의 비용도 세야 한다. |
| [Small Nash Equilibrium Certificates in Very Large Games](https://arxiv.org/html/2006.16387v2) — NeurIPS 2020 | 미확장 부분을 하한·상한으로 남기는 pseudogame, 인증 가능한 gap | 작은 certificate가 항상 존재하지 않는다. 효용 bound 및 근사 solver의 오차가 필요하며, 느슨한 bound와 관리비용 때문에 느려질 수 있다. |
| [Toward Optimal Policy Population Growth in Two-Player Zero-Sum Games](https://proceedings.iclr.cc/paper_files/paper/2024/file/a6ab94e907d24814e3a3ceda39a2ec64-Paper-Conference.pdf) — ICLR 2024 | 한쪽만 제한한 게임을 푸는 ADO, 중간 혼합 정책의 평가 | 정책 모집단 PSRO와 지금의 행동 단위 DO를 구분한다. payoff가 깊은 탐색으로 변하는 상황에는 정확 ADO의 단조 보장을 적용할 수 없다. [저자 코드](https://github.com/indylab/sp-psro) |
| [Analysis of Hannan Consistent Selection for Monte Carlo Tree Search in Simultaneous Move Games](https://arxiv.org/html/1804.09045v3) — 초판 2018, 확인본 2019 | joint action 평균화와 충분한 탐색의 중요성 | 각 노드의 no-regret 선택만으로 전체 동시행동 트리의 수렴이 보장되지는 않는다. 정확 행렬 풀이를 표본 업데이트로 바꿀 때 특히 필요하다. |
| [Selecting Computations: Theory and Applications](https://arxiv.org/html/1207.5879) — UAI 2012 | 계산 선택을 최종 결정 개선과 계산비용의 문제로 다룸 | 상호의존 셀·공유 캐시에 단순 표본 모델을 그대로 쓸 수 없다. 한 번의 계산만 보는 중단 규칙은 너무 일찍 멈출 수 있다. |
| [Monte Carlo *-Minimax Search](https://www.ijcai.org/Proceedings/13/Papers/093.pdf) — IJCAI 2013 | chance 잔여질량과 효용 경계, Star1/Star2 계열의 계산 생략 | 순차행동 alpha-beta를 동시행동 행렬에 그대로 적용하지 않는다. 샘플링 오차와 관리 오버헤드가 있으며, 우선 exact chance를 유지하는 interval 실험이 적합하다. |
| [Combining Deep Reinforcement Learning and Search for Imperfect-Information Games](https://arxiv.org/html/2007.13544v2) — ReBeL, NeurIPS 2020 | PBS에 조건부인 정보상태별 가치, 탐색과 belief 갱신 | scalar evaluator만 교체해서 재현할 수 없다. 정책망은 선택사항이지만 PBS 가치와 정책 일관성이 필요하다. [공식 코드](https://github.com/facebookresearch/rebel)는 Liar’s Dice를 제공한다. |
| [Monte Carlo Continual Resolving for Online Strategy Computation in Imperfect Information Games](https://arxiv.org/html/1812.07351v2) — AAMAS 2019 | 관측 뒤 재검색과 이전 전략의 일관성, outcome-sampling MCCFR | 상대 counterfactual value가 필요하다. regret 재사용의 실용 변형과 이론 보장을 구분한다. 낮은 exploitability와 특정 상대에 대한 승률은 별개다. |
| [Depth-Limited Solving for Imperfect-Information Games](https://arxiv.org/html/1805.08195) — NeurIPS 2018 | leaf에서 여러 continuation 전략을 제시하는 중간 경로 | 같은 information set에서 정책 선택을 공유해야 한다. 세계별 최적 정책 선택은 strategy fusion을 일으킨다. 작은 정책 포트폴리오는 근사다. |
| [Finding and Certifying (Near-)Optimal Strategies in Black-Box Extensive-Form Games](https://arxiv.org/html/2009.07384v3) — AAAI 2021 | black-box 게임의 증명 가능한 gap, regret minimizer와 confidence sequence | 우리는 exact chance와 임의 상태 접근이 있어 일부 제약을 피한다. 우선 2020 certificate 방식이 더 단순한 출발점이다. |

## 두 연구 갈래

- **PBS/CFR/continual resolving**은 숨은 정보 아래에서 전략과 재검색의 일관성을 다룬다. public history, range, information set, counterfactual value가 주요 계약이다. 같은 정보집합에서 실제 숨은 세계에 따라 다른 전략을 선택하면 안 된다.
- **Certificate/chance bound**는 계산하지 않은 부분을 남겼을 때의 오차와 중단을 다룬다. 선택 순서에 휴리스틱을 써도 되지만, 인증에는 실제 값을 포함하는 경계가 필요하다.

현재 프로토타입은 완전관측으로 제한하여 첫 갈래의 미구현을 명시한다. 두 번째 갈래의 인증도 아직 구현하지 않았다. [ReBeL](https://arxiv.org/html/2007.13544v2), [Small Certificates](https://arxiv.org/html/2006.16387v2)

## 구현에 반영한 평가 구분

1. `local_leaf_matrix_gap`: 현재 leaf 추정값으로 만든 행렬을 RM+가 얼마나 잘 풀었는지. 휴리스틱이 틀려도 0일 수 있다.
2. `reference_root_br_gap`: 동일 horizon/evaluator로 독립 완전탐색한 공통 root 행렬에 후보의 두 혼합전략을 대입한 최대 일방 편차의 합. 전체 continuation 정책의 원게임 exploitability가 아니다.
3. `reference_value_error`: 후보의 추정 root 값과 위 기준값의 차이. 정책 자체의 손실과 동일하지 않다.
4. 전이 호출·가중 비용·RM+ 풀이 횟수 및 반복 수를 분리한다. 확장 횟수만으로 CPU 시간을 대신하지 않는다.

## 향후 인증 계약

실제 payoff가 L ≤ A ≤ U이면, 후보 혼합 x,y의 해당 행렬에 대한 편차 gap은 `max_i(Uy)_i - min_j(x^T L)_j`로 제한할 수 있다. chance의 이미 계산한 가중합을 S, 잔여 질량을 q, 유효한 잔여값 경계를 [l,u]라 하면 기대값은 [S+ql,S+qu]에 있다. solver·수치 오차를 포함하고, 미계산 확률질량을 제거한 뒤 재정규화해서 인증하지 않는다. 이는 [certificate 논문](https://arxiv.org/html/2006.16387v2)을 현재 행렬 구조에 적용하기 위한 계약이며, 현재 코드가 이 인증을 구현했다는 뜻은 아니다.

진행 순서: 비용/품질 지표 분리 → 확장/갱신 예산 조절 → 지지도 밖 대응의 탐색 검증 → 유효한 bound를 제공할 수 있는 경우의 인증 실험. PBS 통합과 학습 CFV·self-play는 별도 장기 작업이다.
