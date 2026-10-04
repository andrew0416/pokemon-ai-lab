# 동일 계산량 속도 비교

Source `cab569eec64dc3213bedf934f69e7c4a78ef38c4` · parent `ec9b72e149b9d72e762e4023a84365e3f72a2ff5`.
[GitHub 실행 37085660921](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37085660921) 성공.

Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz의 논리 CPU 하나에 고정한 generic x86-64 release. 한 실행파일에서 옵션만 전환했다.
모든 비교는 S14 소유권 전달까지 적용된 같은 탐색 방식끼리 이력 저장만 Vec/정수 링크로 바꿨다. 트리·정책·오차 인증·계산 횟수·중단 사유가 정확히 같다.

| 설정 | 후보 | 기준 ms | 후보 ms | 시간 감소 |
|---|---|---:|---:|---:|
| stats-w1-d2-history-growing | interned | 93.748 | 92.889 | +0.73% |
| stats-w1-d2-history-exhaustive | interned | 69.729 | 68.739 | +1.43% |
| stats-w2-d2-history-growing | interned | 171.119 | 165.062 | +3.49% |
| stats-w2-d2-history-exhaustive | interned | 133.790 | 127.858 | +4.43% |
| pivot-w1-d1-history-growing | interned | 23.526 | 22.646 | +3.38% |
| pivot-w1-d1-history-exhaustive | interned | 21.446 | 21.720 | -1.54% |
| sand-damage-w1-d1-history-growing | interned | 300.940 | 308.390 | -2.42% |
| sand-damage-w1-d1-history-exhaustive | interned | 300.245 | 302.569 | -0.82% |
| stats-w4-d2-history-growing | interned | 338.968 | 322.152 | +4.94% |
| stats-w4-d2-history-exhaustive | interned | 264.476 | 247.894 | +6.29% |
| narrow-w1-d3-history-growing | interned | 68.333 | 66.682 | +2.37% |
| narrow-w1-d3-history-exhaustive | interned | 41.874 | 40.879 | +2.30% |

108개 ABBA/BAAB 블록·432개 시간 표본·900회 계산. 시간비의 중앙값으로 감소율을 계산했다.
Core101·기존 소유권6·새 엔진 이력2·기능 ON94·기본 OFF17·CLI1 검사가 통과했다. 최대128결정·강제 해시충돌·104개 도메인 호출 이력·모든 예산 중단 경계를 대조했다. 기존48설정/766상태 검사도 유지했다.
9개 블록의 신뢰구간은 한 러너 내부의 변동만 나타낸다. 작은 효과는 다른 실행에서 재확인해야 한다.
독립 LP 근거: 전체 트리 49건, 확장 중간/완료 트리 96건. 새 CI에서 두 LP 묶음을 재검증했다. 기존 문제의 반복 검증이며 새로운 145문제라는 뜻은 아니다. 강제 교체·예산 소진·실패 시 직전 결과 보존은 Rust 차등 검사에 포함한다.
승률·판단 품질은 평가하지 않았다. 정보 모델은 기존 snapshot-information-v1이며 완전한 공개 전투 로그는 아니다. 기능 기본 OFF, 생산 엔진과 공유 Git 체크아웃은 유지했다.
