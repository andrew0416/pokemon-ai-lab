"""Offline reanalysis from original S27 worker transcripts. Python standard library only.

Does not execute engine code, access a network, or reproduce a new timing run.
Run from any directory: python -X utf8 -B <this-file> [--source-root checkout] [--deep]
"""
from pathlib import Path
import argparse,copy,hashlib,json,random,statistics,zipfile

HERE=Path(__file__).resolve().parent
def need(ok,message):
    if not ok:raise ValueError(message)
def load(path):return json.loads(path.read_bytes())
def sha(data):return hashlib.sha256(data).hexdigest()
def identity(row):return {k:row[k] for k in ('witness','semantic_witness','diagnostics')}
def semantic(row):
    d=copy.deepcopy(row['diagnostics']);d.pop('pipeline',None);d.pop('paper',None)
    return dict(semantic_witness=row['semantic_witness'],diagnostics=d)
def statistic(pairs):
    ratios=[v['ratio'] for v in pairs];rng=random.Random(26100427)
    boot=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(4000))
    return dict(ratio_median=statistics.median(ratios),ratio_bootstrap95=[boot[100],boot[3899]],
        time_reduction_percent=100*(1-statistics.median(ratios)),
        baseline_ms=statistics.median(v['baseline_ms'] for v in pairs),candidate_ms=statistics.median(v['candidate_ms'] for v in pairs),pairs=pairs)

def verify(source_root,deep=False):
    manifest=load(HERE/'MANIFEST.json')
    for rel,digest in manifest['files'].items():
        p=HERE/rel;need(p.resolve().is_relative_to(HERE),'manifest traversal')
        need(sha(p.read_bytes())==digest,'bundle hash mismatch: '+rel)
    current=load(HERE/'CURRENT.json');lineage=load(HERE/'LINEAGE.json');archives=load(HERE/'ARCHIVES.json')
    for record in archives:
        p=HERE/record['path'];need(sha(p.read_bytes())==record['sha256'],'archive identity')
        with zipfile.ZipFile(p) as z:
            need(len([i for i in z.infolist() if not i.is_dir()])==record['files'],'archive member count')
            if deep:need(z.testzip() is None,'archive CRC')
    selected=next(v for v in archives if v['experiment']=='s27-selected-stack-20261004')
    with zipfile.ZipFile(HERE/selected['path']) as z:
        read=lambda n:json.loads(z.read(n))
        lines=lambda n:[json.loads(v) for v in z.read(n).splitlines()]
        plan=load(HERE/'protocol/plan.json')
        check,speed,profile=[read(v+'/summary.json') for v in ('check','speed','profile')]
        pin=read('provenance.json')
        need(pin['source_sha']==current['experimental_head']==check['source_sha']==speed['source_sha']==profile['source_sha'],'source pin mismatch')
        need(pin['baseline_library']==current['direct_baseline'] and pin['selected_library']==current['selected_library'],'library pin mismatch')
        need(pin['selected_library_unchanged'] and pin['reference_library_unchanged'],'library changed')
        need(pin['cli_sha256']==sha((source_root/'engine/search/src/bin/lab-phase-speed.rs').read_bytes()),'source CLI mismatch')
        need((source_root/'engine/benchmarks/s26_stack/plan.json').read_bytes()==(HERE/'protocol/plan.json').read_bytes(),'source plan mismatch')
        need(all(v['passed'] for v in (check,speed,profile)),'upstream failure')
        need(not check['timing'] and not check['same_binary_local_check'] and speed['timing'] and profile['instrumented'],'wrong measurement mode')
        need(check['witnesses']==speed['witnesses'],'check/speed parity')
        need(len(check['cases'])==len(plan['cases'])==10 and all(v['eligible'] for v in check['cases']),'censored/changed case set')
        bins={Path(p).name:h for h,p in (v.split() for v in z.read('binaries.sha256').decode().splitlines())}
        need(len(set(bins.values()))==3,'binary reuse')
        for mode in ('check','speed'):
            need(read(mode+'/environment.json')['binary_sha256']=={k:bins[k] for k in ('baseline','selected')},'worker binary mismatch')
        need(profile['binary_sha256']==bins['profile'],'profile binary mismatch')
        results={(v['case'],v['comparison']):v for v in speed['results']}
        need(len(results)==32 and all(not v['censored'] for v in results.values()),'comparison count')
        samples_total=0;recomputed=[]
        for case in plan['cases']:
            name=case['id'];expected=check['witnesses'][name]
            need(semantic(expected['b'])==semantic(expected['s'])==semantic(expected['o']),name+' semantic mismatch')
            qs={}
            for version in ('baseline','selected'):
                raw=lines('speed/'+name+'-'+version+'-stdout.jsonl')
                need(raw[0]==dict(type='ready',timing=True),'worker header')
                qs[version]=iter(raw[1:])
            bindings={'b':'baseline','s':'selected','o':'selected'}
            def sample(key):
                row=next(qs[bindings[key]])
                need(identity(row)==expected[key],name+' per-sample parity')
                need(row['type']=='sample' and row['count']==1 and row['timing'] and row['ns_per_solve']>0,'invalid sample')
                return row['ns_per_solve']
            for key in 'bso':sample(key)
            pairs=[('baseline-aa','b','b'),('selected-aa','s','s'),('direct-stack','b','s')]
            if name==plan['preregistered']['followup_off_case']:pairs += [('off-aa','o','o'),('history-off-regression','b','o')]
            captures={label:[] for label,_,_ in pairs}
            for block in range(plan['rounds']):
                for offset in range(len(pairs)):
                    label,a,b=pairs[(offset+block)%len(pairs)]
                    values=[sample(key) for key in ([a,b,b,a] if block%2==0 else [b,a,a,b])]
                    am=statistics.mean([values[0],values[3]] if block%2==0 else values[1:3])
                    bm=statistics.mean(values[1:3] if block%2==0 else [values[0],values[3]])
                    captures[label].append(dict(block=block,baseline_ms=am/1e6,candidate_ms=bm/1e6,ratio=bm/am));samples_total+=4
            need(all(next(q,None) is None for q in qs.values()),'unconsumed speed rows')
            for label,_,_ in pairs:
                calc=statistic(captures[label]);saved=results[name,label]
                need(all(saved[k]==v for k,v in calc.items()),name+' statistics differ')
                recomputed.append(dict(case=name,comparison=label,**calc))
        need(samples_total==1152,'paired compute count')
        indexed={(v['case'],v['comparison']):v for v in recomputed};direct=[]
        def classify(row,labels):
            lo,hi=row['ratio_bootstrap95'];band=max(abs(v-1) for label in labels for v in indexed[row['case'],label]['ratio_bootstrap95'])
            return dict(case=row['case'],time_reduction_percent=row['time_reduction_percent'],baseline_ms=row['baseline_ms'],candidate_ms=row['candidate_ms'],
                ratio_bootstrap95=[lo,hi],direction='lower' if hi<1 else 'higher' if lo>1 else 'unresolved',
                outside_aa=hi<1-band or lo>1+band,aa_halfwidth_percent=100*band)
        for row in recomputed:
            if row['comparison']=='direct-stack':direct.append(classify(row,['baseline-aa','selected-aa']))
        off=classify(indexed[plan['preregistered']['followup_off_case'],'history-off-regression'],['baseline-aa','off-aa'])
        phase=[];captures=0
        for case in plan['cases']:
            name=case['id'];raw=lines('profile/'+name+'-stdout.jsonl')
            need(raw[-1]['type']=='profiled','profile footer')
            for variant,binding in [('off','o'),('selected-five','s')]:
                vals=[v for v in raw if v['type']=='profile' and v['variant']==variant]
                need(len(vals)==5,'profile count');captures+=len(vals)
                e=check['witnesses'][name][binding]
                for v in vals:
                    need(v['witness']==e['witness'] and v['diagnostics']==e['diagnostics'],'profile result parity')
                    need(v['balanced'] and len(v['phases'])==19 and sum(p['exclusive_ns'] for p in v['phases'])==v['compute_ns'],'unbalanced exclusive ledger')
                saved=next(v for v in profile['profiles'] if v['case']==name and v['variant']==variant)
                for i,p in enumerate(saved['phases']):
                    percent=100*sum(v['phases'][i]['exclusive_ns'] for v in vals)/sum(v['compute_ns'] for v in vals)
                    need(p['aggregate_exclusive_percent']==percent and p['exclusive_ms']==statistics.median(v['phases'][i]['exclusive_ns']/1e6 for v in vals),'phase summary mismatch')
                    need(all(v['phases'][i]['calls']==p['calls'] for v in vals),'phase calls differ')
                if variant=='selected-five':
                    obs=next(v for v in saved['phases'] if v['phase']=='observation')
                    k=min(case['threads'],4,e['diagnostics']['growth']['attempted_transitions'])
                    phase.append(dict(case=name,observation_percent=obs['aggregate_exclusive_percent'],calls=obs['calls'],
                        optimistic_parallelism=k,optimistic_whole_reduction_percent=obs['aggregate_exclusive_percent']*(1-1/k)))
        need(captures==100,'capture count')
    # These verdicts are derived, never used as input to the arithmetic above.
    good=[v for v in direct if v['direction']=='lower' and v['outside_aa']]
    bad=[v for v in direct if v['direction']=='higher' and v['outside_aa']]
    scenarios=sorted({w['scenario'] for c in plan['cases'] for w in c['worlds']})
    return dict(passed=True,source=current['experimental_head'],archives_verified=len(archives),deep_crc=deep,
        distinct_top_level_scenarios=len(scenarios),scenario_paths=scenarios,
        compared_cases=len(direct),paired_blocks=len(recomputed)*9,paired_compute_samples=samples_total,phase_captures=captures,
        faster_count=len(good),faster_reduction_range=[min(v['time_reduction_percent'] for v in good),max(v['time_reduction_percent'] for v in good)],
        regressions=bad,off_repeat=off,direct=direct,phases=phase,
        next_candidate_implemented=False,production_adopted=False,
        scope='Offline S27 raw-data reanalysis; earlier archives integrity checked, earlier experiment statistics not all recomputed; no new engine/timing/oracle run.')

def main():
    p=argparse.ArgumentParser();p.add_argument('--source-root',type=Path,default=HERE.parents[2]);p.add_argument('--deep',action='store_true');p.add_argument('--output',type=Path)
    args=p.parse_args();result=verify(args.source_root.resolve(),args.deep)
    if args.output:
        with args.output.open('x',encoding='utf8') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
    print(json.dumps(result,ensure_ascii=False,indent=2))
if __name__=='__main__':main()
