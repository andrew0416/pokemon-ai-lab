"""Fresh P16 checkpoint scratch proof on frozen R1; never reuse accuracy results."""
import json
import os
from pathlib import Path
import re
import tomllib
import ci
import compact_probe as process
import run as bench
import borrowed_child_keys as borrowed

SOURCE_SHA='94a1976a6e24a5247d1a6c45b0e0cc791000d093'
SOURCE_FILES={'engine/search/Cargo.toml': '4781d1a8dc67a3d627c7fd1c9c5e6832c2a2fd17b1f75ced9999d9ec3ec3b0ff', 'engine/search/src/nash.rs': '6fbaa17adac185cf283ef2ea99a024e0be1ebec80f794e77439ec09e48660c84', 'engine/search/src/nash/scratch_observer.rs': '998ca28bb9adf4fd7c0c2f0f6df48bf019fd8109b4895e541696695d87e46e37', 'engine/search/tests/nash_scratch.rs': 'b5f13cae050f9fae2fd95473e525c6b7a7e3344da6d964342eac41db5f603f21', 'engine/search/tests/p16_reference.rs': 'd2d4227955c6e190cf9b3dbe10950d97523910a22e5fc061350c006a16d4b66a'}
MODE='nash-scratch'
PROBE='ci_nash_scratch_observer'
PROBE_PATH='engine/search/examples/'+PROBE+'.rs'
PUBLIC_TEST=borrowed.PUBLIC_TEST
COMMON_TESTS=(
    'exact_reference_matrix_records_preserve_bits_and_stopping',
    'malformed_public_matrices_preserve_panics_and_trailing_value_behavior',
    'isolated_allocator_proves_checkpoint_allocations_removed',
)
OBSERVER_TEST='observer_counts_checkpoint_work_and_owned_materializations'
LIMITS=(0,1,15,16,17,31,32,33,65)
PROOF_VARIANTS=tuple((storage,arm,kind) for storage,kind in
                    (('dense','observer'),('compact','observer'),('compact','plain'))
                    for arm in ('baseline','candidate'))
COUNT_FIELDS={'solve_calls','checkpoints','iterations','normalization_allocations',
              'evaluation_scratch_allocations','output_materializations'}
require=borrowed.require

def validate_counts(text,on):
    lines=re.findall(r'^P16 activation: (.+)$',text,re.M)
    require(len(lines)==1,'P16 actual observer missing or repeated')
    value=process._json(lines[0])
    require(isinstance(value,dict) and set(value)=={'nash','borrowed'},'P16 observer sections differ')
    n=value['nash']
    require(isinstance(n,dict) and set(n)==COUNT_FIELDS and all(type(v) is int and v>=0 for v in n.values()),
            'P16 observer fields malformed')
    calls,q=n['solve_calls'],n['checkpoints']
    require(calls>0 and q>=calls and n['iterations']>0,'P16 workload did not execute successful solve checkpoints')
    expected=(0,0,calls) if on else (2*q,q,q)
    require(tuple(n[k] for k in ('normalization_allocations','evaluation_scratch_allocations','output_materializations'))==expected,
            'P16 checkpoint allocation accounting differs from the selected runtime')
    borrowed.validate_counts('P13 activation: '+json.dumps(value['borrowed']),True)
    return value

def compare_counts(before,after):
    for name in ('solve_calls','checkpoints','iterations'):
        require(before['nash'][name]==after['nash'][name],'P16 changed '+name)
    require(before['borrowed']==after['borrowed'],'P16 changed R1 borrowed-key work')
    require(before['nash']['normalization_allocations']+before['nash']['evaluation_scratch_allocations']>
            after['nash']['normalization_allocations']+after['nash']['evaluation_scratch_allocations'],
            'P16 scratch allocation work did not decrease')

def variant_features(arm,compact,observer):
    flags=ci.feature_args(MODE,arm)[1].split(',')
    _,expected,_=ci.fingerprint_expectations(MODE,arm)
    if not compact:
        flags.remove('lab-engine/'+ci.COMPACT_FEATURE)
        expected['lab-engine'][ci.COMPACT_FEATURE]=False
    if observer:
        flags.append('lab-search/'+ci.NASH_OBSERVER)
        expected['lab-search'][ci.NASH_OBSERVER]=True
    return ','.join(flags),expected

def actual_features(arm):
    flags,expected=variant_features(arm,True,True)
    flags+=',lab-search/'+ci.BORROWED_OBSERVER
    expected['lab-search'][ci.BORROWED_OBSERVER]=True
    return flags,expected

def verify_declarations(root):
    require(re.fullmatch('[0-9a-f]{40}',SOURCE_SHA) and SOURCE_FILES,'P16 source not frozen and bound')
    paths={name:root/f'engine/{name}/Cargo.toml' for name in ('core','search','scenario')}
    features={name:ci.read_features(path) for name,path in paths.items()}
    runtime,observer=ci.NASH_FEATURE,ci.NASH_OBSERVER
    require(features['search'].get(runtime)==[] and features['search'].get(observer)==[],
            'P16 search-only runtime or independent observer differs')
    for name in ('core','scenario'):
        require(runtime not in features[name] and observer not in features[name], 'P16 unexpectedly forwarded')
    require(features['core'].get(ci.BORROWED_FEATURE)==[] and ci.BORROWED_OBSERVER not in features['core']
            and features['search'].get(ci.BORROWED_FEATURE)==['lab-engine/'+ci.BORROWED_FEATURE]
            and features['search'].get(ci.BORROWED_OBSERVER)==[],'R1 P13 feature declaration changed')
    for name,path in paths.items():ci.reject_default_experiments(path,features[name])
    tests=tomllib.loads(paths['search'].read_text(encoding='utf-8')).get('test',[])
    for target in ('nash_scratch','borrowed_child_keys'):
        rows=[r for r in tests if r.get('name')==target]
        require(len(rows)==1 and rows[0].get('path')=='tests/'+target+'.rs'
                and not rows[0].get('required-features'),'P16 integration registration differs: '+target)
    for path,digest in SOURCE_FILES.items():require(ci.sha(root/path)==digest,'P16 immutable source changed: '+path)
    return {'default_off':True,'independent_search_observer':True,'source_file_sha256':SOURCE_FILES}

def validate_tests(text,*,on,observer):
    passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
    expected=(*COMMON_TESTS,*((OBSERVER_TEST,) if observer else ()))
    require(sorted(passed)==sorted(expected),'P16 named tests missing, skipped or unexpected')
    summaries=borrowed.summaries(text)
    require(len(summaries)==2 and summaries[0][:4]==(1,0,0,0)
            and summaries[0][4]==len(expected)-1 and summaries[1]==(len(expected),0,0,0,0),
            'P16 standalone parent/allocator subprocess coverage differs')
    child=re.escape(COMMON_TESTS[2])
    markers=re.findall(r'^(?:test '+child+r' \.\.\. )?P16_ALLOC (.+)$',text,re.M)
    require(len(markers)==1,'P16 allocation proof absent or repeated')
    allocation=process._json(markers[0])
    require(isinstance(allocation,dict) and set(allocation)=={'candidate','records'}
            and type(allocation['candidate']) is bool and allocation['candidate']==on,'P16 allocator runtime mismatch')
    rows=allocation['records'];require(isinstance(rows,list) and len(rows)==len(LIMITS),'P16 allocation case count differs')
    for row,limit in zip(rows,LIMITS):
        q=max(1,(limit+15)//16)
        want={'limit':limit,'iterations':max(1,limit),'checkpoints':q,'reference':9+5*q,'actual':11 if on else 9+5*q}
        require(isinstance(row,dict) and row==want and all(type(v) is int for v in row.values()),'P16 scoped allocation equation differs')
    markers=re.findall(r'^P16_ACTIVATION (.+)$',text,re.M)
    require(len(markers)==int(observer),'P16 bounded observer absent or enabled in plain build')
    counts=None
    if observer:
        counts=process._json(markers[0]);q=sum(max(1,(limit+15)//16) for limit in LIMITS)
        want=dict(solve_calls=len(LIMITS),checkpoints=q,iterations=sum(max(1,v) for v in LIMITS),
                  normalization_allocations=0 if on else 2*q,
                  evaluation_scratch_allocations=0 if on else q,output_materializations=len(LIMITS) if on else q)
        require(counts==want and all(type(v) is int for v in counts.values()),'P16 bounded checkpoint accounting differs')
    return {'passed':len(passed),'required_named_tests':list(expected),'summaries':summaries,
            'allocator':allocation,'counts':counts}

def validate_records(path,*,malformed=False):
    raw=path.read_bytes();require(raw.endswith(b'\n') and b'\r' not in raw,'P16 complete LF JSONL required')
    rows=[process._json(line) for line in raw.splitlines()]
    def integer(value):return type(value) is int and value>=0
    def bit(value):return integer(value) and value<2**32
    def output(value,n,m):
        require(isinstance(value,dict) and set(value)=={'rows','cols','value','exploitability','iterations'},'P16 complete equilibrium output missing')
        require(isinstance(value['rows'],list) and len(value['rows'])==n and isinstance(value['cols'],list) and len(value['cols'])==m,
                'P16 equilibrium strategy shape differs')
        require(all(bit(v) for v in (*value['rows'],*value['cols'],value['value'],value['exploitability']))
                and integer(value['iterations']),'P16 float bits or iteration count missing')
    if malformed:
        expected=[(n,m,length,limit,reduced) for n,m in ((0,0),(0,2),(2,0),(1,1),(2,3),(3,2))
                  for length in range(n*m+4) for limit in (0,1,16,17) for reduced in (False,True)]
        require(len(rows)==len(expected)==296,'P16 malformed case coverage differs')
        for row,identity in zip(rows,expected):
            require(isinstance(row,dict) and set(row)=={'rows','cols','len','limit','reduced','want','got'},'P16 malformed record fields differ')
            require(tuple(row[k] for k in ('rows','cols','len','limit','reduced'))==identity and type(row['reduced']) is bool,
                    'P16 malformed order/input differs')
            require(row['want']==row['got'],'P16 malformed reference and result differ')
            value=row['got'];require(isinstance(value,dict) and len(value)==1,'P16 malformed result missing')
            if set(value)=={'Ok'}:output(value['Ok'],row['rows'],row['cols'])
            else:require(set(value)=={'Err'} and isinstance(value['Err'],str) and value['Err'],'P16 exact panic text missing')
    else:
        require(len(rows)==3073,'P16 matrix case coverage differs')
        require(sum(r.get('reduced') is True for r in rows)==73,'P16 reduced solve coverage differs')
        for row in rows:
            require(isinstance(row,dict) and set(row)=={'rows','cols','values','limit','tolerance_bits','reduced','output'},'P16 matrix record fields differ')
            require(integer(row['rows']) and integer(row['cols']) and row['rows']>0 and row['cols']>0
                    and isinstance(row['values'],list) and len(row['values'])==row['rows']*row['cols']
                    and all(bit(v) for v in row['values']) and bit(row['tolerance_bits'])
                    and type(row['reduced']) is bool and type(row['limit']) is int and row['limit'] in LIMITS,
                    'P16 matrix input shape/bits differ')
            output(row['output'],row['rows'],row['cols'])
        require(set(r['limit'] for r in rows)==set(LIMITS),'P16 checkpoint boundary coverage missing')
    return {'records':len(rows),'sha256':ci.sha(path),'bytes':len(raw)}

def validate_full_regressions(result):
    evidence={}
    for arm in ('baseline','candidate'):
        path=result/(arm+'-build-receipt.json');row=json.loads(path.read_text(encoding='utf-8'))
        require(row.get('status')=='success' and row.get('reused') is False and row.get('selection')==MODE,
                'P16 requires fresh full regressions')
        log=result/(arm+'-build.log');text=log.read_text(encoding='utf-8')
        passed=re.findall(r'^test (\S+) \.\.\. ok$',text,re.M)
        require(COMMON_TESTS and all(passed.count(name)==1 for name in (*COMMON_TESTS,PUBLIC_TEST,*borrowed.ON_TESTS[2:])),
                'P16/R1 named full-regression tests missing')
        evidence[arm]={'receipt_sha256':ci.sha(path),'log_sha256':ci.sha(log)}
    return evidence

def validate(workspace):
    workspace=Path(workspace).resolve();result=workspace/'ci-results'
    out=result/'nash-scratch-validation';out.mkdir(exist_ok=False)
    receipt={'status':'running','selection':MODE,'cached_results_reused':False,'performance_measurement':False,
             'common_runtime':'R1; E/F/P11/P12/P14/P15 OFF; dense disables only P10',
             'bounded_profile':'debug opt0','public_activation_profile':'release opt3',
             'commands':[],'variants':{},'activation':{}}
    def save():(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    def text(stem):return stem.with_suffix('.stdout').read_text(encoding='utf-8')+'\n'+stem.with_suffix('.stderr').read_text(encoding='utf-8')
    def execute(argv,env,stem):return process._run(argv,workspace/'candidate/engine',env,stem,ci.COMMAND_TIMEOUT_SECONDS,receipt['commands'],save)
    def environment(label):
        env=process.environment(workspace/('target-'+label))
        for name in ('LAB_P16_MATRIX_RECORDS','LAB_P16_MALFORMED_RECORDS','LAB_P16_ALLOC_CHILD','LAB_P13_RECORDS','LAB_P13_UNIT_RECORDS','LAB_P13_ALLOC_CHILD','LAB_ENGINE_STATS'):env.pop(name,None)
        env.update(RAYON_NUM_THREADS='1',RUST_MIN_STACK='16777216',CARGO_PROFILE_DEV_OPT_LEVEL='0',
                   CARGO_PROFILE_TEST_OPT_LEVEL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0')
        return env
    save()
    try:
        request=json.loads((result/'request.json').read_text(encoding='utf-8'))
        require(request['candidate_feature']==MODE and request['baseline_sha']==request['candidate_sha']==SOURCE_SHA,
                'P16 fixed same-source R1 request required')
        receipt['declarations']=verify_declarations(workspace/'candidate')
        receipt['full_regressions']=validate_full_regressions(result)
        labels=['p16-'+s+'-'+a+'-'+k for s,a,k in PROOF_VARIANTS]
        require(all(not os.path.lexists(workspace/('target-'+label)) for label in labels),'P16 proof targets must be fresh')
        public=[];matrix=[];malformed=[]
        for storage,arm,kind in PROOF_VARIANTS:
            compact,observer,on=storage=='compact',kind=='observer',arm=='candidate'
            label='p16-'+storage+'-'+arm+'-'+kind;env=environment(label)
            flags,expected=variant_features(arm,compact,observer);item={'features':flags};receipt['variants'][label]=item
            env['LAB_P16_MATRIX_RECORDS']=str(out/(label+'-matrix.jsonl'))
            env['LAB_P16_MALFORMED_RECORDS']=str(out/(label+'-malformed.jsonl'));stem=out/(label+'-nash')
            execute(['cargo','test','--locked','-p','lab-search','--test','nash_scratch','--features',flags,
                     '--','--show-output','--test-threads=1'],env,stem)
            item['nash_tests']=validate_tests(text(stem),on=on,observer=observer)
            for field,variable,target,bad in (('matrix','LAB_P16_MATRIX_RECORDS',matrix,False),
                                            ('malformed','LAB_P16_MALFORMED_RECORDS',malformed,True)):
                path=Path(env[variable]);item[field]=validate_records(path,malformed=bad);target.append(path)
            env['LAB_P13_RECORDS']=str(out/(label+'-public.jsonl'));stem=out/(label+'-public')
            execute(['cargo','test','--locked','-p','lab-search','--test','borrowed_child_keys','--features',flags,
                     '--','--show-output','--test-threads=1'],env,stem)
            item['public_test']=borrowed.validate_tests(text(stem),(PUBLIC_TEST,))
            item['public']=borrowed.validate_records(Path(env['LAB_P13_RECORDS']));public.append(Path(env['LAB_P13_RECORDS']))
            packages={'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json',
                'test-integration-test-nash_scratch.json','test-integration-test-borrowed_child_keys.json')}
            item['compiler_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,packages,expected,True,profile='debug')
            if compact and observer:
                require(ci.sha(workspace/'candidate'/PROBE_PATH)==ci.sha(Path(__file__).with_name('nash_scratch_probe.rs')),
                        'P16 observer injection differs')
                actual_flags,actual_expected=actual_features(arm)
                execute(['cargo','build','--locked','--release','-p','lab-search','--example',PROBE,'--features',actual_flags],
                        env,out/(label+'-probe-build'))
                item['probe_feature_evidence']=ci.preserve_expected_fingerprints(workspace,label,{
                    'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json','example-'+PROBE+'.json')},actual_expected,True)
                binary=workspace/('target-'+label)/'release/examples'/PROBE
                require(binary.is_file() and not binary.is_symlink(),'P16 observer binary missing')
                item['probe_binary_sha256']=ci.sha(binary)
                cases,_=bench.load_cases('narrow',root=workspace/'controller')
                require([c['name'] for c in cases]==['coaching','sand'],'P16 fixed workloads changed')
                receipt['activation'][arm]={}
                for case in cases:
                    stem=out/(label+'-'+case['name'])
                    execute([str(binary),str(workspace/'controller'/case['scenario']),'1',case['position']],env,stem)
                    output=stem.with_suffix('.stdout');bench.validate_output(bench.strict_json(output.read_text(encoding='utf-8')))
                    counts=validate_counts(stem.with_suffix('.stderr').read_text(encoding='utf-8'),on)
                    receipt['activation'][arm][case['name']]={'counts':counts,'output_sha256':ci.sha(output),
                                                            'output_bytes':output.stat().st_size,'output_file':output.name}
            save()
        require(all(path.read_bytes()==public[0].read_bytes() for path in public),'P16 public search output/work records differ')
        require(all(path.read_bytes()==matrix[0].read_bytes() for path in matrix),'P16 Nash reference bits records differ')
        require(all(path.read_bytes()==malformed[0].read_bytes() for path in malformed),'P16 malformed/panic records differ')
        for case in ('coaching','sand'):
            before,after=receipt['activation']['baseline'][case],receipt['activation']['candidate'][case]
            require((out/before['output_file']).read_bytes()==(out/after['output_file']).read_bytes(),'P16 actual search output/work differs')
            compare_counts(before['counts'],after['counts'])
        receipt.update(status='success',source_sha=SOURCE_SHA,complete_jsonl_byte_equal=True,
                       private_jsonl_byte_equal=True,actual_public_search_activated=True,R1_P13_activation_preserved=True)
    except Exception as error:
        receipt.update(status='failed',error=f'{type(error).__name__}: {error}');raise
    finally:save()
    return receipt
