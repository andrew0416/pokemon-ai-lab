# 2026-10-04 작업 공유 — 여기서 시작

현재 탐색 작업본, 독립·폐기 후보의 이력, 원시 실험 근거를 보존한 임시 브랜치다. 생산 병합이나 기본 옵션 변경이 아니다. 로컬 Obsidian vault는 브랜치에 포함하지 않는다.

1. [CURRENT.json](CURRENT.json)에서 기준·후보·ON 옵션·미해결 과제를 확인한다.
2. [CLAIMS.md](CLAIMS.md)를 읽고 사실·해석·설계 가설을 구별한다.
3. 아래 명령으로 원시 측정 기록에서 S27 통계를 독립적으로 다시 계산한다.
4. [작업 이력](LINEAGE.md)과 [다음 후보의 설계](NEXT-CANDIDATE.md)를 읽고, 별도 실험 브랜치에서 이어간다.

```text
python -X utf8 -B engine/reports/handoff-20261004/verify.py --deep --output replay-result.json
```

Python 3.12 이상, 표준 라이브러리만 필요하다. 위치를 옮겨도 Windows 절대경로나 로그인 없이 동작한다. 엔진 빌드·새 성능 측정·네트워크 호출을 하지 않는다. `--deep`은 과거 41개 원시 archive의 CRC까지 검사한다. 기본 실행도 모든 보존 파일/ZIP의 SHA256과 최신 원시 sample·phase 계산을 확인한다. 다른 실행 환경의 새로운 시간 수치가 완전히 같아야 한다는 뜻은 아니다.

S27의 결과는 10조건 중 9조건에서 8.00~14.83% 시간 단축, 작은 메가/회복 조건에서 8.03% 지연이다. 모든 조건의 tree/solution/search-work 지문은 일치했다. **최근 다섯 pipeline 옵션의 직접 효과**이며 프로젝트 초기 대비 전체 가속이 아니다. 한 실행에서 여러 번 측정한 것을 독립 대전 표본으로 세지 않는다.

`history/`는 원래 결과·판정·감사 코드·소스 patch를 보존한다. 원본 문서의 로컬 절대경로는 출처 기록이며, 다른 기기에서는 [LINEAGE.json](LINEAGE.json)의 고정 소스와 `archives/`를 사용한다. 과거 감사 코드는 원형 보존용이고 일괄 실행 가능한 portable 도구라고 주장하지 않는다. 새 `verify.py`의 직접 재계산 범위는 S27이다. [ARCHIVES.json](ARCHIVES.json)은 실패 실행도 포함하므로 모든 archive를 성공 근거로 취급하지 않는다.

`wip/`는 본 엔진 `lab-engine`의 21개 변경 파일을 읽기 전용으로 잡은 별도 미커밋 스냅샷이다. 탐색 실험 소스에는 합치지 않았고 이번 공유 작업에서 새 검증도 하지 않았다. `wip/SNAPSHOT.json`의 출처 HEAD·해시를 확인한 뒤 독립 checkout에서만 복원한다.

[REPRODUCE.md](REPRODUCE.md)는 새 측정과 기존 수치 재계산을 구별한다. [VAULT-PROVENANCE.md](VAULT-PROVENANCE.md)는 원자 지식 조회 경로와 오래된 근거의 한계를 설명한다.
