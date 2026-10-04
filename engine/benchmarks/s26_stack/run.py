"""Direct cumulative comparison of frozen libraries, with separately measured phases."""
from pathlib import Path
import argparse,copy,hashlib,json,os,platform,queue,random,statistics,subprocess,threading
HERE=Path(__file__).resolve().parent

def write(path,value):
    path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')

def normalized(row):
    value=copy.deepcopy(row['diagnostics'])
    value.pop('pipeline',None);value.pop('paper',None)
    return dict(semantic_witness=row['semantic_witness'],diagnostics=value)

def identity(row):
    return {key:row[key] for key in ('witness','semantic_witness','diagnostics')}

def summarize(pairs):
    ratios=[v['ratio'] for v in pairs];rng=random.Random(26100427)
    samples=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(4000))
    ratio=statistics.median(ratios)
    return dict(ratio_median=ratio,time_reduction_percent=100*(1-ratio),ratio_bootstrap95=[samples[100],samples[3899]],
        baseline_ms=statistics.median(v['baseline_ms'] for v in pairs),candidate_ms=statistics.median(v['candidate_ms'] for v in pairs),pairs=pairs)

class Worker:
    def __init__(self,binary,request,prefix,timing):
        self.log=Path(str(prefix)+'-stdout.jsonl').open('w',encoding='utf-8')
        self.err=Path(str(prefix)+'-stderr.txt').open('w',encoding='utf-8')
        self.queue=queue.Queue()
        self.process=subprocess.Popen([str(binary.resolve()),'--worker' if timing else '--worker-check',str(request.resolve())],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.err,text=True,encoding='utf-8',bufsize=1)
        def reader():
            for line in self.process.stdout:
                self.log.write(line);self.log.flush();self.queue.put(line)
            self.queue.put(None)
        self.thread=threading.Thread(target=reader,daemon=True);self.thread.start()
        assert self.receive()==dict(type='ready',timing=timing)
    def receive(self):
        line=self.queue.get(timeout=180)
        assert line is not None,'worker terminated; inspect stderr'
        return json.loads(line)
    def ask(self,variant):
        self.process.stdin.write(json.dumps(dict(variant=variant,count=1))+'\n');self.process.stdin.flush()
        return self.receive()
    def close(self):
        self.process.stdin.close()
        try:self.process.wait(timeout=15)
        except subprocess.TimeoutExpired:self.process.kill();self.process.wait();raise
        self.thread.join(timeout=5);self.log.close();self.err.close()
        assert self.process.returncode==0

def request(case):
    value=copy.deepcopy(case)
    for world in value['worlds']:world['scenario']=(HERE.parents[2]/world['scenario']).as_posix()
    return value

def compare(args,plan,reference):
    timing=args.mode=='speed'
    binaries=dict(baseline=args.reference or args.selected,selected=args.selected)
    env=dict(source_sha=os.environ.get('GITHUB_SHA'),timing=timing,platform=platform.platform(),
        same_binary_local_check=args.reference is None,rounds=plan['rounds'] if timing else 0,
        binary_sha256={key:hashlib.sha256(value.read_bytes()).hexdigest() for key,value in binaries.items()},
        library_pins={key:plan[key] for key in ('baseline_source','selected_library_source')},
        scope='compute only; input/pool load, witness, JSON and result destruction excluded; destruction finishes before next request')
    if timing:
        assert args.reference is not None and reference['passed'] and not reference['timing']
        cpus=sorted(os.sched_getaffinity(0));os.sched_setaffinity(0,set(cpus[:4]))
        env.update(cpus=cpus[:4],load_before=os.getloadavg(),cpuinfo=Path('/proc/cpuinfo').read_text().split('\n\n')[0])
    write(args.out/'environment.json',env)
    bindings={'b':('baseline',plan['off']),'s':('selected',plan['selected']),'o':('selected',plan['off'])}
    witnesses={};cases=[];results=[]
    for case in plan['cases']:
        path=args.out/(case['id']+'-request.json');write(path,request(case));workers={}
        pairs=[('baseline-aa','b','b'),('selected-aa','s','s'),('direct-stack','b','s')]
        if case['id']==plan['preregistered']['followup_off_case']:
            pairs += [('off-aa','o','o'),('history-off-regression','b','o')]
        captures={name:[] for name,_,_ in pairs};expected={}
        try:
            for name,binary in binaries.items():workers[name]=Worker(binary,path,args.out/(case['id']+'-'+name),timing)
            for key,(name,variant) in bindings.items():
                row=workers[name].ask(variant);expected[key]=identity(row)
                assert row['type']=='sample' and row['timing']==timing
                if reference:assert expected[key]==reference['witnesses'][case['id']][key],(case['id'],key,'cross-build result mismatch')
            assert normalized(expected['b'])==normalized(expected['s'])==normalized(expected['o']),(case['id'],'tree/solution/work mismatch')
            eligible=all(v['diagnostics']['complete'] and v['diagnostics']['converged'] for v in expected.values())
            witnesses[case['id']]=expected
            cases.append(dict(case=case['id'],coverage=case['coverage'],eligible=eligible,comparisons=[name for name,_,_ in pairs]))
            if timing and eligible:
                for block in range(plan['rounds']):
                    for offset in range(len(pairs)):
                        name,ak,bk=pairs[(offset+block)%len(pairs)];values=[]
                        keys=[ak,bk,bk,ak] if block%2==0 else [bk,ak,ak,bk]
                        for key in keys:
                            version,variant=bindings[key];row=workers[version].ask(variant)
                            assert identity(row)==expected[key] and row['count']==1 and row['ns_per_solve']>0
                            values.append(row['ns_per_solve'])
                        a=statistics.mean([values[0],values[3]] if block%2==0 else values[1:3])
                        b=statistics.mean(values[1:3] if block%2==0 else [values[0],values[3]])
                        captures[name].append(dict(block=block,baseline_ms=a/1e6,candidate_ms=b/1e6,ratio=b/a))
                results.extend(dict(case=case['id'],coverage=case['coverage'],comparison=name,censored=False,**summarize(captures[name])) for name,_,_ in pairs)
            elif timing:
                results.extend(dict(case=case['id'],coverage=case['coverage'],comparison=name,censored=True,reason='incomplete or solver cap') for name,_,_ in pairs)
        finally:
            for worker in workers.values():worker.close()
        print(json.dumps(cases[-1]),flush=True)
    write(args.out/'summary.json',dict(passed=True,timing=timing,source_sha=os.environ.get('GITHUB_SHA'),
        same_binary_local_check=args.reference is None,cases=cases,witnesses=witnesses,results=results,default_off=True,
        library_unchanged=True,wins_evaluated=False,scope=env['scope']))

def profile(args,plan,reference):
    assert reference and reference['passed'];profiles=[]
    for case in plan['cases']:
        q=request(case);q['profile_rounds']=plan['profile_rounds']
        path=args.out/(case['id']+'-request.json');write(path,q)
        child=subprocess.run([str(args.selected.resolve()),'--profile',str(path.resolve())],capture_output=True,timeout=180)
        (args.out/(case['id']+'-stdout.jsonl')).write_bytes(child.stdout)
        (args.out/(case['id']+'-stderr.txt')).write_bytes(child.stderr)
        assert child.returncode==0,(case['id'],child.stderr.decode(errors='replace'))
        rows=[json.loads(line) for line in child.stdout.splitlines()]
        assert rows[-1]['type']=='profiled'
        for variant,binding in [('off','o'),('selected-five','s')]:
            samples=[v for v in rows if v['type']=='profile' and v['variant']==variant]
            assert len(samples)==plan['profile_rounds']
            for value in samples:
                expected=reference['witnesses'][case['id']][binding]
                assert value['witness']==expected['witness'] and value['diagnostics']==expected['diagnostics']
                assert value['balanced'] and sum(v['exclusive_ns'] for v in value['phases'])==value['compute_ns']
            phases=[dict(phase=v['phase'],calls=v['calls'],exclusive_ms=statistics.median(s['phases'][i]['exclusive_ns']/1e6 for s in samples),
                aggregate_exclusive_percent=100*sum(s['phases'][i]['exclusive_ns'] for s in samples)/sum(s['compute_ns'] for s in samples)) for i,v in enumerate(samples[0]['phases'])]
            profiles.append(dict(case=case['id'],coverage=case['coverage'],variant=variant,samples=len(samples),
                compute_ms=statistics.median(v['compute_ns']/1e6 for v in samples),phases=phases,diagnostics=samples[0]['diagnostics']))
        print(json.dumps(dict(case=case['id'],profiles_passed=True)),flush=True)
    write(args.out/'summary.json',dict(passed=True,timing=True,instrumented=True,source_sha=os.environ.get('GITHUB_SHA'),profiles=profiles,
        scope='separate phase diagnostics, never used as clean speed ratios',binary_sha256=hashlib.sha256(args.selected.read_bytes()).hexdigest()))

def main():
    p=argparse.ArgumentParser();p.add_argument('--selected',type=Path,required=True);p.add_argument('--reference',type=Path)
    p.add_argument('--out',type=Path,required=True);p.add_argument('--mode',choices=['check','speed','profile'],required=True)
    p.add_argument('--parity',type=Path);args=p.parse_args()
    if args.mode!='check':assert os.environ.get('GITHUB_ACTIONS')=='true','No local performance runs'
    args.out.mkdir(parents=True,exist_ok=False)
    os.environ.update(LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',OMP_NUM_THREADS='1')
    plan=json.loads((HERE/'plan.json').read_bytes());assert len(plan['cases'])==10 and plan['rounds']==9
    assert set(plan['selected']['pipeline'])=={'frontier_index','owned_compiler','incremental_sequence','compressed_checks','encoded_history'}
    assert all(plan['selected']['pipeline'].values()) and plan['off']['pipeline']=={}
    ref=json.loads(args.parity.read_bytes()) if args.parity else None
    if args.mode=='profile':profile(args,plan,ref)
    else:compare(args,plan,ref)

if __name__=='__main__':main()
