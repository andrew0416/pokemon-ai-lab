# S26c 야간 최적화 최종 후보

[전체 결과와 유지 판단](NIGHT-RESULTS.md) · [마지막 원시자료 검산](AUDIT-37034440983.json) · [관측 계약의 정적 비교](SOURCE-PARITY.json).

최종 source `081124aaae5e0fe5aa1d8c5b9117803abd866875`, 브랜치 `codex/s26c-writer-20261003`.

기존 성장 탐색의 같은 계산을 10.4~24.5%, 기존 전체 열거의 같은 계산을 7.4~13.3% 단축했다. 전체 깊이를 반드시 끝까지 계산한다면 여전히 전체 열거 경로가 유리한 국면이 많다. 승률은 평가하지 않았다.

`experiment-belief-workspace` 기능은 기본 OFF다. 기존 `tree::solve`, `tree::engine::build`, `tree::engine::growing` 경로가 남아 있다. 일반 x86-64 release로 측정했으며 AVX2 전용 명령을 추가하지 않았다.

실험 라이브러리 호출은 다음과 같다.

- 전체 열거: `tree::engine::build_writing(...)`으로 구성한 트리를 `tree::workspace::cached::solve(&tree, config)`로 푼다.
- 성장 탐색: `tree::engine::growing_reusing(...)`에 `reuse::Settings`를 전달한다. 결합본의 `storage`는 `in_place`, `workspace`, `compiler`, `static_values`, `direct_write`가 모두 `true`다.
- `static_values=true`는 자체 작업배열을 쓰는 cached solver를 선택하므로 `workspace` 설정보다 우선한다.
- 개별 기능 비교: `lab-belief-workspace-speed`의 JSON `kind`는 `growing`, `writer`, `all-cached`, `all-writer`, `exhaustive`, `exhaustive-writing` 등을 지원한다. 입력 예시는 `source/engine/benchmarks/s26_writer/plan.json`에 있다.
- 실행: `cargo build --release -p lab-search --bin lab-belief-workspace-speed --features experiment-belief-workspace`. 성능 수집 스크립트는 GitHub runner에서만 작동하며 로컬 `--check`는 시간 측정 없이 결과 일치만 검사한다.

관측 허용 필드는 하나의 `snapshot_impl`에서 공유한다. 직접 기록 경로는 임시 `format!` 결과 대신 같은 문자열에 `write!`한다. 새로운 기믹의 관측 필드를 추가할 때 기준/후보가 함께 바뀐다. 불법 정보나 전체 State의 Debug 값을 관측 키로 추가하지 않았다.

5회 GitHub 비교, 1262 paired blocks/5048 timing samples, 각 후보의 독립 LP 145개 기준 문제 재검증, 마지막 ON 88/OFF 17/CLI 1 검사 통과. 새로운 독립 문제 수와 반복 실행 수를 구별했다. 기존 생산 코드와 공유 체크아웃은 변경하지 않았다.

실험 소스는 동결했다. `prepare.py`의 최초 준비 도중 발생한 로컬 구성 오류와 `resume_prepare.py`, 검증 이력도 보존했다. 최종 검증 대상은 `source/`와 `publication/receipt.json`의 해시다.
