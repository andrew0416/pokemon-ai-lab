"""Fresh-process, rotated RSS comparisons with and without allocator instrumentation."""
from pathlib import Path
import argparse,copy,hashlib,json,os,random,statistics,subprocess
import run as common
from cross_version import ALL4,ALL5,ALL6,option,identity,request,summary
def main():
 p=argparse.ArgumentParser()
 for name in ('old-speed','old-memory','speed','memory'):p.add_argument('--'+name,type=Path,required=True)
 p.add_argument('--parity',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 assert os.environ.get('GITHUB_ACTIONS')=='true';a.out.mkdir(parents=True,exist_ok=False)
 paths=vars(a);ref=json.loads(a.parity.read_bytes());assert ref['passed']
 case=next(c for c in json.loads((common.HERE/'plan.json').read_bytes())['profile_cases'] if c['id']=='sand-damage-w1-d1')
 choices=[('old-four','old',option('all-four',ALL4)),('old-four-aa','old',option('all-four',ALL4)),('old-five','old',ALL5),('current-five','current',ALL5),('current-six','current',ALL6)]
 env=dict(os.environ,LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1')
 records=[]
 for block in range(12):
  for mode in (['plain','instrumented'] if block%2==0 else ['instrumented','plain']):
   for offset in range(len(choices)):
    label,version,var=choices[(block+offset)%len(choices)]
    q=request(case);q['baseline']=var;q['variants']=[]
    tag=f'{mode}-{block}-{label}';path=a.out/(tag+'-request.json');common.write(path,q);rss=a.out/(tag+'-rss.txt')
    key=('old_' if version=='old' else '')+('memory' if mode=='instrumented' else 'speed');binary=paths[key]
    command=['/usr/bin/time','-f','%M','-o',str(rss.resolve()),str(binary.resolve()),'--memory' if mode=='instrumented' else '--check',str(path.resolve())]
    child=subprocess.run(command,capture_output=True,env=env,timeout=90)
    (a.out/(tag+'-stdout.jsonl')).write_bytes(child.stdout);(a.out/(tag+'-stderr.txt')).write_bytes(child.stderr)
    assert child.returncode==0,(tag,child.stderr.decode(errors='replace'))
    lines=[json.loads(line) for line in child.stdout.splitlines()]
    data=lines[0] if mode=='instrumented' else next(v for v in lines if v['type']=='baseline')
    expected=ref['witnesses'][case['id']+'|'+common.route(ALL6 if label=='current-six' else ALL5)]
    assert identity(data)==expected,(tag,'result mismatch')
    peak=int(rss.read_text());assert 0<peak<512*1024
    row=dict(mode=mode,block=block,label=label,process_peak_rss_kib=peak,witness=data['witness'])
    if mode=='instrumented':row.update({k:data[k] for k in ('allocations','reallocations','requested_growth_bytes','peak_above_baseline_bytes','returned_live_above_baseline_bytes','retained_after_result_drop_bytes')})
    records.append(row)
  print(json.dumps(dict(block=block,passed=True)),flush=True)
 results=[]
 for mode in ('plain','instrumented'):
  for name,b,c in [('aa','old-four','old-four-aa'),('history','old-four','old-five'),('wrapper','old-five','current-five'),('boundary','current-five','current-six')]:
   pairs=[]
   for block in range(12):
    left=next(r for r in records if r['mode']==mode and r['block']==block and r['label']==b);right=next(r for r in records if r['mode']==mode and r['block']==block and r['label']==c)
    pairs.append(dict(block=block,ratio=right['process_peak_rss_kib']/left['process_peak_rss_kib'],baseline_rss_kib=left['process_peak_rss_kib'],candidate_rss_kib=right['process_peak_rss_kib']))
   s=summary(pairs);s['rss_reduction_percent']=s.pop('time_reduction_percent')
   results.append(dict(mode=mode,comparison=name,**s))
 common.write(a.out/'summary.json',dict(passed=True,source_sha=os.environ.get('GITHUB_SHA'),timing=False,processes=len(records),blocks=12,rows=records,comparisons=results,
  binary_sha256={k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in paths.items() if k in ('old_speed','old_memory','speed','memory')},
  scope='whole-child peak RSS including loading/pool/compute/witness; plain and instrumented builds analyzed separately; rotated fresh processes, no wall-time gain claims'))
if __name__=='__main__':main()
