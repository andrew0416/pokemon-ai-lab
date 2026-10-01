"""P1e4 source identity, exact additive registrations, and compiled feature contract."""
from pathlib import Path
import re,sys,subprocess,tomllib
HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'p1e2_adoption'))
import shared as s
from shared import c,base_ci
import tail as candidate_tail
import oracle_compare
FEATURE='experiment-exact-stream-oracle'
ORIGINAL='a4a881420ca35335c6e0ac4c73fd9e686dd617c0'
CANDIDATE='56376838ffea91169321895f8e81abec2137dd46'
NEW_FILES={'engine/core/src/turn/exact_stream_oracle.rs','engine/scenario/src/bin/lab-exact-stream-oracle.rs'}
REGISTRATIONS={
 'engine/core/Cargo.toml':('[features]\n','# P1e4 validation-only external-memory concrete oracle; default OFF.\nexperiment-exact-stream-oracle = []\n',''),
 'engine/core/src/turn/mod.rs':('mod update;\n','#[cfg(feature = "experiment-exact-stream-oracle")]\n#[doc(hidden)]\npub mod exact_stream_oracle;\n',''),
 'engine/scenario/Cargo.toml':('[features]\n','experiment-exact-stream-oracle = ["lab-engine/experiment-exact-stream-oracle"]\n','\n[[bin]]\nname = "lab-exact-stream-oracle"\npath = "src/bin/lab-exact-stream-oracle.rs"\nrequired-features = ["experiment-exact-stream-oracle"]\n')}
CORE_TESTS=['turn::exact_stream_oracle::tests::'+name for name in [
 'chooser_dynamic_paths_keep_conditional_mass_and_roll_multiplicity','dictionary_rejects_debug_collision_and_resource_limits',
 'dictionary_roundtrip_preserves_last_reserve_and_pending','external_merge_matches_overlap_and_nonuniform_weights',
 'external_rows_keep_correlations_and_last_reserve','failed_emission_rolls_back_actual_battle_work_and_clears_tls',
 'wire_rejects_truncation_invalid_mass_and_scratch_limit']]
BIN_TESTS=['tests::'+name for name in ['concrete_stream_matches_original_flat_and_factored','concrete_stream_preserves_real_full_suspension','frozen_plan_requires_exact_bytes','resource_failure_after_real_stage_keeps_input_and_has_no_manifest']]
SUITES={'oracle_core':CORE_TESTS,'oracle_bin':BIN_TESTS,'candidate_bin':candidate_tail.TESTS}
ARMS=('oracle','candidate')
TARGETS={'oracle':'lab-exact-stream-oracle','candidate':'lab-joint-export'}
TIMEOUTS={'oracle':1200,'candidate':300}
RESULTS='p1e4-oracle-results'

def registration_bytes(name,old):
    anchor,insert,append=REGISTRATIONS[name];anchor=anchor.encode()
    c.require(old.count(anchor)==1,'Registration anchor differs')
    return old.replace(anchor,anchor+insert.encode(),1)+append.encode()

def binding():
    bound=c.strict_json((HERE/'source-binding.json').read_bytes())
    for key in ('oracle_source_sha','candidate_source_sha'):c.require(re.fullmatch('[0-9a-f]{40}',bound[key]),'Unbound source '+key)
    c.require(bound['oracle_source_parent']==ORIGINAL and bound['candidate_source_sha']==CANDIDATE,'Source lineage differs')
    c.require(bound['oracle_source_ref']=='refs/heads/codex/p1e4-oracle-source-20261001','Unexpected oracle ref')
    pins=bound['oracle_source_file_sha256'];c.require(set(pins)==NEW_FILES|set(REGISTRATIONS),'Unexpected oracle delta')
    for value in pins.values():c.require(re.fullmatch('[0-9a-f]{64}',value),'Unbound source hash')
    c.require(bound['oracle_named_suites']==SUITES and bound['oracle_registrations']=={k:list(v) for k,v in REGISTRATIONS.items()},'Source/test contract differs')
    c.require(bound['oracle_feature']==FEATURE and bound['oracle_limits']==oracle_compare.LIMITS,'Oracle feature/guards differ')
    for key in ('oracle_source_receipt_sha256','oracle_root_review_sha256','oracle_logic_receipt_sha256'):c.require(re.fullmatch('[0-9a-f]{64}',bound[key]),'Unbound proof '+key)
    c.require(bound['candidate_binding_sha256']==c.sha(s.HERE/'source-binding.json'),'Candidate binding differs')
    c.require(bound['inherited_tail_process_sha256']==c.sha(s.HERE/'tail_process.py') and bound['inherited_joint_comparator_sha256']==c.sha(s.HERE/'compare_joint.py'),'Inherited validation helper changed')
    c.require(bound['case_id']=='opening-0429' and bound['plan_sha256']=='2a6ec43ac6e821e1d710a94542f80f6ab04f37416406fb6a932b40f3d6de89ec','Frozen selection differs')
    c.require(bound['timeout_seconds']==TIMEOUTS and bound['rss_limit_bytes']==6*1024**3 and bound['job_timeout_minutes']==35 and bound['adoption_approved'] is False,'Resource/claim contract differs')
    return bound

def verify_oracle(root,bound):
    c.require(base_ci.command(['git','rev-parse','HEAD'],root)==bound['oracle_source_sha'],'Oracle SHA differs')
    c.require(base_ci.command(['git','rev-parse','HEAD^'],root)==ORIGINAL,'Oracle parent differs')
    c.require(base_ci.command(['git','status','--porcelain'],root)=='','Oracle checkout modified')
    changed=set(base_ci.command(['git','diff','--name-only',ORIGINAL,'HEAD'],root).splitlines())
    c.require(changed==NEW_FILES|set(REGISTRATIONS),'Out-of-scope oracle runtime change')
    for name,digest in bound['oracle_source_file_sha256'].items():c.require(c.sha(c.safe_file(root,name))==digest,'Oracle file differs')
    for name in REGISTRATIONS:
        old=subprocess.check_output(['git','show',ORIGINAL+':'+name],cwd=root)
        c.require((root/name).read_bytes()==registration_bytes(name,old),'Existing function/manifest bytes changed')
    for manifest in ('engine/core/Cargo.toml','engine/scenario/Cargo.toml'):
        features=tomllib.loads((root/manifest).read_text())['features']
        c.require(FEATURE not in features.get('default',[]),'Oracle must remain default OFF')
    return {'source_sha':bound['oracle_source_sha'],'parent':ORIGINAL,'changed_file_sha256':bound['oracle_source_file_sha256'],'existing_function_bodies_byte_unchanged':True,'feature_default_on':False}

def verify_sources(workspace,bound):
    return {'oracle':verify_oracle(workspace/'oracle',bound),'candidate':s.verify_source(workspace/'source',s.binding())}

def source_root(workspace,arm):return workspace/('oracle' if arm=='oracle' else 'source')
def features(arm):return s.p1.features(False)+[FEATURE] if arm=='oracle' else s.p1.features(True)
def target_root(workspace,arm):return workspace/('target-p1e4-'+arm)
def environment(workspace,arm):
    c.require(arm in ARMS,'Unknown oracle arm');env=base_ci.environment(workspace);env['CARGO_TARGET_DIR']=str(target_root(workspace,arm));return env

def commands(arm):
    if arm=='oracle':
        args=['--features',','.join(['lab-engine/'+name for name in s.p1.features(False)]+[FEATURE])]
        return [('oracle_core',['cargo','test','--locked','--release','-p','lab-engine','--lib','--features',','.join(features(arm)),'turn::exact_stream_oracle::tests::','--','--test-threads=1','--show-output']),
          ('oracle_bin',['cargo','test','--locked','--release','-p','lab-scenario','--bin',TARGETS[arm],*args,'--','--test-threads=1','--show-output']),
          ('build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',TARGETS[arm],*args])]
    c.require(arm=='candidate','Unknown build arm');args=s.p1.feature_args(True)
    return [('candidate_bin',['cargo','test','--locked','--release','-p','lab-scenario','--bin',TARGETS[arm],*args,'--','--test-threads=1','--show-output']),
      ('build',['cargo','build','--locked','--release','-p','lab-scenario','--bin',TARGETS[arm],*args])]

def command_log_name(arm,label):return arm+'-'+label+'.log'
def test_proof(label,text):return s.old_ci.named_test_proof(text,SUITES[label],None if label=='oracle_core' else 0)

def fingerprints(workspace,arm):
    root=target_root(workspace,arm)/'release/.fingerprint';rows=[]
    c.require(not list(root.glob('lab-search-*')),'Unexpected search crate')
    engine={'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if arm=='oracle' else set())
    packages={'lab-engine':engine,'lab-scenario':{'lib-lab_scenario.json','bin-'+TARGETS[arm]+'.json','test-bin-'+TARGETS[arm]+'.json'}}
    for package,names in packages.items():
        seen=set();expected=set(features(arm)) if package=='lab-engine' else ({FEATURE} if arm=='oracle' else set())
        for folder in sorted(root.glob(package+'-*')):
            for name in sorted(names):
                path=folder/name
                if not path.is_file():continue
                value=c.strict_json(path.read_bytes());actual=value['features']
                if isinstance(actual,str):actual=c.strict_json(actual)
                c.require(isinstance(actual,list) and len(actual)==len(set(actual)) and set(actual)==expected,'Actual compiled features differ')
                c.require(value['rustflags']==['-Ctarget-cpu=x86-64'],'Generic target differs')
                seen.add(name);rows.append({'path':str(path.relative_to(root)),'sha256':c.sha(path),'features':actual,'content':value})
        c.require(seen==names,'Missing compiled fingerprints '+arm+' '+package)
    return rows
