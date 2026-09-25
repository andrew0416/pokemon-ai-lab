# Pokémon AI Lab — 로컬 더블 실험실

위치: `D:\pokemon-ai-lab`. Python 3.12 / Node 22. 챔피언스 M-C 더블을 기본으로 합니다.

## 바로 실행

PowerShell에서:

```powershell
cd D:\pokemon-ai-lab
.\run.ps1 doctor
.\run.ps1 validate teams\gravity-original.json
.\run.ps1 calc scenarios\armor-cannon-vs-rillaboom.json
.\run.ps1 battle --p1 vgc_myopic --p2 random --games 2
.\run.ps1 battle --p1 vgc --p2 vgc_myopic --games 4
node scripts\calibration.cjs
.\run.ps1 checks
.\run.ps1 experiment experiments\paired-team-screen.json
```

`battle`은 실제 Showdown BattleStream에서 6마리 중 4마리를 고르는 더블배틀을 끝까지 실행합니다. 계정·웹서버가 필요 없습니다. `server`는 별도로 127.0.0.1:8765에 로컬 Showdown 서버를 띄웁니다(Ctrl+C 종료).

팀을 바꾸려면 `--team1 teams\gravity-revised.json --team2 teams\psyspam-popular.json`을 지정합니다. `--seed 12345`로 반복 실험을 재현합니다. 게임 수는 자리 교환 쌍을 만들기 위해 짝수입니다. `--max-decisions` 초과는 패배/무승부 대신 실패입니다.

## 연결된 구성

- `vendor/pokemon-showdown`: 공식 최신 소스의 Champions M-C 시뮬레이터. 정확한 커밋은 실행 manifest에 기록.
- `vendor/pokemon-vgc-ai`: Nolelle의 Champions 더블 봇. `vgc_myopic`은 1턴 평가, `vgc`/`vgc_shallow`는 상류 구현의 탐색 기준선. 챔피언스 데이터와 별도 내장 데미지 평가기를 사용합니다.
- `poke-env`: 더블 관측 파싱, 행동 형식, random/maxpower/heuristic 기준선.
- `@smogon/calc` 0.12.0: 독립적인 데미지 계산기. **Generations.get(0)가 Champions**, 9는 일반 9세대입니다. 내장 봇 계산기를 이 라이브러리로 바꾼 것은 아닙니다. 독립 계산과 Showdown 실제 결과를 교차 검증합니다.
- `vendor/foul-play`: 싱글 전용 참고 소스. Rust 엔진 설치는 MSVC linker 미발견으로 완료되지 않았습니다. 더블 실행에 필요하지 않습니다.
- `vendor/vgc-bench`: 더블 연구·학습용 참고 소스. 학습 의존성/가중치는 설치하지 않았습니다. 자체 poke-env 포크와 고정 Showdown을 요구하므로 주 환경과 섞지 않습니다.

등록된 더블 봇은 `random`, `maxpower`, `heuristic`, `vgc_myopic`, `vgc`, `vgc_shallow`, `vgc_horizon`입니다. 실험실에서 학습된 새 모델/챔피언급 실력을 주장하지 않습니다.

## 팀·데이터의 의미

`gravity-original.json`은 첨부 파티에서 밀로틱을 하이드로펌프/얼다바람으로 변경한 버전입니다. **성격·Stat Point 배분은 화면에 없어서 테스트용으로 가정했습니다.** `gravity-revised.json`은 어흥염/고릴타 제안안이며 추천을 검증하기 위한 실험군입니다. `psyspam-popular.json`은 공개 집계에서 인기 순위 1위로 확인한 비+사이스팸 6마리 조합입니다. Wolfe의 M-C Kickoff Cup 6위 공개 팀시트(갑주무사/에써르♀/포푸니크/펠리퍼/가디안/대쓰여너)에서 기술·도구·특성·성격을 그대로 가져왔습니다. **공개되지 않은 Stat Points만 실험용으로 가정했습니다.** 원문 세팅은 `teams/psyspam-wolfe-ots.txt`, 출처·집계 범위·가정은 `teams/psyspam-popular.provenance.json`에 분리했습니다. 특정 사이트의 인기 집계이며 전 세계 공식 랭크 사용률 1위라는 뜻은 아닙니다.

JSON의 `evs` 키는 Showdown 호환 이름일 뿐, Champions에서는 **능력치당 최대 32, 합계 66의 Stat Points**입니다. 252/510 EV를 넣으면 안 됩니다. 팀은 매 실행마다 공식 M-C validator를 통과해야 합니다.

## 결과와 정보 경계

`runs/<timestamp>/`에 버전/팀 해시/규칙/시드가 담긴 manifest, 승패, 자리 교환 쌍별 점수, 선택 행동, 의사결정 trace, 각 플레이어 시점 로그를 저장합니다. 두 정책은 자기 시점의 관측만 받습니다. 상류 direct runner의 private-root attachment는 사용하지 않습니다. 로그에는 자기 포켓몬의 비공개 정보가 포함될 수 있으므로 p1/p2 시점 파일을 구분합니다.

봇 예외 후 random fallback은 실패 처리합니다. 계산기 수치는 **기술이 명중했을 때의 피해량**이고, 선후공·날따름·집중 공격·상태이상까지 증명하지 않습니다. 상대 능력치 추정은 상류 프로젝트의 M-B priors가 일부 사용됩니다. 이는 M-C 최적 상대 모델이 아닙니다.

자리 교환 쌍 단위 bootstrap 구간은 고정 매치업의 진단일 뿐입니다. 2~4게임은 설치 점검입니다. 팀의 강함을 비교하려면 별도 팀 holdout, 여러 상대 정책, 충분한 시드, 팀 단위 군집 구간이 필요합니다.

새 방법론 설계는 `METHODOLOGY.md`, 실행 가능한 비교 계획은 `experiments/paired-team-screen.json`을 보세요.

## 출처

- https://github.com/smogon/pokemon-showdown (MIT)
- https://github.com/smogon/damage-calc (MIT)
- https://github.com/Nolelle/pokemon-vgc-ai (원본 LICENSE 확인)
- https://github.com/hsahovic/poke-env (MIT)
- https://github.com/pmariglia/foul-play (원본 LICENSE 유지)
- https://github.com/cameronangliss/vgc-bench (MIT)

원본 vendor 코드는 별도 Git 저장소로 유지합니다. 이 실험실은 경로 어댑터와 실행·증거 수집 계층을 제공합니다. 외부 서비스나 공개 래더에는 자동 접속하지 않습니다.

## 재설치와 검증

현재 설치는 바로 실행할 수 있습니다. 같은 버전을 다시 설치하려면 Node 22, Git, uv가 있는 PowerShell에서 `.\setup.ps1`을 실행합니다. `source-lock.json`, `package-lock.json`, `requirements-doubles.lock`에 버전을 고정했습니다. 새 폴더에서의 전체 재설치는 별도로 검증하지 않았습니다. 기존 vendor 커밋이 다르면 보존하고 중단합니다.

`checks`는 로컬 규칙 검사와 상류의 능력치/피해 ground-truth 검사를 실행합니다. 상류에 남은 M-B 문자열 고정 검사 하나는 제외하고 실제 M-C 모드·더블·4마리 선출·팀 합법성 검사로 대체했습니다. 나머지 실패는 그대로 실패로 보고합니다. 새 실험의 모든 검사가 통과해도 봇의 전략적 판단이 올바르다는 보장은 아닙니다.

사이스팸 출처:
- https://agentrotom.com/ (공개 표본의 인기 점수)
- https://agentrotom.com/meta/gardevoir (6마리 조합 66 lists / 1,152 sightings, 2026-09-20 조회)
- https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/wolfey/teamlist (실제 세팅)

## 업데이트: 사진 기준 파티와 모래팟

`gravity-original.json`은 2026-09-20 추가 사진의 실제 성격·배분·기술을 반영했습니다. 깜까미/밀로틱은 대담 HP32/방어32/특방2, 가디안은 조심 HP32/방어10/특공24, 리자몽은 조심 HP2/특공32/스피드32, 애프룡은 고집 HP2/공격32/스피드32, 파밀리쥐는 명랑 HP1/공격32/스피드32입니다. 두 엔진의 36개 능력치가 사진과 일치합니다. 밀로틱 냉동빔이 기준이며 `gravity-icy-wind.json`은 얼다바람만 바꾼 비교안입니다. 과거 가정 배분은 `gravity-assumed-v1.json`으로 보관합니다. 이전 실험 수치는 새 기준 파티의 결과가 아닙니다.

`sand-owen.txt`: Owen의 M-C 모래팟. 보만다/에써르♂/포푸니크/타부자고/마기라스/몰드류. 도구·기술·성격·Stat Points 모두 공개 원본을 보존했습니다. 흔히 쓰인 모래+에써르 변형이며 가장 많이 쓰인 모래 6마리 조합이라고 주장하지 않습니다.
출처: https://pokepast.es/605ab0da3552400d

```powershell
.\run.ps1 battle --team2 teams\sand-owen.txt --p1 vgc --p2 vgc_myopic --games 10
.\run.ps1 suite experiments\psyspam-and-sand.json
node scripts\verify_baseline.cjs
```

suite는 사이스팸/모래 각 상대에 대해 원본/변경안을 같은 시드·자리 교환으로 비교합니다. 결과는 매치업별로 유지합니다. `gravity-revised.json`의 어흥염/고릴타 변경안은 확정 개선안이 아니며 교체 포켓몬의 배분은 실험 가정입니다.
