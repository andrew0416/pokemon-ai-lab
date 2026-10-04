"""Recompute paired samples and bind correctness, source and artifact provenance."""
from pathlib import Path
from collections import defaultdict
import json,hashlib,statistics,math,re,sys,random
import publish
ROOT=Path(__file__).resolve().parent;RUN=int(sys.argv[1])
read=lambda p:json.loads(p.read_bytes())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
prefix=publish.BRANCH.split('/')[1].rsplit('-',1)[0]
P=ROOT/f'publication/run-{RUN}/{prefix}-{RUN}-1'
receipt=read(ROOT/'publication/receipt.json');download=read(P/'download-receipt.json')
for name,digest in download['files'].items():assert sha(P/name)==digest,name
assert publish.review()[1]['changed_sha256']==receipt['changed_sha256']
assert publish.snapshot()==receipt['shared_git']
assert (P/'source-sha.txt').read_text().strip()==receipt['source_sha']
statuses=[read(p) for p in (ROOT/'publication').glob('status-*.json')]
status=next(s for s in reversed(statuses) if s['id']==RUN and s['conclusion']=='success')
assert status['head_sha']==receipt['source_sha']
summary=read(P/'paired/summary.json');environment=read(P/'paired/environment.json')
assert summary['passed'] and summary['timing'] and summary['wins_evaluated'] is False
bench='s26_parallel'
plan=read(ROOT/f'source/engine/benchmarks/{bench}/plan.json')
assert len(summary['cases'])==len(plan['cases'])
assert environment['plan_sha256']==sha(ROOT/f'source/engine/benchmarks/{bench}/plan.json')
assert environment['source_sha']==receipt['source_sha']
assert environment['pinned_cpu'] in environment['available_cpus']
assert set(environment['parallel_cpus']) <= set(environment['available_cpus']) and 1 <= len(environment['parallel_cpus']) <= 4
assert all(c['cumulative_child_peak_rss_kib'] <= 512*1024 for c in summary['cases'])
groups=defaultdict(list);blocks=samples=solves=0
for case in summary['cases']:
    assert case['exit_code']==0
    rows=[json.loads(l) for l in (P/f"paired/{case['case']}-raw.jsonl").read_text().splitlines()]
    assert rows[-1]['type']=='completed' and rows[-1]['rounds']==plan['rounds']
    baseline=next(r['diagnostics'] for r in rows if r['type']=='baseline')
    for result in [r for r in summary['results'] if r['case']==case['case']]:
        variant=result['variant']
        validation=next(r for r in rows if r['type']=='validation' and r['variant']['id']==variant['id'])
        assert validation['diagnostics']==result['diagnostics'] and baseline==result['baseline_diagnostics']
        if result['comparison']=='exact-same-work':
            assert validation['diagnostics']==baseline
        else:
            assert result['comparison']=='same-finite-game'
            d=validation['diagnostics'];assert d['horizon_complete'] and baseline['horizon_complete']
            for k in ['nodes','information_sets','transitions','root_policy','value','gap']:
                assert d[k]==baseline[k],k
        assert baseline['solver_converged']
        pairs=[r for r in rows if r['type']=='paired' and r['variant']==variant['id']]
        assert len(pairs)==plan['rounds']
        values=[]
        for pair,reported in zip(pairs,result['pairs']):
            assert [s['treatment'] for s in pair['samples']]==(['baseline','candidate','candidate','baseline'] if pair['block']%2==0 else ['candidate','baseline','baseline','candidate'])
            assert all(math.isfinite(s['ns_per_solve']) and s['ns_per_solve']>0 and 1<=s['batch']<=64 for s in pair['samples'])
            b=sum(s['ns_per_solve'] for s in pair['samples'] if s['treatment']=='baseline')/2e6
            c=sum(s['ns_per_solve'] for s in pair['samples'] if s['treatment']=='candidate')/2e6
            for k,v in dict(baseline_ms=b,candidate_ms=c,ratio=c/b).items():assert abs(reported[k]-v)<1e-9
            values.append(dict(baseline_ms=b,candidate_ms=c,ratio=c/b))
            blocks+=1;samples+=4;solves+=sum(s['batch'] for s in pair['samples'])
        ratio=statistics.median(p['ratio'] for p in values)
        assert abs(result['ratio_median']-ratio)<1e-9
        assert abs(result['time_reduction_percent']-100*(1-ratio))<1e-8
        rng=random.Random(261003);ratios=[p['ratio'] for p in values]
        bootstrap=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(4000))
        assert all(abs(a-b)<1e-12 for a,b in zip(result['ratio_bootstrap95'],[bootstrap[100],bootstrap[3899]]))
        case_group=re.sub(r'-seed\d+$','',case['case'])
        groups[(case_group,variant['id'])].append(dict(seed=variant.get('seed'),ratio=ratio,
            ratio_bootstrap95=result['ratio_bootstrap95'],pairs=values))
aggregates=[]
for (case,mode),seeds in groups.items():
    pairs=[p for s in seeds for p in s['pairs']];ratio=statistics.median(p['ratio'] for p in pairs)
    aggregates.append(dict(case=case,mode=mode,baseline_ms=statistics.median(p['baseline_ms'] for p in pairs),
        candidate_ms=statistics.median(p['candidate_ms'] for p in pairs),ratio=ratio,reduction_percent=100*(1-ratio),seeds=seeds))
if prefix.endswith('confirm'):
    inherited=read(ROOT.parent/'s10-static-backup-20261003/AUDIT-37032057405.json')
    assert inherited['source_sha']==receipt['parent'] and inherited['passed']
    lp=inherited['lp_certificates']
else:
    lp={name:read(P/f'{name}/summary.json')['passed'] for name in ['tree-lp','growing-lp']}
assert lp=={'tree-lp':49,'growing-lp':96}
cpu=next(l.split(':',1)[1].strip() for l in environment['cpuinfo'].splitlines() if l.startswith('model name'))
test_counts={}
for name,expected in [('core',101),('owned',7),('on',92),('off',17),('cli',1)]:
    content=(P/(name+'.log')).read_text(encoding='utf-8')
    match=re.search(r'test result: ok\. (\d+) passed; 0 failed;',content)
    assert match and int(match[1])==expected,name
    test_counts[name]=expected
assert 'S26E_EXACT mechanics=8 pairs=48 endings=766' in (P/'owned.log').read_text(encoding='utf-8')

contracts=sorted({r['comparison'] for r in summary['results']})
assert contracts==['exact-same-work']
audit=dict(source_sha=receipt['source_sha'],parent=receipt['parent'],run=RUN,passed=True,cpu=cpu,
    binary_sha256=environment['binary_sha256'],artifact_sha256=download['archive_sha256'],
    cases=len(summary['cases']),treatments=len(summary['results']),paired_blocks=blocks,timing_samples=samples,timed_solves=solves,
    comparison_contracts=contracts,test_counts=test_counts,bootstrap_recomputed=True,lp_inherited=prefix.endswith("confirm"),lp_certificates=lp,wins_evaluated=False,shared_git_unchanged=True,groups=aggregates)
with (ROOT/f'AUDIT-{RUN}.json').open('x',encoding='utf-8') as f:json.dump(audit,f,indent=2);f.write('\n')
print(json.dumps({k:v for k,v in audit.items() if k!='groups'},indent=2))
for g in aggregates:print(json.dumps({k:v for k,v in g.items() if k!='seeds'}))
