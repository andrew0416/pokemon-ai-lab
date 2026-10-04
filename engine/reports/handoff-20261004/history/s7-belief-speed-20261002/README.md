# S26c 속도 측정

엔진·탐색 소스는 변경하지 않고 같은 릴리스 실행파일에서 기존 전체 계산과 선택적 확장을 비교했다.

- [속도 결과](RESULTS.md)
- [측정 계약](source/engine/benchmarks/s26_speed/CONTRACT.md)
- [원시 데이터 검산](AUDIT-37025196260.json)
- [측정 계획](source/engine/benchmarks/s26_speed/plan.json)

GitHub 실행 `37025196260` 성공, 측정 commit `5b79d6e30b1fc0c39da227700ca3b169b60691b2`.
기본 OFF, 생산 코드 불변. 부분 계산의 시간 감소와 동등 깊이 계산의 비용은 따로 표시했다.
