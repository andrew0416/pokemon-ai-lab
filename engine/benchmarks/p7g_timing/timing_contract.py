"""Same frozen source; only P7d compilation feature differs. Reuse audited P7f accuracy."""
from pathlib import Path,PurePosixPath
import json,os,re,sys
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p7f_coverage'))
import coverage_contract as prior
from coverage_contract import c,base_ci,s
import tail_process
SOURCE='87e7fdcb47b24e407e81d9d5688714dfee6de35d'
PARENT='9e60cfb973410f8ca00102460654bfa1f221a64c'
ARMS=('off','on')
TARGET='lab-distribution-bench'
PACKAGE='p7g-build'
RSS=6*1024**3
CHILD_SECONDS=300
SHARD_SECONDS=2100
PARTITIONS=4
AUDIT_SHA='79a50dbc4e299a196aace1176345e0c8967af36a24f9c05844869d0dee9bb0db'
REPORT_SHA='8d399a418f4e79e1220b2186a46d9ec93f3a91f35a01646589d9fb8766e097d8'
def binding():
    b=c.strict_json((HERE/'source-binding.json').read_bytes())
    c.require(b['schema']==1 and b['source_sha']==SOURCE and b['controller_parent']==PARENT and b['feature_default_on'] is False,'Wrong timing binding')
    c.require(b['p7f_source_binding_sha256']==c.sha(prior.HERE/'source-binding.json') and prior.binding()['source_sha']==SOURCE,'P7f source evidence changed')
    c.require(b['audit_sha256']==c.sha(HERE/'reused-p7f-audit.json')==AUDIT_SHA and b['report_sha256']==c.sha(HERE/'reused-p7f-results.json')==REPORT_SHA,'Reused correctness receipts changed')
    audit=c.strict_json((HERE/'reused-p7f-audit.json').read_bytes());report=c.strict_json((HERE/'reused-p7f-results.json').read_bytes())
    for value in (audit,report):c.require(value['status']=='PASS_SCOPED_COVERAGE' and value['source_sha']==SOURCE and value['controller_sha']==PARENT and value['adoption_approved'] is False,'Reused correctness source/verdict differs')
    c.require(audit['full500_complete'] and audit['verified_full500_cases']==500 and audit['broad104_complete'] and audit['broad_cases']==104 and audit['issues']==[],'Incomplete reused correctness')
    c.require(report['full500_passed']==report['full500_requested']==500 and report['broad_passed']==report['broad_requested']==104 and report['off_reference_payload_bytes_exact'] and report['audit_sha256']==AUDIT_SHA,'Reused result contract differs')
    ledger=c.strict_json(prior.PLAN_LEDGER.read_bytes());proofs=audit['raw_full500_case_proofs']
    c.require(len(proofs)==500 and len({row['case_id'] for row in proofs})==500,'Missing reused raw accuracy cases')
    plans={row['id']:row['sha256'] for row in ledger['plans']}
    c.require({row['case_id']:row['plan_sha256'] for row in proofs}==plans,'Accuracy was for different decisions')
    for row in proofs:
        p=row['on_proof'];c.require(p['passed'] and p['full_non_hp_state_suspension_dictionary_exact'] and p['full_joint_party_hp_support_exact'] and p['max_abs_probability_error']<=1e-12 and p['normalized_tv']<=1e-9,'Reused joint accuracy failed')
    c.require(b['corpus_sha256']==c.CORPUS_SHA and b['plan_ledger_sha256']==c.sha(prior.PLAN_LEDGER)==prior.PLAN_LEDGER_SHA,'Frozen input ledger changed')
    c.require(b['benchmark_sha256']==prior.prior.FROZEN[s.p1.BENCHMARK],'Frozen timing producer changed')
    return b
def features(arm):
    c.require(arm in ARMS,'Unknown timing arm');return prior.features(arm)
def environment(workspace,arm):
    env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(target(workspace,arm));return env
def target(workspace,arm):return workspace/('target-p7g-'+arm)
def package_root(workspace):return workspace/PACKAGE
def binary(workspace,arm):return package_root(workspace)/'bin'/arm/'benchmark'
def corpus(workspace):return prior.corpus(workspace)
def shard_cases(workspace,index):
    c.require(type(index) is int and 0<=index<4,'Unknown timing shard');return corpus(workspace)[index::4]
def source_proof(workspace,b):
    root=workspace/'source'
    c.require(base_ci.command(['git','rev-parse','HEAD'],root)==SOURCE and base_ci.command(['git','status','--porcelain'],root)=='','Wrong/modified source')
    expected={**prior.prior.binding()['changed_file_sha256'],**prior.binding()['test_only_file_sha256']}
    for name,digest in expected.items():c.require(c.sha(c.safe_file(root,name))==digest,'Frozen runtime/test changed')
    c.require(c.sha(c.safe_file(root,s.p1.BENCHMARK))==b['benchmark_sha256'],'Benchmark producer changed')
    return {'source_sha':SOURCE,'runtime_sha':prior.RUNTIME,'benchmark_sha256':b['benchmark_sha256'],'reused_accuracy_audit_sha256':AUDIT_SHA,'source_file_sha256':expected,'runtime_unchanged':True}
def expected_source_proof(b):
    return {'source_sha':SOURCE,'runtime_sha':prior.RUNTIME,'benchmark_sha256':b['benchmark_sha256'],'reused_accuracy_audit_sha256':AUDIT_SHA,'source_file_sha256':{**prior.prior.binding()['changed_file_sha256'],**prior.binding()['test_only_file_sha256']},'runtime_unchanged':True}
def commands(arm):
    feat=['--features',','.join('lab-engine/'+v for v in features(arm))]
    rows=[]
    if arm=='on':
        for label,filter_,names in [('p7d_core','turn::first_hit_tests::',prior.prior.binding()['core_tests']),('p7f_core','turn::first_hit_coverage_tests::',prior.binding()['coverage_tests'])]:
            rows.append((label,['cargo','test','--locked','--release','-p','lab-engine','--lib','--features',','.join(features(arm)),filter_,'--','--test-threads=1','--show-output'],names))
    rows.append(('benchmark_tests',['cargo','test','--locked','--release','-p','lab-scenario','--bin',TARGET,*feat,'--','--test-threads=1','--show-output'],list(c.TESTS)))
    rows.append(('benchmark_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',TARGET,*feat],None))
    return rows
def log_name(arm,label):return arm+'-'+label+'.log'
def named_proof(label,text,names):return s.old_ci.named_test_proof(text,names,0 if label=='benchmark_tests' else None)
def fingerprint_names(arm):
    return {'lab-engine':{'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if arm=='on' else set()),'lab-scenario':{'lib-lab_scenario.json','bin-'+TARGET+'.json','test-bin-'+TARGET+'.json'}}
def check_fingerprints(rows,root,arm):
    expected=fingerprint_names(arm);seen={k:set() for k in expected};paths=set()
    for row in rows:
        path=c.safe_file(root,row['path']);value=c.strict_json(path.read_bytes())
        package=next((name for name in expected if row['path'].startswith(name+'-')),None)
        c.require(package is not None and path.name in expected[package] and row['path'] not in paths,'Wrong/duplicated actual compiler target');paths.add(row['path'])
        features_=value['features'];features_=c.strict_json(features_) if isinstance(features_,str) else features_
        c.require(value==row['content'] and c.sha(path)==row['sha256'] and value['rustflags']==['-Ctarget-cpu=x86-64'] and isinstance(features_,list) and len(features_)==len(set(features_)) and set(features_)==(set(features(arm)) if package=='lab-engine' else set()),'Actual feature/CPU proof differs')
        seen[package].add(path.name)
    c.require(seen==expected,'Missing actual compiler target')
def fingerprints(workspace,arm):
    root=target(workspace,arm)/'release/.fingerprint';rows=[]
    c.require(not list(root.glob('lab-search-*')),'Unexpected search compilation')
    for package,names in fingerprint_names(arm).items():
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if path.is_file():rows.append({'path':path.relative_to(root).as_posix(),'sha256':c.sha(path),'content':c.strict_json(path.read_bytes())})
    check_fingerprints(rows,root,arm);return rows
def machine(cpu):
    data={'cpu_affinity':[cpu],'runner_image':{k:os.environ.get(k) for k in ('ImageOS','ImageVersion','RUNNER_OS','RUNNER_ARCH','RUNNER_ENVIRONMENT')},'platform':sys.platform,'python':sys.version}
    path=Path('/proc/cpuinfo')
    if path.is_file():
        raw=path.read_text();data['cpuinfo_sha256']=c.sha(path)
        data['model_names']=sorted(set(line.partition(':')[2].strip() for line in raw.splitlines() if line.startswith('model name')))
    return data
