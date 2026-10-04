S26e 후보 1: 완성 상태 직접 전달

S12 최적화본 `081124aaae5e0fe5aa1d8c5b9117803abd866875`에 대한 독립 실험이다. 기존 전체 열거/성장 탐색과 같은 게임·정책·오차 인증·작업량을 유지하면서 전투 결과의 표현만 변경한다. 기본 OFF이며 일반 x86-64 CPU를 사용한다.

기존 CFR 연결부는 엔진이 이미 완성한 상태에서 변경 명령을 만들고, 부모 상태를 복사한 뒤 명령을 적용하여 같은 자식 상태를 다시 만든다. `experiment-owned-endings`는 P9의 완성된 배치를 소비하여 상태와 교체 이어가기 정보를 소유권으로 전달한다. 새 `experiment-owned-transitions` 경로에서만 이 API를 사용한다.

- `EngineDomain::transitions_owned`: 일반 Turn + flat enumeration에만 적용한다. 나머지 단계·factored 경로는 기존 `Domain::transitions`로 돌아간다.
- `FinalStates::into_owned`: 모든 분기 성공 및 확률 병합 뒤에만 결과를 전달한다. State/예약 큐를 복제하거나 변경 명령을 만들지 않는다.
- `tree::engine::build_owned`: 전체 트리 구성에 적용한다. 풀이기는 S12의 cached CFR를 그대로 사용한다.
- `tree::engine::growing_owned`: 기존 `reuse::Settings`를 그대로 받는다. 관측 허용 필드, 완전 회상, 확장 순서, 중단/실패 계약과 풀이 주기를 변경하지 않는다.
- 기존 `build_writing`, `growing_reusing`, `Domain::transitions`는 기준 경로로 유지한다. 기존 평가기와 배틀 규칙에는 변경이 없다.

차등 검사는 전체 숨은 상태·확률 비트·분기 순서·상태 해시·Suspension을 비교한다. 8개 기존 fixture × 난수 모드 3개 × 좌석 2개 = 48설정, 766개 결과를 비교했으며, 기절 교체/턴 중 교체/기믹 비허용/불법 입력/미지원 상태/예산 중단을 별도로 검사한다. Hyper Beam fixture는 setup에서 명중하여 실제 재충전 휘발이 있는 분기를 명시적으로 고른다. 이는 새 48개 독립 국면을 생성했다는 뜻이 아니다.

활성화 검사에서는 single-hit의 271개 결과와 7,315개 변경 명령 생성이 후보에서 0개가 되며, 271개 상태를 직접 받는지 확인한다. FactoredScope(true)에서는 기준의 명령 생성 경로가 그대로 실행되는지 확인한다. 관측용 기능은 시간 측정 바이너리에서 제외한다.

로컬 실행은 비최적화 로직 검사만 한다. 성능은 `engine/benchmarks/s26_owned/run.py`로 GitHub에서만 측정한다. S12의 6국면을 전체 열거와 성장 탐색으로 각각 비교하며, 같은 실행파일에서 후보만 OFF/ON으로 바꾼다. 비교마다 ABBA/BAAB 9블록, 한 논리 CPU, flat enumeration을 명시적으로 고정한다. 승률은 평가하지 않는다.

구현 초기 테스트 구성 오류도 보존한다: scenario Position에 없는 suspension 필드 접근을 수정했고, recharge fixture의 명중 setup 분기를 골랐으며, 처음부터 교체 단계인 ko-replace 대신 실제 턴 결과로 KO가 발생하는 memento-final-gambit을 사용하여 소유권 전달 후 교체 이어가기를 검사했다. 엔진 결과 불일치를 기대값 변경으로 덮은 수정은 없다.

GitHub 실행 `37083242086`은 성공했다. 고정 소스는 `ec9b72e149b9d72e762e4023a84365e3f72a2ff5`이며 `source/`는 이후 수정하지 않는다. 원시 108블록·432표본·882회 계산과 신뢰구간을 다시 검산했다. 기존 S12 전체 적용본 대비 성장 탐색 1.60~4.47%, 전체 열거 1.48~3.47% 추가 시간 감소이며 12설정 모두 같은 결과·같은 작업량이다. 세부 표는 `RESULTS.md`, 검산 및 소스/바이너리/artifact 식별자는 `AUDIT-37083242086.json`과 `publication/receipt.json`에 있다.

유지 후보로 기록한다. 한 러너에서 관측한 작은 추가 개선이므로 다른 CPU와 더 넓은 국면에서 같은 감소율을 보장하지 않는다. 메모리 최고 사용량은 측정하지 않았다. 기존 생산/R1/P15/P11 상태는 변경하지 않는다.
