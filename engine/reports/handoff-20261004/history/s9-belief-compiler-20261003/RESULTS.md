# 동일 계산량 속도 비교

Source `ab57e82bd923666891ad31b3c563b6414080f27e` · parent `bc7ae14351430b2f1cc485ed937bc690aaa5cfa3`.
[GitHub 실행 37030978967](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37030978967) 성공.

Intel(R) Xeon(R) 6973P-C의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
모든 짝에서 전체 트리·정책·오차 인증·계산 횟수·중단 사유의 정확한 일치를 검사했다. 부분 확장 행도 같은 부분 트리를 비교한다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-b4 | in-place | 9.389 | 8.869 | +5.68% |
| stats-w1-d2-b4 | compiler | 9.377 | 9.315 | +0.89% |
| stats-w1-d2-b4 | in-place-compiler | 9.400 | 8.789 | +6.40% |
| stats-w1-d2-b4 | all | 9.400 | 8.717 | +6.93% |
| stats-w1-d2-b10000 | in-place | 85.169 | 73.982 | +13.36% |
| stats-w1-d2-b10000 | compiler | 84.189 | 80.616 | +3.77% |
| stats-w1-d2-b10000 | in-place-compiler | 84.640 | 71.954 | +15.23% |
| stats-w1-d2-b10000 | all | 84.997 | 71.478 | +16.51% |
| stats-w2-d2-b4 | in-place | 18.092 | 17.245 | +4.62% |
| stats-w2-d2-b4 | compiler | 17.964 | 17.803 | +0.80% |
| stats-w2-d2-b4 | in-place-compiler | 18.028 | 17.053 | +5.24% |
| stats-w2-d2-b4 | all | 18.071 | 16.943 | +6.37% |
| stats-w2-d2-b10000 | in-place | 153.150 | 136.755 | +10.41% |
| stats-w2-d2-b10000 | compiler | 154.384 | 145.832 | +5.31% |
| stats-w2-d2-b10000 | in-place-compiler | 152.484 | 131.601 | +13.75% |
| stats-w2-d2-b10000 | all | 153.284 | 130.236 | +14.87% |
| pivot-w1-d1-b10000 | in-place | 18.243 | 18.298 | -0.38% |
| pivot-w1-d1-b10000 | compiler | 18.235 | 18.115 | +0.62% |
| pivot-w1-d1-b10000 | in-place-compiler | 18.222 | 18.111 | +0.63% |
| pivot-w1-d1-b10000 | all | 18.250 | 18.145 | +0.67% |
| sand-damage-w1-d1-b10000 | in-place | 256.401 | 256.312 | -0.21% |
| sand-damage-w1-d1-b10000 | compiler | 257.359 | 257.527 | +0.01% |
| sand-damage-w1-d1-b10000 | in-place-compiler | 258.172 | 259.621 | -0.31% |
| sand-damage-w1-d1-b10000 | all | 258.050 | 257.660 | +0.45% |

280개 ABBA/BAAB 블록·1120개 시간 표본·3024회 계산. 시간비의 중앙값으로 감소율을 계산했다.
7개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP: 전체 트리 49건, 확장 중간/완료 트리 96건 통과. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
