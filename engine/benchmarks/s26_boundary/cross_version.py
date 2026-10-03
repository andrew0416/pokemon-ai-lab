"""Interleave pinned versions with an identical, benchmark-only worker transport."""
from pathlib import Path
import argparse,copy,hashlib,json,os,platform,queue,random,statistics,subprocess,threading
import run as common
HERE=Path(__file__).resolve().parent
ALL4=dict(frontier_index=True,owned_compiler=True,incremental_sequence=True,compressed_checks=True)
PAPER=dict(rule='lcfr',checks='periodic',sequence=True)
def option(name,flags):return dict(id=name,paper=PAPER,pipeline=flags)
OFF=option('new-off',{});ALL5=option('all-history',dict(ALL4,encoded_history=True));ALL6=option('all-six',dict(ALL4,encoded_history=True,borrowed_boundary=True))
def identity(row):return dict(witness=row['witness'],diagnostics=row['diagnostics'])
def summary(samples):
 ratios=[x['ratio'] for x in samples];rng=random.Random(261006)
 bootstrap=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(4000));ratio=statistics.median(ratios)
 return dict(ratio_median=ratio,time_reduction_percent=100*(1-ratio),ratio_bootstrap95=[bootstrap[100],bootstrap[3899]],pairs=samples)
def request(case):
 q=copy.deepcopy(case)
 for w in q['worlds']:w['scenario']=(HERE.parents[2]/w['scenario']).as_posix()
 return q
class Worker:
 def __init__(self,binary,path,prefix,timing):
  self.log=Path(str(prefix)+'-stdout.jsonl').open('w',encoding='utf-8');self.err=Path(str(prefix)+'-stderr.txt').open('w',encoding='utf-8')
  self.q=queue.Queue();self.p=subprocess.Popen([str(binary.resolve()),'--worker' if timing else '--worker-check',str(path.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.err,text=True,encoding='utf-8',bufsize=1)
  def reader():
   for line in self.p.stdout:self.log.write(line);self.log.flush();self.q.put(line)
   self.q.put(None)
  self.thread=threading.Thread(target=reader,daemon=True);self.thread.start()
  assert self.receive()==dict(type='ready',timing=timing)
 def receive(self):
  line=self.q.get(timeout=180)
  assert line is not None,'worker terminated; inspect saved stderr'
  return json.loads(line)
 def ask(self,variant):
  self.p.stdin.write(json.dumps(dict(variant=variant,count=1))+'\n');self.p.stdin.flush();return self.receive()
 def close(self):
  self.p.stdin.close()
  try:self.p.wait(timeout=15)
  except subprocess.TimeoutExpired:self.p.kill();self.p.wait();raise
  self.thread.join(timeout=5);self.log.close();self.err.close();assert self.p.returncode==0
def main():
 p=argparse.ArgumentParser();p.add_argument('--s24',type=Path);p.add_argument('--s25',type=Path);p.add_argument('--current',type=Path,required=True);p.add_argument('--parity',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--self-check',action='store_true');a=p.parse_args()
 timing=not a.self_check
 if timing:assert os.environ.get('GITHUB_ACTIONS')=='true'
 a.out.mkdir(parents=True,exist_ok=False);plan=json.loads((HERE/'plan.json').read_bytes());ref=json.loads(a.parity.read_bytes());assert ref['passed']
 env=dict(LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1',OMP_NUM_THREADS='1');os.environ.update(env)
 paths=dict(s24=a.s24 or a.current,s25=a.s25 or a.current,current=a.current)
 meta=dict(source_sha=os.environ.get('GITHUB_SHA'),timing=timing,platform=platform.platform(),binaries={k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in paths.items()},sample_count=1,rounds=9 if timing else 1,scope='one warmed compute; inputs, pool, stdin, witness/JSON and result drop excluded; separate persistent processes receive interleaved commands')
 if timing:
  allowed=sorted(os.sched_getaffinity(0));os.sched_setaffinity(0,set(allowed[:4]));meta.update(available_cpus=allowed,parallel_cpus=allowed[:4],load_before=os.getloadavg(),cpuinfo=Path('/proc/cpuinfo').read_text().split('\n\n')[0])
 common.write(a.out/'environment.json',meta)
 bindings={'a':('s24',OFF),'b':('s25',OFF),'c':('current',OFF),'d':('s25',ALL5),'e':('current',ALL5),'f':('current',ALL6)}
 pairs=[('s24-aa','a','a'),('s24-to-s25-off','a','b'),('s25-aa','b','b'),('s25-to-current-off','b','c'),('s25-history-aa','d','d'),('s25-to-current-history','d','e')]
 results=[];witnesses={}
 cases=plan['profile_cases'] if timing else [plan['profile_cases'][-1]]
 for case in cases:
  path=a.out/(case['id']+'-request.json');common.write(path,request(case));workers={};expected={};samples={name:[] for name,_,_ in pairs}
  try:
   for name,binary in paths.items():workers[name]=Worker(binary,path,a.out/(case['id']+'-'+name),timing)
   for key,(version,var) in bindings.items():
    row=workers[version].ask(var);expect=ref['witnesses'][case['id']+'|'+common.route(var)]
    assert identity(row)==expect,(case['id'],key,'preflight mismatch');expected[key]=expect
   witnesses[case['id']]=expected
   for block in range(meta['rounds']):
    for offset in range(len(pairs)):
     name,ak,bk=pairs[(offset+block)%len(pairs)];values=[]
     for key in ([ak,bk,bk,ak] if block%2==0 else [bk,ak,ak,bk]):
      version,var=bindings[key];row=workers[version].ask(var);assert identity(row)==expected[key]
      assert ('ns_per_solve' in row)==timing
      if timing:values.append(row['ns_per_solve'])
     if timing:
      am=statistics.mean([values[0],values[3]] if block%2==0 else values[1:3]);bm=statistics.mean(values[1:3] if block%2==0 else [values[0],values[3]])
      samples[name].append(dict(block=block,baseline_ms=am/1e6,candidate_ms=bm/1e6,ratio=bm/am))
   if timing:results.extend(dict(case=case['id'],comparison=name,**summary(samples[name])) for name,_,_ in pairs)
  finally:
   for worker in workers.values():worker.close()
  print(json.dumps(dict(case=case['id'],passed=True)),flush=True)
 common.write(a.out/'summary.json',dict(passed=True,timing=timing,self_check=a.self_check,source_sha=os.environ.get('GITHUB_SHA'),results=results,witnesses=witnesses,performance_scope=meta['scope'],baseline_source='d90c80b4001e43807d404454280f1511329ef952',history_source='2bb9cf595f675d269406bbe783e88154ed517cb4'))
if __name__=='__main__':main()
