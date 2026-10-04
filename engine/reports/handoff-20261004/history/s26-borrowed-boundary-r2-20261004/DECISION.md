# S26k 경계 조회 판정

상태: `do-not-promote`. 기본 OFF, 생산 미채택.

경계 조회 복사 생략의 기존 5개 옵션 대비 시간 단축률은 -1.14~+1.08%였다. 8조건 중 비율 구간이 1 아래인 경우는 0개이고 A/A 범위까지 벗어난 개선은 0개다.

24개 후보 비교에서 개선을 확정할 조건이 없었습니다. 할당 감소는 확인됐지만 실제 시간 이득으로 이어졌다는 근거가 부족하므로 누적 유지 조합에 추가하지 않습니다. 기본 OFF 실험 코드와 아이디어만 보존합니다.

## 이전 버전 비용

S24→S25 OFF 단축률 -4.49~+0.74%, A/A 밖 개선 0/8·회귀 1/8; S25→S26 OFF 단축률 -1.83~+1.57%, A/A 밖 개선 0/8·회귀 0/8; S25→S26 전체5개(경계 OFF) 단축률 -1.61~+2.26%, A/A 밖 개선 0/8·회귀 0/8. 구체적 회귀 관측: s24-to-s25-off narrow-w1-d3-cadence1 시간 4.49% 증가. 원인과 다른 세션의 반복성은 미확정이다.

비율은 후보/기준이다. OFF 비용은 컴파일·layout 변화까지 포함하며 특정 소스 수정만의 인과 효과가 아니다. 이 비교와 별도 블록의 ON 수치를 곱해 누적 가속률로 보고하지 않는다.

## sand 메모리

plain의 S25 이력 추가 RSS 변화 -0.13% (후보/기준 비율 구간 0.9638–1.0161, A/A 밖 False); instrumented의 S25 이력 추가 RSS 변화 -1.96% (후보/기준 비율 구간 0.9614–1.0336, A/A 밖 False). 이전 +5.58% 증가가 이번 세션에서는 재현되지 않았다. 증가가 없음을 증명한 것은 아니다.

12블록씩의 fresh-process 대조다. 일반·계측 빌드를 합치지 않으며 RSS 변동만으로 allocator fragmentation 또는 누수를 단정하지 않는다.

## 검증 범위

122/118/17 테스트, 1,537쌍 전체 JSON, 독립 인증 3,405건 중 cap 12건의 수치 일치, 56개 엔진 구성·중간 상태·콜백·rollback·비AVX2 에뮬레이션을 통과했다. 유한 corpus이며 승률은 평가하지 않았다.

첫 실행은 공유 Cargo target의 바이너리 재사용으로 속도 측정 전에 실패했다. 라이브러리 바이트를 보존한 별도 브랜치에서 target을 분리했고 실행파일 구분 검사를 추가한 성공 실행만 결과로 채택했다.

[전체 결과](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s26-borrowed-boundary-r2-20261004/RESULTS.md>) · [기계 조회](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s26-borrowed-boundary-r2-20261004/DECISION.json>)
