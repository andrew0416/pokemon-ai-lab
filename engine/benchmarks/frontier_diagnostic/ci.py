"""Fresh observer OFF/ON builds and fingerprints for a source-bound P17 diagnosis."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tomllib
from common import c,base_ci,binding,features,feature_args,fixed_case,SOURCE_PARENT,BENCHMARK,BENCHMARK_SHA,FEATURE

def environment(workspace,arm):
    c.require(arm in ('off','on'),'Unknown arm')
    env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(workspace/('target-p17-'+arm));return env
def verify_source(root,bound):
    command=base_ci.command
    c.require(command(['git','rev-parse','HEAD'],root)==bound['source_sha'],'Unexpected source SHA')
    c.require(command(['git','rev-parse','HEAD^'],root)==SOURCE_PARENT,'P17 must directly inherit original benchmark source')
    c.require(command(['git','status','--porcelain'],root)=='','Source has unreviewed changes')
    names=command(['git','diff','--name-only',SOURCE_PARENT,bound['source_sha']],root).splitlines()
    c.require(set(names)==set(bound['changed_file_sha256']),'Source delta differs from exact frozen allowlist')
    for name,digest in bound['changed_file_sha256'].items():c.require(c.sha(c.safe_file(root,name))==digest,'Observer source changed')
    c.require(c.sha(c.safe_file(root,BENCHMARK))==BENCHMARK_SHA,'Inherited benchmark changed')
    manifest=tomllib.loads((root/'engine/core/Cargo.toml').read_text())
    c.require(manifest['features'].get(FEATURE)==[] and FEATURE not in manifest['features'].get('default',[]),'Observer must be independent and default OFF')
    return {'source_sha':bound['source_sha'],'parent':SOURCE_PARENT,'changed_file_sha256':bound['changed_file_sha256'],
        'inherited_benchmark_sha256':BENCHMARK_SHA}

def fingerprints(workspace,arm,include_unit=False):
    root=workspace/('target-p17-'+arm)/'release/.fingerprint'
    required={'lab-engine':{'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if include_unit else set()),
        'lab-scenario':{'lib-lab_scenario.json','bin-lab-distribution-bench.json','test-bin-lab-distribution-bench.json'}}
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    rows=[]
    for package,names in required.items():
        seen=set();wanted=set(features(arm=='on')) if package=='lab-engine' else set()
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                value=c.strict_json(path.read_bytes());actual=value['features']
                if isinstance(actual,str):actual=c.strict_json(actual)
                c.require(len(actual)==len(set(actual)) and set(actual)==wanted,'Compiled feature closure differs')
                c.require(value.get('rustflags')==['-Ctarget-cpu=x86-64'],'Compiled target flags differ')
                seen.add(name);rows.append({'path':str(path.relative_to(root)),'sha256':c.sha(path),'features':actual,'content':value})
        c.require(seen==names,'Missing actual fingerprint '+package+' '+arm)
    return rows

def prepare(workspace):
    folder=workspace/'p17-diagnostic-results';folder.mkdir(exist_ok=False)
    record={'status':'preparing','diagnostic_only':True,'timing_comparable':False,'repairs_full500_metrics':False}
    try:
        bound=binding();record['source']=verify_source(workspace/'source',bound)
        for case in ('opening-0000','opening-0429'):fixed_case(workspace,case)
        env=environment(workspace,'off')
        record.update(status='prepared',controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),
            corpus_sha256=c.CORPUS_SHA,control_case='opening-0000',diagnostic_case='opening-0429',
            features={arm:features(arm=='on') for arm in ('off','on')},
            rustflags=env['RUSTFLAGS'],lab_environment_absent=not any(key.startswith('LAB_') for key in env),
            rustc=base_ci.command(['rustc','-Vv'],workspace),lscpu=base_ci.command(['lscpu'],workspace),
            available_cpus=sorted(os.sched_getaffinity(0)),limits={'control_seconds':60,'diagnostic_seconds':300,'rss_bytes':6*1024**3})
    except Exception as error:record.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(folder/'provenance.json',record)

def build(workspace):
    folder=workspace/'p17-diagnostic-results';path=folder/'build-receipt.json'
    c.require(not path.exists(),'Existing build receipt cannot bypass fresh tests')
    record={'status':'building','cached_regression_reused':False,'commands':[],'arms':{}}
    try:
        bound=binding();verify_source(workspace/'source',bound);c.corpus(workspace/'controller')
        for arm in ('off','on'):
            env=environment(workspace,arm);target=workspace/('target-p17-'+arm)
            c.require(not any((target/'release/.fingerprint').glob('lab-*')),'Cached workspace compilation cannot bypass fresh tests')
            commands=[('harness_tests',['cargo','test','--locked','--release','-p','lab-scenario','--bin','lab-distribution-bench',*feature_args(arm=='on'),'--','--test-threads=1'],c.TESTS),
                ('harness_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin','lab-distribution-bench',*feature_args(arm=='on')],None)]
            if arm=='on':commands.append(('observer_tests',['cargo','test','--locked','--release','-p','lab-engine','--lib',*feature_args(True),bound['observer_test_filter'],'--','--test-threads=1'],bound['required_observer_tests']))
            for label,argv,tests in commands:
                log=folder/(arm+'-'+label+'.log')
                with log.open('xb') as stream:
                    result=subprocess.run(argv,cwd=workspace/'source/engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
                record['commands'].append({'arm':arm,'label':label,'argv':argv,'returncode':result.returncode,'log':log.name,'log_sha256':c.sha(log)})
                c.require(result.returncode==0,'Fresh '+arm+' '+label+' failed')
                if tests:
                    text=log.read_text();passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
                    c.require(sorted(passed)==sorted(tests),'Named tests incomplete')
                    counts=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',text,re.M)
                    c.require(len(counts)==1 and counts[0][:4]==(str(len(tests)),'0','0','0'),'Test counts incomplete')
                    if label=='harness_tests':c.require(counts[0][4]=='0','Harness tests filtered')
            binary=target/'release/lab-distribution-bench';c.require(binary.is_file() and not binary.is_symlink(),'Missing measured binary')
            record['arms'][arm]={'features':features(arm=='on'),'compiler_features':fingerprints(workspace,arm,arm=='on'),
                'binary':str(binary),'binary_sha256':c.sha(binary)}
            for row in record['arms'][arm]['compiler_features']:
                original=target/'release/.fingerprint'/row['path'];copy=folder/'fingerprints'/arm/row['path']
                copy.parent.mkdir(parents=True,exist_ok=True)
                with copy.open('xb') as stream:stream.write(original.read_bytes())
                c.require(c.sha(copy)==row['sha256'],'Fingerprint copy changed')
        record.update(status='success',source_sha=bound['source_sha'],corpus_sha256=c.CORPUS_SHA)
    except Exception as error:record.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,record)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','build'));parser.add_argument('--workspace',required=True,type=Path)
    args=parser.parse_args();globals()[args.stage](args.workspace.resolve())
if __name__=='__main__':main()
