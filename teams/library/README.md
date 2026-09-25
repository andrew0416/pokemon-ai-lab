# Pokémon Champions M-C 파티 자료집

수집일: 2026-09-20. 더블 28개, 싱글 17개. 공개 원본과 시뮬레이터 변환본을 함께 보존했다. 이 목록은 전체 사용률 순위가 아닌 공개 자료 표본이다.

## 사용 방법

`index.json`에서 포맷·태그·검증 상태로 고른다. 각 디렉터리의 `metadata.json`에 출처와 변환 가정, `team.json`/`team.txt`에 변환 세팅이 있다. 원본은 `source.txt` 또는 `source-cards.json`이다. 기술·도구·종족 유효성은 로컬 Champions 엔진으로 검증했다.

- 더블: 공개 SP까지 갖춘 4개가 시험 준비 상태다. 다른 24개는 SP가 미공개이며, 그중 1개는 도구명 오류도 있다.
- 싱글: 16개가 변환·검증을 통과했고 1개는 기술 오류로 제외했다. 작성자 자체 보고와 실험 후기는 대회 실적으로 해석하지 않는다.
- 레벨 50 및 미기재 IV 기본값 등은 엔진 가정이다. `simulation_ready`는 모든 수치가 작성자에 의해 확인되었다는 뜻이 아니다.
- SP 미공개 팀의 빈 `evs`를 원본의 무투자 배분으로 간주하지 않는다. 시험용 배분은 별도 파일에 가정과 목적을 기록한 뒤 사용한다.

## 확인이 필요한 원본

- `crown-ryukeivgc`: 고릴타의 도구가 `Miracle Berry`로 기록되어 현행 로컬 포맷 검증 실패. `Miracle Seed` 등으로 추측 교정하지 않았다.
- `pizza-armarouge-tr`: 원문 카드의 보스로라 `Trick Room`이 기술 검증 실패. 원본 보존, 시험 제외.
- `sarami-raichu-experiment`: 메가라이츄X 관련 실험/반성 글의 구성으로, 검증 통과가 성능이나 대중성을 뜻하지 않는다.

## 더블 시험 설계에 반영할 상대군

| 상대군 | 우선 확인할 자료 | 확인할 질문 |
|---|---|---|
| 빠른 사이스팸 | psy-cona | 생명의구슬 카디나르마가 행동하기 전에 수면을 넣을 수 있는가? |
| 비+사이스팸 | kickoff-wolfey, psy-nihat | 에써르 수면 의존도, 비 공격수와 카디나르마 처리 순서 |
| 사이스팸+모래 | psy-sand-udon, sand-owen | 몰드류·포푸니크의 앞/뒤 선출을 모두 견디는가? |
| 강한 트릭룸 | kickoff-karlin22 | 브리무음의 매직미러와 트릭룸 발동을 고려해 승리 조건을 세울 수 있는가? |
| 풀필드·보만다 | kickoff-sableyevgc, kickoff-jhinting | 아쿠스타가 고릴타 선공기와 위협 때문에 손해를 보는가? |
| 코칭·눈 | coaching-panda, kickoff-gerard | 아쿠스타의 물리 공격 성능과 악 선공기 취약성이 어떻게 작용하는가? |
| 비·혼합 날씨 | kickoff-gwendolyte, kickoff-aveornot | 쾌청을 쓰는 턴이 중력·수면보다 이득인 상황은 무엇인가? |
| 최근 균형 조합 | crown-eternalton, crown-cecil9, crown-tachyon112358 | 사이스팸 밖에서 교체가 가져오는 손익은 무엇인가? |
| 멸망의노래 | perish-mrada | 수면과 교체·멸망 턴 관리로 승리 조건을 유지하는가? |

가디안과 아쿠스타를 비교할 때 다른 다섯 마리는 고정한다. 가디안의 두 번째 최면술·특수 광역 타점과 아쿠스타의 속도·물리 물 타점·필드 제거를 각각 측정한다. 한 번의 수면 운이나 상대 실수로 교체 효과를 확정하지 않는다. 이 수집 작업에서는 대전을 추가 실행하지 않았다.

## 더블

| ID / 출처 | 구성 | 태그 | 상태 |
|---|---|---|---|
| [balance-ddee](https://play.limitlesstcg.com/tournament/6a6a3192937230b102d48538/player/ddee/teamlist) | Floette-Eternal, Sneasler, Incineroar, Rillaboom, Gholdengo, Raichu | grassy-terrain | SP 미공개 |
| [coaching-panda](https://pokepast.es/b4465e52a2df6d1e) | Rillaboom, Kingambit, Sneasler, Baxcalibur, Froslass, Basculegion | snow-or-froslass, grassy-terrain | 배분 공개·검증 통과 |
| [crown-cecil9](https://play.limitlesstcg.com/tournament/6a8096b05a30714095571417/player/cecil9/teamlist) | Arcanine-Hisui, Rillaboom, Floette-Eternal, Gholdengo, Raichu, Garchomp | grassy-terrain | SP 미공개 |
| [crown-eternalton](https://play.limitlesstcg.com/tournament/6a8096b05a30714095571417/player/eternalton/teamlist) | Raichu, Gholdengo, Rillaboom, Sylveon, Staraptor, Arcanine-Hisui | grassy-terrain, tailwind | SP 미공개 |
| [crown-ryukeivgc](https://play.limitlesstcg.com/tournament/6a8096b05a30714095571417/player/ryukeivgc/teamlist) | Sylveon, Farigiraf, Garchomp, Charizard, Kingambit, Rillaboom | sun, grassy-terrain, trick-room-option | 검토 필요·시험 제외 |
| [crown-tachyon112358](https://play.limitlesstcg.com/tournament/6a8096b05a30714095571417/player/tachyon112358/teamlist) | Garchomp, Rillaboom, Sneasler, Gholdengo, Arcanine-Hisui, Raichu | grassy-terrain | SP 미공개 |
| [kickoff-aveornot](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/aveornot/teamlist) | Charizard, Venusaur, Pelipper, Archaludon, Sneasler, Indeedee-F | psychic-terrain, rain, sun, trick-room-option, tailwind | SP 미공개 |
| [kickoff-balmung](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/balmung/teamlist) | Floette-Eternal, Charizard, Kingambit, Whimsicott, Basculegion, Garchomp | sun, tailwind | SP 미공개 |
| [kickoff-beedrillvgc](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/beedrillvgc/teamlist) | Charizard, Venusaur, Sinistcha, Incineroar, Floette-Eternal, Garchomp | sun, trick-room-option | SP 미공개 |
| [kickoff-conkledonk](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/conkledonk/teamlist) | Charizard, Garchomp, Incineroar, Venusaur, Toxapex, Kingambit | sun | SP 미공개 |
| [kickoff-gerard](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/gerard/teamlist) | Baxcalibur, Ninetales-Alola, Sneasler, Basculegion, Rillaboom, Incineroar | snow-or-froslass, grassy-terrain | SP 미공개 |
| [kickoff-gwendolyte](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/gwendolyte/teamlist) | Salamence, Sneasler, Politoed, Archaludon, Rillaboom, Swampert | rain, grassy-terrain, tailwind | SP 미공개 |
| [kickoff-hollowedhollowed](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/hollowedhollowed/teamlist) | Floette-Eternal, Raichu, Gholdengo, Sneasler, Incineroar, Rillaboom | grassy-terrain | SP 미공개 |
| [kickoff-jhinting](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/jhinting/teamlist) | Salamence, Incineroar, Rillaboom, Raichu, Farigiraf, Sneasler | grassy-terrain, trick-room-option | SP 미공개 |
| [kickoff-joshawott](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/joshawott/teamlist) | Froslass, Charizard, Incineroar, Rillaboom, Milotic, Chesnaught | snow-or-froslass, grassy-terrain | SP 미공개 |
| [kickoff-karlin22](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/karlin22/teamlist) | Indeedee-F, Camerupt, Hatterene, Armarouge, Gallade, Crabominable | psychic-terrain, psyspam, trick-room-option | SP 미공개 |
| [kickoff-prongs](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/prongs/teamlist) | Gengar, Incineroar, Sableye, Primarina, Annihilape, Tinkaton | perish-song | SP 미공개 |
| [kickoff-sableyevgc](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/sableyevgc/teamlist) | Salamence, Rillaboom, Sneasler, Arcanine-Hisui, Kingambit, Basculegion | grassy-terrain, tailwind | SP 미공개 |
| [kickoff-shadezero](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/shadezero/teamlist) | Golisopod, Delphox, Rillaboom, Politoed, Farigiraf, Pawmot | rain, grassy-terrain, trick-room-option | SP 미공개 |
| [kickoff-thepostmanp](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/thepostmanp/teamlist) | Froslass, Pawmot, Glimmora, Talonflame, Kingambit, Basculegion | snow-or-froslass, tailwind | SP 미공개 |
| [kickoff-thosewhoknow](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/thosewhoknow/teamlist) | Floette-Eternal, Sneasler, Indeedee, Basculegion, Dragonite, Garchomp | psychic-terrain, psyspam, trick-room-option | SP 미공개 |
| [kickoff-wolfey](https://play.limitlesstcg.com/tournament/6aa00639a4272c53be64abbd/player/wolfey/teamlist) | Golisopod, Indeedee-F, Sneasler, Pelipper, Gardevoir, Basculegion | psychic-terrain, psyspam, rain, trick-room-option, tailwind | SP 미공개 |
| [perish-mrada](https://play.limitlesstcg.com/tournament/6a7f315b5a30714095570103/player/m_rada/teamlist) | Gengar, Snorlax, Incineroar, Scrafty, Dragonite, Rillaboom | grassy-terrain, perish-song | SP 미공개 |
| [psy-cona](https://pokepast.es/6b76ea3a9125afd1) | Indeedee-F, Armarouge, Staraptor, Sneasler, Basculegion, Golisopod | psychic-terrain, psyspam, trick-room-option, tailwind | 배분 공개·검증 통과 |
| [psy-lello](https://play.limitlesstcg.com/tournament/6a9c37baa4272c53be647d8a/player/lellocartello/teamlist) | Metagross, Indeedee, Sneasler, Arcanine-Hisui, Salamence, Milotic | psychic-terrain, psyspam, trick-room-option | SP 미공개 |
| [psy-nihat](https://play.limitlesstcg.com/tournament/6a6a3192937230b102d48538/player/nihataglar/teamlist) | Baxcalibur, Indeedee-F, Swampert, Golisopod, Pelipper, Armarouge | psychic-terrain, psyspam, rain, trick-room-option, tailwind | SP 미공개 |
| [psy-sand-udon](https://pokepast.es/fdc2970699476b2c) | Armarouge, Indeedee-F, Salamence, Sneasler, Excadrill, Tyranitar | psychic-terrain, psyspam, sand, tailwind | 배분 공개·검증 통과 |
| [sand-owen](https://pokepast.es/605ab0da3552400d) | Salamence, Indeedee, Sneasler, Gholdengo, Tyranitar, Excadrill | psychic-terrain, psyspam, sand, tailwind | 배분 공개·검증 통과 |

## 싱글

| ID / 출처 | 구성 | 태그 | 상태 |
|---|---|---|---|
| [hina-psychic-sneasler](https://pokesol.app/u/hina/articles/3da3589e9f461806) | Indeedee, Sneasler, Armarouge, Froslass, Baxcalibur, Archaludon | psychic-terrain, psyspam, snow-or-froslass | 배분 공개·검증 통과 |
| [masagon-lucario-salamence](https://pokesol.app/u/masagon_poke/articles/21a8188ae8bfd902) | Salamence, Hippowdon, Gholdengo, Primarina, Rillaboom, Lucario | sand, grassy-terrain | 배분 공개·검증 통과 |
| [pizza-armarouge-tr](https://pokesol.app/u/pizza_yasan_0329/articles/1f41bcac4f820b78) | Indeedee-F, Armarouge, Aggron, Mimikyu, Ninetales-Alola, Baxcalibur | psychic-terrain, psyspam, snow-or-froslass, trick-room-option | 검토 필요·시험 제외 |
| [robin-blastoise-psychic](https://pokesol.app/u/robin_poke/articles/f8e38fbea9d37e13) | Indeedee, Blastoise, Sneasler, Ninetales-Alola, Garchomp, Armarouge | psychic-terrain, psyspam, snow-or-froslass | 배분 공개·검증 통과 |
| [sarami-raichu-experiment](https://pokesol.app/u/sarami120/articles/96df6ceb6b59bac7) | Raichu, Sneasler, Ninetales-Alola, Samurott-Hisui, Pyroar, Archaludon | snow-or-froslass | 배분 공개·검증 통과 |
| [tomoe-garchomp-golisopod](https://pokesol.app/u/tomoe_475/articles/40e7a95724bc6013) | Primarina, Garchomp, Golisopod, Rillaboom, Umbreon, Rotom-Wash | grassy-terrain | 배분 공개·검증 통과 |
| [tororo-goodra-grassy](https://pokesol.app/u/tororororo/articles/ac019be84c573b87) | Goodra-Hisui, Rillaboom, Baxcalibur, Indeedee, Skeledirge, Salamence | psychic-terrain, psyspam, grassy-terrain | 배분 공개·검증 통과 |

| [alc-hippo-archaludon](https://pokesol.app/u/alc_tomotarou/articles/fd1076f5bf5f36a8) | Hippowdon, Salamence, Archaludon, Baxcalibur, Metagross, Sylveon | bulky-balance, hippowdon-salamence | 배분 공개·검증 통과 |
| [asqkura-bulky-setup](https://pokesol.app/u/asqkura/articles/c369b17e7dbb6dc5) | Salamence, Baxcalibur, Gholdengo, Primarina, Volcarona, Hippowdon | bulky-setup, dragon-dance | 배분 공개·검증 통과 |
| [hiroki-mence-theory](https://pokesol.app/u/hiroki_poke/articles/c7bf310026fe1cf7) | Salamence, Rotom-Wash, Corviknight, Meowscarada, Garchomp, Samurott-Hisui | theory, dragon-dance | 배분 공개·검증 통과 |
| [hujiko-no-mega](https://pokesol.app/u/hujiko/articles/4225cdecaf52efec) | Meowscarada, Corviknight, Bellibolt, Samurott-Hisui, Mimikyu, Greninja | no-mega, balance | 배분 공개·검증 통과 |
| [kurotama-baxcalibur](https://pokesol.app/u/kurotama_887/articles/ead268ad572c8062) | Baxcalibur, Venusaur, Cinderace, Skarmory, Mimikyu, Rotom-Wash | baxcalibur-setup, hazards | 배분 공개·검증 통과 |
| [mutsu-baxcalibur](https://pokesol.app/u/mutsu_0120/articles/68e14a2f6eb6f350) | Baxcalibur, Charizard, Empoleon, Toxapex, Grimmsnarl, Excadrill | screens, toxic-spikes, baxcalibur-setup | 배분 공개·검증 통과 |
| [myuto-mence-offense](https://pokesol.app/u/myutopoke123/articles/165b9b930396ac7a) | Garchomp, Scizor, Mimikyu, Greninja, Salamence, Hippowdon | offense, hippowdon-salamence | 배분 공개·검증 통과 |
| [tama-screens-revival](https://pokesol.app/u/tama_poke0216/articles/9d0b9790b3a18738) | Salamence, Glimmora, Pawmot, Pyroar, Aegislash, Primarina | screens, revival-blessing, bulky-setup | 배분 공개·검증 통과 |
| [tororo-raichu-electric](https://pokesol.app/u/tororo_poke/articles/027fce2f98316393) | Raichu, Sneasler, Basculegion, Baxcalibur, Ninetales-Alola, Garchomp | electric-terrain, seed-sneasler, snow | 배분 공개·검증 통과 |
| [yuto-rain-perish](https://pokesol.app/u/070810yuto/articles/9b071422d65222d1) | Basculegion, Archaludon, Golisopod, Pelipper, Gengar, Arboliva | rain, perish-song | 배분 공개·검증 통과 |

## 이번 싱글 확장

기존 7개에 중복 없는 10개를 추가했다. 10개 모두 로컬 M-C 싱글 검증을 통과했다. 실전 후기 외에 사전 이론·노메가 제약·실험 구성도 포함되며 각 metadata의 evidence_note로 구분한다. 모두 같은 작성자 플랫폼에서 수집한 표본으로, 전체 메타의 대표성이나 대중성을 보장하지 않는다.

- mutsu-baxcalibur의 아머까오는 상대 물리벽 예시였으므로 원본 카드 7개를 보존하고 작성자의 실제 6마리만 변환했다.
- yuto-rain-perish의 일렉트로빔은 사이트 내부 기술 ID가 공식 ID와 달랐다. 원문의 일본어명·타입·위력을 기준으로 Electro Shot에 연결하고 변환 기록을 남겼다.
- hujiko-no-mega는 M-C 태그와 M-5/게시일이 혼재하므로 시점 해석에 주의한다. hiroki-mence-theory는 시즌 개시 전 이론 구성이다.

## 규칙·자료 해석

[공식 M-C 안내](https://champions-news.pokemon-home.com/en/page/816.html). 이벤트 제목·날짜와 포맷 검증을 함께 확인했다. 일부 외부 플랫폼의 오래된 M-B 표기는 메타데이터에 경고로 남겼다. 상세 실적은 각 원본 및 `metadata.json`에서 확인한다.
