"""Independent process per configuration; allocator diagnostics are not timings."""
from pathlib import Path
import argparse,copy,hashlib,json,os,subprocess,sys
import run as common
def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--parity',type=Path,required=True);p.add_argument('--local-check',action='store_true');a=p.parse_args()
    if not a.local_check and os.environ.get('GITHUB_ACTIONS')!='true':raise SystemExit('Memory experiment runs on the GitHub runner; --local-check validates accounting only.')
    a.out.mkdir(parents=True);plan=json.loads((common.HERE/'plan.json').read_bytes());reference=json.loads(a.parity.read_bytes());rows=[]
    env=dict(os.environ,LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1',OMP_NUM_THREADS='1',OPENBLAS_NUM_THREADS='1')
    cases=plan['profile_cases'][-1:] if a.local_check else plan['profile_cases']
    for case in cases:
        choices=[case['baseline'],*case['variants']]
        if a.local_check:choices=[v for v in choices if v['id'] in ('new-off','all-four')]
        for variant in choices:
            for repeat in range(1 if a.local_check else 3):
                request=copy.deepcopy(case);request['baseline']=variant;request['variants']=[]
                for w in request['worlds']:w['scenario']=(common.HERE.parents[2]/w['scenario']).as_posix()
                tag=case['id']+'-'+variant['id']+'-'+str(repeat);path=a.out/(tag+'.json');common.write(path,request)
                command=[str(a.binary.resolve()),'--memory',str(path.resolve())]
                rss=a.out/(tag+'-rss.txt')
                if not a.local_check:command=['/usr/bin/time','-f','%M','-o',str(rss.resolve()),*command]
                result=subprocess.run(command,capture_output=True,env=env,timeout=300)
                (a.out/(tag+'-stderr.txt')).write_bytes(result.stderr)
                assert result.returncode==0,(tag,result.stderr.decode(errors='replace'))
                data=json.loads(result.stdout);assert data['type']=='memory' and not data['timing']
                expected=reference['witnesses'][case['id']+'|'+common.route(variant)]
                assert dict(witness=data['witness'],diagnostics=data['diagnostics'])==expected
                assert data['peak_requested_bytes']>=data['baseline_requested_bytes']
                assert data['peak_above_baseline_bytes']>=data['returned_live_above_baseline_bytes']
                assert data['allocations']>0 and data['result_drop_freed_bytes']>0
                if not a.local_check:data['process_peak_rss_kib']=int(rss.read_text());assert data['process_peak_rss_kib']<512*1024
                common.write(a.out/(tag+'-result.json'),data)
                rows.append(dict(case=case['id'],repeat=repeat,**data))
        print(json.dumps(dict(case=case['id'],passed=True)),flush=True)
    common.write(a.out/'summary.json',dict(passed=True,source_sha=os.environ.get('GITHUB_SHA'),timing=False,local_accounting_check=a.local_check,configuration_processes=len(rows),rows=rows,
        binary_sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),scope='Rust requested heap bytes include parallel workers; RSS fresh child process peak includes load/pool, allocator metadata and non-Rust memory; allocator instrumentation excluded from speed'))
if __name__=='__main__':main()
