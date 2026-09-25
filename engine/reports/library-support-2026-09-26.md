# 라이브러리 더블 팀 지원 현황 (자동 생성)

`cargo run -p lab-scenario --release --bin lab-library -- --out <이 파일>`로 만든다. `teams/library/doubles/m-c/*/team.json` 28팀을 VGC 형식(`gen9championsvgc2026regmc`, 팀 프리뷰에서 4마리 선출)으로 고정 상대(무효 특성·도구 없음, 매 턴 방어)와 붙인다. 팀마다 선출 `1234`·`3456`·`5612`(모든 멤버가 한 번씩 선두)에서 (1) 배틀 시작 등장 효과, (2) 선두 둘이 방어(없으면 첫 기술)하는 첫 턴, (3) 선두 각각의 기술 4개, (4) 메가진화 가능한 선두의 메가진화를 엔진으로 실행한다. 이유는 엔진이 낸 `TurnError::Unsupported` 문자열 그대로이고, 묶을 때 앞의 `<포켓몬>: `만 뗐다. 벤치 멤버의 등장·교체, 선택하지 않은 기술 조합, 둘째 턴 이후는 검사하지 않는다. 직접 편집하지 않는다.

## 요약

- 팀 28개, 로더 통과 28개.
- 세 선출 모두 시작 + 방어 턴이 실행되는 팀: 28개 (거부 0개).
- 선출 84개(팀 × 3) 중 시작 + 방어 턴이 실행되는 선출: 84개.
- 모든 검사(기술·메가진화 포함) 통과: 28개.

## 거부 이유 (팀 수순)

| 이유 | 팀 수 | 팀 (포켓몬) |
|---|---:|---|

## 팀별

| 팀 | 라이브러리 상태 | 시작+방어 턴 | 전체 | 거부 검사 수 | 불법 선택 | 첫 거부 |
|---|---|---|---|---:|---:|---|
| balance-ddee | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| coaching-panda | validated | 통과 | 통과 | 0 | 0 |  |
| crown-cecil9 | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| crown-eternalton | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| crown-ryukeivgc | needs-review | 통과 | 통과 | 0 | 0 |  |
| crown-tachyon112358 | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-aveornot | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-balmung | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-beedrillvgc | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-conkledonk | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-gerard | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-gwendolyte | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-hollowedhollowed | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-jhinting | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-joshawott | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-karlin22 | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-prongs | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-sableyevgc | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-shadezero | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-thepostmanp | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-thosewhoknow | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| kickoff-wolfey | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| perish-mrada | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| psy-cona | validated | 통과 | 통과 | 0 | 0 |  |
| psy-lello | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| psy-nihat | source-complete-sp-unknown | 통과 | 통과 | 0 | 0 |  |
| psy-sand-udon | validated | 통과 | 통과 | 0 | 0 |  |
| sand-owen | validated | 통과 | 통과 | 0 | 0 |  |
