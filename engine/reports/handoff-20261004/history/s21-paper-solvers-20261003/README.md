# S21 / S26f 논문 기반 1~3 구현

최종 소스는 `source/`, frozen commit은 `b10b08477f196ce0b9b361fa7878e311fdf1d4c8`이다.
실험 브랜치는 `codex/s26f-papers-20261003`이며 생산에는 적용하지 않았다.

- [결과와 판단](RESULTS.md), [기계 조회 수치](RESULTS.json), [후속 후보](DECISION.json)
- [구현 계약과 논문 대응](source/engine/benchmarks/s26_papers/README.md)
- [Actions](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37119405706)
- `publication/`: 변경 해시·공유 Git 보존 증거·runner 원본 artifact
- `validation/`: 시간 측정 없이 수행한 로컬 correctness. `final-r2` 이후 warm lambda 조정의 검증은 `focused`, `oracle-warm-final`, `check-warm-final`이다.
- `vault/`: 8개 새 원자 노트와 6개 기존 노트의 검토·설치·새 오류 0 증거

`prepare_*`, `wire_*`, `make_speed.py`, `add_sequence_plan.py`는 최초 편집 당시 기록이다.
완성된 source에 다시 실행하면 안 된다. 이 폴더와 소스를 다음 실험으로 덮어쓰지 말고,
새 폴더·브랜치에서 이어간다. 재실행 결과도 새 출력 디렉터리에 남긴다.

첫 warm 초기화의 lambda=1 고정안은 로컬 엔진 사례에서 cap까지 느리게 수렴해,
최종본에서 feasible root 경계 쪽으로 16회 이분 조정하는 방식으로 교체했다.
초기 검사의 `validation/check-final/*warm*-raw.jsonl`과 최종 검사의
`validation/check-warm-final/*warm*-raw.jsonl`을 구분한다. 전자는 최종 소스의
실측 결과가 아니며 소스 커밋이 고정되기 전 중간 검증 자료다.

남은 제안은 보드의 `S26f-sequence-confirm`, `S26f-sequence-policy-gate`,
`S26f-tight-gap-growth`에 등록했다. 이번 요청의 구현·검증 완료와 생산 채택을 구분한다.
