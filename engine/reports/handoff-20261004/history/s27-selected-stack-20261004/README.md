# S27 유지 조합의 직접 누적 비교

완료: [GitHub 37173941021](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/37173941021), 352초. 소스 `8c1bd43c0589ea74b99de439f38ae6f24cf87fd5`.

S24 OFF 대비 S25 유지 5개 옵션 ON은 9조건에서 8.00~14.83% 짧았다. 작은 메가/회복 조건은 0.273→0.292ms로 8.03% 길어졌다. 10조건 모두 계산 결과와 탐색량이 일치했다. 프로젝트 초기부터의 모든 최적화 효과를 측정한 것은 아니다.

- [전체 수치·검증 범위](RESULTS.md), [원시 수치 고정](RESULTS.json)
- [다음 큰 병목과 보존 계약](DECISION.md), [구조화된 판정](DECISION.json)
- [원시 CI 산출물](publication/run-37173941021/s26l-stack-37173941021-1), [출판 소스·해시](publication/receipt.json)
- [측정 전 입력 제외 이력](INPUT-SELECTION.json), [라이브러리 불변 검사](SOURCE-PARITY.json)
- [볼트 반영 확인](vault-final/install-receipt.json), [후속 과제 등록](board-register-receipt.json)

다음 후보는 엔진의 순수 관측 생성을 기존 전이 워커에서 함께 준비하는 설계 하나다. 아직 구현하지 않았으며, 전체 compute 5% 이상 단축을 별도 실험의 목표로 잡았다. 작은 국면 회귀와 OFF 경로 비용의 원인은 열린 과제로 남겼다.

로컬은 정확성만, 시간은 GitHub에서만 측정했다. 라이브러리를 바꾸지 않아 기존 oracle·비AVX2 근거를 재사용했으며 이번에 재실행했다고 세지 않는다. 실험 기본 OFF, 생산·KEEP_R1·P15 불변, 승률 미평가. 볼트는 새 원자 노트 5개를 반영했고 기존 전역 오류 14건·오래된 참조 1069건은 그대로다.
