"""Fresh correctness then timing for the default-OFF Full hit-suffix candidate."""
from pathlib import Path
import re,sys,subprocess,tomllib,os
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p7f_coverage'))
import coverage_contract as coverage
from coverage_contract import c,base_ci,s,compare_broad,compare_joint,tail_process
prior=coverage.prior
PARENT='4015fc0ec563ab8ac0b9483745a7b83e9f630390'
REFERENCE='87e7fdcb47b24e407e81d9d5688714dfee6de35d'
FEATURE='experiment-factored-hit-suffix'
ARMS=('reference','off','on')
BUILD_ARMS=ARMS
TIMING_ARMS=('off','on')
RSS=6*1024**3
CHILD_SECONDS=300
SHARD_SECONDS=1500
TIMING_SECONDS=2100
PARTITIONS=4
PACKAGE='p7h-build'
TARGET='lab-joint-export'
BENCHMARK='lab-distribution-bench'
BROAD='lazy_ko_damage_expanded'
PLAN_LEDGER=coverage.PLAN_LEDGER
PLAN_LEDGER_SHA=coverage.PLAN_LEDGER_SHA
def binding():
    b=c.strict_json((HERE/'source-binding.json').read_bytes())
    c.require(b['schema']==1 and b['source_parent']==REFERENCE and b['reference_sha']==REFERENCE and b['controller_parent']==PARENT,'Wrong candidate lineage')
    c.require(re.fullmatch('[0-9a-f]{40}',b['source_sha']) and b['source_ref']=='refs/heads/codex/p7h-hit-suffix-source-20261002','Source not frozen')
    pins=b['changed_file_sha256']
    c.require(isinstance(pins,dict) and pins and all(name=='engine/core/Cargo.toml' or (name.startswith('engine/core/src/') and name.endswith('.rs') and '..' not in name.split('/')) for name in pins),'Unapproved source delta')
    for digest in pins.values():c.require(re.fullmatch('[0-9a-f]{64}',digest),'Unbound source file')
    for key in ('source_receipt_sha256','source_review_sha256','logic_receipt_sha256'):c.require(re.fullmatch('[0-9a-f]{64}',b[key]),'Unbound source validation')
    c.require(b['feature']==FEATURE and b['feature_default_on'] is False and b['feature_dependency']=='experiment-factored-first-hit','Feature contract differs')
    s.p1.named_tests(b['core_tests']);c.require(isinstance(b['core_test_filter'],str) and ((b['core_test_filter']=='p7h' and all('p7h' in name for name in b['core_tests'])) or (b['core_test_filter'].startswith('turn::') and all(name.startswith(b['core_test_filter']) for name in b['core_tests']))),'New tests unbound')
    c.require(b['prior_p7f_binding_sha256']==c.sha(coverage.HERE/'source-binding.json') and coverage.binding()['source_sha']==REFERENCE,'Reference evidence changed')
    c.require(b['plan_ledger_sha256']==c.sha(PLAN_LEDGER)==PLAN_LEDGER_SHA and b['corpus_sha256']==c.CORPUS_SHA,'Frozen input contract changed')
    return b
def candidate_sha():return binding()['source_sha']
def features(arm):
    c.require(arm in ARMS,'Unknown arm')
    return prior.features('on')+([FEATURE] if arm=='on' else [])
def source_root(workspace,arm):return workspace/('reference' if arm=='reference' else 'source')
def source_sha(b,arm):return REFERENCE if arm=='reference' else b['source_sha']
def target(workspace,arm):return workspace/('target-p7h-'+arm)
def environment(workspace,arm):
    env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(target(workspace,arm));return env
def package_root(workspace):return workspace/PACKAGE
def package_bin(workspace,arm,kind):return package_root(workspace)/'bin'/arm/{'joint':'joint-export','broad':'broad-tests','benchmark':'benchmark'}[kind]
def binary(workspace,arm):return package_bin(workspace,arm,'benchmark')
def bin_kinds(arm):return ('joint','broad')+(() if arm=='reference' else ('benchmark',))
def expected_source_proof(b):
    return {'source_sha':b['source_sha'],'source_parent':REFERENCE,'reference_sha':REFERENCE,'changed_file_sha256':b['changed_file_sha256'],'frozen_producers':prior.FROZEN,'feature':FEATURE,'default_off':True}
def source_fingerprint(workspace,b):
    source=workspace/'source';reference=workspace/'reference'
    for root,expected in ((reference,REFERENCE),(source,b['source_sha'])):
        c.require(base_ci.command(['git','rev-parse','HEAD'],root)==expected and base_ci.command(['git','status','--porcelain'],root)=='','Wrong/modified source')
    c.require(base_ci.command(['git','rev-parse','HEAD^'],source)==REFERENCE,'Candidate must directly inherit87e7')
    c.require(set(base_ci.command(['git','diff','--name-only',REFERENCE,'HEAD'],source).splitlines())==set(b['changed_file_sha256']),'Candidate delta differs')
    for name,digest in b['changed_file_sha256'].items():c.require(c.sha(c.safe_file(source,name))==digest,'Frozen source file changed')
    for root in (source,reference):
        for name,digest in prior.FROZEN.items():c.require(c.sha(c.safe_file(root,name))==digest,'Frozen exporter/benchmark changed')
        for name in (s.BROAD_FILE,'engine/scenario/tests/p1e2_cases.rs'):
            c.require(c.sha(c.safe_file(root,name))==s.binding()['test_only_file_sha256'][name],'Broad104 source changed')
    features_=tomllib.loads((source/'engine/core/Cargo.toml').read_text())['features']
    c.require(features_[FEATURE]==['experiment-factored-first-hit'] and FEATURE not in features_.get('default',[]),'Suffix feature must depend on first-hit and stay defaultOFF')
    return expected_source_proof(b)
def corpus(workspace):return coverage.corpus(workspace)
def shard_cases(workspace,index):
    c.require(type(index) is int and 0<=index<4,'Unknown shard');return corpus(workspace)[index::4]
def broad_contract():return coverage.broad_contract()
def named_count(b):return 84+len(b['core_tests'])
def suites(b,arm):
    args=['--features',','.join('lab-engine/'+f for f in features(arm))];core=['--features',','.join(features(arm))];rows=[]
    def add(label,package,target_,filter_,names,feat,exact=False):
        argv=['cargo','test','--locked','--release','-p',package,*target_,*feat]
        if filter_:argv.append(filter_)
        argv+=['--','--test-threads=1','--show-output']
        if exact:argv.append('--exact')
        rows.append((label,argv,names))
    if arm=='on':
        add('p7d_core','lab-engine',['--lib'],prior.binding()['core_test_filter'],prior.binding()['core_tests'],core)
        add('p7f_core','lab-engine',['--lib'],coverage.binding()['coverage_filter'],coverage.binding()['coverage_tests'],core)
        add('suffix_core','lab-engine',['--lib'],b['core_test_filter'],b['core_tests'],core)
        for row in prior.regression_suites():
            add(row['label'],row['package'],['--lib'] if row['target']=='lib' else ['--test',row['target']],row['filter'],row['tests'],core if row['package']=='lab-engine' else args,row['exact'])
    for kind,bin_,names in [('joint',TARGET,prior.inherited_tail.TESTS)]+([] if arm=='reference' else [('benchmark',BENCHMARK,list(c.TESTS))]):
        add(kind+'_tests','lab-scenario',['--bin',bin_],None,names,args)
        rows.append((kind+'_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',bin_,*args],None))
    add('broad_contract','lab-scenario',['--test',BROAD],'p1e2_corpus_contract',['p1e2_corpus_contract'],args,True)
    return rows
def log_name(arm,label):return arm+'-'+label+'.log'
def proof(label,text,names):return s.old_ci.named_test_proof(text,names,0 if label in ('joint_tests','benchmark_tests') else None)
def fingerprint_names(arm):
    result={'lab-engine':{'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if arm=='on' else set()),
      'lab-scenario':{'lib-lab_scenario.json','bin-'+TARGET+'.json','test-bin-'+TARGET+'.json','test-integration-test-'+BROAD+'.json'}}
    if arm!='reference':result['lab-scenario'].update({'bin-'+BENCHMARK+'.json','test-bin-'+BENCHMARK+'.json'})
    if arm=='on':result['lab-scenario'].update('test-integration-test-'+name+'.json' for name in ('factored','bundle_scenario_04','bundle_scenario_06'))
    return result
def fingerprints(workspace,arm):
    root=target(workspace,arm)/'release/.fingerprint';rows=[]
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    for package,names in fingerprint_names(arm).items():
        found=set()
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                data=c.strict_json(path.read_bytes());actual=data['features'];actual=c.strict_json(actual) if isinstance(actual,str) else actual
                expected=set(features(arm)) if package=='lab-engine' else set()
                c.require(isinstance(actual,list) and len(actual)==len(set(actual)) and set(actual)==expected and data['rustflags']==['-Ctarget-cpu=x86-64'],'Actual feature/CPU differs')
                found.add(name);rows.append({'path':path.relative_to(root).as_posix(),'sha256':c.sha(path),'features':actual,'content':data})
        c.require(found==names,'Missing actual compiler target')
    return rows
def machine(cpu):
    data={'cpu_affinity':[cpu],'runner_image':{k:os.environ.get(k) for k in ('ImageOS','ImageVersion','RUNNER_OS','RUNNER_ARCH','RUNNER_ENVIRONMENT')},'platform':sys.platform,'python':sys.version}
    path=Path('/proc/cpuinfo')
    if path.is_file():
        raw=path.read_text();data['cpuinfo_sha256']=c.sha(path)
        data['model_names']=sorted(set(line.partition(':')[2].strip() for line in raw.splitlines() if line.startswith('model name')))
    return data
