# 동일 계산량 속도 비교

Source `673e9720882aaad4173743282202ea1217eee598` · parent `ab57e82bd923666891ad31b3c563b6414080f27e`.
[GitHub 실행 37032057405](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37032057405) 성공.

AMD EPYC 9V74 80-Core Processor의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
모든 짝에서 전체 트리·정책·오차 인증·계산 횟수·중단 사유의 정확한 일치를 검사했다. 부분 확장 행도 같은 부분 트리를 비교한다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-b4 | in-place-compiler | 13.685 | 13.076 | +4.56% |
| stats-w1-d2-b4 | all | 13.653 | 13.003 | +5.02% |
| stats-w1-d2-b4 | cached | 13.655 | 13.434 | +1.52% |
| stats-w1-d2-b4 | all-cached | 13.675 | 12.845 | +6.05% |
| stats-w1-d2-b10000 | in-place-compiler | 118.765 | 102.915 | +13.10% |
| stats-w1-d2-b10000 | all | 118.818 | 101.593 | +13.70% |
| stats-w1-d2-b10000 | cached | 118.107 | 115.734 | +2.28% |
| stats-w1-d2-b10000 | all-cached | 118.252 | 99.737 | +15.21% |
| stats-w2-d2-b4 | in-place-compiler | 26.402 | 25.299 | +4.07% |
| stats-w2-d2-b4 | all | 26.372 | 25.151 | +4.57% |
| stats-w2-d2-b4 | cached | 26.342 | 25.984 | +1.49% |
| stats-w2-d2-b4 | all-cached | 26.296 | 24.779 | +5.71% |
| stats-w2-d2-b10000 | in-place-compiler | 218.378 | 193.927 | +11.38% |
| stats-w2-d2-b10000 | all | 217.773 | 192.224 | +12.07% |
| stats-w2-d2-b10000 | cached | 217.979 | 212.022 | +2.73% |
| stats-w2-d2-b10000 | all-cached | 217.906 | 188.620 | +13.69% |
| pivot-w1-d1-b10000 | in-place-compiler | 26.547 | 26.373 | -0.02% |
| pivot-w1-d1-b10000 | all | 26.453 | 26.434 | +0.44% |
| pivot-w1-d1-b10000 | cached | 26.582 | 26.412 | +0.63% |
| pivot-w1-d1-b10000 | all-cached | 26.722 | 26.409 | +1.08% |
| sand-damage-w1-d1-b10000 | in-place-compiler | 378.315 | 376.635 | +0.52% |
| sand-damage-w1-d1-b10000 | all | 377.095 | 376.423 | +0.58% |
| sand-damage-w1-d1-b10000 | cached | 377.122 | 365.565 | +3.29% |
| sand-damage-w1-d1-b10000 | all-cached | 377.466 | 364.857 | +3.25% |
| stats-w1-d2-exhaustive | workspace | 79.474 | 79.414 | +0.02% |
| stats-w1-d2-exhaustive | cached | 79.486 | 79.163 | +0.37% |
| stats-w2-d2-exhaustive | workspace | 152.148 | 152.306 | +0.04% |
| stats-w2-d2-exhaustive | cached | 152.278 | 151.750 | +0.56% |
| pivot-w1-d1-exhaustive | workspace | 24.860 | 24.810 | +0.22% |
| pivot-w1-d1-exhaustive | cached | 24.833 | 24.713 | +0.63% |
| sand-damage-w1-d1-exhaustive | workspace | 384.669 | 383.003 | +0.38% |
| sand-damage-w1-d1-exhaustive | cached | 384.337 | 372.409 | +2.97% |

336개 ABBA/BAAB 블록·1344개 시간 표본·2674회 계산. 시간비의 중앙값으로 감소율을 계산했다.
7개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP: 전체 트리 49건, 확장 중간/완료 트리 96건 통과. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
