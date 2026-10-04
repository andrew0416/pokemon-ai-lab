# 주장과 근거의 연결

|ID|한 문장 주장|증거·재계산|판정 범위|
|---|---|---|---|
|H01|현재 공유 소스의 엔진 라이브러리는 S25와 같다.|CURRENT·LINEAGE, S27 SOURCE-PARITY·출판 patch|S27은 CLI/실험 하네스만 변경|
|H02|다섯 옵션 조합은 9/10조건에서 빨랐다.|verify.py의 direct·faster_count·faster_reduction_range|묶음 효과; 각 옵션의 개별 기여가 입증된 것은 아님|
|H03|작은 메가 조건은 느려졌다.|verify.py regressions, 원시 ABBA/BAAB|8노드·2전이·1풀이, 원인 미분리|
|H04|OFF 비용 문제는 해결됐다고 할 수 없다.|verify.py off_repeat, S26 version control|+2.61%, 구간은 1보다 높지만 A/A 밖으로 완전히 벗어나지 않음|
|H05|측정한 10조건의 최종 tree·solution·작업량은 같았다.|정규화 지문/진단, 매 sample full 지문|paper/pipeline 내부 카운터만 정규화; hash 충돌 불가능의 증명은 아님|
|H06|속도 단위는 warmed compute 시간이다.|공통 CLI timed/worker, drop-before-ack|입력·pool 생성·지문·JSON·반환물 파괴 제외|
|H07|단축률은 쌍별 비율의 중앙값이다.|원시 9개 블록과 독립 statistic 계산|양쪽 시간 중앙값을 나눈 값과 다를 수 있음|
|H08|A/A는 설명용 흔들림 기준이다.|같은 경로 21비교 및 4000 bootstrap 재표집|단일 VM; 95% 구간 다중 비교 미보정|
|H09|반복 sample 수는 새로운 게임 수가 아니다.|plan의 10설정/9개 고유 최상위 시나리오 경로와 1152측정의 구별|두 heldout도 추가된 제한된 국면일 뿐|
|H10|반환물 파괴는 다음 worker 계산과 겹치지 않는다.|CLI drop(result) 뒤 emit|그 파괴 시간까지 포함하는 성능은 미측정|
|H11|19개 phase는 호출자 wall의 배타 분해다.|profile raw에서 합계==compute|전이에는 작업 제출·대기 포함, worker CPU 합이 아님|
|H12|관측 단계는 주요 병렬 조건에서 13.8~23.2%다.|verify.py phases|별도 계측 build의 진단치|
|H13|관측 병렬화의 상한 계산은 예측값이 아니다.|f*(1−1/k), k=min(pool,4,총전이)|작은 메가 k≤2, 상한 4.67%; 실제 이동 가능 비율·불균형 미측정|
|H14|다음 후보는 엔진 전용 관측 배치다.|현재 worker/observer 호출 경로, NEXT-CANDIDATE|설계만 선택, 미구현·미측정|
|H15|병렬화는 원래 오류 소비 순서를 유지해야 한다.|builder의 확률검증·push·관측·phase 순서|일반 stateful 콜백은 직렬; speculative 오류 조기 반환 금지|
|H16|미해결 회귀가 있어 생산 채택하지 않았다.|CURRENT open_regressions, S27 DECISION|기본 OFF·scalar 유지·승률 미평가|
|H17|S24/S25 oracle·비AVX2 증거는 재사용했다.|고정 library와 과거 archive, SOURCE-PARITY|이번 handoff에서 재실행한 테스트 수로 합산 금지|
|H18|S26 경계 복사 변경은 선택 조합에 없다.|S26r2 decision, S27 parent가 S25|기존 5개 위 −1.14~+1.08%, 8구간 모두 1 포함|
|H19|과거 증거의 해시는 그대로 두고 원본을 복구했다.|historical-source-blobs/RECOVERY.json|현재 코드 검증으로 격상하지 않음|

같은 결론에 도달한다는 것은 이 제한된 주장과 반례를 재현한다는 뜻이다. 관측 병렬화의 미래 이득이나 모든 기믹의 정확성을 미리 확정한다는 뜻이 아니다.
