"""Independently verify raw policy eligibility and recompute reference BR gaps."""
from pathlib import Path
import hashlib, json, statistics
from datetime import datetime

ROOT=Path(__file__).resolve().parent
RUN=36999721226
DATA=ROOT/'publication'/f'run-{RUN}'
read=lambda p:json.loads(p.read_bytes())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def save(path,value):
    with path.open('x',encoding='utf-8',newline='\n') as f:json.dump(value,f,indent=2,ensure_ascii=False,allow_nan=False);f.write('\n')
def raw(path):return [json.loads(l) for l in path.read_bytes().splitlines(keepends=True) if l.endswith(b'\n')]

receipt=read(ROOT/'publication/receipt.json')
run=next(read(p) for p in sorted((ROOT/'publication').glob('status-*.json'),reverse=True) if read(p)['status']=='completed')
assert run['id']==RUN and run['head_sha']==receipt['source_sha'] and run['conclusion']=='success'
assert len(run['jobs'])==9 and all(j['conclusion']=='success' for j in run['jobs'])
for folder in DATA.iterdir():
    if not folder.is_dir():continue
    download=read(folder/'download-receipt.json')
    assert sha(DATA/(folder.name+'.zip'))==download['archive_sha256']
    for name,digest in download['files'].items(): assert sha(folder/name)==digest
package=DATA/f's25d-package-{RUN}-1'; manifest=read(package/'manifest.json')
assert manifest['source_sha']==receipt['source_sha']
plan=read(ROOT/'source/engine/benchmarks/s25_repair/plan.json')
assert manifest['plan_sha256']==sha(ROOT/'source/engine/benchmarks/s25_repair/plan.json')
for arm,binary in manifest['binaries'].items():
    assert sha(package/binary['name'])==binary['sha256']
    assert binary['fingerprint']['response_sweeps']==(arm=='revised')
old_features=manifest['binaries']['selective']['resolved_features']
new_features=manifest['binaries']['revised']['resolved_features']
assert old_features['lab-engine']==new_features['lab-engine']
assert set(new_features['lab-search'])-set(old_features['lab-search'])=={'experiment-response-sweeps'}
assert set(old_features['lab-search'])<=set(new_features['lab-search'])
report=read(DATA/f's25d-summary-{RUN}-1/summary.json')
assert report['references_completed']==report['references_planned']==5 and report['all_trials_valid']
counts=dict(snapshots=0,quality_checks=0,trials=0,diagnostics=0)

def inspect_trial(stem,trial,limits,ref,actions):
    process=read(stem/'process.json')
    assert trial['valid'] and process==trial['process']
    assert sha(stem/'stdout.jsonl')==process['stdout_sha256'] and sha(stem/'stderr.log')==process['stderr_sha256']
    assert process['exit_code']==0 or process['watchdog']=='deadline'
    rows=raw(stem/'stdout.jsonl')
    assert not any(r['kind']=='error' for r in rows)
    ready=next(r for r in rows if r['kind']=='ready')
    assert ready==trial['ready'] and ready['actions']==actions
    policies=[r for r in rows if r['kind']=='policy'];counts['snapshots']+=len(policies)
    assert policies
    for axis,points,key in [('time',limits,'elapsed_s'),('work',plan['work_checkpoints'],'transitions')]:
        for limit,scored in zip(points,trial[axis]):
            eligible=[p for p in policies if p[key]<=limit];expected=eligible[-1] if eligible else None
            assert scored['limit']==limit and scored['available']==bool(eligible) and scored['policy']==expected
            if expected is not None and ref is not None:
                p=[v/sum(expected['rows']) for v in expected['rows']];q=[v/sum(expected['cols']) for v in expected['cols']]
                assert abs(sum(expected['rows'])-1)<1e-5 and abs(sum(expected['cols'])-1)<1e-5
                n,m=ref['rows'],ref['cols']
                def bound(values):
                    hi=max(sum(values[r*m+c]*q[c] for c in range(m)) for r in range(n))
                    lo=min(sum(p[r]*values[r*m+c] for r in range(n)) for c in range(m))
                    return hi,lo
                hi,lo=bound(ref['matrix']);low=bound(ref['cell_lower']);high=bound(ref['cell_upper'])
                assert abs(scored['quality']['reference_root_br_gap']-(hi-lo))<1e-8
                assert abs(scored['quality']['reference_gap_lower']-max(0,low[0]-high[1]))<1e-8
                assert abs(scored['quality']['reference_gap_upper']-(high[0]-low[1]))<1e-8
                counts['quality_checks']+=1
    selected=trial['time'][-1]; done=next((r for r in reversed(rows) if r['kind']=='done'),None)
    return dict(first_policy_s=policies[0]['elapsed_s'],last_policy=selected['policy'],quality=selected['quality'],
        peak_rss_mib=process['process_peak_rss_kib']/1024,stop=done['stop'] if done else process['watchdog'],done=done,
        process_cpu_s=process['cpu_user_s']+process['cpu_system_s'])

cases=[]
for case in plan['cases']:
    folder=DATA/f's25d-case-{case["id"]}-{RUN}-1';result=read(folder/'result.json')
    assert result['case']==case and result['valid'] and len(result['trials'])==9
    assert result['source_sha']==receipt['source_sha'] and result['package_manifest_sha256']==sha(package/'manifest.json')
    ref=result['reference'].get('report',{}).get('reference')
    actions=result['trials'][0]['ready']['actions']
    if ref:
        reference=raw(folder/'reference/stdout.jsonl')
        assert next(r for r in reference if r['kind']=='reference')['reference']==ref
        assert next(r for r in reference if r['kind']=='ready')['actions']==actions
    arms={mode:[] for mode in plan['arms']}
    for trial in result['trials']:
        stem=folder/f"pair-{trial['repeat']}-{trial['mode']}"
        arms[trial['mode']].append(inspect_trial(stem,trial,case['seconds'],ref,actions)); counts['trials']+=1
    summary={}
    for arm,rows in arms.items():
        summary[arm]=dict(first_policy_s_median=statistics.median(r['first_policy_s'] for r in rows),
            peak_rss_mib_median=statistics.median(r['peak_rss_mib'] for r in rows),
            last_reference_gap_median=statistics.median(r['quality']['reference_root_br_gap'] for r in rows) if ref else None,
            stops=[r['stop'] for r in rows],trials=rows)
    diagnostics={}
    for diagnostic in result['diagnostics']:
        mode=diagnostic['mode']
        diagnostics[mode]=inspect_trial(folder/('completion-'+mode),diagnostic,[plan['completion_diagnostic_seconds']],ref,actions)
        counts['diagnostics']+=1
    env=read(folder/'environment.json')
    models=sorted(set(l.split(':',1)[1].strip() for l in env['cpuinfo'].splitlines() if l.startswith('model name')))
    cases.append(dict(case=case['id'],cpu_models=models,reference_interval=[ref['lower'],ref['upper']] if ref else None,
        arms=summary,diagnostics=diagnostics))
assert counts['trials']==63 and counts['diagnostics']==2
wall=(datetime.fromisoformat(run['updated_at'].replace('Z','+00:00'))-datetime.fromisoformat(run['run_started_at'].replace('Z','+00:00'))).total_seconds()
result=dict(schema=1,status='PASS_MEASUREMENT_AUDIT_NOT_ADOPTION',run=RUN,run_url=run['html_url'],source_sha=receipt['source_sha'],
    run_wall_seconds=wall,counts=counts,fresh_rust_tests=sum(t['passed'] for t in manifest['tests']),python_protocol_tests=5,
    package_manifest_sha256=sha(package/'manifest.json'),cases=cases,default_off=True,
    limitations=report['limitations']+['Revised selector is deterministic; seed replicates measure timing only.',
    'Peak RSS may include inherited Python memory; retained-position counts measure a different quantity.'])
save(ROOT/f'AUDIT-{RUN}.json',result)
print(json.dumps({k:v for k,v in result.items() if k not in ['cases','limitations']},indent=2))
for case in cases:
    print(json.dumps(dict(case=case['case'],arms={k:{a:v for a,v in s.items() if a!='trials'} for k,s in case['arms'].items()},
        diagnostics={k:dict(stop=v['stop'],time=v['done']['elapsed_s'] if v['done'] else None,gap=v['quality']['reference_root_br_gap']) for k,v in case['diagnostics'].items()})))
