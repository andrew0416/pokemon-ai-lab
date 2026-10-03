"""Clean path controls, separate instrumentation, portable same-binary speed pairs."""
from pathlib import Path
import argparse, copy, hashlib, importlib.util, json, math, os, platform, statistics, subprocess, sys

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('s22_summary', HERE.parent/'s26_pipeline/run.py')
old = importlib.util.module_from_spec(spec); spec.loader.exec_module(old)
write = old.write

def route(v):
    assert set(v) <= {'id', 'paper', 'pipeline', 'exact_control'}
    return json.dumps({k:x for k,x in v.items() if k not in ('id','exact_control')},sort_keys=True,separators=(',',':'))

def validate_plan(p):
    assert len(p['profile_cases']) == 8 and len(p['cases']) == 24
    assert sum(len(c['variants']) for c in p['cases']) == 64
    for c in p['cases']:
        b=c['baseline']; variants=c['variants']; group=c['comparison_group']
        if group in ('old-control','new-control'):
            assert route(b)==route(variants[0]), 'A/A must use identical API and parameters'
            assert variants[0]['exact_control']
        if group=='old-control':
            assert 'pipeline' not in b and variants[1]['pipeline']=={}
            assert b['paper']==variants[1]['paper']
        elif group=='new-control':
            assert b['pipeline']=={} and all('pipeline' in v for v in variants)
            assert all(b['paper']==v['paper'] for v in variants)
        elif group=='delta-control':
            assert b['pipeline']=={'owned_compiler':True}
            assert variants[0]['pipeline']=={'owned_compiler':True,'incremental_sequence':True}
        else: raise AssertionError(group)
    assert {c['cadence'] for c in p['profile_cases']}=={1,4}

def collect_witnesses(case, rows, out):
    workload=case.get('workload',case['id'])
    pre=[(case['baseline'], next(r for r in rows if r['type']=='baseline'))]
    pre += [(r['variant'], r) for r in rows if r['type']=='validation']
    assert len(pre)==1+len(case['variants'])
    for variant,row in pre:
        key=workload+'|'+route(variant)
        value=dict(witness=row['witness'],diagnostics=row['diagnostics'])
        if key in out: assert out[key]==value, 'same API arguments changed result or work'
        out[key]=value

def profile_summary(rows):
    grouped={}
    for r in rows:
        if r['type']!='profile':continue
        assert r['balanced']
        assert sum(p['exclusive_ns'] for p in r['phases'])==r['compute_ns']
        grouped.setdefault(r['variant'],[]).append(r)
    result=[]
    for variant, samples in grouped.items():
        result.append(dict(variant=variant,samples=len(samples),diagnostics=samples[0]['diagnostics'],
            compute_ms=statistics.median(r['compute_ns']/1e6 for r in samples),
            returned_result_drop_ms=statistics.median(r['returned_result_drop_ns']/1e6 for r in samples),
            phases=[dict(phase=p['phase'],calls=p['calls'],
                aggregate_exclusive_percent=100*sum(r['phases'][i]['exclusive_ns'] for r in samples)/sum(r['compute_ns'] for r in samples),
                exclusive_ms=statistics.median(r['phases'][i]['exclusive_ns']/1e6 for r in samples),
                inclusive_ms=statistics.median(r['phases'][i]['inclusive_ns']/1e6 for r in samples),
                exclusive_percent=statistics.median(100*r['phases'][i]['exclusive_ns']/r['compute_ns'] for r in samples))
                for i,p in enumerate(samples[0]['phases'])]))
    return result

def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
    p.add_argument('--mode',choices=['check','speed','profile','profile-check'],required=True)
    p.add_argument('--parity',type=Path);a=p.parse_args()
    timing=a.mode in ('speed','profile')
    if timing and os.environ.get('GITHUB_ACTIONS')!='true':raise SystemExit('Local performance runs are disabled. Use check or profile-check.')
    plan=json.loads((HERE/'plan.json').read_bytes());validate_plan(plan)
    binary=a.binary.resolve();repo=HERE.parents[2];a.out.mkdir(parents=True,exist_ok=False)
    env=dict(os.environ,LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',PYTHONHASHSEED='0')
    metadata=dict(platform=platform.platform(),python=sys.version,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
        source_sha=os.environ.get('GITHUB_SHA'),mode=a.mode,timing=timing,plan_sha256=hashlib.sha256((HERE/'plan.json').read_bytes()).hexdigest(),
        portable='-C target-cpu=x86-64; no AVX2 requirement',enumeration='flat',
        scope='compute excludes loading/JSON/returned result drop; phase mode separately reports result drop; transitions are caller wall time including scheduling/wait, not worker CPU sums',
        witness='streaming FNV-1a-128 and byte length of full Tree, Solution and diagnostic text; noncryptographic cross-build regression witness; evidence files use SHA256')
    if timing:
        allowed=sorted(os.sched_getaffinity(0));os.sched_setaffinity(0,set(allowed[:4]))
        metadata.update(available_cpus=allowed,parallel_cpus=allowed[:4],load_before=os.getloadavg(),cpuinfo=Path('/proc/cpuinfo').read_text().split('\n\n')[0])
    write(a.out/'environment.json',metadata)
    conditions=plan['cases'] if a.mode=='speed' else plan['profile_cases']
    statuses=[];results=[];profiles=[];witnesses={}
    flag={'speed':'--measure','check':'--check','profile':'--profile','profile-check':'--profile-check'}[a.mode]
    for case in conditions:
        request=copy.deepcopy(case)
        for world in request['worlds']:world['scenario']=(repo/world['scenario']).as_posix()
        for k in ('rounds','sample_ms','profile_rounds'):request[k]=plan[k]
        request_path=a.out/(case['id']+'-request.json');write(request_path,request)
        log=a.out/(case['id']+'-raw.jsonl');err=a.out/(case['id']+'-stderr.txt')
        with log.open('wb') as stdout,err.open('wb') as stderr:
            try: code=subprocess.run([str(binary),flag,str(request_path.resolve())],env=env,stdout=stdout,stderr=stderr,timeout=300).returncode
            except subprocess.TimeoutExpired:code=-999
        status=dict(case=case['id'],exit_code=code)
        if timing:
            import resource
            status['cumulative_child_peak_rss_kib']=resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            if status['cumulative_child_peak_rss_kib']>512*1024:code=status['exit_code']=-998
        if code==0:
            rows=[json.loads(s) for s in log.read_text().splitlines()]
            assert rows[-1]['type']=={'check':'checked','profile-check':'checked','speed':'completed','profile':'profiled'}[a.mode]
            collect_witnesses(case,rows,witnesses)
            if a.mode=='speed':
                found=old.summarize(rows)
                assert all(r['censored'] or len(r['pairs'])==plan['rounds'] for r in found)
                results.extend(dict(case=case['id'],workload=case['workload'],comparison_group=case['comparison_group'],**r) for r in found)
            elif a.mode=='profile':profiles.extend(dict(case=case['id'],**r) for r in profile_summary(rows))
            elif a.mode=='profile-check':
                measured=[r for r in rows if r['type']=='profile']
                assert len(measured)==1+len(case['variants'])
                assert all('compute_ns' not in r and all(set(p)=={'phase','calls'} for p in r['phases']) for r in measured)
        else:status['error']=err.read_text(errors='replace')[-2000:]
        statuses.append(status);print(json.dumps(status),flush=True)
        if code:break
    parity=False
    if a.parity:
        reference=json.loads(a.parity.read_bytes());assert reference['passed']
        assert witnesses==reference['witnesses'], 'instrumented/uninstrumented full result/work witnesses differ'
        parity=True
    passed=len(statuses)==len(conditions) and all(s['exit_code']==0 for s in statuses)
    summary=dict(passed=passed,timing=timing,mode=a.mode,source_sha=os.environ.get('GITHUB_SHA'),cases=statuses,
        results=results,profiles=profiles,witnesses=witnesses,cross_build_parity=parity,paired_rounds=plan['rounds'],
        wins_evaluated=False,engine_unchanged=True,search_algorithm_unchanged=True)
    if timing:summary['load_after']=os.getloadavg()
    write(a.out/'summary.json',summary)
    raise SystemExit(0 if passed else 1)

if __name__=='__main__':main()
