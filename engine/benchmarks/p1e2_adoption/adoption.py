"""P1e2 fresh correctness gates and bounded expanded-corpus execution."""
import argparse, json, os
from pathlib import Path
import subprocess
import shared as s
from shared import c, base_ci
import compare_broad
import accuracy_process

def fingerprints(workspace,arm):
    root=workspace/('target-p1e2-'+arm)/'release/.fingerprint'
    required={'lab-engine':{'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if arm!='original' else set()),
              'lab-scenario':{'lib-lab_scenario.json','bin-lab-distribution-bench.json','test-bin-lab-distribution-bench.json',
              'test-integration-test-lazy_ko_damage.json','test-integration-test-factored.json',
              'test-integration-test-'+s.BROAD_TARGET+'.json'}}
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    rows=[]
    for package,names in required.items():
        seen=set();expected=set(s.p1.features(arm=='on')) if package=='lab-engine' else set()
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                value=c.strict_json(path.read_bytes());features=value['features']
                if isinstance(features,str):features=c.strict_json(features)
                c.require(isinstance(features,list) and len(features)==len(set(features)) and set(features)==expected,'Actual features differ')
                c.require(value.get('rustflags')==['-Ctarget-cpu=x86-64'],'Actual generic target differs')
                seen.add(name);rows.append({'path':str(path.relative_to(root)),'sha256':c.sha(path),'features':features,'content':value})
        c.require(seen==names,'Missing fingerprints '+arm+' '+package)
    return rows

def compiled_executable(log,target):
    selected=[]
    for line in log.read_text(encoding='utf-8').splitlines():
        if not line.startswith('{'):continue
        value=c.strict_json(line)
        if value.get('reason')=='compiler-artifact' and value['target']['name']==s.BROAD_TARGET and value.get('executable'):
            c.require(value['profile']['test'] is True and value['profile']['opt_level']=='3','Broad executable profile differs')
            c.require(value['features']==[],'Unexpected scenario features')
            path=Path(value['executable'])
            c.require(path.is_file() and not path.is_symlink() and path.resolve().is_relative_to((target/'release/deps').resolve()),'Unsafe broad test executable')
            selected.append({'path':str(path),'sha256':c.sha(path),'compiler_artifact':value})
    c.require(len(selected)==1,'Missing/duplicate broad executable')
    return selected[0]

def broad_command(arm):
    return ['cargo','test','--locked','--release','-p','lab-scenario','--test',s.BROAD_TARGET,
            *s.p1.feature_args(arm=='on'),'--no-run','--message-format=json']

def prepare(workspace):
    folder=workspace/s.RESULTS;folder.mkdir(exist_ok=False)
    value={'status':'preparing','accuracy_only':True,'adoption_approved':False}
    try:
        bound=s.binding()
        value['candidate_source']=s.verify_source(workspace/'source',bound)
        value['original_source']=s.prepare_original(workspace,bound)
        c.corpus(workspace/'controller')
        env=s.environment(workspace,'original')
        rustc=base_ci.command(['rustc','-Vv'],workspace)
        c.require(rustc.startswith('rustc 1.98.1 '),'Toolchain differs')
        value.update(status='prepared',controller_sha=base_ci.command(['git','rev-parse','HEAD'],workspace/'controller'),
          source_sha=bound['source_sha'],source_binding_sha256=c.sha(s.HERE/'source-binding.json'),
          features={arm:s.p1.features(arm=='on') for arm in s.ARMS},rustc=rustc,
          lscpu=base_ci.command(['lscpu'],workspace),available_cpus=sorted(os.sched_getaffinity(0)),
          environment={k:env[k] for k in ('RUSTFLAGS','CARGO_INCREMENTAL','CARGO_PROFILE_RELEASE_OPT_LEVEL',
          'CARGO_PROFILE_RELEASE_DEBUG','CARGO_PROFILE_RELEASE_LTO','CARGO_PROFILE_RELEASE_CODEGEN_UNITS','RAYON_NUM_THREADS')},
          limits={'seconds_per_broad_arm':300,'rss_bytes':6*1024**3,'one_cpu':True},
          corpus_contract=bound['broad_record_contract'],original_corpus_sha256=c.CORPUS_SHA,
          local_scope='Expanded bounded corpus and inherited source tests; no full500 benchmark or adoption.')
    except Exception as error:
        value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(folder/'provenance.json',value)

def build(workspace):
    folder=workspace/s.RESULTS;path=folder/'build-receipt.json'
    c.require(not path.exists(),'Build already attempted')
    value={'status':'building','cached_regression_reused':False,'commands':[],'arms':{},'accuracy_only':True}
    try:
        bound=s.binding();prior=s.prior_binding()
        s.verify_source(workspace/'source',bound);s.verify_original(workspace/'original',bound);c.corpus(workspace/'controller')
        for arm in s.ARMS:
            target=workspace/('target-p1e2-'+arm);env=s.environment(workspace,arm)
            c.require(not any((target/'release/.fingerprint').glob('lab-*')),'Cached workspace artifacts forbidden')
            public=folder/(arm+'-public-records.jsonl')
            c.require(not public.exists(),'Cached public records forbidden')
            for label,argv,tests,filtered in s.old_ci.command_plan(arm,prior):
                log=folder/(arm+'-'+label+'.log');child=dict(env)
                if label=='public_tests':child['LAB_P1E_PUBLIC_RECORDS']=str(public)
                row={'arm':arm,'label':label,'argv':argv,'log':log.name,'lab_environment':{k:v for k,v in child.items() if k.startswith('LAB_')}}
                value['commands'].append(row);base_ci.write(path,value)
                with log.open('xb') as stream:
                    result=subprocess.run(argv,cwd=s.source_root(workspace,arm)/'engine',env=child,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
                row.update(returncode=result.returncode,log_sha256=c.sha(log))
                c.require(result.returncode==0,'Fresh inherited test/build failed '+arm+' '+label)
                if tests:row['test_proof']=s.old_ci.named_test_proof(log.read_text(encoding='utf-8'),tests,filtered)
            c.require(public.is_file() and not public.is_symlink(),'Fresh inherited public records missing')
            # Compile, but do not run the broad suite under Cargo. Direct execution below gets
            # a strict wall/RSS/one-CPU bound and retains a fresh named-test receipt.
            argv=broad_command(arm)
            log=folder/(arm+'-broad-build.log')
            row={'arm':arm,'label':'broad_build','argv':argv,'log':log.name,'lab_environment':{}}
            value['commands'].append(row);base_ci.write(path,value)
            with log.open('xb') as stream:
                result=subprocess.run(argv,cwd=s.source_root(workspace,arm)/'engine',env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=600)
            row.update(returncode=result.returncode,log_sha256=c.sha(log))
            c.require(result.returncode==0,'Broad compile failed')
            executable=compiled_executable(log,target)
            proofs=fingerprints(workspace,arm)
            for proof in proofs:
                src=target/'release/.fingerprint'/proof['path'];dst=folder/'fingerprints'/arm/proof['path']
                dst.parent.mkdir(parents=True,exist_ok=True)
                with dst.open('xb') as out:out.write(src.read_bytes())
                c.require(c.sha(dst)==proof['sha256'],'Fingerprint copy changed')
            value['arms'][arm]={'features':s.p1.features(arm=='on'),'compiler_features':proofs,
                 'broad_executable':executable,'public_records':public.name,'public_records_sha256':c.sha(public)}
        agreement={arm+'_vs_on':s.old_compare.compare_records(folder/(arm+'-public-records.jsonl'),folder/'on-public-records.jsonl') for arm in ('original','off')}
        c.require(all(row['passed'] and row['cases']==10 for row in agreement.values()),'Inherited public agreement failed')
        value.update(status='success',source_sha=bound['source_sha'],source_binding_sha256=c.sha(s.HERE/'source-binding.json'),
                     inherited_public_agreement=agreement,inherited_named_test_executions=sum(row.get('test_proof',{}).get('passed',0) for row in value['commands']))
        c.require(value['inherited_named_test_executions']==55,'Inherited named gate count differs')
    except Exception as error:
        value.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:base_ci.write(path,value)

def verify_build(workspace,bound):
    folder=workspace/s.RESULTS;value=c.strict_json((folder/'build-receipt.json').read_bytes())
    c.require(value['status']=='success' and value['cached_regression_reused'] is False and value['source_sha']==bound['source_sha']
              and value['source_binding_sha256']==c.sha(s.HERE/'source-binding.json'),'Fresh bound build missing')
    c.require(set(value['arms'])==set(s.ARMS) and value['inherited_named_test_executions']==55,'Incomplete build arms/gates')
    for arm in s.ARMS:
        row=value['arms'][arm]
        c.require(row['features']==s.p1.features(arm=='on') and fingerprints(workspace,arm)==row['compiler_features'],'Feature proof changed')
        c.require(c.sha(folder/row['public_records'])==row['public_records_sha256'],'Inherited records changed')
        binary=Path(row['broad_executable']['path'])
        c.require(c.sha(binary)==row['broad_executable']['sha256'],'Compiled broad executable changed')
    expected=[]
    for arm in s.ARMS:
        expected.extend((arm,label,argv,tests,filtered) for label,argv,tests,filtered in s.old_ci.command_plan(arm,s.prior_binding()))
        expected.append((arm,'broad_build',broad_command(arm),None,None))
    c.require(len(value['commands'])==len(expected),'Fresh command list incomplete')
    total=0
    for row,(arm,label,argv,tests,filtered) in zip(value['commands'],expected):
        c.require((row['arm'],row['label'],row['argv'],row['returncode'])==(arm,label,argv,0),'Fresh command identity changed')
        log=folder/(arm+'-'+label+'.log')
        c.require(row['log']==log.name and c.sha(log)==row['log_sha256'],'Fresh command/log changed')
        expected_env={'LAB_P1E_PUBLIC_RECORDS':str(folder/(arm+'-public-records.jsonl'))} if label=='public_tests' else {}
        c.require(row['lab_environment']==expected_env,'Fresh test environment changed')
        if tests:
            proof=s.old_ci.named_test_proof(log.read_text(encoding='utf-8'),tests,filtered)
            c.require(proof==row['test_proof'],'Named proof changed');total+=proof['passed']
        else:c.require('test_proof' not in row,'Unexpected named proof')
        if label=='broad_build':
            c.require(compiled_executable(log,workspace/('target-p1e2-'+arm))==value['arms'][arm]['broad_executable'],'Broad compiler artifact changed')
    c.require(total==55,'Fresh named gate total changed')
    for arm in ('original','off'):
        c.require(s.old_compare.compare_records(folder/(arm+'-public-records.jsonl'),folder/'on-public-records.jsonl')==value['inherited_public_agreement'][arm+'_vs_on'],'Inherited agreement changed')
    return value

def run(workspace):
    folder=workspace/s.RESULTS;path=folder/'accuracy-summary.json'
    c.require(not path.exists(),'Accuracy suite already attempted')
    value={'status':'preparing','accuracy_only':True,'arms':{},'adoption_approved':False,
           'official_full500_metrics':None,'full500_complete':False}
    try:
        bound=s.binding();s.verify_source(workspace/'source',bound);s.verify_original(workspace/'original',bound)
        build=verify_build(workspace,bound);cpu=min(os.sched_getaffinity(0))
        value.update(source_sha=bound['source_sha'],measurement_cpu=cpu,source_binding_sha256=c.sha(s.HERE/'source-binding.json'))
        for arm in s.ARMS:
            directory=folder/'broad'/arm;directory.mkdir(parents=True,exist_ok=False)
            records=directory/'records.jsonl';manifest=directory/'corpus-manifest.json';binary=Path(build['arms'][arm]['broad_executable']['path'])
            c.require(c.sha(binary)==build['arms'][arm]['broad_executable']['sha256'],'Broad binary changed')
            env=s.environment(workspace,arm);env.update(s.SCOPE_ENV);env[s.BROAD_ENV]=str(records);env[s.BROAD_MANIFEST_ENV]=str(manifest)
            argv=[str(binary),'--test-threads=1','--show-output'];stem=directory/'execution'
            row={'arm':arm,'status':'running','argv':argv,'binary_sha256':c.sha(binary),
                 'lab_environment':{k:v for k,v in env.items() if k.startswith('LAB_')},'records_file':str(records.relative_to(folder))}
            value['arms'][arm]=row;base_ci.write(path,value)
            process=accuracy_process.run(argv,s.source_root(workspace,arm)/'engine',env,stem,cpu=cpu)
            row.update(status=process['status'],process=process)
            for channel in ('stdout','stderr'):
                raw=stem.with_suffix('.'+channel);row[channel+'_sha256']=c.sha(raw);row[channel+'_bytes']=raw.stat().st_size
            if records.is_file():row.update(records_sha256=c.sha(records),records_bytes=records.stat().st_size)
            base_ci.write(path,value)
            c.require(process['status']=='ok','Expanded suite incomplete '+arm)
            row['test_proof']=s.old_ci.named_test_proof(stem.with_suffix('.stdout').read_text(encoding='utf-8'),bound['broad_tests'],0)
            c.require(records.is_file() and not records.is_symlink(),'Expanded suite did not emit new records')
            c.require(manifest.is_file() and not manifest.is_symlink(),'Expanded contract test did not emit a fresh manifest')
            c.require(c.strict_json(manifest.read_bytes())==bound['broad_record_contract'],'Actual expanded corpus differs from frozen manifest')
            row.update(corpus_manifest_file=str(manifest.relative_to(folder)),corpus_manifest_sha256=c.sha(manifest))
            # Validate each successful stream by comparing to itself; empty/partial schemas fail.
            row['record_validation']=compare_broad.compare_records(records,records,bound['broad_record_contract'])
            c.require(row['record_validation']['passed'] is True,'Expanded record validation failed')
            base_ci.write(path,value)
        agreement={arm+'_vs_on':compare_broad.compare_records(folder/value['arms'][arm]['records_file'],folder/value['arms']['on']['records_file'],bound['broad_record_contract']) for arm in ('original','off')}
        c.require(all(row['passed'] is True for row in agreement.values()),'Expanded three-arm agreement failed')
        value.update(status='success',expanded_agreement=agreement,inherited_named_test_executions=55,
           broad_named_test_executions=sum(row['test_proof']['passed'] for row in value['arms'].values()),inherited_public_agreement=build['inherited_public_agreement'])
        c.require(value['broad_named_test_executions']==6,'Expanded named tests incomplete')
        value['total_fresh_named_test_executions']=61
        return 0
    except Exception as error:
        value.update(status='failed',error=f'{type(error).__name__}: {error}');return 1
    finally:base_ci.write(path,value)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('stage',choices=('prepare','build','run'));parser.add_argument('--workspace',required=True,type=Path)
    args=parser.parse_args();result=globals()[args.stage](args.workspace.resolve())
    if args.stage=='run':raise SystemExit(result)
if __name__=='__main__':main()
