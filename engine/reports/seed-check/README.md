# seed 검사 스크립트

`../seed-champions-check-2026-09-25.md`의 재현 절차. 임시 파일은 `%TEMP%\lab\`에 둔다.

```bash
# 1. 기준 데이터 (vendor 9e317a6)
node data/export.cjs                                   # engine/data/champions.json
node -e "const {Dex}=require('./vendor/pokemon-showdown/dist/sim');const dex=Dex.mod('champions');const out={};for(const s of dex.species.all()){if(!s.exists)continue;const ls=dex.data.Learnsets[s.id];if(ls&&ls.learnset)out[s.id]=ls.learnset;}require('fs').writeFileSync(process.env.TEMP+'/lab/champions-learnsets.json',JSON.stringify(out));"
#    M-B/M-C 모드 요약(seed_check_mb.py 입력): 보고서 본문의 node 한 줄 스크립트로 %TEMP%/lab/{championsregmb,champions}-ref.json 생성
# 2. seed 쪽 입력: fn15 delta(prepared1/delta.json)는 파일로 있음. npm 바인딩·공식 명단은 원장에서 추출
#    (ledger.sqlite는 21 GB; TEMP/TMP를 D:의 빈 디렉터리로 지정하고 mode=ro로 연다)
# 3. python seed_check.py ; python seed_check_mb.py
```

경로는 스크립트 상단 상수에 있다. seed 디렉터리는 절대 쓰지 않는다(읽기 전용, git 아님).
