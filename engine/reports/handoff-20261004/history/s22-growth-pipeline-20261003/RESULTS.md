# S26g 네 단계 순차 실험 결과

Source `70d5b0b463dc2cab2deb365c67ffcb2e2e7f95b8` · [GitHub Actions](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37123954718)

1→2→3→4 순서로 구현하고 각 단계의 로컬 정확성 검사를 통과한 다음 단계로 진행했다. 속도는 마지막에 같은 GitHub 러너·같은 범용 x86-64 release 실행파일에서 독립 옵션으로 비교했다. 원본 엔진·기존 탐색 경로는 보존했고 모든 새 옵션은 기본 OFF다.

기존 1100건 + 새 652건, 합계 **1752건 독립 LP/전수 BR 인증 검사**가 통과했다. 새 표본은 고정 게임 29개·성장 게임 16개에 여러 옵션·예산을 적용한 652건이다. 새 검사 중 9건은 반복 상한에서 미수렴이며, 반환한 인증 수치는 전수 BR/LP와 일치했다. 독립 게임 652개로 해석하지 않는다.

속도 조건 30개, 비교 36개 중 36개가 공통 gap을 충족했다. 각 비교는 9개 ABBA/BAAB 블록이며, 시간 단축률은 블록별 후보/기준 비율의 중앙값이다. 양수는 빨라짐, 음수는 느려짐이다. 두 시간 중앙값의 단순 비율과는 다를 수 있다.

| 사례 | 1 인덱스 | 2 저장소 재사용 | 3 보상 캐시 추가 | 4 내부 검사 | 모두 ON |
|---|---:|---:|---:|---:|---:|
| stats-w1-d2 | +0.80% | +0.56% | -0.44% | -0.27% | +1.12% |
| stats-w2-d2 | +0.57% | +0.96% | +0.43% | -1.42% | +0.09% |
| pivot-w1-d1 | +0.18% | -0.22% | -0.03% | +0.11% | +0.21% |
| sand-damage-w1-d1 | -0.35% | +0.58% | -0.21% | +0.63% | -0.33% |
| stats-w4-d2 | +0.20% | +0.40% | -1.38% | +0.10% | +0.09% |
| narrow-w1-d3 | +0.41% | -0.04% | +0.38% | +1.05% | +0.44% |

**3번의 기준만 2번 ON 상태다.** 나머지 열은 S21 LCFR+sequence 기준이다. 1턴 국면에서는 1번 선택 루프 및 2/3번 재사용이 실질적으로 작동할 후속 스냅샷이 없으므로 그 차이를 개선 근거로 쓰지 않는다. 각 수치를 합산할 수 없다.

## 전체 측정 및 구간

| 사례 | 옵션 | 기준 ms | 후보 ms | 단축률 | 후보/기준 95% 구간 | 최종 반복 |
|---|---|---:|---:|---:|---|---:|
| stats-w1-d2-growing-1-index | frontier-index | 75.607 | 74.196 | +0.80% | 0.9720–1.0098 | 32 |
| stats-w1-d2-growing-1-index | disabled-control | 74.538 | 73.121 | +0.06% | 0.9166–1.0191 | 32 |
| stats-w1-d2-growing-2-owned | owned-compiler | 74.410 | 72.598 | +0.56% | 0.9601–1.0009 | 32 |
| stats-w1-d2-growing-3-delta | delta-kernel | 73.227 | 73.622 | -0.44% | 0.9329–1.0349 | 32 |
| stats-w1-d2-growing-4-checks | compressed-checks | 74.054 | 74.350 | -0.27% | 0.9880–1.0297 | 32 |
| stats-w1-d2-growing-all | all-four | 72.897 | 73.226 | +1.12% | 0.9703–1.0052 | 32 |
| stats-w2-d2-growing-1-index | frontier-index | 141.670 | 141.123 | +0.57% | 0.9825–1.0123 | 32 |
| stats-w2-d2-growing-1-index | disabled-control | 142.726 | 141.193 | +0.67% | 0.9727–1.0091 | 32 |
| stats-w2-d2-growing-2-owned | owned-compiler | 143.224 | 140.742 | +0.96% | 0.9733–1.0072 | 32 |
| stats-w2-d2-growing-3-delta | delta-kernel | 142.429 | 141.117 | +0.43% | 0.9553–1.0481 | 32 |
| stats-w2-d2-growing-4-checks | compressed-checks | 139.369 | 141.667 | -1.42% | 0.9953–1.0327 | 32 |
| stats-w2-d2-growing-all | all-four | 140.520 | 140.388 | +0.09% | 0.9670–1.0191 | 32 |
| pivot-w1-d1-growing-1-index | frontier-index | 23.365 | 23.293 | +0.18% | 0.9947–0.9989 | 64 |
| pivot-w1-d1-growing-1-index | disabled-control | 23.355 | 23.322 | +0.18% | 0.9923–0.9992 | 64 |
| pivot-w1-d1-growing-2-owned | owned-compiler | 23.267 | 23.424 | -0.22% | 0.9998–1.0242 | 64 |
| pivot-w1-d1-growing-3-delta | delta-kernel | 24.503 | 24.521 | -0.03% | 0.9956–1.0031 | 64 |
| pivot-w1-d1-growing-4-checks | compressed-checks | 24.482 | 24.433 | +0.11% | 0.9959–1.0011 | 64 |
| pivot-w1-d1-growing-all | all-four | 24.408 | 24.356 | +0.21% | 0.9956–1.0005 | 64 |
| sand-damage-w1-d1-growing-1-index | frontier-index | 264.888 | 269.031 | -0.35% | 0.9994–1.0378 | 256 |
| sand-damage-w1-d1-growing-1-index | disabled-control | 268.769 | 267.755 | -0.59% | 0.9806–1.0182 | 256 |
| sand-damage-w1-d1-growing-2-owned | owned-compiler | 269.726 | 268.173 | +0.58% | 0.9865–1.0191 | 256 |
| sand-damage-w1-d1-growing-3-delta | delta-kernel | 271.402 | 271.982 | -0.21% | 0.9913–1.0086 | 256 |
| sand-damage-w1-d1-growing-4-checks | compressed-checks | 285.515 | 283.234 | +0.63% | 0.9783–1.0164 | 256 |
| sand-damage-w1-d1-growing-all | all-four | 267.967 | 269.189 | -0.33% | 0.9881–1.0139 | 256 |
| stats-w4-d2-growing-1-index | frontier-index | 280.125 | 279.881 | +0.20% | 0.9915–1.0139 | 32 |
| stats-w4-d2-growing-1-index | disabled-control | 279.473 | 280.350 | -0.12% | 0.9753–1.0187 | 32 |
| stats-w4-d2-growing-2-owned | owned-compiler | 282.900 | 281.351 | +0.40% | 0.9818–1.0223 | 32 |
| stats-w4-d2-growing-3-delta | delta-kernel | 276.264 | 279.198 | -1.38% | 0.9996–1.0279 | 32 |
| stats-w4-d2-growing-4-checks | compressed-checks | 279.337 | 280.008 | +0.10% | 0.9748–1.0228 | 32 |
| stats-w4-d2-growing-all | all-four | 279.766 | 278.259 | +0.09% | 0.9773–1.0235 | 32 |
| narrow-w1-d3-growing-1-index | frontier-index | 45.439 | 45.560 | +0.41% | 0.9894–1.0032 | 32 |
| narrow-w1-d3-growing-1-index | disabled-control | 45.406 | 45.522 | -0.25% | 0.9852–1.0172 | 32 |
| narrow-w1-d3-growing-2-owned | owned-compiler | 46.031 | 45.173 | -0.04% | 0.9809–1.0024 | 32 |
| narrow-w1-d3-growing-3-delta | delta-kernel | 45.567 | 45.303 | +0.38% | 0.9770–1.0060 | 32 |
| narrow-w1-d3-growing-4-checks | compressed-checks | 46.224 | 46.092 | +1.05% | 0.9712–1.0188 | 32 |
| narrow-w1-d3-growing-all | all-four | 46.074 | 45.787 | +0.44% | 0.9700–1.0228 | 32 |

## 계약과 제한

1/2번은 난수 소비·prior 호출·선택 방문·raw tree·정책·반복수·작업 카운터까지 일치시킨다. 3번은 같은 행동열/정보 제약을 유지하지만 합산 순서 때문에 부동소수점 차이가 생길 수 있다. 4번은 중간 검사 필터이며 종료 후보와 마지막 반복은 반드시 원본 Tree 인증을 거친다. 미수렴을 성공으로 표시하지 않는다.

확장 예산 실패는 컴파일 트리를 넘겨주기 전에 처리한다. 실패한 미공개 builder를 버리고 마지막 해결된 스냅샷을 반환한다. 컴파일·solver 오류는 오류를 반환하며 잘못된 새 정책을 게시하지 않는다.

3번은 변경 말단과 새 하위 트리만 재방문한다. 레이아웃/순서/entry 벡터 구성은 남아 있으며, scale이 변하면 저장한 원시 말단 기여에서 모든 계수를 재합산한다. 이전 regret의 재사용은 하지 않는다.

[구현 명세](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s22-growth-pipeline-20261003/source/engine/benchmarks/s26_pipeline/README.md>) · [기계 조회 결과](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s22-growth-pipeline-20261003/RESULTS.json>)

작은 차이는 다른 러너 세션에서 재현해야 한다. AVX2는 요구하지 않으며 기존 scalar 경로로 사용 가능하다. 승률은 평가하지 않았다.

## 판정과 구조적 작업량

**네 가지 모두 기본 OFF 유지. 이번 표본에서는 추가 전체 속도 향상을 입증하지 못했다.** 전체 ON 단축률은 −0.33%~+1.12%이고, 여섯 사례 모두 paired 95% 구간에 1이 포함된다. 1~3번이 실제로 작동한 네 개 다중 스냅샷 사례에서도 각 단독 변경의 구간에 1이 포함된다.

OFF 제어군에서도 한 사례에 작은 구간상 차이가 나타났다. 또한 frontier-index의 구간상 우세 한 건은 walks=0인 pivot이므로 색인 알고리즘의 이득으로 볼 수 없다. 작은 부호와 단일 구간만으로 채택하지 않는다.

| 사례 | solve 수 | 보상 구조 전체 재구성→증분 | 원본 인증 호출 | CFR 반복 합계 |
|---|---:|---|---|---:|
| stats-w1-d2 | 5 | 5회 전체 → 1회 전체 + 4회 증분 | 7→5 | 224→224 |
| stats-w2-d2 | 5 | 5회 전체 → 1회 전체 + 4회 증분 | 7→5 | 224→224 |
| pivot-w1-d1 | 1 | 후속 스냅샷 없어 캐시 생략 | 2→1 | 64→64 |
| sand-damage-w1-d1 | 1 | 후속 스냅샷 없어 캐시 생략 | 8→1 | 256→256 |
| stats-w4-d2 | 5 | 5회 전체 → 1회 전체 + 4회 증분 | 7→5 | 224→224 |
| narrow-w1-d3 | 7 | 7회 전체 → 1회 전체 + 6회 증분 | 7→7 | 224→224 |

따라서 코드가 작동하지 않아 생긴 결과는 아니다. 측정한 전체 실행에서 절약한 부분이 작거나 새 캐시/색인/압축 필터의 관리 비용으로 상쇄된 것으로 해석할 수 있다. 이번 실험은 구간별 CPU 비용을 직접 분리 계측하지 않았으므로 어느 원인이 지배적인지는 확정하지 않는다.

재평가하려면 먼저 transition 생성·compile·kernel 준비·CFR·인증·선택의 비용을 러너에서 분리하고, 재사용 횟수가 더 많은 국면에서 효과를 확인한다. 현재 표본의 잡음 수준 변화만 반복 측정해 채택을 강행하지 않는다. 이 후속 계측은 제안이며 실행하지 않았다.
