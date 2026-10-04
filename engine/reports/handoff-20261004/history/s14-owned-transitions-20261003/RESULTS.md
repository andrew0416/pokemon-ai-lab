# 동일 계산량 속도 비교

Source `ec9b72e149b9d72e762e4023a84365e3f72a2ff5` · parent `081124aaae5e0fe5aa1d8c5b9117803abd866875`.
[GitHub 실행 37083242086](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37083242086) 성공.

AMD EPYC 9V74 80-Core Processor의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
모든 비교는 S12 최적화가 적용된 같은 탐색 방식끼리 owned 경로만 OFF/ON으로 바꿨다. 트리·정책·오차 인증·계산 횟수·중단 사유가 정확히 같다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-owned-growing | owned | 95.624 | 93.591 | +2.13% |
| stats-w1-d2-owned-exhaustive | owned | 73.846 | 72.074 | +2.42% |
| stats-w2-d2-owned-growing | owned | 181.724 | 175.770 | +3.07% |
| stats-w2-d2-owned-exhaustive | owned | 143.081 | 138.766 | +3.00% |
| pivot-w1-d1-owned-growing | owned | 24.611 | 24.198 | +1.70% |
| pivot-w1-d1-owned-exhaustive | owned | 22.323 | 22.006 | +1.48% |
| sand-damage-w1-d1-owned-growing | owned | 349.211 | 332.489 | +4.47% |
| sand-damage-w1-d1-owned-exhaustive | owned | 337.834 | 323.724 | +3.47% |
| stats-w4-d2-owned-growing | owned | 365.206 | 355.722 | +2.62% |
| stats-w4-d2-owned-exhaustive | owned | 289.100 | 281.464 | +2.64% |
| narrow-w1-d3-owned-growing | owned | 69.444 | 68.484 | +1.60% |
| narrow-w1-d3-owned-exhaustive | owned | 44.941 | 43.990 | +2.27% |

108개 ABBA/BAAB 블록·432개 시간 표본·882회 계산. 시간비의 중앙값으로 감소율을 계산했다.
Core101·소유권 경로6·기능 ON88·기본 OFF17·CLI1 검사가 통과했다. 8개 fixture의 48설정·766개 결과에서 상태·확률·분기 순서·이어가기 정보가 일치했다.
9개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP 근거: 전체 트리 49건, 확장 중간/완료 트리 96건. 새 CI에서 두 LP 묶음을 재검증했다. 기존 문제의 반복 검증이며 새로운 145문제라는 뜻은 아니다. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
