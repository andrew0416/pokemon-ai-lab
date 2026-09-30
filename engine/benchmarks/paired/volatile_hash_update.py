"""P12-only fresh correctness proof; timing targets never receive observers."""
import json
import os
from pathlib import Path
import re
import sys
import tomllib

import ci
import compact_probe as process
import p12_logic as logic

HERE=Path(__file__).resolve().parent
PINS=json.loads((HERE/'p12-source.json').read_text(encoding='utf8'))
SOURCE=PINS['sha']
PROBE_SOURCE='engine/scenario/examples/ci_p12_observer.rs'
PROBE_CASES=['ff-encore-protect-stall','lum-persim-confusion','stockpile','double-hit',
             'uturn-pause','eject-button-uturn']
OBSERVER_TEST='observer_proves_one_location_lookup_and_expected_rank_counts'
COMPACT_UNIT='volatile::compact::tests::replace_keeps_set_registry_capacity_and_spill_representation'
TURN_CONFIGS={('single-hit','Full',False),('single-hit','Full',True),
    *((name,'Median',False) for name in PROBE_CASES)}

def require(value,message):
    if not value: raise ValueError(message)

def verify_declarations(root):
    require(len(PINS['file_sha256'])==9,'P12 source pin count differs')
    for name,digest in PINS['file_sha256'].items():
        require(ci.sha(root/name)==digest,'P12 frozen source changed: '+name)
    for package in ('core','scenario','search'):
        path=root/'engine'/package/'Cargo.toml'
        features=ci.read_features(path)
        ci.reject_default_experiments(path,features)
        if package=='core':
            require(features.get(logic.RUNTIME)==[] and features.get(logic.OBSERVER)==[logic.RUNTIME],
                    'P12 runtime/observer declaration differs')
        elif package=='scenario':
            require(features.get(logic.OBSERVER)==['lab-engine/'+logic.OBSERVER],
                    'P12 scenario observer forwarding differs')
        else:
            require(not {logic.RUNTIME,logic.OBSERVER}.intersection(features) and not any(
                v.rsplit('/',1)[-1] in {logic.RUNTIME,logic.OBSERVER}
                for values in features.values() for v in values),'P12 search forwarding is forbidden')
        if package in ('core','scenario'):
            targets=tomllib.loads(path.read_text())['test']
            rows=[r for r in targets if r.get('name')=='volatile_hash_update']
            require(rows==[{'name':'volatile_hash_update','path':'tests/volatile_hash_update.rs'}],
                    'P12 test target must be registered exactly once without required features')
    return {'source_sha':SOURCE,'source_files':PINS['file_sha256'],'default_off':True,'observer_separate':True}

def named_tests(text,names,exact=True):
    passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
    require(bool(passed),'P12 zero tests cannot pass')
    require(all(passed.count(n)==1 for n in names),'P12 named test missing or repeated')
    if exact: require(sorted(passed)==sorted(names),'P12 unexpected/omitted named test')
    rows=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',text,re.M)
    require(len(rows)==1 and tuple(map(int,rows[0]))==(len(passed),0,0,0,0),
            'P12 test summary missing, failed, ignored or filtered')
    return len(passed)

def validate_full_regressions(result):
    proof={}
    for arm in ('baseline','candidate'):
        receipt=json.loads((result/(arm+'-build-receipt.json')).read_text())
        require(receipt.get('status')=='success' and receipt.get('reused') is False
                and receipt.get('selection')==ci.VOLATILE_UPDATE_MODE,'P12 requires fresh full regressions')
        text=(result/(arm+'-build.log')).read_text(encoding='utf8')
        passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
        names=list(logic.CORE_TESTS)+list(logic.SCENARIO_TESTS)
        if arm=='candidate': names+=logic.UNIT_TESTS+[COMPACT_UNIT]
        require(all(passed.count(n)==1 for n in names),'P12 full regression omitted required tests')
        require(OBSERVER_TEST not in passed,'P12 observer leaked into full runtime-only regressions')
        proof[arm]={'named_tests':names,'log_sha256':ci.sha(result/(arm+'-build.log')),
                    'receipt_sha256':ci.sha(result/(arm+'-build-receipt.json'))}
    return proof

def validate_records(path,kind):
    raw=path.read_bytes()
    require(raw and raw.endswith(b'\n') and b'\r' not in raw,'P12 records must be complete LF JSONL')
    rows=[process._json(line) for line in raw.splitlines()]
    require(all(isinstance(r,dict) for r in rows),'P12 records must be objects')
    cover=rows[-1]
    require(cover.get('kind')=='coverage' and not any('kind' in r for r in rows[:-1]),'P12 completion differs')
    require(all(type(v) is int and v>=0 for k,v in cover.items() if k!='kind'),'P12 coverage counts must be integers')
    def outcomes(values):
        require(isinstance(values,list) and bool(values),'P12 outcome list empty')
        for value in values:
            require(set(value)=={'probability_bits','instructions','suspension','state','hash'},'P12 full outcome fields missing')
            require(all(type(value[k]) is int and 0<=value[k]<2**64 for k in ('probability_bits','hash')),
                    'P12 exact probability/hash bits malformed')
            require(all(isinstance(value[k],str) and bool(value[k]) for k in ('instructions','suspension','state')),
                    'P12 full Debug evidence missing')
    if kind=='turns.jsonl':
        require(len(rows)==11 and set(cover)=={'kind','positions','outcomes'},'P12 turn record count differs')
        require({(r.get('case'),r.get('rolls'),r.get('factored')) for r in rows[:-1]}==TURN_CONFIGS,
                'P12 fixture/factored/roll coverage differs')
        keys=[]
        for r in rows[:-1]:
            require(set(r)=={'case','position','rolls','factored','before','outcomes'} and
                    isinstance(r['before'],str) and bool(r['before']),'P12 before/full outcome record missing')
            require(type(r['position']) is int and r['position']>=0 and type(r['factored']) is bool,
                    'P12 position/factored type differs')
            keys.append((r['case'],r['position'],r['rolls'],r['factored']))
            outcomes(r['outcomes'])
        require(len(keys)==len(set(keys)) and cover=={'kind':'coverage','positions':10,'outcomes':1096}
                and sum(len(r['outcomes']) for r in rows[:-1])==cover['outcomes'],'P12 turn coverage totals differ')
    else:
        require(kind=='errors-samples.jsonl' and len(rows)==9,'P12 error/sample record count differs')
        require(cover=={'kind':'coverage','errors':1,'successes':1,'samples':6},'P12 error/sample totals differ')
        errors=successes=0; samples=[]; positions=[]
        for r in rows[:-1]:
            require(type(r.get('position')) is int and r['position']>=0,'P12 error/sample position type differs')
            if r.get('case')=='sample':
                require(set(r)=={'case','position','seed','outcomes'},'P12 sample fields differ')
                require(type(r['seed']) is int,'P12 sample seed must be integer')
                samples.append((r['position'],r['seed']));outcomes(r['outcomes'])
            else:
                require(set(r)=={'case','position','before','result'} and r['case']=='aa-power-construct-faint'
                    and isinstance(r['before'],str) and bool(r['before']),'P12 error fixture fields differ')
                positions.append(r['position'])
                if set(r['result'])=={'error'}:
                    errors+=1;require(isinstance(r['result']['error'],str) and bool(r['result']['error']),'P12 error absent')
                else:
                    require(set(r['result'])=={'outcomes'},'P12 result fields differ')
                    successes+=1;outcomes(r['result']['outcomes'])
        require(sorted(positions)==[0,1] and errors==successes==1 and
                sorted(samples)==[(i,s) for i in (0,1) for s in (0,7,42)],'P12 real error or fixed seeds omitted')
    return {'records':len(rows),'bytes':len(raw),'sha256':ci.sha(path),'coverage':cover}

def validate_logic(root,target):
    r=json.loads((root/'receipt.json').read_text())
    require(r.get('passed') is True and r.get('ci_only') is True and r.get('full_ci_matrix') is True
            and r.get('performance_measured') is False,'P12 nine-arm proof incomplete')
    require(r.get('selected_arms')==list(logic.ARMS) and r.get('source_files')==PINS['file_sha256'],
            'P12 source or nine-arm matrix differs')
    expected_commands=[];expected_argv={};artifacts=set();counts={};records={}
    for arm,features in logic.ARMS.items():
        expected_commands += [arm+'-units',arm+'-lab-engine-build',arm+'-lab-engine-tests',
                              arm+'-lab-scenario-build',arm+'-lab-scenario-tests']
        flags=['--features',','.join(features)] if features else []
        expected_argv[arm+'-units']=['cargo','test','--locked','--offline','-j1','-p','lab-engine','--lib',*flags,'--','--test-threads=1']
        unit_names=list(logic.UNIT_TESTS) if logic.RUNTIME in features else []
        if logic.RUNTIME in features and logic.COMPACT in features: unit_names.append(COMPACT_UNIT)
        unit_text=(root/(arm+'-units.log')).read_text(encoding='utf8')
        counts[arm+'/unit']=named_tests(unit_text,unit_names,exact=False)
        if logic.RUNTIME not in features:
            require(not any('test '+name+' ... ok' in unit_text for name in logic.UNIT_TESTS),
                    'P12 ON unit test compiled in OFF arm')
        for package in ('lab-engine','lab-scenario'):
            key=arm+'/'+package;artifacts.add(key)
            names=list(logic.CORE_TESTS if package=='lab-engine' else logic.SCENARIO_TESTS)
            if package=='lab-engine' and logic.OBSERVER in features: names.append(OBSERVER_TEST)
            counts[key]=named_tests((root/(arm+'-'+package+'-tests.log')).read_text(encoding='utf8'),names)
            require(r['compiled_features'].get(key)==sorted(features),'P12 actual core feature receipt differs')
            built=[]
            for line in (root/(arm+'-'+package+'-build.log')).read_text(encoding='utf8').splitlines():
                if line.startswith('{'):
                    event=process._json(line)
                    if event.get('reason')=='compiler-artifact': built.append(event)
            core=[a for a in built if a['target']['name']=='lab_engine']
            require(core and all(a['features']==sorted(features) for a in core),'P12 Cargo core artifact features differ')
            tests=[a for a in built if a['target']['name']=='volatile_hash_update' and a.get('executable')]
            require(len(tests)==1,'P12 executable artifact missing/repeated')
            package_features=sorted(features) if package=='lab-engine' else ([logic.OBSERVER] if logic.OBSERVER in features else [])
            require(tests[0]['features']==package_features,'P12 test executable features differ')
            if package=='lab-scenario':
                libs=[a for a in built if a['target']['name']=='lab_scenario']
                require(libs and all(a['features']==package_features for a in libs),'P12 scenario library features differ')
            a=r['artifacts'][key];path=Path(a['executable'])
            require(path.resolve().is_relative_to(target.resolve()) and not path.is_symlink() and
                a['executable']==tests[0]['executable'] and a['sha256']==ci.sha(path) and
                a['core_features']==sorted(features),'P12 executed binary/source feature evidence differs')
            flags=[('lab-engine/'+f if package=='lab-scenario' else f) for f in features]
            if package=='lab-scenario' and logic.OBSERVER in features: flags.append('lab-scenario/'+logic.OBSERVER)
            flags=['--features',','.join(flags)] if flags else []
            expected_argv[arm+'-'+package+'-build']=['cargo','test','--locked','--offline','-j1','-p',package,
                '--test','volatile_hash_update','--no-run','--message-format=json',*flags]
            expected_argv[arm+'-'+package+'-tests']=[str(path),'--nocapture','--test-threads=1']
        records[arm]={name:validate_records(root/(arm+'-records')/name,name)
                      for name in ('turns.jsonl','errors-samples.jsonl')}
    require(set(r['artifacts'])==artifacts and set(r['compiled_features'])==artifacts,'P12 artifact denominator differs')
    require([c['name'] for c in r['commands']]==expected_commands,'P12 command coverage/order differs')
    for c in r['commands']:
        require(c['exit_code']==0 and c['log']==c['name']+'.log' and c['sha256']==ci.sha(root/c['log']),
                'P12 command failed or log changed')
        require(c['command']==expected_argv[c['name']],'P12 Cargo/test command differs or filters coverage')
    comparisons=[]
    for group in ('dense','compact','common5'):
        arms=[group+'-'+arm for arm in ('off','on','observer')]
        for name in ('turns.jsonl','errors-samples.jsonl'):
            paths=[root/(arm+'-records')/name for arm in arms]
            raw=[p.read_bytes() for p in paths]
            require(raw[0]==raw[1]==raw[2],'P12 OFF/ON/observer exact output differs')
            comparisons.append({'group':group,'file':name,'arms':arms,'bytes':len(raw[0]),
                                'records':len(raw[0].splitlines()),'sha256':ci.sha(paths[0])})
    require(r['comparisons']==comparisons,'P12 exact comparison receipt differs')
    return {'receipt_sha256':ci.sha(root/'receipt.json'),'counts':counts,'records':records,'comparisons':comparisons}

def validate_activation(path):
    raw=path.read_bytes();require(raw.endswith(b'\n') and len(raw.splitlines())==1,'P12 observer must emit one complete record')
    r=process._json(raw)
    fields={'schema','kind','cases','positions','outcomes','volatile_updates','original_locations','candidate_locations',
            'original_ranks','candidate_ranks','enumeration_locations','enumeration_ranks','full_state_and_hash_equal','rollback_equal'}
    require(set(r)==fields and r['schema']==1 and r['kind']=='p12-real-instruction-proof' and r['cases']==PROBE_CASES,
            'P12 real-instruction proof fields/fixture coverage differ')
    for name in fields-{'schema','kind','cases','full_state_and_hash_equal','rollback_equal'}:
        require(type(r[name]) is int and r[name]>=0,'P12 counter type/range differs')
    require(r['positions']>=6 and r['outcomes']>0 and r['volatile_updates']>0 and r['enumeration_locations']>0
            and r['enumeration_ranks']>0,'P12 real scenario observer did not activate')
    require(r['original_locations']==3*r['volatile_updates'] and r['candidate_locations']==r['volatile_updates']
            and r['candidate_ranks']<=r['volatile_updates'] and r['original_ranks']>r['candidate_ranks'],
            'P12 actual instruction replay did not reduce lookup/rank work')
    require(r['full_state_and_hash_equal'] is True and r['rollback_equal'] is True,'P12 full state/hash/rollback differs')
    return {'sha256':ci.sha(path),'record':r,'scope':'actual enumerated SetVolatile instruction replay; enumeration counters separate'}

def validate(workspace):
    workspace=Path(workspace).resolve();result=workspace/'ci-results'
    out=result/'volatile-hash-update-validation';out.mkdir(exist_ok=False)
    r={'status':'running','selection':ci.VOLATILE_UPDATE_MODE,'commands':[],
       'cached_results_reused':False,'performance_measurement':False,'source_sha':SOURCE}
    def save(): (out/'receipt.json').write_text(json.dumps(r,indent=2)+'\n',encoding='utf8')
    def execute(argv,env,stem):
        process._run(argv,workspace/'candidate/engine',env,stem,ci.COMMAND_TIMEOUT_SECONDS,r['commands'],save)
    save()
    try:
        request=json.loads((result/'request.json').read_text())
        require(request['candidate_feature']==ci.VOLATILE_UPDATE_MODE and request['baseline_sha']==request['candidate_sha']==SOURCE
                and request['suite']=='narrow' and request['threads']==1 and request['pairs']==10,
                'P12 requires pinned same SHA, narrow, 1t, ten pairs')
        r['declarations']=verify_declarations(workspace/'candidate')
        r['full_regressions']=validate_full_regressions(result)
        target=workspace/'target-p12-logic'
        probe_target=workspace/'target-p12-real-observer'
        require(not any(os.path.lexists(p) for p in (target,probe_target)),'P12 proof targets must be fresh')
        env=process.environment(target)
        env.pop('LAB_ENGINE_STATS',None);env.pop('LAB_P12_RECORDS',None)
        execute([sys.executable,str(HERE/'p12_logic.py'),'--engine',str(workspace/'candidate/engine'),
                 '--output',str(out/'logic'),'--target',str(target)],env,out/'logic-runner')
        r['logic']=validate_logic(out/'logic',target);save()
        label='p12-real-observer';env=process.environment(probe_target)
        env.pop('LAB_ENGINE_STATS',None);env.pop('LAB_P12_RECORDS',None)
        flags=','.join('lab-engine/'+f for f in logic.COMMON+[logic.RUNTIME])+',lab-scenario/'+logic.OBSERVER
        source=workspace/'candidate'/PROBE_SOURCE
        require(ci.sha(source)==ci.sha(HERE/'p12_observer_probe.rs'),'P12 injected observer probe changed')
        r['observer_probe_sha256']=ci.sha(source)
        execute(['cargo','build','--locked','--release','-p','lab-scenario','--example','ci_p12_observer','--features',flags],env,out/'observer-build')
        core={f:f in logic.COMMON+[logic.RUNTIME,logic.OBSERVER] for f in ci.ALL_EXPERIMENT_FEATURES}
        scenario={f:f==logic.OBSERVER for f in ci.ALL_EXPERIMENT_FEATURES}
        r['observer_features']=ci.preserve_expected_fingerprints(workspace,label,
            {'lab-engine':('lib-lab_engine.json',),'lab-scenario':('lib-lab_scenario.json','example-ci_p12_observer.json')},
            {'lab-engine':core,'lab-scenario':scenario},True)
        binary=probe_target/'release/examples/ci_p12_observer'
        r['observer_binary_sha256']=ci.sha(binary)
        execute([str(binary),str(workspace/'candidate/engine')],env,out/'actual-scenario-instructions')
        r['activation']=validate_activation(out/'actual-scenario-instructions.stdout')
        r.update(status='success',complete_jsonl_byte_equal=True)
    except Exception as error:
        r.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:save()
    return r
