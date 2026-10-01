"""Bounded P7f accuracy contract; frozen P7d runtime, new tests only."""
from pathlib import Path
import re,sys,subprocess,tomllib
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p7d_first_hit'))
import pilot_contract as prior
from pilot_contract import c,base_ci,s
import compare_broad,compare_joint,tail_process
REFERENCE=prior.REFERENCE
RUNTIME='8f7bdb4742cec83aac8bf8891e6c5db90142cf34'
PARENT='baaca4dd6f19562dc69ba2a46609a5215bdefea7'
FEATURE=prior.FEATURE
ARMS=prior.ARMS
BUILD_ARMS=prior.BUILD_ARMS
RSS=6*1024**3
CHILD_SECONDS=300
SHARD_SECONDS=1500
PARTITIONS=4
PACKAGE='p7f-build'
TARGET='lab-joint-export'
BROAD='lazy_ko_damage_expanded'
TEST_FILE='engine/core/src/turn/first_hit_coverage_tests.rs'
MOD_FILE='engine/core/src/turn/mod.rs'
REGISTRATION='#[cfg(all(test, feature = "experiment-factored-first-hit"))]\nmod first_hit_coverage_tests;\n'
PLAN_LEDGER=HERE.parent/'p1e5_full500/original-plans.json'
PLAN_LEDGER_SHA='2ebfa8608ab59c501790484c5c040e9ac0a5217e665dc92aff9827cf28107bd4'

def binding():
    b=c.strict_json((HERE/'source-binding.json').read_bytes())
    c.require(b['schema']==1 and b['source_parent']==RUNTIME and b['reference_sha']==REFERENCE and b['controller_parent']==PARENT,'Wrong source lineage')
    c.require(re.fullmatch('[0-9a-f]{40}',b['source_sha']) and b['source_ref']=='refs/heads/codex/p7f-coverage-source-20261002','P7f source unbound')
    c.require(set(b['test_only_file_sha256'])=={TEST_FILE,MOD_FILE},'Wrong test-only delta')
    for digest in b['test_only_file_sha256'].values():c.require(re.fullmatch('[0-9a-f]{64}',digest),'Unbound test source')
    for key in ('source_receipt_sha256','source_review_sha256','logic_receipt_sha256'):c.require(re.fullmatch('[0-9a-f]{64}',b[key]),'Missing source review '+key)
    s.p1.named_tests(b['coverage_tests'])
    c.require(len(b['coverage_tests'])==5 and all(n.startswith('turn::first_hit_coverage_tests::') for n in b['coverage_tests']),'Coverage tests unbound')
    c.require(b['coverage_filter']=='turn::first_hit_coverage_tests::' and b['registration']==REGISTRATION,'Coverage registration/filter differs')
    c.require(b['prior_p7d_binding_sha256']==c.sha(prior.HERE/'source-binding.json') and b['broad_binding_sha256']==c.sha(s.HERE/'source-binding.json'),'Inherited gates changed')
    c.require(b['plan_ledger_sha256']==c.sha(PLAN_LEDGER)==PLAN_LEDGER_SHA,'Original B17 plans changed')
    return b

def features(arm):return prior.features(arm)
def source_root(workspace,arm):return prior.source_root(workspace,arm)
def target(workspace,arm):return workspace/('target-p7f-'+arm)
def environment(workspace,arm):
    env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(target(workspace,arm));return env
def package_root(workspace):return workspace/PACKAGE
def package_bin(workspace,arm,kind):return package_root(workspace)/'bin'/arm/('joint-export' if kind=='joint' else 'broad-tests')
def source_sha(b,arm):return REFERENCE if arm=='reference' else b['source_sha']
def source_fingerprint(workspace,b):
    reference=workspace/'reference';source=workspace/'source'
    for root,expected in ((reference,REFERENCE),(source,b['source_sha'])):
        c.require(base_ci.command(['git','rev-parse','HEAD'],root)==expected and base_ci.command(['git','status','--porcelain'],root)=='','Wrong/modified source')
    c.require(base_ci.command(['git','rev-parse','HEAD^'],source)==RUNTIME,'P7f must directly inherit P7d')
    c.require(set(base_ci.command(['git','diff','--name-only',RUNTIME,'HEAD'],source).splitlines())=={TEST_FILE,MOD_FILE},'Runtime source changed')
    for name,digest in b['test_only_file_sha256'].items():c.require(c.sha(c.safe_file(source,name))==digest,'Test source changed')
    old=subprocess.check_output(['git','show',RUNTIME+':'+MOD_FILE],cwd=source);current=(source/MOD_FILE).read_bytes();block=REGISTRATION.encode()
    c.require(current.count(block)==1 and current.replace(block,b'',1)==old,'Existing module bytes changed beyond cfg(test) registration')
    pb=prior.binding()
    for name,digest in pb['changed_file_sha256'].items():
        if name==MOD_FILE:continue
        c.require(c.sha(c.safe_file(source,name))==digest,'P7d runtime/test changed')
    for arm in ('reference','on'):
        for name,digest in prior.FROZEN.items():c.require(c.sha(c.safe_file(source_root(workspace,arm),name))==digest,'Frozen producer changed')
        for name in (s.BROAD_FILE,'engine/scenario/tests/p1e2_cases.rs'):
            c.require(c.sha(c.safe_file(source_root(workspace,arm),name))==s.binding()['test_only_file_sha256'][name],'Broad fixture source changed')
    f=tomllib.loads((source/'engine/core/Cargo.toml').read_text())['features']
    c.require(f[FEATURE]==[] and FEATURE not in f.get('default',[]) and f[s.p1.FEATURE]==[],'Runtime features/default differ')
    return {'source_sha':b['source_sha'],'source_parent':RUNTIME,'runtime_sha':RUNTIME,'reference_sha':REFERENCE,'test_only_file_sha256':b['test_only_file_sha256'],'runtime_bytes_unchanged':True,'original_producers':prior.FROZEN}
def corpus(workspace):
    value=c.corpus(workspace/'controller');ledger=c.strict_json(PLAN_LEDGER.read_bytes())
    c.require(c.sha(PLAN_LEDGER)==PLAN_LEDGER_SHA and ledger['baseline_run']==36780443039 and ledger['corpus_sha256']==c.CORPUS_SHA,'Wrong historical plan ledger')
    c.require([x['id'] for x in ledger['plans']]==[x['id'] for x in value['cases']],'Plan ledger membership differs')
    return list(zip(value['cases'],ledger['plans']))
def shard_cases(workspace,index):
    c.require(type(index) is int and 0<=index<PARTITIONS,'Unknown shard')
    return [row for i,row in enumerate(corpus(workspace)) if i%PARTITIONS==index]
def broad_contract():return s.binding()['broad_record_contract']

def suites(b,arm):
    args=['--features',','.join('lab-engine/'+f for f in features(arm))];core=['--features',','.join(features(arm))]
    rows=[]
    def add(label,package,target_,filter_,names,feat,exact=False):
        argv=['cargo','test','--locked','--release','-p',package,*target_,*feat]
        if filter_:argv.append(filter_)
        argv+=['--','--test-threads=1','--show-output']
        if exact:argv.append('--exact')
        rows.append((label,argv,names))
    if arm in ('on','edge'):
        old=prior.binding();add('p7d_core','lab-engine',['--lib'],old['core_test_filter'],old['core_tests'],core)
        add('coverage_core','lab-engine',['--lib'],b['coverage_filter'],b['coverage_tests'],core)
    if arm=='on':
        for row in prior.regression_suites():
            add(row['label'],row['package'],['--lib'] if row['target']=='lib' else ['--test',row['target']],row['filter'],row['tests'],core if row['package']=='lab-engine' else args,row['exact'])
    if arm!='edge':
        add('joint_tests','lab-scenario',['--bin',TARGET],None,prior.inherited_tail.TESTS,args)
        rows.append(('joint_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',TARGET,*args],None))
        add('broad_contract','lab-scenario',['--test',BROAD],'p1e2_corpus_contract',['p1e2_corpus_contract'],args,True)
    return rows
def named_count(b):return 74+2*len(b['coverage_tests'])
def log_name(arm,label):return arm+'-'+label+'.log'
def proof(label,text,names):return s.old_ci.named_test_proof(text,names,0 if label=='joint_tests' else None)
def fingerprint_names(arm):
    engine=({'test-lib-lab_engine.json'} if arm=='edge' else {'lib-lab_engine.json'})|({'test-lib-lab_engine.json'} if arm=='on' else set())
    result={'lab-engine':engine}
    if arm!='edge':result['lab-scenario']={'lib-lab_scenario.json','bin-'+TARGET+'.json','test-bin-'+TARGET+'.json','test-integration-test-'+BROAD+'.json'}
    if arm=='on':result['lab-scenario'].update('test-integration-test-'+n+'.json' for n in ('factored','bundle_scenario_04','bundle_scenario_06'))
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
        c.require(found==names,'Missing compiled target '+arm+' '+package)
    return rows
