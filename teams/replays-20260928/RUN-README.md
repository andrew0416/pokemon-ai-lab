# V13-replay-parity: 공개 리플레이 국면 (2026-09-28, Opus VC)

## 수집
- 출처: `https://replay.pokemonshowdown.com/search.json?format=gen9championsvgc2026regmc&page=N` (51개/쪽, 무인증) → 각 `<id>.json`. 요청 간격 1 s. 2026-09-28 최신순 200개(업로드 2026-09-28, `replays.json`/`sources.json`에 id·URL·업로드 시각·수집 시각). 플레이어 이름은 로그에서 p1/p2로 치환, 채팅·타이머·레이팅 줄 삭제(`engine/scripts/replay_fetch.py`).
- 오픈 팀시트(`|showteam|`, 양쪽 동의)는 200 중 4경기뿐. 오픈 팀시트도 SP는 비공개(Showdown `showOpenTeamSheets`가 evs/ivs를 null로 보냄) → **정확 세트 복원이 가능한 경기는 0**.

## 복원 (`engine/scripts/replay_parse.py` → `games/`, 커밋 사본 `teams/replays-20260928/`)
- 세트: OTS면 종·도구·특성·기술·성격, 아니면 로그 공개분(사용 기술, 표시된 도구/특성) + 가정(비공개 특성 = 입장 메시지 없는 첫 특성, 비공개 도구 없음, 스크린/날씨/필드가 5턴 넘게 유지되면 빛의점토/축축한바위류/그라운드코트, Trick 원래 도구 역산, 행동 안 한 턴용 Rest 채움, 미등장 선출은 프리뷰 종 자리표시 세트). SP는 H32/공격32/S2에서 시작해 `lab-replay --fit-sp`가 게임마다 로그 일치로 맞춤(8 프리셋 × 성격, 20 s 예산).
- 행동: 턴 첫 기술 전 교체 = 선택 교체, 이후 교체 = 턴 중 교체(유턴·탈출버튼·부활의축복 포함), 기술 줄의 대상(표시 대상; 불일치면 같은 진영 대상으로 완화 = tier 2), 메가, 행동 없음(`cant`·선기절·Champions 앙코르 교체)은 `unknown`(합법 기술 전부 후보). 경기 중단: 일루전·Ally Switch·변신(5경기).
- 199경기 1,777 결정(턴 1,297 + 교체), `unknown` 행동 628, 턴 중 교체 85.

## 재생·핀 (`lab-replay`, engine/search/src/bin/lab-replay.rs)
각 결정에서 로그 행동과 맞는 합법 선택 쌍을 모두 열거(Full 난수, 무거운 18경기는 Extremes; `LAB_ENGINE_FACTORED=1`)하고 로그 관측(HP %·기절·상태·자리·메가·랭크·알려진 도구·날씨·필드·유사날씨·진영 조건)에 가장 가까운 결과를 핀으로 고름. 결정마다 핀 시나리오 1개(`positions/`, 팀 인라인, `setupStates` 핀, `setupRolls` full|extremes).
- 재생된 결정 1,649 / 1,777 = **핀 국면 1,649개**. 구조 일치(모델 ② 관측 일관성): 첫 불일치 전 1,570, 전체 1,596; 끝까지 일치 169/199경기. tier 2 71결정.
- 재생 중단 사유: 교체 결정인데 엔진 국면은 턴(7) 등 — 대부분 앞선 기절 불일치(가정 SP·난수 누적)의 결과.

## (b) 관측 불일치 후보 30건 (첫 구조 차이, 게임당 1건)
전부 **미분류 후보**다. 원인 1순위는 가정(SP·비공개 특성/도구·HP 누적 오차)이다(예: 2689030716 Serperior 리프스톰 뒤 spa +2 = 공개되지 않은 심술꾸러기). 엔진 버그로 확인된 것은 없다. 아래 표는 재현 핀(`positions/<id>.sNN.json`, NN = 표의 결정 번호 앞 단계)을 붙여 보드에 올릴 재료다.

| replay id | 턴 | 첫 구조 차이 |
|---|---|---|
| 2689025807 | 4 turn | p2:Dragonite boosts engine {} log {"atk":-1} |
| 2689027554 | 1 turn | p1:cotton bro bro item engine "choicescarf" log "lightclay"; p2:Grimmsnarl item engine "lightclay" log "choicescarf" |
| 2689028900 | 4 turn | p1:Pelipper faint engine false log true (engine hp 167/167, log 0%) |
| 2689029391 | 2 turn | p2:Farigiraf status engine "slp" log ""; pseudoWeather engine [] log ["trickroom"] |
| 2689030716 | 6 turn | p2:Serperior boosts engine {} log {"spa":2} |
| 2689032684 | 6 turn | p1:baby ♥ faint engine true log false (engine hp 0/181, log 11%) |
| 2689032994 | 5 turn | p1:Archaludon status engine "slp" log ""; p2:Milotic boosts engine {"accuracy":1,"atk":1,"def":1} log {"accuracy":1,"atk":1,"def":1,"spa":1} |
| 2689034563 | 2 turn | p2:Rillaboom faint engine true log false (engine hp 0/207, log 39%) |
| 2689037253 | 2 turn | p2:Politoed faint engine false log true (engine hp 118/197, log 0%) |
| 2689037304 | 1 turn | p2:Dragonite boosts engine {} log {"atk":-1} |
| 2689037632 | 10 turn | p2:Grimmsnarl status engine "slp" log ""; p2 conditions engine ["lightscreen"] log ["lightscreen", "reflect"] |
| 2689037834 | 6 turn | p2:Primarina faint engine true log false (engine hp 0/157, log 47%) |
| 2689038915 | 5 turn | p2:Charizard faint engine false log true (engine hp 185/185, log 0%) |
| 2689040513 | 1 turn | p2:Gallade faint engine false log true (engine hp 175/175, log 0%) |
| 2689040676 | 5 turn | p2:Corviknight status engine "slp" log "" |
| 2689040708 | 3 turn | p2:Rillaboom faint engine false log true (engine hp 75/177, log 0%) |
| 2689041342 | 6 turn | p2:Whimsicott faint engine true log false (engine hp 0/167, log 2%) |
| 2689044110 | 2 turn | p1:Indeedee boosts engine {} log {"atk":-1} |
| 2689044769 | 7 turn | p1:Grimmsnarl faint engine true log false (engine hp 0/202, log 10%) |
| 2689044888 | 2 turn | p1:Charizard faint engine false log true (engine hp 126/155, log 0%) |
| 2689046149 | 1 replacement | weather engine "" log "raindance" |
| 2689046530 | 2 turn | p1:Arcanine faint engine true log false (engine hp 0/202, log 6%) |
| 2689046674 | 1 turn | p1:Annihilape faint engine false log true (engine hp 190/217, log 0%) |
| 2689046756 | 2 turn | p1:Dragapult faint engine true log false (engine hp 0/195, log 24%) |
| 2689047732 | 4 turn | p1:Garchomp faint engine false log true (engine hp 182/215, log 0%) |
| 2689048598 | 1 turn | p1:Charizard faint engine false log true (engine hp 185/185, log 0%); p2:Archaludon faint engine true log false (engine hp 0/197, log 100%) |
| 2689049039 | 5 turn | p1:Toxicroak faint engine false log true (engine hp 190/190, log 0%) |
| 2689050772 | 3 turn | p1:Basculegion faint engine false log true (engine hp 163/197, log 0%) |
| 2689051543 | 4 turn | p1:Torkoal faint engine false log true (engine hp 177/177, log 0%) |
| 2689051600 | 7 turn | p2:Garchomp faint engine false log true (engine hp 215/215, log 0%) |

## (a) 오라클 대조
- 로컬 점검(작게): 2689024450 s04(full, 1결과)·s06(full staged, 1,470결과), 2689027554 s00(full), 2689027251 s00·2689026876 s00(extremes, 구버전 세트) — 전부 `match`(TV ≤ 5e-16).
- 러너: 잡 `engine/jobs/replays-20260928`(250국면: OTS 4경기 전 국면 + 나머지 게임별 무작위 순환, oracle full staged, 120분) 커밋 a4818c8 → Actions run 36400574774. **보고 시점 queued**(슬롯 20개 공유; api.github.com 무인증 한도 소진으로 조회도 막힘). 결과는 `python engine/scripts/replay_summary.py ingest runs/parity-replays-20260928 36400574774`로 `rows/`·`oracle.json`·`sources.json` 갱신.
- 다음 잡 후보: `pack/list-2.txt`(250: 불일치 국면 20 + 게임별 순환). 나머지 ~1,150국면은 미대조.
- 주의: 잡 1은 파서 수정(앙코르·Trick·날씨 도구) 전 국면이다. 국면 자체는 완전히 규정된 시나리오라 대조는 유효하지만 로그와는 덜 닮았다.

## 한계
- 리플레이 시드 비공개 → 정확 분포 대조는 (a)만. (a)의 세트는 가정이라 "실전 국면"은 상황(선출·필드·HP 분포)만 실전이다.
- 행동 순서·빗나감·급소 같은 사건은 관측 비교에 쓰지 않음(상태만 비교). 휘발(도발·앵콜 등) 미비교.
- 무인증 API 한도(60/h, IP 공유)로 결과 수집이 막힐 수 있다.
