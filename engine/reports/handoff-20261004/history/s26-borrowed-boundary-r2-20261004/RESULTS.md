# S26k 경계 키 대여 조회와 이전 버전 대조

[GitHub 실행](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37163534085) · `3f0b28e7f317c84acffa21a1354351cfb8dcbe3d` · 1283초

이미 등록된 public-history 경계는 `&[String]`으로 조회하고, 처음 등장한 키만 복사한다. 정보 집합·키 내용·등록 순서·콜백·탐색량은 유지한다. `pipeline.borrowed_boundary`는 기본 OFF다.

계측 122개·일반 118개·기본 17개 테스트 통과. 32개 관측 게임에서 1,537쌍의 전체 OFF/ON JSON, 독립 BR/LP 인증 3,405건을 대조했다. 그중 12건은 반복 상한 미수렴이지만 인증 수치는 일치했다. 여러 설정과 부분 탐색을 포함하는 횟수이며 독립 게임 3,405개가 아니다. 56개 엔진 구성의 결과·작업량 지문이 빌드별로 일치했다.

## 같은 바이너리의 속도

표의 각 열은 경계 옵션만 추가한 별도 비교다. 양수는 시간 단축. 비율은 후보/기준이며 1보다 작으면 빠르다. 9개 ABBA/BAAB 블록과 같은 경로 A/A를 사용한다. 서로 다른 열의 단축률을 더하지 않는다.

| 조건 | 단독 | 이력 옵션에 추가 | 기존 5개 전체에 추가 |
|---|---:|---:|---:|
| stats-w1-d2 | -0.10% | +0.76% | +0.92% |
| stats-w2-d2 | -0.62% | -1.73% | +0.47% |
| pivot-w1-d1 | +0.26% | +0.54% | +0.85% |
| sand-damage-w1-d1 | +0.34% | +0.00% | -0.29% |
| stats-w4-d2 | +0.14% | +0.54% | -0.07% |
| narrow-w1-d3 | +0.52% | -1.12% | -1.14% |
| stats-w4-d2-cadence1 | +1.15% | -2.43% | +0.82% |
| narrow-w1-d3-cadence1 | +0.21% | +0.32% | +1.08% |

| 조건 | 후보 | 기준 ms | 후보 ms | 단축률 | 비율 95% 구간 | A/A 밖 |
|---|---|---:|---:|---:|---|---|
| stats-w1-d2 | new-off-aa | 76.360 | 77.412 | -0.96% | 0.9896–1.0356 | False |
| stats-w1-d2 | boundary | 76.431 | 77.279 | -0.10% | 0.9909–1.0132 | False |
| stats-w1-d2 | history-aa | 69.132 | 69.256 | +0.13% | 0.9746–1.0117 | False |
| stats-w1-d2 | history-boundary | 69.966 | 69.978 | +0.76% | 0.9483–1.0146 | False |
| stats-w1-d2 | all-history-aa | 68.761 | 68.179 | -0.62% | 0.9796–1.0241 | False |
| stats-w1-d2 | all-six | 69.185 | 68.313 | +0.92% | 0.9682–1.0438 | False |
| stats-w2-d2 | new-off-aa | 142.645 | 142.158 | -0.56% | 0.9831–1.0274 | False |
| stats-w2-d2 | boundary | 142.935 | 144.331 | -0.62% | 0.9813–1.0459 | False |
| stats-w2-d2 | history-aa | 128.112 | 132.039 | +1.62% | 0.9586–1.0429 | False |
| stats-w2-d2 | history-boundary | 129.646 | 131.123 | -1.73% | 0.9740–1.0343 | False |
| stats-w2-d2 | all-history-aa | 127.820 | 127.190 | +0.23% | 0.9767–1.0116 | False |
| stats-w2-d2 | all-six | 126.644 | 126.625 | +0.47% | 0.9822–1.0081 | False |
| pivot-w1-d1 | new-off-aa | 23.729 | 23.927 | +0.35% | 0.9939–1.0150 | False |
| pivot-w1-d1 | boundary | 24.044 | 23.970 | +0.26% | 0.9909–1.0030 | False |
| pivot-w1-d1 | history-aa | 20.809 | 20.891 | -1.01% | 1.0015–1.0184 | False |
| pivot-w1-d1 | history-boundary | 20.792 | 20.697 | +0.54% | 0.9908–1.0035 | False |
| pivot-w1-d1 | all-history-aa | 21.098 | 21.203 | -0.62% | 0.9924–1.0113 | False |
| pivot-w1-d1 | all-six | 21.361 | 21.210 | +0.85% | 0.9824–1.0058 | False |
| sand-damage-w1-d1 | new-off-aa | 271.068 | 270.392 | +0.38% | 0.9899–1.0123 | False |
| sand-damage-w1-d1 | boundary | 270.361 | 270.769 | +0.34% | 0.9884–1.0206 | False |
| sand-damage-w1-d1 | history-aa | 246.773 | 245.504 | +1.15% | 0.9726–1.0194 | False |
| sand-damage-w1-d1 | history-boundary | 245.992 | 244.778 | +0.00% | 0.9597–1.0154 | False |
| sand-damage-w1-d1 | all-history-aa | 245.971 | 247.255 | -0.78% | 0.9842–1.0262 | False |
| sand-damage-w1-d1 | all-six | 247.654 | 246.461 | -0.29% | 0.9598–1.0209 | False |
| stats-w4-d2 | new-off-aa | 284.816 | 287.383 | -0.68% | 1.0010–1.0452 | False |
| stats-w4-d2 | boundary | 286.126 | 288.918 | +0.14% | 0.9923–1.0192 | False |
| stats-w4-d2 | history-aa | 258.809 | 257.129 | +0.05% | 0.9891–1.0042 | False |
| stats-w4-d2 | history-boundary | 257.442 | 260.032 | +0.54% | 0.9857–1.0140 | False |
| stats-w4-d2 | all-history-aa | 256.346 | 256.177 | +0.42% | 0.9788–1.0118 | False |
| stats-w4-d2 | all-six | 255.019 | 255.410 | -0.07% | 0.9659–1.0100 | False |
| narrow-w1-d3 | new-off-aa | 46.991 | 46.691 | -0.00% | 0.9956–1.0102 | False |
| narrow-w1-d3 | boundary | 47.485 | 46.611 | +0.52% | 0.9828–1.0006 | False |
| narrow-w1-d3 | history-aa | 40.879 | 40.954 | -0.56% | 0.9755–1.0207 | False |
| narrow-w1-d3 | history-boundary | 40.792 | 41.201 | -1.12% | 0.9931–1.0359 | False |
| narrow-w1-d3 | all-history-aa | 40.736 | 40.627 | +0.27% | 0.9775–1.0162 | False |
| narrow-w1-d3 | all-six | 40.540 | 41.176 | -1.14% | 0.9951–1.0295 | False |
| stats-w4-d2-cadence1 | new-off-aa | 303.657 | 305.864 | +0.51% | 0.9857–1.0090 | False |
| stats-w4-d2-cadence1 | boundary | 303.841 | 300.645 | +1.15% | 0.9770–1.0031 | False |
| stats-w4-d2-cadence1 | history-aa | 277.173 | 278.732 | -1.74% | 0.9957–1.0254 | False |
| stats-w4-d2-cadence1 | history-boundary | 275.884 | 279.980 | -2.43% | 1.0048–1.0396 | False |
| stats-w4-d2-cadence1 | all-history-aa | 274.612 | 276.503 | +0.12% | 0.9799–1.0372 | False |
| stats-w4-d2-cadence1 | all-six | 273.952 | 275.074 | +0.82% | 0.9633–1.0197 | False |
| narrow-w1-d3-cadence1 | new-off-aa | 57.022 | 57.044 | -0.33% | 0.9735–1.0114 | False |
| narrow-w1-d3-cadence1 | boundary | 56.100 | 55.877 | +0.21% | 0.9886–1.0059 | False |
| narrow-w1-d3-cadence1 | history-aa | 48.757 | 48.458 | +1.06% | 0.9646–1.0089 | False |
| narrow-w1-d3-cadence1 | history-boundary | 48.997 | 48.953 | +0.32% | 0.9851–1.0218 | False |
| narrow-w1-d3-cadence1 | all-history-aa | 46.120 | 47.310 | -1.21% | 0.9837–1.0385 | False |
| narrow-w1-d3-cadence1 | all-six | 47.311 | 46.282 | +1.08% | 0.9732–1.0147 | False |

## 같은 러너의 이전 버전 대조

S24와 S25의 라이브러리는 해당 커밋 그대로이며, 벤치마크 CLI에만 동일한 worker 통신을 추가했다. 입력·풀 생성, stdin/JSON, 결과 지문 및 반환물 해제는 시간 밖이다. 세 프로세스에 순차 요청하고 각 조건 9블록을 교차 실행했다. OFF 비교는 새 코드가 컴파일된 실험 빌드의 차이를 측정한다. 기존 스칼라 빌드 비용의 측정은 아니다.

| 조건 | 비교 | 단축률 | 비율 95% 구간 | A/A 밖 |
|---|---|---:|---|---|
| stats-w1-d2 | s24-aa | -0.53% | 0.9621–1.0205 | False |
| stats-w1-d2 | s24-to-s25-off | -0.46% | 0.9691–1.0727 | False |
| stats-w1-d2 | s25-aa | -0.32% | 0.9924–1.0215 | False |
| stats-w1-d2 | s25-to-current-off | +1.57% | 0.9480–1.0652 | False |
| stats-w1-d2 | s25-history-aa | +0.77% | 0.9823–1.0204 | False |
| stats-w1-d2 | s25-to-current-history | -1.22% | 0.9469–1.0283 | False |
| stats-w2-d2 | s24-aa | -0.06% | 0.9864–1.0235 | False |
| stats-w2-d2 | s24-to-s25-off | +0.49% | 0.9883–1.0350 | False |
| stats-w2-d2 | s25-aa | +0.30% | 0.9866–1.0249 | False |
| stats-w2-d2 | s25-to-current-off | +0.98% | 0.9641–1.0190 | False |
| stats-w2-d2 | s25-history-aa | -0.35% | 0.9599–1.0184 | False |
| stats-w2-d2 | s25-to-current-history | -0.93% | 0.9869–1.0126 | False |
| pivot-w1-d1 | s24-aa | -0.84% | 0.9884–1.0301 | False |
| pivot-w1-d1 | s24-to-s25-off | -1.72% | 1.0079–1.0390 | False |
| pivot-w1-d1 | s25-aa | +0.54% | 0.9828–1.0099 | False |
| pivot-w1-d1 | s25-to-current-off | -0.70% | 0.9974–1.0315 | False |
| pivot-w1-d1 | s25-history-aa | -0.71% | 0.9916–1.0148 | False |
| pivot-w1-d1 | s25-to-current-history | -0.35% | 0.9813–1.0161 | False |
| sand-damage-w1-d1 | s24-aa | -2.60% | 0.9842–1.0369 | False |
| sand-damage-w1-d1 | s24-to-s25-off | +0.09% | 0.9821–1.0470 | False |
| sand-damage-w1-d1 | s25-aa | -0.71% | 0.9959–1.0095 | False |
| sand-damage-w1-d1 | s25-to-current-off | +0.94% | 0.9776–1.0132 | False |
| sand-damage-w1-d1 | s25-history-aa | +0.62% | 0.9778–1.0247 | False |
| sand-damage-w1-d1 | s25-to-current-history | +2.26% | 0.9687–1.0240 | False |
| stats-w4-d2 | s24-aa | +0.77% | 0.9611–1.0021 | False |
| stats-w4-d2 | s24-to-s25-off | +0.74% | 0.9787–1.0352 | False |
| stats-w4-d2 | s25-aa | -0.69% | 0.9982–1.0195 | False |
| stats-w4-d2 | s25-to-current-off | -1.59% | 0.9843–1.0281 | False |
| stats-w4-d2 | s25-history-aa | +1.89% | 0.9649–1.0040 | False |
| stats-w4-d2 | s25-to-current-history | -0.34% | 0.9880–1.0216 | False |
| narrow-w1-d3 | s24-aa | +0.54% | 0.9813–1.0142 | False |
| narrow-w1-d3 | s24-to-s25-off | -1.13% | 0.9979–1.0432 | False |
| narrow-w1-d3 | s25-aa | -0.13% | 0.9876–1.0325 | False |
| narrow-w1-d3 | s25-to-current-off | -0.37% | 0.9650–1.0348 | False |
| narrow-w1-d3 | s25-history-aa | +0.43% | 0.9698–1.0154 | False |
| narrow-w1-d3 | s25-to-current-history | +2.20% | 0.9559–0.9932 | False |
| stats-w4-d2-cadence1 | s24-aa | -0.20% | 0.9848–1.0159 | False |
| stats-w4-d2-cadence1 | s24-to-s25-off | +0.60% | 0.9825–1.0193 | False |
| stats-w4-d2-cadence1 | s25-aa | +2.56% | 0.9637–1.0029 | False |
| stats-w4-d2-cadence1 | s25-to-current-off | -0.04% | 0.9921–1.0475 | False |
| stats-w4-d2-cadence1 | s25-history-aa | -1.47% | 1.0087–1.0395 | False |
| stats-w4-d2-cadence1 | s25-to-current-history | -1.33% | 0.9742–1.0278 | False |
| narrow-w1-d3-cadence1 | s24-aa | -0.03% | 0.9920–1.0092 | False |
| narrow-w1-d3-cadence1 | s24-to-s25-off | -4.49% | 1.0167–1.0539 | True |
| narrow-w1-d3-cadence1 | s25-aa | +0.64% | 0.9754–1.0019 | False |
| narrow-w1-d3-cadence1 | s25-to-current-off | -1.83% | 1.0070–1.0389 | False |
| narrow-w1-d3-cadence1 | s25-history-aa | -0.12% | 0.9764–1.0113 | False |
| narrow-w1-d3-cadence1 | s25-to-current-history | -1.61% | 0.9871–1.0393 | False |

## sand RSS 재현

각 빌드 유형에서 12블록×5구성, 총 120개의 새 프로세스를 회전 순서로 실행했다. 일반 빌드와 allocator 계측 빌드는 따로 해석한다. RSS는 입력과 풀 등을 포함하는 자식 프로세스 전체 peak다. 단축률 대신 RSS 감소율이며, 양수면 적게 사용한다. paired ratio 중앙값과 각 구성 RSS 중앙값의 비는 서로 다를 수 있다.

| 빌드 | S25 4개 MiB | S25 5개 MiB | 현재 5개 MiB | 현재 6개 MiB |
|---|---:|---:|---:|---:|
| plain | 132.73 | 131.23 | 126.22 | 131.90 |
| instrumented | 129.89 | 130.31 | 129.30 | 127.13 |

| 빌드 | 비교 | RSS 감소율 | 비율 95% 구간 | A/A 밖 |
|---|---|---:|---|---|
| plain | aa | -0.05% | 0.9677–1.0259 | False |
| plain | history | +0.13% | 0.9638–1.0161 | False |
| plain | wrapper | +2.44% | 0.9387–1.0136 | False |
| plain | boundary | -3.80% | 1.0038–1.0672 | False |
| instrumented | aa | -0.68% | 0.9822–1.0479 | False |
| instrumented | history | +1.96% | 0.9614–1.0336 | False |
| instrumented | wrapper | +0.19% | 0.9811–1.0437 | False |
| instrumented | boundary | +1.33% | 0.9588–1.0320 | False |

## 요청 heap과 할당

56구성×3회=168회 별도 프로세스의 중앙값. 요청 heap은 baseline 위 peak이며 속도 근거로 쓰지 않는다.

| 조건 | 구성 | 할당 수 | 누적 요청 MiB | heap peak MiB | 반환물 MiB | RSS MiB |
|---|---|---:|---:|---:|---:|---:|
| stats-w1-d2 | new-off | 261,092 | 323.03 | 24.67 | 5.84 | 30.47 |
| stats-w1-d2 | boundary | 257,880 | 321.74 | 24.68 | 5.84 | 30.62 |
| stats-w1-d2 | history | 243,334 | 321.57 | 24.67 | 5.84 | 30.56 |
| stats-w1-d2 | history-boundary | 240,122 | 320.29 | 24.68 | 5.84 | 30.60 |
| stats-w1-d2 | all-history | 236,770 | 321.06 | 20.04 | 5.92 | 25.69 |
| stats-w1-d2 | all-six | 233,554 | 319.77 | 20.04 | 5.91 | 25.74 |
| stats-w2-d2 | new-off | 509,904 | 621.97 | 31.23 | 7.02 | 37.18 |
| stats-w2-d2 | boundary | 502,498 | 619.00 | 31.23 | 7.02 | 37.30 |
| stats-w2-d2 | history | 474,390 | 619.08 | 31.23 | 7.02 | 37.31 |
| stats-w2-d2 | history-boundary | 466,982 | 616.10 | 31.23 | 7.02 | 37.20 |
| stats-w2-d2 | all-history | 463,695 | 618.51 | 25.92 | 7.18 | 31.35 |
| stats-w2-d2 | all-six | 456,287 | 615.53 | 25.92 | 7.18 | 31.48 |
| pivot-w1-d1 | new-off | 62,734 | 65.41 | 10.71 | 3.42 | 16.70 |
| pivot-w1-d1 | boundary | 62,020 | 65.13 | 10.71 | 3.42 | 16.66 |
| pivot-w1-d1 | history | 56,739 | 63.60 | 10.71 | 3.42 | 16.56 |
| pivot-w1-d1 | history-boundary | 56,025 | 63.32 | 10.71 | 3.42 | 16.54 |
| pivot-w1-d1 | all-history | 56,404 | 63.54 | 10.71 | 3.42 | 16.56 |
| pivot-w1-d1 | all-six | 55,690 | 63.26 | 10.71 | 3.42 | 16.58 |
| sand-damage-w1-d1 | new-off | 535,431 | 388.58 | 88.39 | 28.76 | 134.19 |
| sand-damage-w1-d1 | boundary | 519,829 | 382.97 | 88.39 | 28.76 | 128.34 |
| sand-damage-w1-d1 | history | 447,288 | 358.81 | 88.39 | 28.76 | 132.46 |
| sand-damage-w1-d1 | history-boundary | 431,687 | 353.20 | 88.39 | 28.76 | 127.27 |
| sand-damage-w1-d1 | all-history | 447,211 | 355.79 | 88.39 | 28.76 | 131.89 |
| sand-damage-w1-d1 | all-six | 431,611 | 350.18 | 88.39 | 28.76 | 133.88 |
| stats-w4-d2 | new-off | 1,009,962 | 1222.06 | 44.54 | 9.40 | 50.64 |
| stats-w4-d2 | boundary | 994,165 | 1215.71 | 44.54 | 9.40 | 50.58 |
| stats-w4-d2 | history | 938,928 | 1216.29 | 44.54 | 9.40 | 50.66 |
| stats-w4-d2 | history-boundary | 923,132 | 1209.95 | 44.54 | 9.40 | 50.61 |
| stats-w4-d2 | all-history | 919,562 | 1215.68 | 37.79 | 9.71 | 42.85 |
| stats-w4-d2 | all-six | 903,767 | 1209.33 | 37.80 | 9.71 | 42.83 |
| narrow-w1-d3 | new-off | 155,004 | 195.01 | 19.46 | 4.46 | 25.62 |
| narrow-w1-d3 | boundary | 152,838 | 194.09 | 19.46 | 4.47 | 25.46 |
| narrow-w1-d3 | history | 142,054 | 191.04 | 19.46 | 4.46 | 25.60 |
| narrow-w1-d3 | history-boundary | 139,886 | 190.11 | 19.46 | 4.46 | 25.49 |
| narrow-w1-d3 | all-history | 136,790 | 189.82 | 15.68 | 4.50 | 21.39 |
| narrow-w1-d3 | all-six | 134,622 | 188.90 | 15.68 | 4.50 | 21.47 |
| stats-w4-d2-cadence1 | new-off | 1,061,959 | 1272.68 | 45.76 | 9.40 | 52.04 |
| stats-w4-d2-cadence1 | boundary | 1,046,165 | 1266.34 | 45.77 | 9.40 | 51.93 |
| stats-w4-d2-cadence1 | history | 990,929 | 1266.92 | 45.77 | 9.40 | 51.90 |
| stats-w4-d2-cadence1 | history-boundary | 975,131 | 1260.57 | 45.76 | 9.40 | 51.99 |
| stats-w4-d2-cadence1 | all-history | 934,826 | 1261.68 | 38.00 | 9.71 | 42.70 |
| stats-w4-d2-cadence1 | all-six | 919,030 | 1255.34 | 38.00 | 9.71 | 42.81 |
| narrow-w1-d3-cadence1 | new-off | 187,461 | 238.40 | 20.28 | 4.46 | 26.39 |
| narrow-w1-d3-cadence1 | boundary | 185,293 | 237.47 | 20.28 | 4.46 | 26.44 |
| narrow-w1-d3-cadence1 | history | 174,511 | 234.43 | 20.28 | 4.46 | 26.48 |
| narrow-w1-d3-cadence1 | history-boundary | 172,343 | 233.50 | 20.28 | 4.46 | 26.32 |
| narrow-w1-d3-cadence1 | all-history | 152,560 | 228.87 | 15.81 | 4.50 | 21.57 |
| narrow-w1-d3-cadence1 | all-six | 150,392 | 227.95 | 15.77 | 4.50 | 21.57 |

## 계측과 호환성

5회씩 56구성에서 19개 exclusive 단계를 확인했다. 중간 상태 대조 합계: {'compiled_snapshots': 434, 'sequence_snapshots': 124, 'contractions': 774, 'gap_checks': 372}. 비AVX2 QEMU Nehalem에서 CPUID를 확인한 뒤 2국면×7구성과 기존 scalar가 native 전체 결과와 일치했다. 물리적인 구형 CPU의 속도 검증은 아니다.

단일 러너 세션·유한 국면·다중 검정 미보정이라는 한계가 있다. A/A 범위는 설명용 노이즈 판단이다. 생산 엔진과 기본 설정은 그대로이며 승률은 평가하지 않았다.

[구현·측정 계약](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s26-borrowed-boundary-r2-20261004/source/engine/benchmarks/s26_boundary/README.md>)
