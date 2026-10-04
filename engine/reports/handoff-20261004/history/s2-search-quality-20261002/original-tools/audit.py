"""Recompute the key claims from downloaded raw JSONL, independently of CI scoring."""
from pathlib import Path
import hashlib,json,math,statistics
from datetime import datetime

ROOT=Path(__file__).resolve().parent
RUN=36995572342
DATA=ROOT/'publication'/f'run-{RUN}'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
read=lambda p:json.loads(p.read_bytes())

def save(p,v):
    with p.open('x',encoding='utf-8',newline='\n') as f:json.dump(v,f,ensure_ascii=False,indent=2,allow_nan=False);f.write('\n')

receipt=read(ROOT/'publication/receipt.json')
statuses=[read(p) for p in (ROOT/'publication').glob('status-*.json')]
run=next(r for r in statuses if r['status']=='completed')
assert run['id']==RUN and run['head_sha']==receipt['source_sha'] and run['conclusion']=='success'
assert len(run['jobs'])==9 and all(j['conclusion']=='success' for j in run['jobs'])
for folder in DATA.iterdir():
    if not folder.is_dir():continue
    download=read(folder/'download-receipt.json')
    assert sha(DATA/(folder.name+'.zip'))==download['archive_sha256']
    for name,digest in download['files'].items():assert sha(folder/name)==digest
package=DATA/f's25-package-{RUN}-1'
manifest=read(package/'manifest.json')
assert manifest['source_sha']==receipt['source_sha'] and sha(package/'lab-search-quality')==manifest['binary_sha256']
assert manifest['plan_sha256']==sha(ROOT/'source/engine/benchmarks/s25_quality/plan.json')
assert [x['passed'] for x in manifest['tests']]==[44,1,17]
plan=read(ROOT/'source/engine/benchmarks/s25_quality/plan.json')
report=read(DATA/f's25-summary-{RUN}-1/summary.json')
assert report['references_completed']==report['references_planned']==5 and report['all_trials_valid']
case_summaries=[];snapshots=0;checked=0;counterexample=None
for case in plan['cases']:
    folder=DATA/f's25-case-{case["id"]}-{RUN}-1'
    result=read(folder/'result.json')
    assert result['case']==case and result['valid'] and len(result['trials'])==6
    assert result['source_sha']==receipt['source_sha'] and result['package_manifest_sha256']==sha(package/'manifest.json')
    ref=result['reference'].get('report',{}).get('reference')
    raw_reference=None
    if ref is not None:
        raw_reference=[json.loads(l) for l in (folder/'reference/stdout.jsonl').read_text().splitlines()]
        assert [r for r in raw_reference if r['kind']=='reference'][0]['reference']==ref
    arms={mode:[] for mode in ['baseline','selective']}
    for trial in result['trials']:
        stem=folder/f"pair-{trial['repeat']}-{trial['mode']}"
        process=read(stem/'process.json')
        assert process==trial['process'] and sha(stem/'stdout.jsonl')==process['stdout_sha256']
        lines=(stem/'stdout.jsonl').read_bytes().splitlines(keepends=True)
        records=[json.loads(line) for line in lines if line.endswith(b'\n')]
        assert not any(r['kind']=='error' for r in records)
        policies=[r for r in records if r['kind']=='policy'];snapshots+=len(policies)
        ready=next(r for r in records if r['kind']=='ready')
        assert ready==trial['ready']
        if raw_reference:assert ready['actions']==raw_reference[0]['actions']
        for axis,limits,key in [('time',case['seconds'],'elapsed_s'),('work',plan['work_checkpoints'],'transitions')]:
            for limit,scored in zip(limits,trial[axis]):
                eligible=[p for p in policies if p[key]<=limit]
                expected=eligible[-1] if eligible else None
                assert scored['limit']==limit and scored['available']==bool(eligible) and scored['policy']==expected
                if expected is not None and ref is not None:
                    p=[v/sum(expected['rows']) for v in expected['rows']]
                    q=[v/sum(expected['cols']) for v in expected['cols']]
                    n,m=ref['rows'],ref['cols']
                    a=[ref['matrix'][r*m:(r+1)*m] for r in range(n)]
                    hi=max(sum(x*y for x,y in zip(row,q)) for row in a)
                    lo=min(sum(p[r]*a[r][c] for r in range(n)) for c in range(m))
                    assert abs(scored['quality']['reference_root_br_gap']-(hi-lo))<1e-8
                    assert abs(scored['quality']['estimated_value_error']-abs(expected['estimated_value']-ref['value']))<1e-8
                    checked+=1
        last=trial['time'][-1]
        selected=last['policy']
        completed=next((r for r in reversed(records) if r['kind']=='done'),None)
        arm=dict(first_policy_s=policies[0]['elapsed_s'],available_at_last=last['available'],
            last_cutoff=case['seconds'][-1],quality=last['quality'],last_policy=selected,
            peak_rss_mib=process['process_peak_rss_kib']/1024,process_cpu_s=process['cpu_user_s']+process['cpu_system_s'],
            stop=completed['stop'] if completed else process['watchdog'],
            final_work=completed,process_exit_code=process['exit_code'])
        arms[trial['mode']].append(arm)
        if case['id']=='half-hp-median' and trial['mode']=='selective' and trial['repeat']==0:
            p=selected['rows'];pn=sum(p);m=ref['cols'];n=ref['rows']
            response=min(range(m),key=lambda c:sum(p[r]/pn*ref['matrix'][r*m+c] for r in range(n)))
            counterexample=dict(case=case['id'],seed=1,elapsed_s=selected['elapsed_s'],
                local_gap=selected['local_gap'],reference_gap=last['quality']['reference_root_br_gap'],
                reference_gap_bounds=[last['quality']['reference_gap_lower'],last['quality']['reference_gap_upper']],
                dominant_row=max(range(n),key=lambda r:p[r]),dominant_row_probability=max(p)/pn,
                reference_best_response_col=response,response_action=ready['actions'][1][response],
                value_under_reference_best_response=sum(p[r]/pn*ref['matrix'][r*m+response] for r in range(n)),
                work=selected['work'])
    summary={}
    for mode,values in arms.items():
        summary[mode]=dict(first_policy_s_median=statistics.median(v['first_policy_s'] for v in values),
            peak_rss_mib_median=statistics.median(v['peak_rss_mib'] for v in values),
            last_reference_gap_median=statistics.median(v['quality']['reference_root_br_gap'] for v in values) if ref else None,
            stops=[v['stop'] for v in values],trials=values)
    environment=read(folder/'environment.json')
    cpu_models=sorted(set(line.split(':',1)[1].strip() for line in environment['cpuinfo'].splitlines() if line.startswith('model name')))
    case_summaries.append(dict(case=case['id'],cpu_models=cpu_models,reference_status=result['reference']['status'],
        reference_interval=[ref['lower'],ref['upper']] if ref else None,arms=summary))

wall=(datetime.fromisoformat(run['updated_at'].replace('Z','+00:00'))-datetime.fromisoformat(run['run_started_at'].replace('Z','+00:00'))).total_seconds()
result=dict(schema=1,status='PASS_MEASUREMENT_AUDIT_NOT_ADOPTION',source_sha=receipt['source_sha'],run=RUN,
    run_url=run['html_url'],run_wall_seconds=wall,completed_jobs=9,measured_trials=42,completed_references=5,
    fresh_rust_tests=62,python_protocol_tests=5,raw_policy_snapshots=snapshots,quality_records_recomputed=checked,
    package_manifest_sha256=sha(package/'manifest.json'),summary_sha256=sha(DATA/f's25-summary-{RUN}-1/summary.json'),
    cases=case_summaries,counterexample=counterexample,
    decision='Keep default OFF. Faster initial policies do not establish stronger play; severe finite-budget counterexample and node-limit memory costs observed.',
    limits=report['limitations']+['Peak RSS includes the pre-exec child and scenario setup; small-process values can have an inherited Python-memory floor.',
    'Snapshot timestamp is taken at the completed-policy callback before the current JSON write; it is search availability, not IPC delivery latency.'])
save(ROOT/f'AUDIT-{RUN}.json',result)
print(json.dumps({k:result[k] for k in ['status','run_wall_seconds','measured_trials','completed_references','fresh_rust_tests','raw_policy_snapshots','quality_records_recomputed','decision']},indent=2))
