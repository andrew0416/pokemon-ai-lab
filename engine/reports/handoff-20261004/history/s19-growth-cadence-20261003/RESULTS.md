# 실험 결과

후보6: 하나의 풀이된 스냅샷에서 최대1/2/4개 공개 그룹을 선택해 묶음 확장한 뒤 CFR을 초기화하여 한 번 푼다. 대조군은 고정된 S18 구성(교체순차/나머지4스레드)이다. cadence1은 same-work, cadence2/4는 전체 canonical game·semantic policy·최종 인증이 같은 same-finite-game 비교다.

Source `7f4ca4397e098d7877ba4f25b79e1164d21a080a`. [GitHub 실행 37089670577](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37089670577).
AMD EPYC 9V45 96-Core Processor; generic x86-64. 기본 OFF, 생산 미적용. 승률은 평가하지 않았다.

| 국면 | 후보 | 기준 ms | 후보 ms | 시간 감소 | 시간비 95% 구간 |
|---|---|---:|---:|---:|---|
| stats-w1-d2-cadence-growing | cadence-1 | 44.597 | 44.242 | +0.28% | 0.9879–1.0084 |
| stats-w1-d2-cadence-growing | cadence-2 | 43.697 | 40.261 | +6.96% | 0.8981–0.9383 |
| stats-w1-d2-cadence-growing | cadence-4 | 44.123 | 38.763 | +12.04% | 0.8688–0.8847 |
| stats-w2-d2-cadence-growing | cadence-1 | 81.224 | 82.332 | -1.60% | 1.0028–1.0489 |
| stats-w2-d2-cadence-growing | cadence-2 | 83.511 | 78.677 | +6.03% | 0.9210–0.9735 |
| stats-w2-d2-cadence-growing | cadence-4 | 82.755 | 74.479 | +9.10% | 0.8829–0.9225 |
| pivot-w1-d1-cadence-growing | cadence-1 | 13.629 | 13.653 | +0.29% | 0.9540–1.0127 |
| pivot-w1-d1-cadence-growing | cadence-2 | 13.416 | 13.373 | +0.33% | 0.9712–1.0027 |
| pivot-w1-d1-cadence-growing | cadence-4 | 13.459 | 13.459 | +0.59% | 0.9833–1.0080 |
| sand-damage-w1-d1-cadence-growing | cadence-1 | 166.456 | 168.604 | -1.29% | 0.9979–1.0294 |
| sand-damage-w1-d1-cadence-growing | cadence-2 | 168.469 | 169.222 | +0.16% | 0.9838–1.0139 |
| sand-damage-w1-d1-cadence-growing | cadence-4 | 167.217 | 167.581 | +0.60% | 0.9795–1.0143 |
| stats-w4-d2-cadence-growing | cadence-1 | 162.959 | 163.039 | -0.63% | 0.9697–1.0205 |
| stats-w4-d2-cadence-growing | cadence-2 | 171.425 | 152.806 | +10.57% | 0.8683–0.9118 |
| stats-w4-d2-cadence-growing | cadence-4 | 165.152 | 142.209 | +13.89% | 0.8519–0.8899 |
| narrow-w1-d3-cadence-growing | cadence-1 | 30.303 | 29.898 | -0.53% | 0.9739–1.0288 |
| narrow-w1-d3-cadence-growing | cadence-2 | 30.345 | 26.721 | +12.33% | 0.8590–0.8837 |
| narrow-w1-d3-cadence-growing | cadence-4 | 29.623 | 23.744 | +19.05% | 0.7904–0.8131 |

cadence4를 다중턴 성능 유지 후보로 보관한다. 다중턴4국면에서 추가9.10~19.05% 감소했으며 최종 게임·정책·오차 인증이 일치했다. 1턴두국면은 실제 중간 풀이 수 감소가 없으므로 약0.59~0.60%를 알고리즘 이득으로 보지 않는다. 부분 예산에서 선택궤적과 반환 부분트리는 달라질 수 있어 생산 기본값은 OFF다. 기존49+96 LP 외에 기존24생성문제의168개 cadence 인증, 첫 그룹 완성 후 둘째 그룹 실패의 rollback을 검증했다. 승률·미확장 전체게임의 gap 향상은 주장하지 않는다.

162개 짝 블록, 648개 시간 표본, 2052회 계산. 표의 감소율은 블록별 시간비 중앙값을 사용하므로 별도 중앙값 ms의 비와 다를 수 있다.
구간은 한 러너 내 변동이며 기기 간 재현성을 나타내지 않는다. 실제 전투 6국면 계열에 대한 제한된 실험이다.
검사 통과: {'core': 101, 'owned': 7, 'on': 95, 'off': 17, 'cli': 1}. 기존 독립 LP 전체49/성장96건을 이 CI에서 재검증했다. 새145문제를 생성한 것은 아니다.
원시 표본·bootstrap·소스 SHA·아티팩트 해시를 재검산했다. 기본 OFF/범용 CPU 빌드와 순차 대체 경로를 유지한다.
