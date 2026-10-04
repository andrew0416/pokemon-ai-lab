# S26j 증분 이력 키 최적화 실험

[GitHub 실행](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37131081510) · `2bb9cf595f675d269406bbe783e88154ed517cb4` · 507초

부모의 private-history 문자열을 이어 써서 과거 관측을 다시 Debug 포맷하지 않는 저장 방식이다. 새 행동·관측만 같은 Memory Debug 구현으로 포맷한다. 각 플레이어의 정보, public history 순서, 콜백 호출, 탐색·인증 알고리즘은 그대로다. `pipeline.encoded_history`는 기본 OFF다.

121개 계측/중간 상태 테스트, 일반 117개·기본 17개 통과. 새 seed와 escaped label을 쓰는 관측 게임 32개에서 769개 전체 OFF/ON JSON 쌍을 비교했다. 독립 BR/LP 인증 1,869건 중 17건은 cap 미수렴이며 인증 수치만 일치했다. 횟수는 여러 설정/부분 탐색을 포함하며 독립 게임 1,869개가 아니다. 엔진 40구성의 전체 결과·작업량 지문이 각 빌드와 일치했다.

## 비계측 속도

각 행은 같은 바이너리·같은 설정에서 이력 옵션만 바꾼 9개 ABBA/BAAB 블록이다. 양수는 시간 단축. 추가 적용의 기준은 기존 4개 옵션 전체 ON이며, 두 열의 단축률을 더하면 안 된다.

| 조건 | 단독 적용 | 비율 95% 구간 | 기존 전체 ON에 추가 | 비율 95% 구간 |
|---|---:|---|---:|---|
| stats-w1-d2 | +8.21% | 0.8888–0.9233 | +11.33% | 0.8607–0.8963 |
| stats-w2-d2 | +10.03% | 0.8787–0.9132 | +10.64% | 0.8751–0.9036 |
| pivot-w1-d1 | +14.42% | 0.8542–0.8596 | +14.24% | 0.8547–0.8581 |
| sand-damage-w1-d1 | +8.70% | 0.9069–0.9290 | +9.70% | 0.8997–0.9110 |
| stats-w4-d2 | +10.97% | 0.8658–0.8943 | +10.42% | 0.8599–0.9289 |
| narrow-w1-d3 | +12.43% | 0.8647–0.8847 | +13.31% | 0.8446–0.8959 |
| stats-w4-d2-cadence1 | +10.69% | 0.8845–0.9150 | +11.18% | 0.8868–0.9099 |
| narrow-w1-d3-cadence1 | +11.52% | 0.8759–0.8894 | +11.41% | 0.8496–0.8968 |

## A/A와 모든 비교

| 조건 | 후보 | 기준 ms | 후보 ms | 단축률 | 비율 구간 | A/A 범위 밖 |
|---|---|---:|---:|---:|---|---|
| stats-w1-d2 | new-off-aa | 54.580 | 54.284 | +0.76% | 0.9822–1.0224 | False |
| stats-w1-d2 | history | 54.559 | 49.229 | +8.21% | 0.8888–0.9233 | True |
| stats-w1-d2 | all-four-aa | 54.224 | 54.258 | +0.37% | 0.9596–1.0225 | False |
| stats-w1-d2 | all-history | 55.275 | 48.637 | +11.33% | 0.8607–0.8963 | True |
| stats-w2-d2 | new-off-aa | 104.544 | 103.983 | +0.07% | 0.9692–1.0258 | False |
| stats-w2-d2 | history | 104.669 | 93.133 | +10.03% | 0.8787–0.9132 | True |
| stats-w2-d2 | all-four-aa | 105.479 | 102.472 | +1.56% | 0.9706–1.0097 | False |
| stats-w2-d2 | all-history | 103.328 | 92.106 | +10.64% | 0.8751–0.9036 | True |
| pivot-w1-d1 | new-off-aa | 18.029 | 18.049 | -0.14% | 0.9984–1.0031 | False |
| pivot-w1-d1 | history | 18.018 | 15.422 | +14.42% | 0.8542–0.8596 | True |
| pivot-w1-d1 | all-four-aa | 18.066 | 18.081 | -0.02% | 0.9972–1.0021 | False |
| pivot-w1-d1 | all-history | 18.091 | 15.494 | +14.24% | 0.8547–0.8581 | True |
| sand-damage-w1-d1 | new-off-aa | 218.883 | 219.805 | -0.38% | 0.9918–1.0132 | False |
| sand-damage-w1-d1 | history | 218.771 | 199.097 | +8.70% | 0.9069–0.9290 | True |
| sand-damage-w1-d1 | all-four-aa | 209.052 | 209.045 | +0.13% | 0.9805–1.0041 | False |
| sand-damage-w1-d1 | all-history | 210.156 | 190.204 | +9.70% | 0.8997–0.9110 | True |
| stats-w4-d2 | new-off-aa | 204.314 | 206.645 | -0.22% | 0.9921–1.0161 | False |
| stats-w4-d2 | history | 208.475 | 184.153 | +10.97% | 0.8658–0.8943 | True |
| stats-w4-d2 | all-four-aa | 207.375 | 207.271 | -0.64% | 0.9895–1.0274 | False |
| stats-w4-d2 | all-history | 204.859 | 182.269 | +10.42% | 0.8599–0.9289 | True |
| narrow-w1-d3 | new-off-aa | 34.435 | 34.409 | -0.69% | 0.9817–1.0174 | False |
| narrow-w1-d3 | history | 34.233 | 29.804 | +12.43% | 0.8647–0.8847 | True |
| narrow-w1-d3 | all-four-aa | 33.762 | 33.449 | -0.30% | 0.9797–1.0159 | False |
| narrow-w1-d3 | all-history | 33.913 | 29.398 | +13.31% | 0.8446–0.8959 | True |
| stats-w4-d2-cadence1 | new-off-aa | 218.448 | 219.753 | -1.11% | 0.9883–1.0201 | False |
| stats-w4-d2-cadence1 | history | 220.996 | 196.945 | +10.69% | 0.8845–0.9150 | True |
| stats-w4-d2-cadence1 | all-four-aa | 217.512 | 215.514 | -0.07% | 0.9824–1.0077 | False |
| stats-w4-d2-cadence1 | all-history | 214.347 | 192.544 | +11.18% | 0.8868–0.9099 | True |
| narrow-w1-d3-cadence1 | new-off-aa | 40.210 | 40.027 | +0.20% | 0.9924–1.0100 | False |
| narrow-w1-d3-cadence1 | history | 40.241 | 35.927 | +11.52% | 0.8759–0.8894 | True |
| narrow-w1-d3-cadence1 | all-four-aa | 39.050 | 38.824 | +1.17% | 0.9731–1.0172 | False |
| narrow-w1-d3-cadence1 | all-history | 39.130 | 34.873 | +11.41% | 0.8496–0.8968 | True |

## 메모리

각 구성 별도 프로세스 3회 중앙값. 요청 heap은 baseline 위 peak, RSS는 입력/풀/allocator를 포함하는 프로세스 peak다.

| 조건 | 구성 | 할당 수 | 누적 요청 MiB | heap peak MiB | 반환물 MiB | RSS MiB |
|---|---|---:|---:|---:|---:|---:|
| stats-w1-d2 | new-off | 261,092 | 323.03 | 24.67 | 5.84 | 30.73 |
| stats-w1-d2 | history | 243,334 | 321.57 | 24.67 | 5.84 | 30.72 |
| stats-w1-d2 | all-four | 254,526 | 322.51 | 20.04 | 5.91 | 25.72 |
| stats-w1-d2 | all-history | 236,768 | 321.06 | 20.04 | 5.91 | 25.85 |
| stats-w2-d2 | new-off | 509,907 | 621.98 | 31.24 | 7.02 | 37.38 |
| stats-w2-d2 | history | 474,390 | 619.08 | 31.23 | 7.02 | 37.33 |
| stats-w2-d2 | all-four | 499,211 | 621.41 | 25.92 | 7.18 | 31.51 |
| stats-w2-d2 | all-history | 463,695 | 618.51 | 25.92 | 7.18 | 31.58 |
| pivot-w1-d1 | new-off | 62,734 | 65.41 | 10.71 | 3.42 | 16.76 |
| pivot-w1-d1 | history | 56,739 | 63.60 | 10.71 | 3.42 | 16.58 |
| pivot-w1-d1 | all-four | 62,399 | 65.35 | 10.71 | 3.42 | 16.89 |
| pivot-w1-d1 | all-history | 56,404 | 63.54 | 10.71 | 3.42 | 16.61 |
| sand-damage-w1-d1 | new-off | 535,429 | 388.57 | 88.39 | 28.76 | 127.40 |
| sand-damage-w1-d1 | history | 447,288 | 358.81 | 88.39 | 28.76 | 127.54 |
| sand-damage-w1-d1 | all-four | 535,352 | 385.55 | 88.39 | 28.76 | 124.53 |
| sand-damage-w1-d1 | all-history | 447,211 | 355.79 | 88.39 | 28.76 | 131.48 |
| stats-w4-d2 | new-off | 1,009,960 | 1222.06 | 44.54 | 9.40 | 50.69 |
| stats-w4-d2 | history | 938,928 | 1216.29 | 44.54 | 9.40 | 50.68 |
| stats-w4-d2 | all-four | 990,594 | 1221.44 | 37.79 | 9.71 | 42.84 |
| stats-w4-d2 | all-history | 919,562 | 1215.68 | 37.79 | 9.71 | 43.00 |
| narrow-w1-d3 | new-off | 155,004 | 195.01 | 19.46 | 4.46 | 25.56 |
| narrow-w1-d3 | history | 142,054 | 191.04 | 19.46 | 4.46 | 25.57 |
| narrow-w1-d3 | all-four | 149,740 | 193.79 | 15.68 | 4.50 | 21.52 |
| narrow-w1-d3 | all-history | 136,790 | 189.82 | 15.68 | 4.50 | 21.50 |
| stats-w4-d2-cadence1 | new-off | 1,061,959 | 1272.68 | 45.76 | 9.40 | 52.09 |
| stats-w4-d2-cadence1 | history | 990,927 | 1266.92 | 45.76 | 9.40 | 52.09 |
| stats-w4-d2-cadence1 | all-four | 1,005,858 | 1267.44 | 38.00 | 9.71 | 42.82 |
| stats-w4-d2-cadence1 | all-history | 934,826 | 1261.68 | 38.00 | 9.71 | 42.87 |
| narrow-w1-d3-cadence1 | new-off | 187,461 | 238.40 | 20.28 | 4.46 | 26.63 |
| narrow-w1-d3-cadence1 | history | 174,511 | 234.43 | 20.28 | 4.46 | 26.64 |
| narrow-w1-d3-cadence1 | all-four | 165,510 | 232.84 | 15.77 | 4.50 | 21.86 |
| narrow-w1-d3-cadence1 | all-history | 152,560 | 228.87 | 15.77 | 4.50 | 21.70 |

## 진단 비용과 호환성

새 이벤트 포맷은 history_copy로 이동한다. 따라서 history_keys 단일 항목의 감소를 전체 절감으로 오인하지 않는다. 아래는 합산한 exclusive 시간 중앙값이며 서로 다른 단계의 중앙값 합이다. 실제 가속률은 위 clean 측정만 사용한다.

| 조건 | 구성 | history_copy+keys ms | menu ms | boundary ms | observation ms |
|---|---|---:|---:|---:|---:|
| stats-w1-d2 | new-off | 8.574 | 0.719 | 6.406 | 8.796 |
| stats-w1-d2 | history | 3.452 | 0.576 | 6.581 | 9.063 |
| stats-w2-d2 | new-off | 17.545 | 1.432 | 9.588 | 17.942 |
| stats-w2-d2 | history | 7.144 | 1.120 | 10.009 | 18.096 |
| pivot-w1-d1 | new-off | 2.522 | 1.829 | 2.259 | 2.870 |
| pivot-w1-d1 | history | 1.046 | 0.912 | 2.236 | 2.894 |
| sand-damage-w1-d1 | new-off | 34.781 | 0.022 | 36.309 | 49.503 |
| sand-damage-w1-d1 | history | 17.342 | 0.020 | 35.913 | 47.834 |
| stats-w4-d2 | new-off | 34.781 | 2.759 | 17.502 | 35.950 |
| stats-w4-d2 | history | 14.421 | 2.108 | 17.951 | 36.480 |
| narrow-w1-d3 | new-off | 6.039 | 1.239 | 4.253 | 4.884 |
| narrow-w1-d3 | history | 1.995 | 0.789 | 4.392 | 5.044 |
| stats-w4-d2-cadence1 | new-off | 35.687 | 2.770 | 17.330 | 36.237 |
| stats-w4-d2-cadence1 | history | 14.511 | 2.137 | 18.004 | 36.257 |
| narrow-w1-d3-cadence1 | new-off | 6.108 | 1.241 | 4.118 | 4.863 |
| narrow-w1-d3-cadence1 | history | 2.037 | 0.798 | 4.380 | 5.202 |

중간 대조: {'compiled_snapshots': 310, 'sequence_snapshots': 124, 'contractions': 774, 'gap_checks': 372}. 비AVX2 QEMU Nehalem에서 CPUID를 확인한 뒤 두 국면×5구성과 기존 스칼라 경로가 native 전체 결과와 일치했다. 실제 구형 CPU 속도 측정은 아니다.

실험 코드의 저장 표현은 바뀌었으므로 같은 바이너리 OFF/ON 비교만으로 이전 커밋 대비 OFF 비용이 0이라고 보장하지 않는다. 기존 스칼라 feature에는 encoded variant가 컴파일되지 않는다. 생산 엔진과 기본 설정은 유지했고 승률을 평가하지 않았다.

[구현·측정 계약](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s25-history-keys-20261003/source/engine/benchmarks/s26_history/README.md>)
