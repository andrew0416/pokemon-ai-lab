# 증분 이력 키: 기본 OFF 유지 후보

8개 조건 모두에서 속도가 개선됐다. 새 옵션 단독 8.21~14.42%, 기존 네 옵션에 추가할 때 9.70~14.24% 단축이다. 서로 다른 기준의 수치를 더하지 않는다. 16개 후보/기준 비율 구간 모두 1 아래이며 같은 API A/A의 설명용 범위도 벗어났다.

정확성·콜백 순서·예산 롤백·비AVX2 에뮬레이션 검증을 통과했다. 이 결과는 유한 표본의 근거이며 모든 미래 기믹의 완전 증명은 아니다. 승률은 평가하지 않았다.

할당 수가 감소했지만 최대 요청 heap은 거의 그대로다. sand 결합 구성의 RSS 중앙값은 124.53→131.48 MiB로 약 5.58% 증가했다. 원인이나 반복성을 단정하지 않고 후속 후보로 기록했다.

구현·실험·문서화는 완료했다. 생산 채택, 기존 커밋 대비 OFF 경로의 동일 러너 비용 비교, RSS 재확인, 관측/경계 등록의 다음 최적화는 수행하지 않았다. 기본 OFF이며 생산 엔진은 그대로다.

[전체 결과](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s25-history-keys-20261003/RESULTS.md>) · [기계 조회](<C:/Users/admin/Documents/ChatGPT/pokemon-ai-lab/search-experiments/s25-history-keys-20261003/DECISION.json>)
