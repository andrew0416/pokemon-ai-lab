"""Fresh common5 P13 proof: exact ownership, output and actual search activation."""
from collections import Counter
import itertools
import json
import os
from pathlib import Path
import re
import tomllib

import ci
import compact_probe as process
import run as bench

SOURCE_SHA = '5d2d3581150b4464cc6586fd0c818be4b4a15d46'
MODE = 'borrowed-child-keys'
PUBLIC_TEST = 'exact_off_on_search_turn_error_and_resume_records'
LIB_PREFIX = 'solve::child_keys_tests::'
COMMON_TESTS = tuple(LIB_PREFIX + name for name in (
    'allocator_subprocess_proves_owned_lookup_allocations_are_removed',
    'exact_private_policy_records',
    'nash_cells_budget_and_transition_errors_do_not_register_children',
    'six_actual_nash_value_hit_miss_error_capture_cases_restore_state',
    'twelve_batch_capture_cases_preserve_slots_stats_and_disabled_capacity_zero'))
ON_TESTS = tuple(LIB_PREFIX + name for name in (
    'collision_guard_distinguishes_owned_payloads_and_different_some_suspensions',
    'forced_collision_chain_retains_full_state_and_suspension_across_growth')) + tuple(
    'tt::borrowed_tests::' + name for name in (
    'borrowed_option_hash_input_and_collision_eq_match_owned_keys',
    'capacity_full_update_order_disabled_and_clear_are_unchanged',
    'collision_bucket_distinguishes_two_some_suspensions'))
PROBE = 'ci_borrowed_child_keys_observer'
PROBE_PATH = 'engine/search/examples/' + PROBE + '.rs'
SOURCE_FILES = {'engine/core/Cargo.toml': '1abe06fc489023e600383b3ed32bd20bb007ac58883b9100217ff2ede83ee476', 'engine/core/src/hash.rs': 'c0de382811d2e8311750c5b47349791cb22087dcdd503d3a474580bde3fc009f', 'engine/search/Cargo.toml': 'c98062acf030365d9e36895488d6dc7ab072d0fadd443c5558aa0d46949d0088', 'engine/search/src/solve.rs': '06606b857a0afdfee9288c71ee066d2cba6e92e80b59c063d16b22c45998576e', 'engine/search/src/solve/child_keys.rs': '753d32893f42a5323e943f690b46a709d1ffdf389c2bac26b779f7b98cb0e248', 'engine/search/src/solve/child_keys_tests.rs': '4c27d64e2c9384f9f1eda90ce93ebac53d3aa550daf4e5f437cc159fbc05d27b', 'engine/search/src/tt.rs': '111b95dd89c8cba91f99f08cbd0a82963f49046c9e73bdf92e79bedcbe5810ec', 'engine/search/src/tt_borrowed_tests.rs': '3ccfb3526f021592720c7d2909a895e439499063c05eb57b064a7d61ce3be06c', 'engine/search/tests/borrowed_child_keys.rs': '0a3f990d057dec8e8e815c7bc350c044b88577679394bc6ef037260ad0685056', 'engine/search/tests/p13_support.rs': '262c42b8db4b507a364dff8adfbe6b72bc2483db94f8d457379c64647822d8e1'}
COUNT_FIELDS = {'key_captures','job_captures','borrowed_queries','seen_hits','seen_collisions','seen_links'}
KINDS = {'toy':32,'depth3':2,'terminal':2,'unsupported':2,'successful-resume':1,'successful-replacement':1,'fixture':7}
PUBLIC_FIELDS = {
 'toy':{'after','analysis','before','cache_stats','chance','equilibrium_bits','factored','kind','matrix_bits','nash_value_bits','shallow_equilibrium_bits','shallow_matrix_bits','slots','stats','threads','tt'},
 'depth3':{'after','analysis','before','kind','matrix','slots','stats','value'},
 'terminal':{'after','before','kind','refusal','result','slots','stats','value_bits'},
 'unsupported':{'after','before','kind','refusal','result','slots','stats','value_bits'},
 'successful-resume':{'after','before','kind','stats','suspension','value_bits'},
 'successful-replacement':{'after','before','kind','stats','value_bits'},
 'fixture':{'before','ending','index','instructions','kind','name','probability_bits','result','reversed','stats','suspension','value_bits'},
}

def require(value,message):
    if not value:raise ValueError(message)

def verify_declarations(root):
    paths={name:root/f'engine/{name}/Cargo.toml' for name in ('core','search','scenario')}
    features={name:ci.read_features(path) for name,path in paths.items()}
    runtime,observer=ci.BORROWED_FEATURE,ci.BORROWED_OBSERVER
    require(features['core'].get(runtime)==[] and observer not in features['core'],'P13 core helper declaration differs')
    require(features['search'].get(runtime)==['lab-engine/'+runtime] and features['search'].get(observer)==[],
            'P13 search runtime forwarding or independent observer differs')
    require(runtime not in features['scenario'] and observer not in features['scenario'],'Unexpected scenario forwarding')
    for name,path in paths.items():ci.reject_default_experiments(path,features[name])
    rows=[r for r in tomllib.loads(paths['search'].read_text(encoding='utf-8')).get('test',[]) if r.get('name')=='borrowed_child_keys']
    require(len(rows)==1 and rows[0].get('path')=='tests/borrowed_child_keys.rs' and not rows[0].get('required-features'),
            'P13 exact integration test registration differs')
    require(len(SOURCE_FILES)==10,'P13 implementation hash pins are missing')
    for path,digest in SOURCE_FILES.items():require(ci.sha(root/path)==digest,'P13 immutable source changed: '+path)
    return {'default_off':True,'independent_search_observer':True,'source_file_sha256':SOURCE_FILES}

def summaries(text):
    return [tuple(map(int,row)) for row in re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',text,re.M)]

def validate_tests(text,expected,*,lib=False,on=False,compact=False):
    passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
    require(all(passed.count(name)==1 for name in expected),'P13 required named test skipped or duplicated')
    counts=summaries(text)
    require(counts and counts[-1][0]>=len(expected) and counts[-1][1:]==(0,0,0,0),'P13 test suite incomplete')
    if not lib:require(passed==[PUBLIC_TEST] and counts==[(1,0,0,0,0)],'P13 public target must run exactly one test')
    else:
        require(len(counts)==2 and counts[0][0:4]==(1,0,0,0),'P13 allocator isolated child did not execute')
        for name in ON_TESTS:require((name in passed)==on,'P13 conditional test feature mismatch')
        alloc=re.findall(r'allocator old=(\d+) actual=(\d+) candidate=(true|false)',text)
        reference=2 if compact else 1
        require(alloc==[(str(reference),str(0 if on else reference),str(on).lower())],'P13 real allocation removal proof differs')
    return {'passed':len(passed),'required_named_tests':list(expected),'summaries':counts}

def validate_records(path,*,private=False):
    raw=path.read_bytes()
    require(raw.endswith(b'\n') and b'\r' not in raw,'P13 complete LF JSONL required')
    rows=[process._json(line) for line in raw.splitlines()]
    require(all(isinstance(row,dict) for row in rows),'P13 record is not an object')
    if private:
        require(len(rows)==5 and [r.get('capacity') for r in rows[:3]]==[0,1,2]
                and [r.get('reverse') for r in rows[3:]]==[False,True],'P13 private policy coverage differs')
        for i,row in enumerate(rows):
            fields={'broken','capacity','nodes','omitted','state','stats','tt_len','turns','unsupported','values'} if i<3 else {'broken','nodes','omitted','result','reverse','state','turns','unsupported'}
            require(set(row)==fields and isinstance(row['state'],str) and row['state'],'P13 full private state/counters missing')
    else:
        require(Counter(r.get('kind') for r in rows)==KINDS,'P13 public case counts differ')
        combos={(r['slots'],r['chance'],r['threads'],r['tt'],r['factored']) for r in rows if r['kind']=='toy'}
        require(combos==set(itertools.product((1,2),('Expect','Worst'),(1,2),(False,True),(False,True))),'P13 bounded search configuration coverage differs')
        fixtures=[(r.get('name'),r.get('index')) for r in rows if r['kind']=='fixture']
        require(fixtures==[('aa-power-construct',i) for i in range(4)]+[('ability-change-fails',0),('eject-button-uturn',0),('eject-button-uturn',1)],'P13 turn fixture ordering differs')
        for row in rows:
            require(set(row)==PUBLIC_FIELDS[row['kind']],'P13 complete public record fields differ')
            for key in ('before','after','ending','reversed'):
                if key in row:
                    state=row[key]
                    require(set(state)=={'debug','full_hash','position_hash'} and isinstance(state['debug'],str) and bool(state['debug'])
                            and all(type(state[k]) is int and 0<=state[k]<2**64 for k in ('full_hash','position_hash')),'P13 full State Debug/hash missing')
            require(len(row['stats'])==7 and all(type(v) is int and v>=0 for v in row['stats']),'P13 integer statistics missing')
            if 'after' in row:require(row['after']==row['before'],'P13 search restoration failed')
            if row['kind']=='fixture':require(row['reversed']==row['before'] and isinstance(row['instructions'],str) and isinstance(row['suspension'],str),'P13 exact instructions/suspension/rollback missing')
    return {'records':len(rows),'sha256':ci.sha(path),'bytes':len(raw)}

def validate_counts(text,on):
    lines=re.findall(r'^P13 activation: (.+)$',text,re.M)
    require(len(lines)==1,'P13 public search observer evidence missing or repeated')
    value=process._json(lines[0])
    require(set(value)==COUNT_FIELDS and all(type(v) is int and v>=0 for v in value.values()),'P13 observer fields malformed')
    require(value['job_captures']>0,'P13 workload did not create child jobs')
    if on:require(value['key_captures']==0 and value['borrowed_queries']>0 and value['seen_links']>0,'P13 borrowed path did not activate')
    else:require(value['key_captures']>0 and value['borrowed_queries']==value['seen_links']==value['seen_collisions']==0,'P13 owned reference path differs')
    return value

def compare_counts(before,after):
    require(before['key_captures']==after['borrowed_queries']>0 and after['key_captures']==0
            and before['job_captures']==after['job_captures']>0 and before['seen_hits']==after['seen_hits'],
            'P13 actual capture reduction or work-preservation equation failed')

def validate_full_regressions(result):
    evidence={}
    for arm in ('baseline','candidate'):
        path=result/(arm+'-build-receipt.json');row=json.loads(path.read_text(encoding='utf-8'))
        require(row.get('status')=='success' and row.get('reused') is False and row.get('selection')==MODE,'P13 requires fresh full regressions')
        text=(result/(arm+'-build.log')).read_text(encoding='utf-8');passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
        require(passed.count(PUBLIC_TEST)==1,'P13 public records test missing from full regressions')
        for name in ON_TESTS[2:]:require((passed.count(name)==1)==(arm=='candidate'),'P13 conditional TT tests missing/miscompiled')
        evidence[arm]={'receipt_sha256':ci.sha(path),'log_sha256':ci.sha(result/(arm+'-build.log'))}
    return evidence

def variant_features(arm,compact,observer):
    flags=ci.feature_args(MODE,arm)[1].split(',')
    _,expected,_=ci.fingerprint_expectations(MODE,arm)
    if not compact:
        flags.remove('lab-engine/'+ci.COMPACT_FEATURE);expected['lab-engine'][ci.COMPACT_FEATURE]=False
    if observer:
        flags.append('lab-search/'+ci.BORROWED_OBSERVER);expected['lab-search'][ci.BORROWED_OBSERVER]=True
    return ','.join(flags),expected

def validate(workspace):
    workspace=Path(workspace).resolve();result=workspace/'ci-results';out=result/'borrowed-child-keys-validation';out.mkdir(exist_ok=False)
    receipt={'status':'running','selection':MODE,'cached_results_reused':False,'performance_measurement':False,
             'common_runtime':'old4+P8d; E/F/P11/P12 OFF; dense axis only disables P10',
             'bounded_profile':'debug opt0','public_activation_profile':'release opt3','commands':[],'variants':{},'activation':{}}
    def save():(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    def text(stem):return stem.with_suffix('.stdout').read_text(encoding='utf-8')+'\n'+stem.with_suffix('.stderr').read_text(encoding='utf-8')
    def execute(argv,env,stem):return process._run(argv,workspace/'candidate/engine',env,stem,ci.COMMAND_TIMEOUT_SECONDS,receipt['commands'],save)
    def environment(label):
        env=process.environment(workspace/('target-'+label))
        for name in ('LAB_P13_RECORDS','LAB_P13_UNIT_RECORDS','LAB_P13_ALLOC_CHILD','LAB_ENGINE_STATS'):env.pop(name,None)
        env.update(RAYON_NUM_THREADS='1',RUST_MIN_STACK='16777216',
                   CARGO_PROFILE_DEV_OPT_LEVEL='0',CARGO_PROFILE_TEST_OPT_LEVEL='0',
                   CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0');return env
    save()
    try:
        request=json.loads((result/'request.json').read_text(encoding='utf-8'))
        require(request['candidate_feature']==MODE and request['baseline_sha']==request['candidate_sha']==SOURCE_SHA,'P13 fixed same-source common5 request required')
        receipt['declarations']=verify_declarations(workspace/'candidate');receipt['full_regressions']=validate_full_regressions(result)
        labels=['p13-'+storage+'-'+arm+'-'+kind for storage in ('dense','compact') for arm in ('baseline','candidate') for kind in ('plain','observer')]
        require(all(not os.path.lexists(workspace/('target-'+label)) for label in labels),'P13 proof targets must be fresh')
        public=[];private=[]
        for storage,arm,kind in itertools.product(('dense','compact'),('baseline','candidate'),('plain','observer')):
            compact=storage=='compact';observer=kind=='observer';on=arm=='candidate';label='p13-'+storage+'-'+arm+'-'+kind
            env=environment(label);flags,expected=variant_features(arm,compact,observer);item={'features':flags};receipt['variants'][label]=item
            if observer:
                env['LAB_P13_UNIT_RECORDS']=str(out/(label+'-private.jsonl'));stem=out/(label+'-lib')
                execute(['cargo','test','--locked','-p','lab-search','--lib','--features',flags,'--','--show-output','--test-threads=1'],env,stem)
                item['named_tests']=validate_tests(text(stem),COMMON_TESTS+(ON_TESTS if on else ()),lib=True,on=on,compact=compact)
                item['private']=validate_records(Path(env['LAB_P13_UNIT_RECORDS']),private=True);private.append(Path(env['LAB_P13_UNIT_RECORDS']))
            env['LAB_P13_RECORDS']=str(out/(label+'.jsonl'));stem=out/(label+'-public')
            execute(['cargo','test','--locked','-p','lab-search','--test','borrowed_child_keys','--features',flags,'--','--show-output','--test-threads=1'],env,stem)
            validate_tests(text(stem),(PUBLIC_TEST,));item['public']=validate_records(Path(env['LAB_P13_RECORDS']));public.append(Path(env['LAB_P13_RECORDS']))
            packages={'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json','test-integration-test-borrowed_child_keys.json')+ (('test-lib-lab_search.json',) if observer else ())}
            item['compiler_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,packages,expected,True,profile='debug')
            if compact and observer:
                require(ci.sha(workspace/'candidate'/PROBE_PATH)==ci.sha(Path(__file__).with_name('borrowed_child_keys_probe.rs')),'P13 observer wrapper injection differs')
                execute(['cargo','build','--locked','--release','-p','lab-search','--example',PROBE,'--features',flags],env,out/(label+'-probe-build'))
                item['probe_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,{'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json','example-'+PROBE+'.json')},expected,True)
                binary=workspace/('target-'+label)/'release/examples'/PROBE
                require(binary.is_file() and not binary.is_symlink(),'P13 observer binary missing')
                item['probe_binary_sha256']=ci.sha(binary)
                cases,_=bench.load_cases('narrow',root=workspace/'controller')
                require([c['name'] for c in cases]==['coaching','sand'],'P13 fixed workload selection changed')
                receipt['activation'][arm]={}
                for case in cases:
                    stem=out/(label+'-'+case['name']);execute([str(binary),str(workspace/'controller'/case['scenario']),'1',case['position']],env,stem)
                    output=stem.with_suffix('.stdout');bench.validate_output(bench.strict_json(output.read_text(encoding='utf-8')))
                    counts=validate_counts(stem.with_suffix('.stderr').read_text(encoding='utf-8'),on)
                    receipt['activation'][arm][case['name']]={'counts':counts,'output_sha256':ci.sha(output),'output_bytes':output.stat().st_size,'output_file':output.name}
            save()
        require(all(p.read_bytes()==public[0].read_bytes() for p in public),'P13 public OFF/ON/observer/storage records differ')
        require(all(p.read_bytes()==private[0].read_bytes() for p in private),'P13 private policy records differ')
        for case in ('coaching','sand'):
            before=receipt['activation']['baseline'][case];after=receipt['activation']['candidate'][case]
            require((out/before['output_file']).read_bytes()==(out/after['output_file']).read_bytes(),'P13 real search output/work differs')
            compare_counts(before['counts'],after['counts'])
        receipt.update(status='success',source_sha=SOURCE_SHA,complete_jsonl_byte_equal=True,private_jsonl_byte_equal=True,actual_public_search_activated=True,capture_subcases_per_runtime=36)
    except Exception as error:
        receipt.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:save()
    return receipt
