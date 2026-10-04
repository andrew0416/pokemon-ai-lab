"""Reconstruct paired samples and phase rows from immutable runner transcripts."""
from pathlib import Path
from datetime import datetime
import copy, hashlib, importlib.util, json, statistics, sys

R=Path(__file__).resolve().parent;P=R/'publication';run=int(sys.argv[1])
folder=P/f'run-{run}'/f's26l-stack-{run}-1'
read=lambda p:json.loads(p.read_bytes())
receipt=read(P/'receipt.json');download=read(folder/'download-receipt.json')
plan=read(R/'source/engine/benchmarks/s26_stack/plan.json')
spec=importlib.util.spec_from_file_location('stack_run',R/'source/engine/benchmarks/s26_stack/run.py')
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
def blob(name):
    data=(folder/name).read_bytes()
    assert hashlib.sha256(data).hexdigest()==download['files'][name],name
    return data
def load(name):return json.loads(blob(name))
def lines(name):return [json.loads(v) for v in blob(name).splitlines()]
def save(name,value):
    with (R/name).open('x',encoding='utf8') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')

for name in download['files']:blob(name)
status=[read(p) for p in sorted(P.glob('status-*.json')) if read(p).get('id')==run][-1]
assert status['status']=='completed' and status['conclusion']=='success'
assert status['head_sha']==receipt['source_sha']==blob('source-sha.txt').decode().strip()
prov=load('provenance.json')
assert prov['baseline_library']==plan['baseline_source'] and prov['selected_library']==receipt['parent']==plan['selected_library_source']
assert prov['source_sha']==receipt['source_sha'] and prov['selected_library_unchanged'] and prov['reference_library_unchanged']
assert prov['cli_sha256']==receipt['changed_sha256']['engine/search/src/bin/lab-phase-speed.rs']
assert set(prov['source_changes'])==set(receipt['changed_sha256'])
check,speed,profile=[load(p+'/summary.json') for p in ('check','speed','profile')]
assert all(v['passed'] and v['source_sha']==receipt['source_sha'] for v in (check,speed,profile))
assert not check['timing'] and not check['same_binary_local_check']
assert speed['timing'] and not speed['same_binary_local_check'] and profile['instrumented']
assert check['witnesses']==speed['witnesses'] and len(check['cases'])==10
assert all(v['eligible'] for v in check['cases'])
binaries={Path(p).name:sha for sha,p in (v.split() for v in blob('binaries.sha256').decode().splitlines())}
assert len(set(binaries.values()))==3
for mode in ('check','speed'):
    env=load(mode+'/environment.json')
    assert env['binary_sha256']=={key:binaries[key] for key in ('baseline','selected')}
assert profile['binary_sha256']==binaries['profile']
rows={(v['case'],v['comparison']):v for v in speed['results']}
assert len(rows)==32 and all(not v['censored'] for v in rows.values())
sample_count=0
for case in plan['cases']:
    id=case['id'];expected=check['witnesses'][id]
    assert h.normalized(expected['b'])==h.normalized(expected['s'])==h.normalized(expected['o'])
    queues={}
    for version in ('baseline','selected'):
        checked=lines(f'check/{id}-{version}-stdout.jsonl')
        assert checked[0]==dict(type='ready',timing=False)
        permitted=[expected['b']] if version=='baseline' else [expected['s'],expected['o']]
        assert all(h.identity(v) in permitted and v['timing'] is False for v in checked[1:])
        raw=lines(f'speed/{id}-{version}-stdout.jsonl')
        assert raw[0]==dict(type='ready',timing=True)
        assert all(h.identity(v) in permitted and v['timing'] and v['count']==1 and v['ns_per_solve']>0 for v in raw[1:])
        # The initial request for each binding is warmup/check, outside the paired blocks.
        queues[version]=iter(raw[1:])
    bindings={'b':'baseline','s':'selected','o':'selected'}
    for key in 'bso':assert h.identity(next(queues[bindings[key]]))==expected[key]
    pairs=[('baseline-aa','b','b'),('selected-aa','s','s'),('direct-stack','b','s')]
    if id==plan['preregistered']['followup_off_case']:
        pairs += [('off-aa','o','o'),('history-off-regression','b','o')]
    reconstructed={name:[] for name,_,_ in pairs}
    for block in range(plan['rounds']):
        for offset in range(len(pairs)):
            name,a,b=pairs[(offset+block)%len(pairs)]
            keys=[a,b,b,a] if block%2==0 else [b,a,a,b];values=[]
            for key in keys:
                v=next(queues[bindings[key]])
                assert h.identity(v)==expected[key]
                values.append(v['ns_per_solve']);sample_count+=1
            am=statistics.mean([values[0],values[3]] if block%2==0 else values[1:3])
            bm=statistics.mean(values[1:3] if block%2==0 else [values[0],values[3]])
            reconstructed[name].append(dict(block=block,baseline_ms=am/1e6,candidate_ms=bm/1e6,ratio=bm/am))
    for q in queues.values():assert next(q,None) is None
    for name,_,_ in pairs:
        expected_summary=h.summarize(reconstructed[name]);actual=rows[id,name]
        assert all(actual[k]==v for k,v in expected_summary.items()),(id,name)
assert sample_count==32*9*4

for row in rows.values():
    lo,hi=row['ratio_bootstrap95']
    row['within_run_direction']='lower' if hi<1 else 'higher' if lo>1 else 'unresolved'
    labels=['baseline-aa','selected-aa'] if row['comparison']=='direct-stack' else ['baseline-aa','off-aa']
    if row['comparison'].endswith('-aa'):labels=[row['comparison']]
    band=max(abs(v-1) for label in labels for v in rows[row['case'],label]['ratio_bootstrap95'])
    row['aa_envelope_percent']=100*band
    row['outside_aa_envelope']=hi<1-band or lo>1+band

phase_rows=[];captures=0
for case in plan['cases']:
    id=case['id'];raw=lines('profile/'+id+'-stdout.jsonl')
    assert raw[-1]['type']=='profiled'
    for variant,binding in [('off','o'),('selected-five','s')]:
        samples=[v for v in raw if v['type']=='profile' and v['variant']==variant]
        assert len(samples)==5;captures+=len(samples)
        expected=check['witnesses'][id][binding]
        for s in samples:
            assert s['witness']==expected['witness'] and s['diagnostics']==expected['diagnostics']
            assert s['balanced'] and len(s['phases'])==19
            assert sum(p['exclusive_ns'] for p in s['phases'])==s['compute_ns']
        actual=next(v for v in profile['profiles'] if v['case']==id and v['variant']==variant)
        assert actual['compute_ms']==statistics.median(v['compute_ns']/1e6 for v in samples)
        for i,p in enumerate(actual['phases']):
            assert p['phase']==samples[0]['phases'][i]['phase']
            assert p['exclusive_ms']==statistics.median(v['phases'][i]['exclusive_ns']/1e6 for v in samples)
            assert p['aggregate_exclusive_percent']==100*sum(v['phases'][i]['exclusive_ns'] for v in samples)/sum(v['compute_ns'] for v in samples)
            assert all(v['phases'][i]['calls']==p['calls'] for v in samples)
        phase_rows.append(actual)
assert len(phase_rows)==20 and captures==100
direct=[v for v in rows.values() if v['comparison']=='direct-stack']
regression=rows[plan['preregistered']['followup_off_case'],'history-off-regression']
started=datetime.fromisoformat(status['run_started_at'].replace('Z','+00:00'))
finished=max(datetime.fromisoformat(v['completed_at'].replace('Z','+00:00')) for v in status['jobs'])
result=dict(passed=True,run_id=run,run_url=status['html_url'],source_sha=receipt['source_sha'],baseline=plan['baseline_source'],
    selected_library=plan['selected_library_source'],elapsed_ci_seconds=(finished-started).total_seconds(),
    eligible_cases=10,heldout_cases=2,comparisons=32,aa_comparisons=21,paired_blocks=288,paired_compute_samples=sample_count,
    profile_captures=captures,profile_rows=len(phase_rows),phase_count=19,direct=direct,history_off_repeat=regression,
    profiles=phase_rows,all_speed=list(rows.values()),binary_sha256=binaries,
    new_validation='focused CLI build/clippy and cross-version full tree/solution/work witnesses only',
    reused_validation='S24/S25 libraries byte-identical; prior oracle and scalar/non-AVX2 evidence reused, not rerun',
    caveats=['one hosted runner session; bounded fixtures; unadjusted bootstrap intervals and descriptive A/A envelope',
        'five recent pipeline flags over common retained engine/storage/solver configuration, not entire project history',
        'compute excludes input/pool load, witness, JSON and returned result destruction',
        'phase timing uses a separate instrumented binary; not headline speed',
        'FNV128 witnesses are noncryptographic, not exhaustive proof of future mechanics'],
    default_off=True,production_adopted=False,win_rates_evaluated=False)
save('RESULTS.json',result)
table=['|条件|基準 ms|5項 ON ms|時間短縮|比率95%区間|A/A外|','|---|---:|---:|---:|---|---|']
for v in direct:
    table.append(f'|{v["case"]}|{v["baseline_ms"]:.3f}|{v["candidate_ms"]:.3f}|{v["time_reduction_percent"]:+.2f}%|{v["ratio_bootstrap95"][0]:.4f}–{v["ratio_bootstrap95"][1]:.4f}|{v["outside_aa_envelope"]}|')
table[0]='|조건|기준 ms|5개 ON ms|시간 단축|비율 95% 구간|A/A 밖|'
text=f'# S27 유지 5개 옵션의 직접 누적 비교\n\n[GitHub 실행]({result["run_url"]}), 소스 `{result["source_sha"]}`, {result["elapsed_ci_seconds"]:.0f}초.\n\n'
text+='S24 OFF에서 S25 유지 5개 ON으로 직접 비교했다. 공통 엔진·저장·LCFR 설정은 고정했다. 프로젝트 초기부터의 전체 누적 효과가 아니다.\n\n'+'\n'.join(table)+'\n\n'
text+=f'OFF 반복 조건: {regression["case"]}, 시간 변화 {100*(regression["ratio_median"]-1):+.2f}%, 비율 구간 {regression["ratio_bootstrap95"]}. 이전 원인을 해결했다는 뜻은 아니다.\n\n'
text+='10조건 모두 complete/converged 및 교차 버전 tree·solution·탐색량 지문 일치. 32비교(21 A/A), 288블록, 1152 계산 sample을 원시 통신 기록에서 재구성했다. 별도 100회 phase 계측의 19개 배타 시간 합계도 확인했다.\n\n'
text+='새 라이브러리 변경이 없어 기존 S24/S25 BR/LP·일반·비AVX2 검증을 재사용한다. 이번에 그 테스트를 재실행했다고 세지 않는다. 비암호학적 지문은 완전한 증명이 아니다.\n\n'
text+='한 러너의 한정된 표본이며 95% 구간은 다중 비교 보정 전이다. A/A는 설명용 흔들림 기준이다. 반환물 파괴는 다음 요청 전에 완료하지만 계산 시간에서는 제외한다. 승률·판단 품질·메모리는 이번에 측정하지 않았다. 기본 OFF, 생산 미채택.\n'
(R/'RESULTS.md').write_text(text,encoding='utf8')
print(json.dumps(dict(passed=True,run_id=run,elapsed_seconds=result['elapsed_ci_seconds'],
    reductions=[dict(case=v['case'],percent=round(v['time_reduction_percent'],3),direction=v['within_run_direction'],outside_aa=v['outside_aa_envelope']) for v in direct],
    off_change_percent=100*(regression['ratio_median']-1)),ensure_ascii=False))
