# 실험 결과

후보4: 기존 비말단 노드는 재사용하고 새 노드·변경 말단만 변환한다. 외부 ID 정렬·DFS·완전 회상·도달 가능성 검증은 매번 유지한다. 기준은 S14 all-owned이며 후보3은 OFF다. publication receipt의 correctness_scope 문구는 이전 템플릿이 남았으므로 실제 범위는 SOURCE-PARITY와 이 보고서를 따른다.

Source `d32fd5ae016e6fc23465b6e63de209919469e7ed`. [GitHub 실행 37088113085](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37088113085).
AMD EPYC 7763 64-Core Processor; generic x86-64. 기본 OFF, 생산 미적용. 승률은 평가하지 않았다.

| 국면 | 후보 | 기준 ms | 후보 ms | 시간 감소 | 시간비 95% 구간 |
|---|---|---:|---:|---:|---|
| stats-w1-d2-incremental-growing | incremental | 97.500 | 93.495 | +4.11% | 0.9566–0.9615 |
| stats-w2-d2-incremental-growing | incremental | 180.284 | 172.566 | +4.27% | 0.9523–0.9665 |
| pivot-w1-d1-incremental-growing | incremental | 24.497 | 24.933 | -1.34% | 1.0033–1.0340 |
| sand-damage-w1-d1-incremental-growing | incremental | 316.537 | 315.087 | +0.29% | 0.9887–1.0029 |
| stats-w4-d2-incremental-growing | incremental | 360.291 | 343.051 | +4.56% | 0.9513–0.9590 |
| narrow-w1-d3-incremental-growing | incremental | 67.765 | 61.284 | +9.41% | 0.9009–0.9074 |

다중 턴 성장의 4개 비교에서 4.11~9.41% 감소와 동률을 제외한 구간을 확인했다. 다중 턴 유지 후보로 누적한다. 1턴 교체는 1.34% 회귀하고 모래1턴은 동률 구간이므로 다음 단계에서는 1턴을 원래 컴파일 경로로 유지한다.

54개 짝 블록, 216개 시간 표본, 378회 계산. 표의 감소율은 블록별 시간비 중앙값을 사용하므로 별도 중앙값 ms의 비와 다를 수 있다.
구간은 한 러너 내 변동이며 기기 간 재현성을 나타내지 않는다. 실제 전투 6국면 계열에 대한 제한된 실험이다.
검사 통과: {'core': 101, 'owned': 6, 'on': 90, 'off': 17, 'cli': 1}. 기존 독립 LP 전체49/성장96건을 이 CI에서 재검증했다. 새145문제를 생성한 것은 아니다.
원시 표본·bootstrap·소스 SHA·아티팩트 해시를 재검산했다. 기본 OFF/범용 CPU 빌드와 순차 대체 경로를 유지한다.
