"""P7d pilot: immutable source, corpus, build and feature identities."""
from pathlib import Path
import re,sys,tomllib
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p1e2_adoption'))
import shared as s
from shared import c,base_ci
import tail as inherited_tail
import compare_joint
FEATURE='experiment-factored-first-hit'
REFERENCE='56376838ffea91169321895f8e81abec2137dd46'
PARENT='6508953962bc46ed7c6a7780a0e859b3444231ff'
RESULTS='p7d-first-hit-results'
ARMS=('reference','off','on')
BUILD_ARMS=ARMS+('edge',)
BINS={'joint':'lab-joint-export','benchmark':'lab-distribution-bench'}
FROZEN={s.p1.BENCHMARK:s.p1.BENCHMARK_SHA,
 'engine/scenario/src/bin/lab-joint-export.rs':'07ba3fdc3e09b90ea8e9dc90cc4bdc8edd96e96eeafea0ebc80fcaa843d11a43'}
CASE_IDS=['opening-'+v for v in ('0000','0001','0099','0110','0129','0133','0279','0429')]
TIMED_IDS=['opening-'+v for v in ('0099','0110','0129','0279','0429')]
ALLOWED={'engine/core/Cargo.toml'}|{'engine/core/src/turn/'+v+'.rs' for v in ('mod','battle','branch','frontier','moves','lazy','first_hit','first_hit_tests')}
ALLOWED.update({'engine/core/src/turn/frontier/lazy_ko_tests.rs','engine/core/src/turn/moves/p1e_damage_tests.rs'})
RSS=6*1024**3
PHASE_SECONDS=900
CHILD_SECONDS=300

def binding():
    b=c.strict_json((HERE/'source-binding.json').read_bytes())
    c.require(b['schema']==1 and b['source_parent']==REFERENCE and b['reference_sha']==REFERENCE and b['controller_parent']==PARENT,'Source lineage changed')
    c.require(re.fullmatch('[0-9a-f]{40}',b['source_sha']) and b['source_ref']=='refs/heads/codex/p7d-first-hit-source-20261002','P7d source unbound')
    pins=b['changed_file_sha256']
    c.require(isinstance(pins,dict) and 'engine/core/Cargo.toml' in pins and set(pins)<=ALLOWED,'Unbound/unapproved source delta')
    for digest in pins.values():c.require(re.fullmatch('[0-9a-f]{64}',digest),'Source hash unbound')
    for key in ('source_receipt_sha256','source_review_sha256','logic_receipt_sha256'):
        c.require(re.fullmatch('[0-9a-f]{64}',b[key]),'Unbound source gate '+key)
    s.p1.named_tests(b['core_tests'])
    c.require(all(n.startswith('turn::first_hit_tests::tests::') for n in b['core_tests']),'Wrong private test filter')
    c.require(b['core_test_filter']=='turn::first_hit_tests::' and b['feature']==FEATURE and b['feature_default_on'] is False,'Feature/test contract changed')
    for relative,digest in b['inherited_helper_sha256'].items():
        c.require(relative in ('p1e2_adoption/compare_joint.py','p1e2_adoption/tail_process.py','turn_distribution/contract.py','lazy_ko_damage/common.py'),'Unexpected inherited helper')
        c.require(c.sha(c.safe_file(HERE.parent,relative))==digest,'Inherited helper changed')
    c.require(len(b['inherited_helper_sha256'])==4 and b['pilot_manifest_sha256']==c.sha(HERE/'pilot-manifest.json'),'Pilot/helper pins incomplete')
    return b

def pilot(workspace):
    value=c.strict_json((HERE/'pilot-manifest.json').read_bytes())
    corpus=c.corpus(workspace/'controller')
    c.require(value['schema']==1 and value['reference_sha']==REFERENCE and value['corpus_sha256']==c.CORPUS_SHA,'Wrong pilot corpus')
    c.require(value['case_ids']==CASE_IDS and value['timing_case_ids']==TIMED_IDS and value['selection_run']==36780443039,'Pilot changed')
    c.require(value['timing_blocks']==3 and value['order']==['off','on','on','off'] and value['sampler_metric_role']=='ancillary','Timing protocol changed')
    c.require([r['id'] for r in value['cases']]==CASE_IDS,'Pilot plans incomplete')
    rows=[]
    for selected in value['cases']:
        case=corpus['cases'][int(selected['id'].split('-')[1])]
        plan=c.safe_file(HERE,'plans/'+case['id']+'.json')
        c.require(case['id']==selected['id'] and c.sha(plan)==selected['plan_sha256'],'Frozen B17 plan changed')
        c.require(plan.stat().st_size==selected['plan_bytes'],'Plan bytes differ')
        c.description(c.line(plan),case);rows.append((case,plan))
    return value,rows

def source_root(workspace,arm):
    c.require(arm in BUILD_ARMS,'Unknown arm');return workspace/('reference' if arm=='reference' else 'source')
def source_sha(b,arm):return REFERENCE if arm=='reference' else b['source_sha']
def features(arm):
    c.require(arm in BUILD_ARMS,'Unknown arm')
    return list(c.CORE_FEATURES)+([] if arm=='edge' else [s.p1.FEATURE])+([FEATURE] if arm in ('on','edge') else [])
def target(workspace,arm):return workspace/('target-p7d-'+arm)
def environment(workspace,arm):
    env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(target(workspace,arm));return env
def bin_kinds(arm):return ('joint',) if arm=='reference' else (() if arm=='edge' else ('joint','benchmark'))
def regression_suites():
    value=c.strict_json((HERE/'regression-suites.json').read_bytes())
    c.require(value['schema']==1 and value['total_tests']==35 and sum(len(r['tests']) for r in value['suites'])==35,'Regression contract changed')
    return value['suites']
def expected_count(b):return 32+2*len(b['core_tests'])+35

def verify_sources(workspace,b):
    c.require(base_ci.command(['git','rev-parse','HEAD'],workspace/'reference')==REFERENCE,'Wrong immutable reference')
    c.require(base_ci.command(['git','status','--porcelain'],workspace/'reference')=='','Reference modified')
    root=workspace/'source'
    c.require(base_ci.command(['git','rev-parse','HEAD'],root)==b['source_sha'],'Candidate SHA changed')
    c.require(base_ci.command(['git','rev-parse','HEAD^'],root)==REFERENCE,'Candidate parent changed')
    c.require(base_ci.command(['git','status','--porcelain'],root)=='','Candidate modified')
    changed=set(base_ci.command(['git','diff','--name-only',REFERENCE,'HEAD'],root).splitlines())
    c.require(changed==set(b['changed_file_sha256']),'Source delta differs')
    for name,digest in b['changed_file_sha256'].items():c.require(c.sha(c.safe_file(root,name))==digest,'Source file differs')
    for arm in ('reference','on'):
        for name,digest in FROZEN.items():c.require(c.sha(c.safe_file(source_root(workspace,arm),name))==digest,'Frozen producer changed')
    for row in regression_suites():c.require(c.sha(c.safe_file(root,row['source_path']))==row['source_sha256'],'Existing regression source differs')
    f=tomllib.loads((root/'engine/core/Cargo.toml').read_text())['features']
    c.require(f[FEATURE]==[] and FEATURE not in f.get('default',[]) and f[s.p1.FEATURE]==[],'Features must stay independent default OFF')
    return {'reference_sha':REFERENCE,'source_sha':b['source_sha'],'source_parent':REFERENCE,'changed_file_sha256':b['changed_file_sha256'],'frozen_producers':FROZEN}

def commands(arm,b):
    core=features(arm);args=['--features',','.join('lab-engine/'+v for v in core)]
    rows=[]
    if arm in ('on','edge'):
        rows.append(('core',['cargo','test','--locked','--release','-p','lab-engine','--lib','--features',','.join(core),b['core_test_filter'],'--','--test-threads=1','--show-output'],b['core_tests']))
    if arm=='on':
        for suite in regression_suites():
            testargs=['--lib'] if suite['target']=='lib' else ['--test',suite['target']]
            feat=['--features',','.join(core)] if suite['package']=='lab-engine' else args
            argv=['cargo','test','--locked','--release','-p',suite['package'],*testargs,*feat,suite['filter'],'--','--test-threads=1','--show-output']
            if suite['exact']:argv.append('--exact')
            rows.append((suite['label'],argv,suite['tests']))
    for kind in bin_kinds(arm):
        names=inherited_tail.TESTS if kind=='joint' else list(c.TESTS)
        rows.append((kind+'_tests',['cargo','test','--locked','--release','-p','lab-scenario','--bin',BINS[kind],*args,'--','--test-threads=1','--show-output'],names))
        rows.append((kind+'_build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',BINS[kind],*args],None))
    return rows

def command_log_name(arm,label):return arm+'-'+label+'.log'
def test_proof(label,text,names):return s.old_ci.named_test_proof(text,names,None if label=='core' or label.startswith('regress_') else 0)
def fingerprints(workspace,arm):
    root=target(workspace,arm)/'release/.fingerprint';rows=[]
    engine=({'test-lib-lab_engine.json'} if arm=='edge' else {'lib-lab_engine.json'})|({'test-lib-lab_engine.json'} if arm=='on' else set())
    packages={'lab-engine':engine}
    if arm!='edge':packages['lab-scenario']={'lib-lab_scenario.json'}|{prefix+BINS[k]+'.json' for k in bin_kinds(arm) for prefix in ('bin-','test-bin-')}
    if arm=='on':packages['lab-scenario'].update('test-integration-test-'+name+'.json' for name in ('factored','bundle_scenario_04','bundle_scenario_06'))
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    for package,names in packages.items():
        seen=set();expected=set(features(arm)) if package=='lab-engine' else set()
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                value=c.strict_json(path.read_bytes());actual=value['features']
                if isinstance(actual,str):actual=c.strict_json(actual)
                c.require(isinstance(actual,list) and len(actual)==len(set(actual)) and set(actual)==expected,'Actual features differ')
                c.require(value['rustflags']==['-Ctarget-cpu=x86-64'],'Actual CPU target differs')
                rows.append({'path':str(path.relative_to(root)),'sha256':c.sha(path),'features':actual,'content':value});seen.add(name)
        c.require(seen==names,'Missing actual compiler fingerprint '+arm+' '+package)
    return rows
