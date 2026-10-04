# 다중 턴 정보집합 CFR 실험

다중 턴·중간 교체·기절 후 교체를 처리하며, 같은 관측과 자신의 행동 기억을 가진 결정은
전략을 공유한다. 공개/개인 posterior와 숨은 세계를 받지 않는 continuation cursor를 제공한다.

- [구현 및 검증 결과](RESULTS.md)
- [API·관측 계약](source/engine/benchmarks/s26_tree/CONTRACT.md)
- [CFR와 최선 응수](source/engine/search/src/bayesian/tree.rs)
- [정보집합 트리와 Cursor](source/engine/search/src/bayesian/tree/builder.rs)
- [엔진 관측 모델](source/engine/search/src/bayesian/tree/engine.rs)
- [CLI](source/engine/search/src/bin/lab-belief.rs)
- [실제 2턴 요청](validation/engine-cli/two-turn-hidden-stats-request.json)
- [검산 영수증](AUDIT-37014537106.json)

기본 OFF. 현재 엔진 연결은 명시적인 경계 상태 관측 추상화이며 완전한 전투 로그 모델이 아니다.
커밋 `12a28a527d6b67856f77b2de4f077b9cf4b41d90`, CI `37014537106` 성공.
