"""Prespecified candidate screen followed by unseen-seed/search-policy validation."""
import copy
import hashlib
import json
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path
import numpy as np

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'runs'/('improvement-'+datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
def save(p,obj):
    p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(json.dumps(obj,ensure_ascii=False,indent=2),encoding='utf-8')
def read(p): return json.loads(p.read_text(encoding='utf-8-sig'))
original=read(ROOT/'teams/gravity-original.json')
revised=read(ROOT/'teams/gravity-revised.json')
rilla=next(p for p in revised if p['species']=='Rillaboom')
teams={'original':original,'icy_wind':read(ROOT/'teams/gravity-icy-wind.json')}
teams['rillaboom_only']=copy.deepcopy(original)
teams['rillaboom_only'][4]=copy.deepcopy(rilla)
teams['targeted']=copy.deepcopy(teams['rillaboom_only'])
teams['targeted'][1]['moves']=['Hydro Pump','Recover','Hypnosis','Icy Wind']
teams['targeted'][2]['moves']=['Hyper Voice','Gravity','Psyshock','Protect']
teams['previous_revised']=revised
changes={
 'original':'사진 원본',
 'icy_wind':'밀로틱 냉동빔 → 얼다바람',
 'rillaboom_only':'애프룡 → 고릴타. 나머지 원본 유지',
 'targeted':'애프룡 → 고릴타; 밀로틱 얼다바람; 가디안 최면술/기합구슬 → 사이코쇼크/방어',
 'previous_revised':'이전 제안: 리자몽 → 어흥염, 애프룡 → 고릴타, 가디안 사이코쇼크/방어',
}
opponents={'psyspam':'teams/psyspam-popular.json','sand':'teams/sand-owen.txt'}
plan={'screen':{'games_per_matchup':16,'seed':917231,'policy':'vgc_myopic'},
      'confirmation':{'games_per_matchup':24,'seed':842917,'policy':'vgc'},
      'selection':'Highest equal-weight score across two matchups; ties prefer earlier/simpler candidate. Confirm best non-original against original; no further tuning.',
      'candidates':changes,'opponents':opponents,
      'limits':['Only two fixed opponent teams, not team holdout.','Psyspam allocations and replacement allocations are assumptions.',
                'Different policy and unseen seeds are validation within this panel, not proof of human/ladder strength.'],
      'original_sha256':hashlib.sha256((ROOT/'teams/gravity-original.json').read_bytes()).hexdigest()}
save(OUT/'plan.json',plan)
for name,team in teams.items():
    save(OUT/f'{name}.json',team)
    save(OUT/f'{name}.provenance.json',{'source':'Screenshot baseline plus prespecified experiment modifications','changes':changes[name],
        'replacements':'Rillaboom/Incineroar SP allocations are experimental assumptions, unchanged members retain screenshot stats.'})
env=dict(os.environ,PYTHONUTF8='1',PYTHONHASHSEED='0')
for name in teams:
    subprocess.run(['node',str(ROOT/'scripts/tooling.cjs'),'team',str(OUT/f'{name}.json')],check=True,capture_output=True,text=True,encoding='utf-8',env=env)
results={'screen':{},'confirmation':{}}
def run(stage,name,opponent):
    cfg=plan[stage]
    log=OUT/f'{stage}-{name}-{opponent}.stdout.txt'
    command=[sys.executable,'-X','utf8',str(ROOT/'scripts/lab.py'),'battle',
        '--team1',str(OUT/f'{name}.json'),'--team2',str(ROOT/opponents[opponent]),
        '--p1',cfg['policy'],'--p2',cfg['policy'],'--games',str(cfg['games_per_matchup']),'--seed',str(cfg['seed'])]
    with log.open('w',encoding='utf-8') as stream:
        subprocess.run(command,cwd=ROOT,env=env,stdout=stream,stderr=subprocess.STDOUT,check=True,timeout=300)
    artifact=next(line.removeprefix('Artifacts: ') for line in log.read_text(encoding='utf-8').splitlines() if line.startswith('Artifacts: '))
    summary=read(Path(artifact)/'summary.json')
    results[stage].setdefault(name,{})[opponent]=summary
    save(OUT/'results.json',results)
    print(f'{stage} | {name} | {opponent}: {summary["A_wins"]}/{summary["games"]} wins, {summary["draws"]} draws',flush=True)
for name in teams:
    for opponent in opponents: run('screen',name,opponent)
best=max([name for name in teams if name!='original'],key=lambda name:sum(x['A_score'] for x in results['screen'][name].values()))
save(OUT/'selection.json',{'candidate':best,'changes':changes[best],'rule':plan['selection']})
print('Confirmation candidate: '+best,flush=True)
for name in ['original',best]:
    for opponent in opponents: run('confirmation',name,opponent)
comparison={}
for opponent in opponents:
    base=np.array(results['confirmation']['original'][opponent]['pair_scores'])
    candidate=np.array(results['confirmation'][best][opponent]['pair_scores'])
    delta=candidate-base
    boot=np.random.default_rng(4567).choice(delta,(10000,len(delta)),replace=True).mean(axis=1)
    comparison[opponent]={'delta':float(delta.mean()),'paired_bootstrap_95':np.quantile(boot,[.025,.975]).tolist()}
save(OUT/'comparison.json',comparison)
rows=['# 개선용 대전 결과','',f'확인 후보: **{changes[best]}**','',
      '승률은 실제 인간 대전의 예상 승률이 아니라 고정 팀·봇 조건의 결과입니다.','',
      '| 단계/후보 | 사이스팸 승/대전 | 모래팟 승/대전 |','|---|---:|---:|']
for stage,data in results.items():
    for name,matches in data.items():
        rows.append('| '+stage+' / '+changes[name]+' | '+' | '.join(f'{matches[o]["A_wins"]}/{matches[o]["games"]}' for o in opponents)+' |')
rows+=['','확인 단계는 선별에 쓰지 않은 시드와 탐색 정책(vgc)을 사용했습니다. 각 수치에는 자리 교환 경기가 포함되며 서로 독립인 경기로 간주하지 않습니다.','',
       '| 확인 단계 개선폭 | 점수 차이 | 자리 쌍 bootstrap 95% 구간 |','|---|---:|---:|']
for opponent,c in comparison.items():
    lo,hi=c['paired_bootstrap_95']; rows.append(f'| {opponent} | {c["delta"]*100:+.1f}%p | {lo*100:+.1f} ~ {hi*100:+.1f}%p |')
rows+=['','두 고정 상대에 한정된 조건부 비교입니다. 새로운 상대 팀에 대한 검증은 하지 않았습니다. 사이스팸 SP와 교체 포켓몬 SP는 가정값입니다.',
       '',f'전체 로그와 팀 스냅샷: `{OUT}`']
(OUT/'report.md').write_text('\n'.join(rows),encoding='utf-8')
print('REPORT: '+str(OUT),flush=True)
