# S25a: 비용 예산 기반 선택적 탐색

상태: **실험 구현 및 범위를 한정한 로직 검증 완료**. 속도·대전 실력 향상은 미측정. 기존 생산 코드와 KEEP-R1은 변경하지 않았다. [검증 결과](RESULTS.md) · [논문 조사](SEARCH-LITERATURE.md).

기준 소스는 P7h 실험의 `c54becc9fb7bb9101cac363f389d4a84792016ab` 사본이다. `baseline-manifest.json`에 4,083개 파일의 SHA256을 기록했다. 이전 feature들은 기존 기본값을 유지한다. 새 코드는 search의 기본 OFF `experiment-budgeted-search`와 별도 `lab-budget` 실행파일에만 있다. AVX2를 요구하지 않으며 검증은 generic x86-64로 수행한다.

## 범위와 원리

1. 현재 노드의 동시 행동을 RM+ 행렬 게임으로 푼다. Double Oracle은 제한 지지도 밖의 전체 행동에 대한 best response를 검사하고 필요한 셀만 요청한다.
2. 행동쌍의 전이는 한 번 열거하여 보관한다. 그 이후의 backup은 이미 계산한 전이를 재사용한다.
3. 현재 전략과 PUCT 우선순위를 반씩 섞어 다음 확장 위치를 선택한다. 사전확률은 독립 `Prior`이며 기본은 균등이다. 신경망이 필수는 아니다.
4. 선택한 leaf를 확장하고 영향을 받은 조상 행렬을 다시 푼다. payoff가 바뀌므로 Double Oracle의 지지도 탐색도 다시 한다. 완결된 root만 외부에 공개한다.
5. 예산이 도중에 소진되면 직전 완결 root 전략을 반환한다. 최초 root도 완성하지 못하면 전략을 만들었다고 표시하지 않는다.
6. 턴과 교체 전이는 각각 비용을 받는다. 전이 호출 전에 비용을 차감하여 허용 예산을 넘기지 않는다. 교체는 정상 턴 깊이를 소비하지 않으며, 마지막 턴의 중간교체·기절 교체도 평가 전에 푼다.
7. 확률질량은 제거하거나 재정규화하지 않는다. 유효하지 않은 확률·평가·prior, 미지원 엔진 동작은 오류다. 확률이 0인 결과만 저장하지 않는다.

이는 **완전정보·고정 평가기·제한 horizon에서의 실험**이다. Pokai의 public-belief-state CFR, 학습 가치망, 세계별 counterfactual value는 구현하지 않았다. suspended turn에는 실제 양쪽 commitment가 들어 있으므로 실전 비공개 정보 안전성이 있다고 주장하지 않는다. 기존 `Evaluator`는 그대로 교체할 수 있다.

## 예산과 안전한 반환

- 예산은 전이 호출의 가중 비용이다. 1회 열거 내부의 분기 수와 RM+ 반복 비용은 별도로 달라지므로 wall time 상한이나 속도 지표가 아니다.
- 최초 root는 모든 필요한 best response 계산을 마쳐야 전략이 생긴다. 예산이 작으면 `policy: null`이다.
- 마지막 실패한 업데이트의 작업량도 통계에 포함되지만, 반환 전략은 마지막 완결 backup의 snapshot이다.
- `FrontierExhausted`는 설정한 horizon 안의 확장을 마쳤다는 뜻이다. RM+의 허용오차 충족은 `local_matrix_tolerance_met`로 따로 표시한다. 현재 CLI smoke의 기본 2,000회 제한에서는 gap 0.01805가 tolerance 0.01보다 커서 이 값이 false다.
- `max_nodes`는 저장 트리의 제한이다. 엔진 호출이 반환하기 전에 발생하는 일시 메모리 사용을 제한하지는 못한다.
- deterministic seed의 단일 스레드 실험이다. 아직 worker 병렬화나 public-state 정보집합, transposition DAG, 다른 턴에 대한 세션 재사용은 없다.
- 종료 효용은 ±WIN/0이며 빠른 승리에 별도 depth bonus를 주지 않는다. 기존 solver와 정책을 비교할 때 이 평가 계약을 맞춰야 한다.

## 실행

소스의 `engine`에서:

```text
cargo run --locked -p lab-search --features experiment-budgeted-search --bin lab-budget -- oracle/scenarios/eject-button-uturn.json --position 0 --depth 2 --budget 1000 --rolls full --eval heuristic
```

초기 국면이 여럿이면 `--position N`을 명시해야 한다. `--side p2`, `--dense`, `--all-targets`, `--seed`, `--turn-cost`, `--switch-cost`, `--max-nodes`, `--iterations`, `--tolerance`, `--exploration`을 지원한다. setup 국면 준비는 탐색 예산 밖이며 출력에 이를 표시한다.

`--rolls full`이 기본이다. Median 등 축약 모드는 출력에 표시하며 정확 난수 열거라고 해석하지 않는다. 오류나 root 미완성 시 CLI는 실패 종료코드를 낸다. `--eval`은 기존 material/heuristic/file 가중치를 사용한다.

## 검증 계획

로컬은 opt0 bounded logic 검사만 수행한다. 예산 경계, incumbent 보존, chance 질량, 강제교체, 노드 제한, 입력 복원, 오류, 결정적 seed, 혼합 전략, DO의 전체 행동 best response를 확인한다. 작은 합성 게임과 실제 엔진 fixture를 독립 all-action 재귀와 대조한다. 별도의 기존 search 회귀와 feature OFF 빌드를 수행한다.

최종 r4 검증: ON library 34건(신규17+기존17), OFF 기존17건, CLI4조건, clippy warnings deny 및 CLI 의존성을 제외한 library 빌드 통과. 합성 8게임×5예산×3seed=120조건의 quality grid를 기록했다. 실제 fixture는 eject-button-uturn Full/Median과 ko-replace의 파생 좁은 상태다. 후자는 setup과 KO·교체·날씨 등장 상태를 유지하고, 실존 party의 다음 기술만 Harden 하나로 제한했다. 전체 원본 fixture의 모든 후속 기술 조합을 검증한 것으로 세지 않는다.

`local_leaf_matrix_gap`은 현재 임시 행렬의 내부 품질이다. 이 수치를 원게임 exploitability 또는 선택적 탐색의 판단 품질로 보고하지 않는다. 합성 사례는 공통 완전탐색 기준 행렬에서 `reference_root_br_gap`과 `reference_value_error`를 따로 기록한다. 자세한 이론 근거와 후속 검증 조건은 [논문 조사](SEARCH-LITERATURE.md)에 있다.

실제 Pokémon의 동일 시간 예산·판단 품질·메모리 비교는 보드 S25b 후속이다. 이 실험에 GitHub Actions를 자동 dispatch하지 않는다.
