"""Prepare/test/build the pinned harness; no existing runtime source may change."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import contract as c

def write(path,value):
    path.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8',newline='\n')
def command(argv,cwd):return subprocess.check_output(argv,cwd=cwd,text=True).strip()
def environment(workspace):
    c.require(platform.system()=='Linux' and platform.machine()=='x86_64','Linux x86-64 required')
    c.require(os.environ.get('RUSTFLAGS')=='-Ctarget-cpu=x86-64','Generic x86-64 target required')
    for key in ('CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','CARGO_BUILD_TARGET','RUSTDOCFLAGS'):
        c.require(not os.environ.get(key),'Unreviewed compiler environment: '+key)
    env=os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(workspace/'target-distribution'),CARGO_INCREMENTAL='0',
        CARGO_PROFILE_RELEASE_OPT_LEVEL='3',CARGO_PROFILE_RELEASE_DEBUG='1',CARGO_PROFILE_RELEASE_LTO='off',
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16',RAYON_NUM_THREADS='1',RUST_MIN_STACK='16777216',RUST_TEST_THREADS='1',
        PYTHONHASHSEED='0',OPENBLAS_NUM_THREADS='1',OMP_NUM_THREADS='1')
    for key in list(env):
        if key.startswith('LAB_'):del env[key]
    return env
def verify_source(root):
    c.require(re.fullmatch('[0-9a-f]{40}',c.SOURCE_SHA) and c.SOURCE_FILES,'Source is not bound')
    c.require(set(c.SOURCE_FILES)=={'engine/scenario/src/bin/lab-distribution-bench.rs'},'Only the new benchmark bin may differ')
    c.require(command(['git','rev-parse','HEAD'],root)==c.SOURCE_SHA,'Unexpected source HEAD')
    c.require(command(['git','rev-parse','HEAD^'],root)==c.CANONICAL_SOURCE,'Source must directly inherit canonical R1')
    c.require(command(['git','status','--porcelain'],root)=='','Source checkout has unreviewed changes')
    names=command(['git','diff','--name-only',c.CANONICAL_SOURCE,c.SOURCE_SHA],root).splitlines()
    c.require(set(names)==set(c.SOURCE_FILES),'Existing R1 runtime was changed')
    for path,digest in c.SOURCE_FILES.items():c.require(c.sha(c.safe_file(root,path))==digest,'Benchmark source changed')
    return {'sha':c.SOURCE_SHA,'canonical_parent':c.CANONICAL_SOURCE,'file_sha256':c.SOURCE_FILES,'runtime_unchanged':True}
def prepare(workspace):
    result=workspace/'turn-distribution-results';result.mkdir(exist_ok=False)
    record={'status':'preparing','source_sha':c.SOURCE_SHA,'requested_cases':c.COUNT,'corpus_count':500}
    try:
        record['source']=verify_source(workspace/'source');manifest=c.corpus(workspace/'controller')
        env=environment(workspace)
        record.update(status='prepared',controller_sha=command(['git','rev-parse','HEAD'],workspace/'controller'),
            corpus_sha256=c.CORPUS_SHA,case_ids=[v['id'] for v in manifest['cases'][:c.COUNT]],
            features=list(c.CORE_FEATURES),search_crate_invoked=False,observers_enabled=False,
            environment={k:env.get(k) for k in ('RUSTFLAGS','CARGO_INCREMENTAL','CARGO_PROFILE_RELEASE_OPT_LEVEL',
                'CARGO_PROFILE_RELEASE_DEBUG','CARGO_PROFILE_RELEASE_LTO','CARGO_PROFILE_RELEASE_CODEGEN_UNITS','RAYON_NUM_THREADS')},
            lab_environment_absent=not any(k.startswith('LAB_') for k in env),
            rustc=command(['rustc','-Vv'],workspace),cargo=command(['cargo','-V'],workspace),
            uname=platform.uname()._asdict(),lscpu=command(['lscpu'],workspace),
            available_cpus=sorted(os.sched_getaffinity(0)),
            timing_scope='API kernel_ns; describe and metric CPU excluded; process resources separately recorded',
            limits={'seconds_per_process':60,'rss_bytes_per_process':6*1024**3,'rss_poll_seconds':0.01})
    except Exception as error:
        record.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:write(result/'provenance.json',record)

def fingerprints(workspace):
    root=workspace/'target-distribution/release/.fingerprint'
    required={'lab-engine':('lib-lab_engine.json',),'lab-scenario':(
        'lib-lab_scenario.json','test-bin-lab-distribution-bench.json','bin-lab-distribution-bench.json')}
    expected={'lab-engine':set(c.CORE_FEATURES),'lab-scenario':set()};seen={p:set() for p in required};rows=[]
    c.require(not list(root.glob('lab-search-*')),'Turn benchmark unexpectedly compiled the search crate')
    for package,names in required.items():
        for folder in sorted(root.glob(package+'-*')):
            for name in names:
                path=folder/name
                if not path.is_file():continue
                data=c.strict_json(path.read_bytes());features=data['features']
                if isinstance(features,str):features=c.strict_json(features)
                c.require(isinstance(features,list) and len(features)==len(set(features)) and set(features)==expected[package],
                    'Actual compiled feature closure differs: '+str(path))
                seen[package].add(name);rows.append({'path':str(path.relative_to(root)),
                    'sha256':c.sha(path),'features':features,'content':data})
        c.require(seen[package]==set(names),'Missing actual compiler fingerprints: '+package)
    return rows

def build(workspace):
    result=workspace/'turn-distribution-results';record={'status':'building','commands':[],'cached_regression_reused':False}
    receipt=result/'build-receipt.json';c.require(not receipt.exists(),'Build receipt already exists')
    try:
        verify_source(workspace/'source');c.corpus(workspace/'controller');env=environment(workspace)
        target=workspace/'target-distribution'
        c.require(not any((target/'release/.fingerprint').glob('lab-*')),'Cached workspace crates cannot bypass fresh tests')
        commands=[['cargo','test','--locked','--release','-p','lab-scenario','--bin','lab-distribution-bench',*c.FEATURE_ARGS,'--','--test-threads=1'],
                  ['cargo','build','--locked','--release','-p','lab-scenario','--bin','lab-distribution-bench',*c.FEATURE_ARGS]]
        for index,argv in enumerate(commands):
            path=result/f'build-{index}.log'
            with path.open('xb') as stream:
                run=subprocess.run(argv,cwd=workspace/'source/engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=1800)
            record['commands'].append({'argv':argv,'returncode':run.returncode,'log':path.name,'log_sha256':c.sha(path)})
            c.require(run.returncode==0,'Fresh benchmark test/build failed')
            if index==0:
                text=path.read_text(encoding='utf-8');passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
                c.require(sorted(passed)==sorted(c.TESTS),'Named benchmark tests missing or unexpected')
                counts=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',text,re.M)
                c.require(counts==[(str(len(c.TESTS)),'0','0','0','0')],'Benchmark tests incomplete or filtered')
        record['compiler_features']=fingerprints(workspace)
        binary=target/'release/lab-distribution-bench';c.require(binary.is_file() and not binary.is_symlink(),'Missing benchmark binary')
        record.update(status='success',source_sha=c.SOURCE_SHA,binary=str(binary),binary_sha256=c.sha(binary),
                      corpus_sha256=c.CORPUS_SHA,features=list(c.CORE_FEATURES))
    except Exception as error:
        record.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:write(receipt,record)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=['prepare','build']);parser.add_argument('--workspace',type=Path,required=True)
    args=parser.parse_args();globals()[args.stage](args.workspace.resolve())
if __name__=='__main__':main()
