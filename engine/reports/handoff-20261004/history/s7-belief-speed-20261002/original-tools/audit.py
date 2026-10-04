"""Recompute speed ratios from raw paired samples and pin all provenance."""
from pathlib import Path
from collections import defaultdict
from datetime import datetime
import hashlib,json,math,statistics
import publish
ROOT=Path(__file__).resolve().parent;RUN=37025196260
P=ROOT/f'publication/run-{RUN}/s26c-speed-{RUN}-1'
read=lambda p:json.loads(p.read_bytes());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
publication=read(ROOT/'publication/receipt.json');download=read(P/'download-receipt.json')
for name,digest in download['files'].items():assert sha(P/name)==digest
review=publish.review()[1];assert review['changed_sha256']==publication['changed_sha256']
assert publish.snapshot()==publication['shared_git']
assert (P/'source-sha.txt').read_text().strip()==publication['source_sha']
status=read(sorted((ROOT/'publication').glob('status-*.json'))[-1])
assert status['id']==RUN and status['conclusion']=='success' and status['head_sha']==publication['source_sha']
summary=read(P/'paired/summary.json');env=read(P/'paired/environment.json')
assert summary['passed'] and summary['timing'] and len(summary['cases'])==4 and len(summary['results'])==26
assert summary['wins_evaluated'] is False and env['pinned_cpu'] in env['available_cpus']
assert env['source_sha']==publication['source_sha'] and env['plan_sha256']==sha(ROOT/'source/engine/benchmarks/s26_speed/plan.json')
groups=defaultdict(list);total_blocks=total_samples=total_calls=0
for case in summary['cases']:
 assert case['exit_code']==0
 rows=[json.loads(s) for s in (P/f"paired/{case['case']}-raw.jsonl").read_text().splitlines()]
 assert rows[-1]['type']=='completed' and rows[-1]['rounds']==7
 baseline=next(r['diagnostics'] for r in rows if r['type']=='baseline')
 for r in [v for v in summary['results'] if v['case']==case['case']]:
  variant=r['variant'];measurements=[x for x in rows if x['type']=='paired' and x['variant']==variant['id']]
  assert len(measurements)==7 and sorted(x['block'] for x in measurements)==list(range(7))
  validation=next(x for x in rows if x['type']=='validation' and x['variant']['id']==variant['id'])
  assert validation['diagnostics']==r['diagnostics'] and r['baseline_diagnostics']==baseline
  ratios=[];bases=[];candidates=[]
  for pair,reported in zip(measurements,r['pairs']):
   order=[v['treatment'] for v in pair['samples']]
   assert order==(['baseline','candidate','candidate','baseline'] if pair['block']%2==0 else ['candidate','baseline','baseline','candidate'])
   assert all(v['ns_per_solve']>0 and math.isfinite(v['ns_per_solve']) and 1<=v['batch']<=64 for v in pair['samples'])
   b=sum(v['ns_per_solve'] for v in pair['samples'] if v['treatment']=='baseline')/2e6
   c=sum(v['ns_per_solve'] for v in pair['samples'] if v['treatment']=='candidate')/2e6
   ratio=c/b
   for k,x in [('baseline_ms',b),('candidate_ms',c),('ratio',ratio)]:assert abs(reported[k]-x)<1e-10
   bases.append(b);candidates.append(c);ratios.append(ratio)
   total_samples+=4;total_calls+=sum(v['batch'] for v in pair['samples'])
  ratio=statistics.median(ratios)
  for k,x in [('baseline_ms',statistics.median(bases)),('candidate_ms',statistics.median(candidates)),('ratio_median',ratio),('time_reduction_percent',100*(1-ratio)),('speedup',1/ratio)]:assert abs(r[k]-x)<1e-9
  lo,hi=r['ratio_bootstrap95'];assert lo<=ratio<=hi
  d=r['diagnostics'];assert d['solver_converged']
  if variant['require_complete']:
   assert d['horizon_complete'] and d['growth']['frontier_groups']==0
   for k in ('nodes','information_sets','transitions'):assert d[k]==baseline[k]
   assert abs(d['value']-baseline['value'])<1e-7
  total_blocks+=len(measurements)
  groups[(case['case'],variant['id'].split('-seed')[0])].append(dict(
   seed=variant['seed'],baseline_ms=r['baseline_ms'],candidate_ms=r['candidate_ms'],ratio=ratio,
   ratio_bootstrap95=r['ratio_bootstrap95'],pairs=r['pairs'],
   diagnostics={k:v for k,v in d.items() if k!='root_policy'},
   baseline_diagnostics={k:v for k,v in baseline.items() if k!='root_policy'}))
assert total_blocks==182 and total_samples==728
aggregate=[]
for (case,budget),seeds in groups.items():
 pairs=[p for s in seeds for p in s['pairs']];ratio=statistics.median(p['ratio'] for p in pairs)
 aggregate.append(dict(case=case,budget=budget,seed_count=len(seeds),
  baseline_ms=statistics.median(p['baseline_ms'] for p in pairs),candidate_ms=statistics.median(p['candidate_ms'] for p in pairs),
  ratio_median=ratio,time_reduction_percent=100*(1-ratio),speedup=1/ratio,
  seed_ratio_range=[min(s['ratio'] for s in seeds),max(s['ratio'] for s in seeds)],
  horizon_complete=all(s['diagnostics']['horizon_complete'] for s in seeds),seeds=seeds))
seconds=int((datetime.fromisoformat(status['jobs'][0]['completed_at'])-datetime.fromisoformat(status['run_started_at'])).total_seconds())
result=dict(source_sha=publication['source_sha'],algorithm_parent=publication['parent'],run=RUN,conclusion='success',
 run_wall_seconds=seconds,cases=4,treatments=26,paired_blocks=total_blocks,timing_samples=total_samples,timed_solves=total_calls,
 release=True,portable_x86_64=True,kernels_changed=False,wins_evaluated=False,performance_measured=True,
 cpu_model=next((s.split(':',1)[1].strip() for s in env['cpuinfo'].splitlines() if s.startswith('model name')),'unknown'),
 cpu=env['pinned_cpu'],binary_sha256=env['binary_sha256'],archive_sha256=download['archive_sha256'],groups=aggregate,
 shared_git=publication['shared_git'])
with (ROOT/f'AUDIT-{RUN}.json').open('x',encoding='utf-8') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps({k:v for k,v in result.items() if k not in ('groups','shared_git')},indent=2))
for g in aggregate:print(json.dumps({k:v for k,v in g.items() if k!='seeds'}))
names={'stats-w1-d2':'능력치 후보 1개·2턴','stats-w2-d2':'능력치 후보 2개·2턴','pivot-w1-d1':'U-turn·탈출버튼 1턴','sand-damage-w1-d1':'모래·공격 1턴'}
lines=['# S26c 속도 측정', '',f"source `{publication['source_sha']}` · 알고리즘 기준 `{publication['parent']}`.",
 f'[GitHub run {RUN}](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/{RUN}) 성공. CI 작업 종료까지 {seconds}초.', '',
 f"CPU {result['cpu_model']}, 논리 CPU {result['cpu']}에 고정한 단일 스레드. generic x86-64 release 실행파일 하나에서 옵션을 전환했다.",
 '기존 전체 계산과 선택적 계산의 시뮬레이션·CFR 소스는 수정하지 않았다. 파일 읽기·setup·JSON 출력·최종 반환 트리 해제는 제외하며 탐색 중 내부 복사/해제와 재풀이는 포함한다.', '',
 '## 측정값', '',
 '7개 ABBA/BAAB 블록과 선택 시드 3개(교체·모래는 1개)의 중앙값이다. 아래 감소율은 짝지은 시간비의 중앙값으로 계산하므로 두 시간 중앙값의 단순 비율과 약간 다를 수 있다.', '',
 '| 국면 | 계산 범위 | 전체 계산 ms | 선택적 계산 ms | 시간 변화 |',
 '|---|---|---:|---:|---:|']
for g in aggregate:
 scope='동일 깊이 끝까지' if g['horizon_complete'] else f"공개 이력 {g['budget'][1:]}개 확장(루트 포함)"
 change=f"{100*(1-g['ratio_median']):.1f}% 감소" if g['ratio_median']<1 else f"{g['ratio_median']:.2f}배 소요"
 lines.append(f"| {names[g['case']]} | {scope} | {g['baseline_ms']:.3f} | {g['candidate_ms']:.3f} | {change} |")
lines+=['','## 해석과 근거','',
 '부분 확장은 일부 후속 계산을 생략한 지연시간이며 동등한 전체 탐색의 가속률이 아니다. 전체 확장 행은 요청 깊이의 노드·정보집합·전이 수와 루트 값·정책이 기존 방식과 일치한 경우만 포함했다.',
 '범위는 작은 상태변화 중심 2턴 국면과 1턴 교체/공격 국면 4개다. 동일 러너 안에서의 반복 오차를 측정했으며 다른 CPU·실전 전체 메뉴로 일반화하지 않는다.',
 '각 확장 때 후보 트리를 복사하고 CFR를 재시작하는 비용이 현재 성장 구현에 포함되어 있다. 실제 반복 풀이 횟수는 아래와 같다. 이는 비용 분해 프로파일은 아니며 총시간의 정확한 원인 비율을 뜻하지 않는다.', '',
 '| 2개 후보·2턴, seed 1 | 엔진 전이 | CFR 풀이 횟수 | 누적 CFR 반복 |',
 '|---|---:|---:|---:|']
for g in aggregate:
 if g['case']!='stats-w2-d2':continue
 s=next(s for s in g['seeds'] if s['seed']==1);d=s['diagnostics'];w=d['growth']
 if g['budget']=='b1':
  b=s['baseline_diagnostics'];lines.append(f"| 기존 전체 계산 | {b['transitions']} | 1 | {b['iterations']} |")
 lines.append(f"| {g['budget']} | {d['transitions']} | {w['solves']} | {w['total_cfr_iterations']} |")
lines+=['',f'원시 측정 {total_samples}개, 짝 비교 {total_blocks}개, timed solve {total_calls}회. 결과 안정성 검사는 타이밍 밖에서 수행했다.',
 f'`AUDIT-{RUN}.json`과 `publication/run-{RUN}/`에 입력, 원시 샘플, CPU 정보, 시드별 수치 및 해시를 보관했다.',
 '기본 OFF 유지. 공유 체크아웃 HEAD·상태·인덱스와 생산/R1 코드는 보존했다. 승률은 측정하지 않았다.','']
(ROOT/'RESULTS.md').write_text('\n'.join(lines),encoding='utf-8')
