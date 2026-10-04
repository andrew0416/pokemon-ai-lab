# 동일 계산량 속도 비교

Source `c830e52dd104743a2f0cc55f07e6b4018bc9591a` · parent `673e9720882aaad4173743282202ea1217eee598`.
[GitHub 실행 37033041022](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37033041022) 성공.

AMD EPYC 7763 64-Core Processor의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
same-work 행은 트리·정책·오차 인증·계산 횟수·중단 사유가 정확히 같다. vs-exhaustive 행은 전체 정규화 트리·모든 정책·오차 인증이 같지만 성장 경로의 중간 풀이 횟수는 더 많다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-same-work | all-cached | 120.938 | 102.054 | +15.72% |
| stats-w1-d2-vs-exhaustive | growing | 82.644 | 121.603 | -46.84% |
| stats-w1-d2-vs-exhaustive | all-cached | 82.627 | 102.593 | -23.89% |
| stats-w2-d2-same-work | all-cached | 230.152 | 195.247 | +15.48% |
| stats-w2-d2-vs-exhaustive | growing | 162.474 | 226.287 | -39.36% |
| stats-w2-d2-vs-exhaustive | all-cached | 162.701 | 192.746 | -18.50% |
| pivot-w1-d1-same-work | all-cached | 27.144 | 26.858 | +0.94% |
| pivot-w1-d1-vs-exhaustive | growing | 25.245 | 27.043 | -6.69% |
| pivot-w1-d1-vs-exhaustive | all-cached | 25.275 | 26.604 | -5.30% |
| sand-damage-w1-d1-same-work | all-cached | 377.328 | 363.772 | +3.63% |
| sand-damage-w1-d1-vs-exhaustive | growing | 371.466 | 375.789 | -1.05% |
| sand-damage-w1-d1-vs-exhaustive | all-cached | 371.963 | 363.497 | +2.11% |
| stats-w4-d2-same-work | all-cached | 474.419 | 390.291 | +17.69% |
| stats-w4-d2-vs-exhaustive | growing | 320.673 | 467.257 | -45.82% |
| stats-w4-d2-vs-exhaustive | all-cached | 320.355 | 387.214 | -20.75% |
| narrow-w1-d3-same-work | all-cached | 91.831 | 70.234 | +23.09% |
| narrow-w1-d3-vs-exhaustive | growing | 48.779 | 90.047 | -82.48% |
| narrow-w1-d3-vs-exhaustive | all-cached | 48.571 | 69.716 | -43.90% |

198개 ABBA/BAAB 블록·792개 시간 표본·1278회 계산. 시간비의 중앙값으로 감소율을 계산했다.
7개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP 근거: 전체 트리 49건, 확장 중간/완료 트리 96건. confirm은 동일 커널인 S10의 검증을 상속하고, writer는 새 CI에서 재검증했다. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
