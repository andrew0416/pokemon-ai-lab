"""Bounded single-runner speed-only comparison; paired blocks, one release binary."""
from pathlib import Path
import argparse,copy,hashlib,json,math,os,platform,random,statistics,subprocess,sys,time

def write(p,v):p.write_text(json.dumps(v,indent=2)+'\n',encoding='utf-8')
def summarize(rows):
    base=next(r['diagnostics'] for r in rows if r['type']=='baseline');results=[]
    for validation in [r for r in rows if r['type']=='validation']:
        variant=validation['variant'];pairs=[]
        for r in rows:
            if r['type']!='paired' or r['variant']!=variant['id']:continue
            b=statistics.mean(s['ns_per_solve'] for s in r['samples'] if s['treatment']=='baseline')
            c=statistics.mean(s['ns_per_solve'] for s in r['samples'] if s['treatment']=='candidate')
            assert min(b,c)>0 and math.isfinite(b+c)
            pairs.append(dict(block=r['block'],baseline_ms=b/1e6,candidate_ms=c/1e6,ratio=c/b))
        rng=random.Random(261003);ratios=[p['ratio'] for p in pairs]
        bootstrap=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(4000))
        ratio=statistics.median(ratios)
        results.append(dict(variant=variant,diagnostics=validation['diagnostics'],pairs=pairs,
            baseline_ms=statistics.median(p['baseline_ms'] for p in pairs),candidate_ms=statistics.median(p['candidate_ms'] for p in pairs),
            ratio_median=ratio,time_reduction_percent=100*(1-ratio),speedup=1/ratio,
            ratio_bootstrap95=[bootstrap[100],bootstrap[3899]],baseline_diagnostics=base,
            comparison=variant.get('comparison','exact-same-work')))
    return results

def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--check',action='store_true');a=p.parse_args()
    if not a.check and os.environ.get('GITHUB_ACTIONS')!='true':raise SystemExit('Timing is restricted to the authorized GitHub runner; use --check locally.')
    here=Path(__file__).resolve().parent;repo=here.parents[2];plan=json.loads((here/'plan.json').read_bytes());binary=a.binary.resolve()
    a.out.mkdir(parents=True,exist_ok=False)
    env=dict(os.environ,LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1')
    metadata=dict(platform=platform.platform(),python=sys.version,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
        source_sha=os.environ.get('GITHUB_SHA'),runner_os=os.environ.get('RUNNER_OS'),timing=not a.check,
        mode='generic x86-64 release; one binary runtime selection',enumeration='flat; LAB_ENGINE_FACTORED=0',plan_sha256=hashlib.sha256((here/'plan.json').read_bytes()).hexdigest())
    if not a.check:
        allowed=sorted(os.sched_getaffinity(0));os.sched_setaffinity(0,set(allowed[:4]))
        metadata.update(available_cpus=allowed,pinned_cpu=allowed[0],parallel_cpus=allowed[:4],load_before=os.getloadavg(),
            cpuinfo=Path('/proc/cpuinfo').read_text().split('\n\n')[0],sample_scope='build + CFR to result; excludes loading/JSON/final result destruction')
    write(a.out/'environment.json',metadata);all_results=[];statuses=[]
    for case in plan['cases']:
        request=copy.deepcopy(case)
        for world in request['worlds']:world['scenario']=(repo/world['scenario']).as_posix()
        request['rounds']=plan['rounds'];request['sample_ms']=plan['sample_ms']
        path=a.out/(case['id']+'-request.json');write(path,request)
        log=a.out/(case['id']+'-raw.jsonl');error=a.out/(case['id']+'-stderr.txt')
        with log.open('wb') as stdout,error.open('wb') as stderr:
            try:
                run=subprocess.run([str(binary),'--check' if a.check else '--measure',str(path.resolve())],stdout=stdout,stderr=stderr,env=env,timeout=300)
                code=run.returncode
            except subprocess.TimeoutExpired:code=-999
        status=dict(case=case['id'],exit_code=code)
        if not a.check:
            import resource
            peak=resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            status['cumulative_child_peak_rss_kib']=peak
            if peak > 512*1024: code=status['exit_code']=-998
        if code==0:
            rows=[json.loads(s) for s in log.read_text().splitlines()]
            assert rows[-1]['type']==('checked' if a.check else 'completed')
            if not a.check:
                results=summarize(rows)
                assert all(len(r['pairs'])==plan['rounds'] for r in results)
                all_results.extend(dict(case=case['id'],**r) for r in results)
        else:status['error']=error.read_text(errors='replace')[-3000:]
        statuses.append(status);print(json.dumps(status),flush=True)
    passed=all(s['exit_code']==0 for s in statuses)
    summary=dict(passed=passed,timing=not a.check,paired_rounds=plan['rounds'],cases=statuses,results=all_results,
        wins_evaluated=False,engine_unchanged=False,search_workspace_candidate=True,owned_transition_candidate=True,source_sha=os.environ.get('GITHUB_SHA'))
    if not a.check:summary['load_after']=os.getloadavg()
    write(a.out/'summary.json',summary)
    if not a.check:
        lines=['# S26e growth-cadence speed-only benchmark','',
            'One generic x86-64 release binary; serial baseline and bounded Rayon pools share up to four permitted runner CPUs. Nine ABBA/BAAB blocks; fixed identical selection seed.',
            'Intervals describe paired block variability on this runner, not variability between machines. Same-work pairs preserve all counters; finite-game pairs preserve the entire canonical tree and policy but growing performs intermediate solves.','',
            '| Case | Variant | Baseline ms | Candidate ms | Time reduction | Complete horizon |',
            '|---|---|---:|---:|---:|---|']
        for r in all_results:
            lines.append(f"| {r['case']} | {r['variant']['id']} | {r['baseline_ms']:.3f} | {r['candidate_ms']:.3f} | {r['time_reduction_percent']:+.2f}% | {r['diagnostics']['horizon_complete']} |")
        (a.out/'RESULTS.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
    raise SystemExit(0 if passed else 1)
if __name__=='__main__':main()
