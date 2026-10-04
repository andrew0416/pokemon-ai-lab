# 동일 계산량 속도 비교

Source `bc7ae14351430b2f1cc485ed937bc690aaa5cfa3` · parent `5b79d6e30b1fc0c39da227700ca3b169b60691b2`.
[GitHub 실행 37029881806](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37029881806) 성공.

AMD EPYC 7763 64-Core Processor의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
모든 짝에서 전체 트리·정책·오차 인증·계산 횟수·중단 사유의 정확한 일치를 검사했다. 부분 확장 행도 같은 부분 트리를 비교한다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-b4 | in-place | 14.374 | 13.869 | +3.64% |
| stats-w1-d2-b4 | workspace | 14.395 | 14.217 | +1.35% |
| stats-w1-d2-b4 | combined | 14.311 | 13.740 | +4.03% |
| stats-w1-d2-b10000 | in-place | 121.157 | 108.610 | +10.10% |
| stats-w1-d2-b10000 | workspace | 120.500 | 119.263 | +0.80% |
| stats-w1-d2-b10000 | combined | 120.190 | 106.993 | +10.68% |
| stats-w1-d2-exhaustive | workspace | 83.526 | 83.365 | +0.27% |
| stats-w2-d2-b4 | in-place | 27.917 | 27.109 | +3.25% |
| stats-w2-d2-b4 | workspace | 27.937 | 27.758 | +0.02% |
| stats-w2-d2-b4 | combined | 28.000 | 26.832 | +4.06% |
| stats-w2-d2-b10000 | in-place | 226.430 | 209.132 | +8.55% |
| stats-w2-d2-b10000 | workspace | 227.747 | 225.068 | +0.85% |
| stats-w2-d2-b10000 | combined | 227.706 | 207.080 | +8.83% |
| stats-w2-d2-exhaustive | workspace | 164.207 | 164.218 | +0.39% |
| pivot-w1-d1-b10000 | in-place | 27.402 | 27.338 | +0.55% |
| pivot-w1-d1-b10000 | workspace | 27.342 | 27.351 | +0.23% |
| pivot-w1-d1-b10000 | combined | 27.379 | 27.274 | +0.54% |
| pivot-w1-d1-exhaustive | workspace | 25.530 | 25.414 | +0.42% |
| sand-damage-w1-d1-b10000 | in-place | 383.013 | 381.434 | -0.16% |
| sand-damage-w1-d1-b10000 | workspace | 378.884 | 379.329 | -0.08% |
| sand-damage-w1-d1-b10000 | combined | 380.854 | 380.871 | -0.26% |
| sand-damage-w1-d1-exhaustive | workspace | 380.965 | 380.923 | -0.37% |

238개 ABBA/BAAB 블록·952개 시간 표본·1778회 계산. 시간비의 중앙값으로 감소율을 계산했다.
7개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP: 전체 트리 49건, 확장 중간/완료 트리 96건 통과. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
