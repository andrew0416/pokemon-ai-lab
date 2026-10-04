# 선택적 정보집합 CFR 실험

S26b의 유한 트리 탐색에 공개 이력 단위의 선택적 확장, PUCT/평균 정책 혼합 선택,
필수 교체 처리와 예산 중단 롤백을 추가했다. 기본 OFF.

- [결과와 범위](RESULTS.md)
- [알고리즘·사용 계약](source/engine/benchmarks/s26_tree/GROWING.md)
- [구현](source/engine/search/src/bayesian/tree/builder/growing.rs)
- [실제 국면 요청](validation/engine-cli/two-turn-one-continuation-request.json)
- [검산 근거](AUDIT-37020601917.json)

커밋 `26999d068dd541ac0f2cfcdafc8a091ce4427e12`, CI `37020601917` 성공.
엔진 전체 전투 로그 관측, 학습 CFV, 안전한 재탐색, 속도·승률 평가는 남아 있다.
