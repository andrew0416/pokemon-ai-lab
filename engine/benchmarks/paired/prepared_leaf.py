"""Fresh R1/P15 exact and activation gate. Observer work is never timed."""
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

SOURCE_SHA='71d5a84a97f6a3ce05430e1108538aed43fdc7f1'
SOURCE_FILES={'engine/core/Cargo.toml': 'e2934dcff52840f996fff05659d2297755d3d2d3ba822e0970e055690526e247', 'engine/core/src/turn/final_states.rs': '17feb1e5f6f4f354aab7a57f38ea75e46a2cd2ebf605eb7023dfedc0aa5d74d5', 'engine/core/src/turn/mod.rs': 'cbc710ba09fc38ef43f2b4ba92aaa2253b03ff3d4a1742ca10d80a72dc5c96d7', 'engine/core/src/turn/prepared.rs': 'e0a54c7f78632fb8a2b7d40e8d00afe70fa39f86431ffb1e807a0b93ed690595', 'engine/search/Cargo.toml': 'be9d1573c3d7368d314fe73ffb6aa8bc1fabe36e3b2d719002c5eb2f1f5ed25f', 'engine/search/src/prepared.rs': '87e03e2252dec642d2fb8e208a956e57c8b4ba6eb4e357cf4dead392fc5a5cd8', 'engine/search/src/solve.rs': '8d00c0c9e2d94164b0a7e8a434ec11aa02bdde33753248028fb314b76b219303', 'engine/search/src/solve/leaf_endings.rs': '907025e2415914c53022d3d859abea69f5df8b460cecb32e96a90c78dcb17508', 'engine/search/src/solve/prepared_leaf_tests.rs': '425c89906af80dfea615161aef4f160ef55e1b3f89d2eac877dffef1143336de', 'engine/search/tests/prepared_leaf.rs': '883896627d6d38099370a1fca6dd94b81ce5505cf31ab78437574bed888f7d80', 'engine/search/tests/prepared_turn.rs': '610a992eaeee05e933c51319b758492037f0795ad3f043bf2e30b91b19614ef2'}
MODE='prepared-leaf'
PROBE='ci_prepared_leaf_observer'
PROBE_PATH='engine/search/examples/'+PROBE+'.rs'
PUBLIC_TEST='exact_feature_and_config_toggles_preserve_public_search_records'
ON_TESTS=('owned_batch_shares_validation_and_preserves_complete_endings',
          'cached_error_priority_rules_normalization_and_declined_paths_match')
OBS_TEST='real_serial_validation_reductions_preserve_leaf_work_and_outputs'
PRIVATE_TESTS=tuple('solve::prepared_leaf_tests::'+name for name in (
    'asymmetric_side_mapping_nan_cutoff_and_evaluator_order_match',
    'serial_budget_precedes_validation_and_nonleaf_declines',
    'execution_error_returns_no_evaluator_visit_and_restores_snapshot'))
CORE_TESTS=tuple('turn::final_states::tests::'+name for name in (
    'p9_late_enumeration_error_exposes_no_partial_batch',
    'p9_visit_starts_after_final_probability_merge'))
LEAF_FIELDS={'batches','visits','materialized_outcomes','emitted_instructions'}
SHARING_FIELDS={'requests','declined','batches','errors'}
BORROWED_FIELDS={'key_captures','job_captures','borrowed_queries','seen_hits','seen_collisions','seen_links'}
PROOF_VARIANTS=tuple((storage,arm,kind) for storage,kind in
                    (('dense','observer'),('compact','observer'),('compact','plain'))
                    for arm in ('baseline','candidate'))


def require(value,message):
    if not value:raise ValueError(message)


def verify_declarations(root):
    require(re.fullmatch('[0-9a-f]{40}',SOURCE_SHA) and SOURCE_FILES,'P15 source is not frozen and bound')
    paths={name:root/f'engine/{name}/Cargo.toml' for name in ('core','search','scenario')}
    features={name:ci.read_features(path) for name,path in paths.items()}
    runtime,observer=ci.PL_FEATURE,ci.PL_OBSERVER
    require(features['core'].get(runtime)==[ci.PREPARED_FEATURE,ci.LEAF_FEATURE]
            and features['core'].get(observer)==[ci.PREPARED_OBSERVER_FEATURE,ci.OBSERVER_FEATURE],
            'P15 core runtime/independent observer dependencies differ')
    require(features['search'].get(runtime)==[ci.PREPARED_FEATURE,ci.LEAF_FEATURE,'lab-engine/'+runtime]
            and features['search'].get(observer)==[ci.PREPARED_OBSERVER_FEATURE,ci.OBSERVER_FEATURE,'lab-engine/'+observer],
            'P15 search forwarding/independent observer dependencies differ')
    require(runtime not in features['scenario'] and observer not in features['scenario'],'Unexpected P15 scenario forwarding')
    require(features['core'].get(ci.BORROWED_FEATURE)==[] and ci.BORROWED_OBSERVER not in features['core']
            and features['search'].get(ci.BORROWED_FEATURE)==['lab-engine/'+ci.BORROWED_FEATURE]
            and features['search'].get(ci.BORROWED_OBSERVER)==[], 'R1 P13 runtime/independent observer forwarding changed')
    for name,path in paths.items():ci.reject_default_experiments(path,features[name])
    rows=[r for r in tomllib.loads(paths['search'].read_text(encoding='utf-8')).get('test',[]) if r.get('name')=='prepared_leaf']
    require(len(rows)==1 and rows[0].get('path')=='tests/prepared_leaf.rs' and not rows[0].get('required-features'),
            'P15 integration test registration differs')
    for path,digest in SOURCE_FILES.items():require(ci.sha(root/path)==digest,'P15 immutable source changed: '+path)
    return {'default_off':True,'independent_observer':True,'R1_P13_forwarding_preserved':True,'source_file_sha256':SOURCE_FILES}


def nonnegative_counts(value,fields,message):
    require(isinstance(value,dict) and set(value)==fields
            and all(type(n) is int and 0<=n<2**64 for n in value.values()),message)


def validate_counts(text,on):
    lines=re.findall(r'^P15 activation: (.+)$',text,re.M)
    require(len(lines)==1,'P15 public search observer evidence missing or repeated')
    value=process._json(lines[0])
    require(isinstance(value,dict) and set(value)=={'prepared_leaf','validation','leaf','borrowed'},'P15 observer sections differ')
    sharing,leaf,borrowed=value['prepared_leaf'],value['leaf'],value['borrowed']
    nonnegative_counts(sharing,SHARING_FIELDS,'P15 sharing fields malformed')
    nonnegative_counts(leaf,LEAF_FIELDS,'P15 leaf fields malformed')
    nonnegative_counts(borrowed,BORROWED_FIELDS,'P15 R1 borrowed-key fields malformed')
    require(sharing['requests']==sharing['declined']+sharing['batches']+sharing['errors'],'P15 sharing accounting differs')
    if on:require(sharing['batches']>0,'P15 workload never shared a leaf validation')
    else:require(all(n==0 for n in sharing.values()),'P15 OFF invoked the gated API')
    require(leaf['batches']>0 and leaf['visits']>0,'P15 workload did not execute P9')
    require(borrowed['key_captures']==0 and borrowed['borrowed_queries']>0 and borrowed['job_captures']>0
            and borrowed['seen_links']>0,'P15 lost R1 borrowed key activation')
    require(isinstance(value['validation'],list) and len(value['validation'])==3
            and all(type(n) is int and 0<n<2**64 for n in value['validation']),'P15 three validator counts missing')
    return value


def compare_counts(before,after):
    require(before['leaf']==after['leaf'],'P15 changed P9 enumeration work')
    require(before['borrowed']==after['borrowed'],'P15 changed R1 P13 lookup/job work')
    require(all(a<b for a,b in zip(after['validation'],before['validation'])),
            'P15 actual workload did not reduce all three validator counts')


def variant_features(arm,compact,observer):
    flags=ci.feature_args(MODE,arm)[1].split(',')
    _,expected,_=ci.fingerprint_expectations(MODE,arm)
    if not compact:
        flags.remove('lab-engine/'+ci.COMPACT_FEATURE);expected['lab-engine'][ci.COMPACT_FEATURE]=False
    if observer:
        flags += ['lab-search/'+ci.PL_OBSERVER,'lab-search/'+ci.BORROWED_OBSERVER]
        for package in expected:
            for name in (ci.PL_OBSERVER,ci.PREPARED_OBSERVER_FEATURE,ci.OBSERVER_FEATURE):expected[package][name]=True
        expected['lab-search'][ci.BORROWED_OBSERVER]=True
    return ','.join(flags),expected


def validate_tests(text,expected,*,filtered=False):
    passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
    require(sorted(passed)==sorted(expected) and len(passed)==len(set(passed)), 'P15 named tests skipped, duplicated or unexpected')
    summaries=[tuple(map(int,row)) for row in re.findall(
        r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',text,re.M)]
    require(len(summaries)==1 and summaries[0][:4]==(len(expected),0,0,0)
            and (filtered or summaries[0][4]==0), 'P15 test execution incomplete')
    if PUBLIC_TEST in expected:
        require(re.findall(r'^P15 PUBLIC CASES (\d+)$',text,re.M)==['54'], 'P15 complete public cases marker missing')
    return {'passed':len(passed),'required_named_tests':list(expected),'summaries':summaries}


def validate_records(path,*,private=False):
    require(not private,'P15 private tests assert exact trace equality internally; there is no private JSONL')
    raw=path.read_bytes()
    require(raw.endswith(b'\n') and b'\r' not in raw,'P15 complete LF JSONL required')
    rows=[process._json(line) for line in raw.splitlines()]
    expected=[]
    for slots in (1,2):
        for side,chance,mode,depth in itertools.product(('One','Two'),('Expect','Worst'),('exact','mixed'),(1,2)):
            expected.append((f'toy-{slots}-{side}-{chance}-{mode}-{depth}',slots,mode))
        for fallback in ('full','factored','parallel','extremes','pessimistic'):
            expected.append((f'fallback-{slots}-{fallback}',slots,'mixed'))
        for budget in (0,1):expected.append((f'budget-{slots}-{budget}',slots,'mixed'))
        for kind in ('terminal','unsupported'):expected.append((f'{kind}-{slots}',slots,'mixed'))
        expected.append((f'deep-{slots}',slots,'deep'))
    expected += [('successful-resume',2,'nash'),('successful-replacement',1,'nash')]
    require(len(rows)==len(expected)==54,'P15 public record count differs')
    for row,identity in zip(rows,expected):
        require(isinstance(row,dict) and set(row)=={'id','slots','mode','signature'}
                and (row['id'],row['slots'],row['mode'])==identity and type(row['slots']) is int,
                'P15 exact case identity, order or fields differ')
        record=row['signature']
        require(isinstance(record,dict) and set(record)=={'before','after','suspension','result','stats'},'P15 signature fields missing')
        for name in ('before','after'):
            state=record[name]
            require(isinstance(state,dict) and set(state)=={'debug','hash','position_hash'}
                    and isinstance(state['debug'],str) and bool(state['debug'])
                    and all(type(state[k]) is int and 0<=state[k]<2**64 for k in ('hash','position_hash')),
                    'P15 full state representation/hash missing')
        require(record['before']==record['after'],'P15 search failed to restore state')
        require(isinstance(record['suspension'],str) and isinstance(record['stats'],list)
                and len(record['stats'])==7 and all(type(n) is int and n>=0 for n in record['stats']),
                'P15 suspension or exact integer statistics missing')
        result=record['result']
        require(isinstance(result,dict) and set(result) in ({'ok'},{'error'}),'P15 output/error must be explicit')
        if 'error' in result:require(isinstance(result['error'],str) and bool(result['error']),'P15 error ordering evidence missing')
        else:
            require(isinstance(result['ok'],dict),'P15 successful analysis malformed')
            keys={'exact':{'debug','value','lines'},'mixed':{'debug','matrix','equilibrium','maximin'},
                  'deep':{'debug','matrix','equilibrium','shallow'},'nash':{'value'}}[row['mode']]
            require(set(result['ok'])==keys,'P15 complete analysis bits/strategies missing')
            if 'debug' in keys:require(isinstance(result['ok']['debug'],str) and bool(result['ok']['debug']),'P15 ordered analysis Debug missing')
            bits=lambda n:type(n) is int and 0<=n<2**32
            for name in keys & {'value','maximin'}:require(bits(result['ok'][name]),'P15 value bits malformed')
            for name in keys & {'lines','matrix'}:
                require(isinstance(result['ok'][name],list) and all(bits(n) for n in result['ok'][name]),'P15 ordered matrix/line bits malformed')
            for name in keys & {'equilibrium','shallow'}:
                eq=result['ok'][name]
                require(isinstance(eq,dict) and set(eq)=={'rows','cols','value','exploitability','iterations'}
                        and all(bits(eq[k]) for k in ('value','exploitability'))
                        and type(eq['iterations']) is int and eq['iterations']>=0
                        and all(isinstance(eq[k],list) and all(bits(n) for n in eq[k]) for k in ('rows','cols')),
                        'P15 complete equilibrium bits malformed')
        if row['id'].startswith('successful-'):require('ok' in result,'P15 successful resume/replacement stopped succeeding')
    return {'records':len(rows),'sha256':ci.sha(path),'bytes':len(raw)}


def validate_bounded_activation(text,on,observer):
    lines=re.findall(r'^P15_ACTIVATION (.+)$',text,re.M)
    require(len(lines)==int(observer),'P15 bounded observer missing or present in plain build')
    if not observer:return None
    value=process._json(lines[0])
    require(isinstance(value,dict) and set(value)=={'ordinary','prepared','p15','leaf'},'P15 bounded observer fields differ')
    for name in ('ordinary','prepared'):
        require(isinstance(value[name],list) and len(value[name])==3
                and all(type(n) is int and n>0 for n in value[name]),'P15 bounded validators missing')
    nonnegative_counts(value['p15'],SHARING_FIELDS,'P15 bounded sharing counts malformed')
    nonnegative_counts(value['leaf'],LEAF_FIELDS,'P15 bounded P9 counts malformed')
    c=value['p15']
    require(c['requests']==c['declined']+c['batches']+c['errors'],'P15 bounded sharing accounting differs')
    require(value['leaf']['batches']>0 and value['leaf']['visits']>0,'P15 bounded P9 work absent')
    if on:
        require(all(a<b for a,b in zip(value['prepared'],value['ordinary']))
                and c['requests']==c['batches']>0 and c['errors']==c['declined']==0,'P15 bounded shared path not active')
    else:require(value['ordinary']==value['prepared'] and all(n==0 for n in c.values()),'P15 OFF bounded path differs')
    return value


def validate_full_regressions(result):
    evidence={}
    for arm in ('baseline','candidate'):
        path=result/(arm+'-build-receipt.json');row=json.loads(path.read_text(encoding='utf-8'))
        require(row.get('status')=='success' and row.get('reused') is False and row.get('selection')==MODE,'P15 requires fresh full release regressions')
        text=(result/(arm+'-build.log')).read_text(encoding='utf-8')
        passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
        require(passed.count(PUBLIC_TEST)==1,'P15 exact public test missing from full regression')
        for name in ON_TESTS+PRIVATE_TESTS:require(passed.count(name)==int(arm=='candidate'),'P15 conditional proof missing, duplicated or compiled into OFF')
        require(passed.count(OBS_TEST)==0,'P15 full release regressions unexpectedly used observers')
        evidence[arm]={'receipt_sha256':ci.sha(path),'log_sha256':ci.sha(result/(arm+'-build.log'))}
    return evidence


def validate(workspace):
    workspace=Path(workspace).resolve();result=workspace/'ci-results';out=result/'prepared-leaf-validation';out.mkdir(exist_ok=False)
    receipt={'status':'running','selection':MODE,'cached_results_reused':False,'performance_measurement':False,
             'common_runtime':'R1=old4+P8d+P13; E/F/P11/P12/P14 OFF; dense only disables P10',
             'bounded_profile':'debug opt0','public_activation_profile':'release opt3','commands':[],'variants':{},'activation':{}}
    def save():(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    def text(stem):return stem.with_suffix('.stdout').read_text(encoding='utf-8')+'\n'+stem.with_suffix('.stderr').read_text(encoding='utf-8')
    def execute(argv,env,stem):return process._run(argv,workspace/'candidate/engine',env,stem,ci.COMMAND_TIMEOUT_SECONDS,receipt['commands'],save)
    def environment(label):
        env=process.environment(workspace/('target-'+label))
        for name in ('LAB_P15_RECORDS','LAB_ENGINE_STATS'):env.pop(name,None)
        env.update(RAYON_NUM_THREADS='1',RUST_MIN_STACK='16777216',CARGO_PROFILE_DEV_OPT_LEVEL='0',
                   CARGO_PROFILE_TEST_OPT_LEVEL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0');return env
    save()
    try:
        request=json.loads((result/'request.json').read_text(encoding='utf-8'))
        require(request['candidate_feature']==MODE and request['baseline_sha']==request['candidate_sha']==SOURCE_SHA,'P15 fixed same-source R1 request required')
        receipt['declarations']=verify_declarations(workspace/'candidate')
        receipt['full_regressions']=validate_full_regressions(result)
        labels=['p15-'+storage+'-'+arm+'-'+kind for storage,arm,kind in PROOF_VARIANTS]
        require(all(not os.path.lexists(workspace/('target-'+label)) for label in labels),'P15 proof targets must be fresh')
        records=[]
        for storage,arm,kind in PROOF_VARIANTS:
            compact,observer,on=storage=='compact',kind=='observer',arm=='candidate'
            label='p15-'+storage+'-'+arm+'-'+kind;env=environment(label);flags,expected=variant_features(arm,compact,observer)
            item={'features':flags};receipt['variants'][label]=item
            if on:
                stem=out/(label+'-private')
                execute(['cargo','test','--locked','-p','lab-search','--lib','--features',flags,
                         'prepared_leaf_tests::','--','--show-output','--test-threads=1'],env,stem)
                item['private_tests']=validate_tests(text(stem),PRIVATE_TESTS,filtered=True)
            env['LAB_P15_RECORDS']=str(out/(label+'.jsonl'));stem=out/(label+'-public')
            execute(['cargo','test','--locked','-p','lab-search','--test','prepared_leaf','--features',flags,
                     '--','--show-output','--test-threads=1'],env,stem)
            names=(PUBLIC_TEST,)+(ON_TESTS if on else ())+((OBS_TEST,) if observer else ())
            item['public_tests']=validate_tests(text(stem),names)
            item['bounded_activation']=validate_bounded_activation(text(stem),on,observer)
            path=Path(env['LAB_P15_RECORDS']);item['public_records']=validate_records(path);records.append(path)
            packages={'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json','test-integration-test-prepared_leaf.json')+(('test-lib-lab_search.json',) if on else ())}
            item['compiler_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,packages,expected,True,profile='debug')
            if compact and observer:
                # Exact legacy no-partial-batch boundaries on both runtime arms.
                core_flags=','.join('lab-engine/'+name for name,active in expected['lab-engine'].items() if active)
                stem=out/(label+'-P9-core')
                execute(['cargo','test','--locked','-p','lab-engine','--lib','--features',core_flags,
                         'final_states::tests::','--','--show-output','--test-threads=1'],env,stem)
                item['P9_core_tests']=validate_tests(text(stem),CORE_TESTS,filtered=True)
                item['P9_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,{'lab-engine':('lib-lab_engine.json','test-lib-lab_engine.json')},expected,True,profile='debug')
                require(ci.sha(workspace/'candidate'/PROBE_PATH)==ci.sha(Path(__file__).with_name('prepared_leaf_probe.rs')),'P15 observer wrapper injection differs')
                execute(['cargo','build','--locked','--release','-p','lab-search','--example',PROBE,'--features',flags],env,out/(label+'-probe-build'))
                item['probe_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,{'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json','example-'+PROBE+'.json')},expected,True)
                binary=workspace/('target-'+label)/'release/examples'/PROBE
                require(binary.is_file() and not binary.is_symlink(),'P15 observer binary missing')
                item['probe_binary_sha256']=ci.sha(binary)
                cases,_=bench.load_cases('narrow',root=workspace/'controller')
                require([case['name'] for case in cases]==['coaching','sand'],'P15 frozen timing workload selection changed')
                receipt['activation'][arm]={}
                for case in cases:
                    stem=out/(label+'-'+case['name']);execute([str(binary),str(workspace/'controller'/case['scenario']),'1',case['position']],env,stem)
                    output=stem.with_suffix('.stdout');bench.validate_output(bench.strict_json(output.read_text(encoding='utf-8')))
                    counts=validate_counts(stem.with_suffix('.stderr').read_text(encoding='utf-8'),on)
                    receipt['activation'][arm][case['name']]={'counts':counts,'output_sha256':ci.sha(output),'output_bytes':output.stat().st_size,'output_file':output.name}
            save()
        require(all(path.read_bytes()==records[0].read_bytes() for path in records),'P15 public records differ across runtime/storage/observer')
        for storage in ('dense','compact'):
            off=receipt['variants']['p15-'+storage+'-baseline-observer']['bounded_activation']
            on=receipt['variants']['p15-'+storage+'-candidate-observer']['bounded_activation']
            require(off['leaf']==on['leaf'] and off['ordinary']==on['ordinary'],'P15 bounded baseline/P9 work changed')
        for case in ('coaching','sand'):
            before,after=receipt['activation']['baseline'][case],receipt['activation']['candidate'][case]
            require((out/before['output_file']).read_bytes()==(out/after['output_file']).read_bytes(),'P15 actual output or integer work differs')
            compare_counts(before['counts'],after['counts'])
        receipt.update(status='success',source_sha=SOURCE_SHA,complete_jsonl_byte_equal=True,
                       actual_prepared_leaf_activation=True,R1_P13_activation_preserved=True)
    except Exception as error:
        receipt.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:save()
    return receipt
