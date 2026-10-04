# 실험 결과

후보5: 불변 엔진의 전이를 최대4개 작업으로 묶고 원래 순서로 반영한다. 기준은 다중턴 S17, 1턴 S14. 같은 바이너리에서 1/2/4개 스레드 설정을 비교했다. 기본 evaluator/일반 Domain 호출은 순차로 유지한다.

Source `19061c2c08b3d5c25f2a696eb12b2b2c8b943435`. [GitHub 실행 37088881275](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37088881275).
AMD EPYC 7763 64-Core Processor; generic x86-64. 기본 OFF, 생산 미적용. 승률은 평가하지 않았다.

| 국면 | 후보 | 기준 ms | 후보 ms | 시간 감소 | 시간비 95% 구간 |
|---|---|---:|---:|---:|---|
| stats-w1-d2-parallel-growing | parallel-1 | 89.074 | 88.953 | +0.07% | 0.9935–1.0062 |
| stats-w1-d2-parallel-growing | parallel-2 | 89.385 | 108.659 | -21.70% | 1.2132–1.2283 |
| stats-w1-d2-parallel-growing | parallel-4 | 89.014 | 84.197 | +5.41% | 0.9363–1.0290 |
| stats-w2-d2-parallel-growing | parallel-1 | 167.976 | 168.878 | -0.53% | 1.0009–1.0100 |
| stats-w2-d2-parallel-growing | parallel-2 | 168.826 | 208.775 | -22.48% | 1.2140–1.2385 |
| stats-w2-d2-parallel-growing | parallel-4 | 169.114 | 159.109 | +6.16% | 0.9244–0.9522 |
| pivot-w1-d1-parallel-growing | parallel-1 | 25.172 | 24.947 | +0.28% | 0.9834–0.9980 |
| pivot-w1-d1-parallel-growing | parallel-2 | 25.001 | 38.038 | -51.76% | 1.3489–1.5922 |
| pivot-w1-d1-parallel-growing | parallel-4 | 24.967 | 39.624 | -57.20% | 1.5643–1.5986 |
| sand-damage-w1-d1-parallel-growing | parallel-1 | 318.266 | 318.454 | -0.17% | 0.9981–1.0074 |
| sand-damage-w1-d1-parallel-growing | parallel-2 | 317.809 | 284.876 | +10.45% | 0.8938–0.9051 |
| sand-damage-w1-d1-parallel-growing | parallel-4 | 317.573 | 276.281 | +13.00% | 0.8570–0.8819 |
| stats-w4-d2-parallel-growing | parallel-1 | 337.585 | 338.146 | -0.27% | 0.9995–1.0050 |
| stats-w4-d2-parallel-growing | parallel-2 | 337.723 | 410.419 | -21.53% | 1.2021–1.2315 |
| stats-w4-d2-parallel-growing | parallel-4 | 337.869 | 319.938 | +5.28% | 0.9403–0.9524 |
| narrow-w1-d3-parallel-growing | parallel-1 | 58.979 | 58.563 | +0.31% | 0.9864–0.9979 |
| narrow-w1-d3-parallel-growing | parallel-2 | 59.105 | 69.141 | -17.28% | 1.1663–1.1839 |
| narrow-w1-d3-parallel-growing | parallel-4 | 59.069 | 57.350 | +3.22% | 0.9636–0.9767 |

조건부 4스레드 옵션으로 보관한다. 5국면에서 3.22~13.00% 감소했지만 교체는57.20% 회귀했다. 2스레드는 모래를 제외하고17.28~51.76% 회귀했다. 자동 기본값으로 적용하지 않는다. 다음 실험의 동일 국면 비교 설정은 교체순차/나머지4스레드로 명시한다. 4논리CPU는2물리코어인 러너였으며 최대 자식 RSS는253312KiB(약247.38MiB); 이는 후보 단독 메모리나 일반적인 엔진 메모리 상한은 아니다. 추가 LP는 일반 Domain 순차 회귀검사이며 병렬 경로는 실제 엔진 전체 트리·정책 및 상태기록 evaluator 대조로 검증했다.

162개 짝 블록, 648개 시간 표본, 1134회 계산. 표의 감소율은 블록별 시간비 중앙값을 사용하므로 별도 중앙값 ms의 비와 다를 수 있다.
구간은 한 러너 내 변동이며 기기 간 재현성을 나타내지 않는다. 실제 전투 6국면 계열에 대한 제한된 실험이다.
검사 통과: {'core': 101, 'owned': 7, 'on': 92, 'off': 17, 'cli': 1}. 기존 독립 LP 전체49/성장96건을 이 CI에서 재검증했다. 새145문제를 생성한 것은 아니다.
원시 표본·bootstrap·소스 SHA·아티팩트 해시를 재검산했다. 기본 OFF/범용 CPU 빌드와 순차 대체 경로를 유지한다.
