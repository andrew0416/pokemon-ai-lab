# 재현의 세 층

## 1. 저장한 결과를 다시 계산

`verify.py --deep`은 모든 bundle SHA256, 41 archive의 CRC, 고정 기준/후보 출처, 다른 실행파일 사용, 10조건의 결과·탐색량 지문, 32비교/288블록/1152회 계산 sample, 100회/19단계 계측을 확인한다. S27의 median ratio와 bootstrap을 원시 worker 줄의 실행 순서에서 다시 만든다. 새 Rust 테스트나 oracle 검증을 실행하지 않는다.

## 2. 정확성을 다시 실행

`engine/search`의 기능은 Cargo feature와 runtime 옵션이 별개다. 최신 실험 entry point는 `lab-phase-speed`, 기능은 `experiment-growth-pipeline`이고 다섯 옵션은 `protocol/plan.json`에 명시돼 있다. `--worker-check`는 시간을 측정하지 않는다. 일반 코어/BR·LP oracle 검증은 고정 S24/S25 source와 각각의 workflow를 따라 재실행해야 한다. 이번 handoff는 기존 통과 기록을 재사용한다. 기존 Full/PBS 기능은 snapshot-information-v1 추상화 위에 있고 완전한 공개 전투 로그를 관측하는 시스템은 아니다.

## 3. 새 속도 실험

새 실험 폴더/브랜치를 만들고 입력·옵션·판정 기준을 먼저 고정한다. `.github/workflows/engine-s26l-stack.yml`을 참고하되 원래 branch trigger를 덮어쓰지 않는다. S24 기준과 S25 후보에 동일한 benchmark CLI를 적용하고, Cargo target을 **서로 분리**한다. 원본 커밋·patch·바이너리 SHA를 확인한다. Rust 1.98.1, generic x86-64, 동일 VM·CPU affinity·pool 크기·완료/오차 기준과 A/A를 유지한다. 계측 바이너리 시간은 속도 표의 분자가 될 수 없다.

원래 입력 순서·scope·censor 규칙을 보존하고 새 시행은 새 결과로 저장한다. 승률 실험은 현재 범위가 아니다. 비AVX2 호환은 scalar fallback을 유지하며 필요한 실행 검증을 추가한다. 기존 QEMU Nehalem 통과는 실제 구형 하드웨어 측정으로 표현하지 않는다.

S26m은 설계 선택 상태다. 일반 Domain의 stateful 관측·평가·prior 순서를 변경하지 말고 엔진의 순수 snapshot 경로만 병렬화한다. 오류는 원래 소비 순서에서만 반환한다. 1/2/4워커와 예산/오류/메모리/두 heldout 회귀를 포함해 검증한다. 최소 두 사전 지정 비자명한 조건에서 전체 compute 5% 이상과 비율 구간 0.95 아래를 목표로 한다. 목표는 관측된 이득이 아니다.
