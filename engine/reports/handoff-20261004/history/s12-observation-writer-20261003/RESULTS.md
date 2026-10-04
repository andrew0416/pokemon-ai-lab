# 동일 계산량 속도 비교

Source `081124aaae5e0fe5aa1d8c5b9117803abd866875` · parent `c830e52dd104743a2f0cc55f07e6b4018bc9591a`.
[GitHub 실행 37034440983](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37034440983) 성공.

AMD EPYC 7763 64-Core Processor의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
same-work 행은 트리·정책·오차 인증·계산 횟수·중단 사유가 정확히 같다. vs-exhaustive 행은 전체 정규화 트리·모든 정책·오차 인증이 같지만 성장 경로의 중간 풀이 횟수는 더 많다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-same-work | writer | 121.022 | 116.324 | +5.52% |
| stats-w1-d2-same-work | all-cached | 121.970 | 102.247 | +16.30% |
| stats-w1-d2-same-work | all-writer | 118.860 | 96.732 | +19.24% |
| stats-w1-d2-vs-exhaustive | exhaustive-writing | 83.919 | 77.404 | +7.85% |
| stats-w1-d2-vs-exhaustive | all-writer | 83.723 | 97.839 | -16.85% |
| stats-w2-d2-same-work | writer | 232.485 | 217.800 | +4.58% |
| stats-w2-d2-same-work | all-cached | 230.329 | 192.710 | +16.00% |
| stats-w2-d2-same-work | all-writer | 229.897 | 181.130 | +21.19% |
| stats-w2-d2-vs-exhaustive | exhaustive-writing | 162.410 | 150.277 | +7.45% |
| stats-w2-d2-vs-exhaustive | all-writer | 162.241 | 182.004 | -12.37% |
| pivot-w1-d1-same-work | writer | 27.247 | 24.706 | +9.40% |
| pivot-w1-d1-same-work | all-cached | 27.261 | 26.916 | +1.47% |
| pivot-w1-d1-same-work | all-writer | 27.280 | 24.415 | +10.41% |
| pivot-w1-d1-vs-exhaustive | exhaustive-writing | 25.546 | 22.927 | +10.03% |
| pivot-w1-d1-vs-exhaustive | all-writer | 25.560 | 24.763 | +3.27% |
| sand-damage-w1-d1-same-work | writer | 386.022 | 346.302 | +10.43% |
| sand-damage-w1-d1-same-work | all-cached | 382.420 | 372.409 | +3.26% |
| sand-damage-w1-d1-same-work | all-writer | 383.945 | 331.769 | +13.16% |
| sand-damage-w1-d1-vs-exhaustive | exhaustive-writing | 375.045 | 325.093 | +13.32% |
| sand-damage-w1-d1-vs-exhaustive | all-writer | 373.899 | 325.675 | +12.45% |
| stats-w4-d2-same-work | writer | 462.078 | 439.625 | +4.99% |
| stats-w4-d2-same-work | all-cached | 460.093 | 385.323 | +16.45% |
| stats-w4-d2-same-work | all-writer | 460.710 | 361.255 | +21.68% |
| stats-w4-d2-vs-exhaustive | exhaustive-writing | 330.638 | 304.019 | +8.01% |
| stats-w4-d2-vs-exhaustive | all-writer | 330.172 | 370.392 | -12.18% |
| narrow-w1-d3-same-work | writer | 87.598 | 84.106 | +4.11% |
| narrow-w1-d3-same-work | all-cached | 87.337 | 68.928 | +21.05% |
| narrow-w1-d3-same-work | all-writer | 86.734 | 65.833 | +24.48% |
| narrow-w1-d3-vs-exhaustive | exhaustive-writing | 48.123 | 44.539 | +7.54% |
| narrow-w1-d3-vs-exhaustive | all-writer | 48.129 | 65.369 | -35.54% |

210개 ABBA/BAAB 블록·840개 시간 표본·1288회 계산. 시간비의 중앙값으로 감소율을 계산했다.
7개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP 근거: 전체 트리 49건, 확장 중간/완료 트리 96건. confirm은 동일 커널인 S10의 검증을 상속하고, writer는 새 CI에서 재검증했다. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
