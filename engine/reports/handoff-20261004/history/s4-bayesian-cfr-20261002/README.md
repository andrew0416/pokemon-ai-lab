# Bayesian matrix + CFR 실험

우리의 전략은 세계 간 공유하고, 상대 전략은 숨은 세계별로 분리하는 한쪽 비공개정보 게임이다.
라이브러리, 명시적인 posterior 갱신, 한 턴 Full 엔진 연결과 CLI를 구현했다.

- 결과·한계: [RESULTS.md](RESULTS.md)
- API 계약: [CONTRACT.md](source/engine/benchmarks/s26_bayesian/CONTRACT.md)
- 풀이: [bayesian.rs](source/engine/search/src/bayesian.rs)
- 엔진 연결: [engine.rs](source/engine/search/src/bayesian/engine.rs)
- CLI: [lab-bayes.rs](source/engine/search/src/bin/lab-bayes.rs)
- 실제 엔진 요청 예: [request.json](validation/cli-engine/request.json)
- 검산 영수증: [AUDIT-37008543602.json](AUDIT-37008543602.json)

커밋 `90901a6bd430a560b240379d985ba3b1c8e2df09`, 기본 OFF, 생산 미적용.
다중 턴 PBS/GT-CFR 구현과 속도·실전 품질 평가는 이 단계의 완료 주장에 포함하지 않는다.
