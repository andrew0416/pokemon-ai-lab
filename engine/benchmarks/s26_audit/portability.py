"""Non-AVX2 CPU emulation, explicitly not real old-hardware timing."""
from pathlib import Path
import argparse,copy,json,os,subprocess
import run as common
def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--scalar',type=Path,required=True);p.add_argument('--probe',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    assert os.environ.get('GITHUB_ACTIONS')=='true';a.out.mkdir(parents=True)
    prefix=['qemu-x86_64','-cpu','Nehalem'];env=dict(os.environ,LAB_ENGINE_FACTORED='0',RAYON_NUM_THREADS='1')
    def execute(name,command):
        r=subprocess.run(command,capture_output=True,timeout=300,env=env)
        (a.out/(name+'-stdout.txt')).write_bytes(r.stdout);(a.out/(name+'-stderr.txt')).write_bytes(r.stderr)
        assert r.returncode==0,(name,r.stderr.decode(errors='replace'));return r.stdout
    version=execute('qemu-version',['qemu-x86_64','--version']).decode().splitlines()[0]
    cpu=json.loads(execute('cpu-probe',[*prefix,str(a.probe.resolve()),'--require-no-avx']))
    assert cpu==dict(avx=False,avx2=False,sse2=True)
    plan=json.loads((common.HERE/'plan.json').read_bytes());cases=[plan['profile_cases'][0],plan['profile_cases'][-1]];captured=[]
    for case in cases:
        q=copy.deepcopy(case);q['threads']=1;q['variants']=[v for v in q['variants'] if v['id'] in ('new-off','all-four')]
        for w in q['worlds']:w['scenario']=(common.HERE.parents[2]/w['scenario']).as_posix()
        path=a.out/(q['id']+'.json');common.write(path,q)
        args=[str(a.binary.resolve()),'--check',str(path.resolve())]
        native=execute(q['id']+'-native',args);emulated=execute(q['id']+'-nehalem',prefix+args)
        assert [json.loads(x) for x in native.splitlines()]==[json.loads(x) for x in emulated.splitlines()]
        captured.append(q['id'])
    def decision(player,key,children):return dict(type='decision',player=player,information=key,actions=['A','B'],children=children)
    q=dict(mode='tree',root=0,nodes=[decision(0,'row',[1,2]),decision(1,'column',[3,4]),decision(1,'column',[5,6]),*[dict(type='terminal',value=v) for v in (1,-1,-1,1)]],solver=dict(iterations=256,tolerance=.01,check_every=32))
    path=a.out/'scalar.json';common.write(path,q);args=[str(a.scalar.resolve()),str(path.resolve())]
    native=json.loads(execute('scalar-native',args));emulated=json.loads(execute('scalar-nehalem',prefix+args));assert native==emulated
    common.write(a.out/'summary.json',dict(passed=True,source_sha=os.environ.get('GITHUB_SHA'),qemu=version,cpu_model='Nehalem',cpuid=cpu,engine_workloads=captured,engine_configurations=6,scalar_legacy_equal=True,actual_old_hardware=False,emulated=True,timing=False))
    print('Non-AVX2 emulated engine and legacy scalar results match native.')
if __name__=='__main__':main()
