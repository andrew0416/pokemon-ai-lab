"""Fail-closed synthetic controller tests; these never execute the engine."""
import copy
import itertools
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import ci
import build_cache
import nash_scratch as gate
import test_p8def_ci as fixtures
import test_run as outputs
import test_borrowed_child_keys_ci as keys

def counts(on):
    return {'nash':dict(solve_calls=3,checkpoints=8,iterations=120,
        normalization_allocations=0 if on else 16,evaluation_scratch_allocations=0 if on else 8,
        output_materializations=3 if on else 8),'borrowed':dict(key_captures=0,job_captures=5,
        borrowed_queries=20,seen_hits=1,seen_collisions=0,seen_links=5)}

def test_output(on=False,observer=False):
    names=(*gate.COMMON_TESTS,*((gate.OBSERVER_TEST,) if observer else ()))
    rows=[]
    for limit in gate.LIMITS:
        q=max(1,(limit+15)//16)
        rows.append(dict(limit=limit,iterations=max(1,limit),checkpoints=q,reference=9+5*q,actual=11 if on else 9+5*q))
    text='\n'.join('test '+name+' ... ok' for name in names)+'\n'
    text+='test '+gate.COMMON_TESTS[2]+' ... P16_ALLOC '+json.dumps(dict(candidate=on,records=rows))+'\nok\n'
    text+=f'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; {len(names)-1} filtered out; finished in 0s\n'
    text+=f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0s\n'
    if observer:
        q=sum(max(1,(v+15)//16) for v in gate.LIMITS)
        text+='P16_ACTIVATION '+json.dumps(dict(solve_calls=9,checkpoints=q,iterations=sum(max(1,v) for v in gate.LIMITS),
            normalization_allocations=0 if on else 2*q,evaluation_scratch_allocations=0 if on else q,
            output_materializations=9 if on else q))+'\n'
    return text

def equilibrium(n=1,m=1):
    return dict(rows=[0]*n,cols=[0]*m,value=0,exploitability=0,iterations=0)

def records(malformed=False):
    if malformed:
        return [dict(rows=n,cols=m,len=length,limit=limit,reduced=reduced,want={'Err':'empty game'},got={'Err':'empty game'})
                for n,m in ((0,0),(0,2),(2,0),(1,1),(2,3),(3,2)) for length in range(n*m+4)
                for limit in (0,1,16,17) for reduced in (False,True)]
    return [dict(rows=1,cols=1,values=[0],limit=gate.LIMITS[i%9],tolerance_bits=0,reduced=i<73,output=equilibrium())
            for i in range(3073)]

class RoutingTests(unittest.TestCase):
    def test_exact_R1_search_only_delta_and_fresh_full_regressions(self):
        base=ci.feature_args(ci.BORROWED_MODE,'candidate')
        self.assertEqual(ci.feature_args(gate.MODE,'baseline'),base)
        self.assertEqual(ci.feature_args(gate.MODE,'candidate'),['--features',base[1]+',lab-search/'+ci.NASH_FEATURE])
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            self.assertFalse(expected['lab-engine'][ci.NASH_FEATURE])
            self.assertEqual(expected['lab-search'][ci.NASH_FEATURE],arm=='candidate')
            for package in expected:
                self.assertTrue(expected[package][ci.BORROWED_FEATURE])
                for flag in (ci.NASH_OBSERVER,ci.MATRIX_FEATURE,ci.MATRIX_OBSERVER,ci.PL_FEATURE,ci.PL_OBSERVER,
                             ci.BORROWED_OBSERVER,ci.NEW_FEATURES['slot-diff'],ci.NEW_FEATURES['stats-off-cost']):
                    self.assertFalse(expected[package][flag])
                values=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                for rogue in ('experiment-inline-runstart','experiment-volatile-hash-update',ci.MATRIX_FEATURE,ci.NASH_OBSERVER,'unknown'):
                    with self.assertRaises(ValueError):ci.validate_strict_feature_closure(package,values+[rogue],expected[package])
                with self.assertRaises(ValueError):ci.validate_strict_feature_closure(package,values[:-1],expected[package])
            commands=ci.build_commands('narrow',gate.MODE,arm)
            self.assertIn('--release',commands[0]);self.assertIn('lab-engine',commands[0]);self.assertIn('lab-scenario',commands[0])
            self.assertNotIn('observer',' '.join(commands[1]))
        self.assertIn(gate.MODE,ci.STRICT_MODES)
        self.assertEqual(set(ci.injected_sources(gate.MODE)),{'engine/search/examples/ci_bench.rs',ci.COMPACT_PROBE_PATH,gate.PROBE_PATH})

    def test_six_bounded_axes_and_actual_dual_observer_keep_R1(self):
        self.assertEqual(len(gate.PROOF_VARIANTS),6)
        for arm,compact,observer in itertools.product(('baseline','candidate'),(False,True),(False,True)):
            flags,expected=gate.variant_features(arm,compact,observer)
            self.assertEqual(expected['lab-engine'][ci.COMPACT_FEATURE],compact)
            self.assertEqual(expected['lab-search'][ci.NASH_OBSERVER],observer)
            self.assertFalse(expected['lab-search'][ci.BORROWED_OBSERVER])
            self.assertTrue(expected['lab-engine'][ci.BORROWED_FEATURE])
            self.assertTrue(expected['lab-search'][ci.BORROWED_FEATURE])
        for arm in ('baseline','candidate'):
            _,expected=gate.actual_features(arm)
            self.assertTrue(expected['lab-search'][ci.BORROWED_OBSERVER])
            self.assertTrue(expected['lab-search'][ci.NASH_OBSERVER])

    def test_actual_probe_uses_frozen_analysis_and_configuration_body(self):
        here=Path(gate.__file__).parent
        raw=(here/'harness.rs').read_text();probe=(here/'nash_scratch_probe.rs').read_text()
        self.assertEqual(raw[raw.index('fn bit('):raw.index('fn main()')],probe[probe.index('fn bit('):probe.index('fn main()')])
        self.assertEqual(raw[raw.index('    let loaded ='):raw.index('    let analysis =')],
                         probe[probe.index('    let loaded ='):probe.index('    lab_search::nash::scratch_observer::reset();')])

    def test_refs_bind_source_one_thread_ten_pairs(self):
        with patch.object(gate,'SOURCE_SHA','d'*40):
            for change in ({},{'THREADS':'2'},{'PAIRS':'2'},{'BASELINE_SHA':'a'*40,'CANDIDATE_SHA':'a'*40}):
                with tempfile.TemporaryDirectory() as temporary:
                    root=Path(temporary);env={'BASELINE_SHA':gate.SOURCE_SHA,'CANDIDATE_SHA':gate.SOURCE_SHA,
                        'GITHUB_SHA':'c'*40,'CANDIDATE_FEATURE':gate.MODE,'SUITE':'narrow','THREADS':'1','PAIRS':'10',
                        'GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(root/'out'),**change}
                    with patch.dict(os.environ,env,clear=True):
                        if change:
                            with self.assertRaisesRegex(ValueError,'P16 requires frozen'):ci.refs(root)
                        else:ci.refs(root)

    def test_cache_cannot_skip_tests_and_binds_exact_injected_probe(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixtures.request(root,gate.MODE)
            with patch.dict(os.environ,{'BUILD_CACHE_ENABLED':'1','BASELINE_CACHE_HIT':'true','GITHUB_REPOSITORY':build_cache.TRUSTED_REPOSITORY}),patch.object(build_cache,'_current_plan') as calls:
                self.assertFalse(build_cache.restore(root,'baseline'))
            calls.assert_not_called()
            source=root/'baseline';injected=ci.injected_sources(gate.MODE)
            for name,path in injected.items():
                target=source/name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(path.read_bytes())
            (source/'engine/Cargo.lock').write_text('fixed lock')
            def git_output(argv,**kwargs):return ('\0'.join(injected)+'\0').encode() if '--others' in argv else b'engine/Cargo.lock\0'
            def command(argv,*args,**kwargs):return 'a'*40 if 'rev-parse' in argv else ''
            with patch.object(build_cache,'_command',side_effect=command),patch.object(build_cache.subprocess,'check_output',side_effect=git_output),patch.object(build_cache,'_runtime_identity',return_value={}),patch.object(build_cache,'_cargo_configuration',return_value={}),patch.object(gate.bench,'load_cases',return_value=([],[])):
                recipe=build_cache.make_recipe(root,'baseline')
                self.assertEqual(recipe['source']['nash_probe_sha256'],ci.sha(source/gate.PROBE_PATH))
                self.assertEqual(recipe['identity']['build_driver']['nash_scratch_probe.rs'],ci.sha(source/gate.PROBE_PATH))
                (source/gate.PROBE_PATH).write_text('modified')
                with self.assertRaisesRegex(ValueError,'P16 observer probe'):build_cache.make_recipe(root,'baseline')

    def test_manifest_rejects_default_forwarding_alias_and_changed_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);runtime,observer=ci.NASH_FEATURE,ci.NASH_OBSERVER
            manifests={'core':'[features]\ndefault=[]\nexperiment-borrowed-child-keys=[]\n','scenario':'[features]\ndefault=[]\n',
                'search':f'[features]\ndefault=[]\n{runtime}=[]\n{observer}=[]\nexperiment-borrowed-child-keys=["lab-engine/experiment-borrowed-child-keys"]\nexperiment-borrowed-child-keys-observer=[]\n'+''.join(f'[[test]]\nname="{t}"\npath="tests/{t}.rs"\n' for t in ('nash_scratch','borrowed_child_keys'))}
            for name,value in manifests.items():
                path=root/f'engine/{name}/Cargo.toml';path.parent.mkdir(parents=True);path.write_text(value)
            path=root/'engine/search/Cargo.toml';original=path.read_text()
            with patch.object(gate,'SOURCE_SHA','d'*40),patch.object(gate,'SOURCE_FILES',{'engine/search/Cargo.toml':ci.sha(path)}):
                self.assertTrue(gate.verify_declarations(root)['default_off'])
                for value in (original.replace(f'{observer}=[]',f'{observer}=["{runtime}"]'),original.replace('default=[]',f'default=["alias"]\nalias=["{runtime}"]'),original+'#changed'):
                    path.write_text(value)
                    with self.assertRaises(ValueError):gate.verify_declarations(root)

class ParserTests(unittest.TestCase):
    def test_checkpoint_allocator_and_observer_are_exact(self):
        for on,observer in itertools.product((False,True),repeat=2):
            raw=test_output(on,observer);gate.validate_tests(raw,on=on,observer=observer)
            for changed in (raw.replace(' ... ok',' ... ignored',1),raw.replace('0 failed','1 failed',1),
                            raw.replace('P16_ALLOC','MISSING'),raw.replace('"reference": 14','"reference": 13',1),
                            raw.replace('"actual": 11','"actual": 12',1) if on else raw.replace('"actual": 14','"actual": 15',1)):
                with self.assertRaises(ValueError):gate.validate_tests(changed,on=on,observer=observer)
            with self.assertRaises(ValueError):gate.validate_tests(raw,on=not on,observer=observer)
            with self.assertRaises(ValueError):gate.validate_tests(raw,on=on,observer=not observer)

    def test_record_fields_panics_and_coverage_are_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'records.jsonl'
            for malformed in (False,True):
                value=records(malformed);keys.jsonl(path,value);gate.validate_records(path,malformed=malformed)
                for change in ('missing','difference','bits'):
                    changed=copy.deepcopy(value)
                    if change=='missing':changed.pop()
                    elif malformed:
                        if change=='difference':changed[0]['got']={'Err':'different'}
                        else:changed[0]['want']=changed[0]['got']={'Ok':equilibrium(1,1)}
                    elif change=='difference':changed[0].pop('output')
                    else:changed[0]['output']['value']=True
                    keys.jsonl(path,changed)
                    with self.assertRaises(ValueError):gate.validate_records(path,malformed=malformed)

    def test_actual_checkpoint_and_R1_work_equations(self):
        for on in (False,True):self.assertEqual(gate.validate_counts('P16 activation: '+json.dumps(counts(on)),on),counts(on))
        gate.compare_counts(counts(False),counts(True))
        for field in gate.COUNT_FIELDS:
            changed=counts(True);changed['nash'][field]=True
            with self.assertRaises(ValueError):gate.validate_counts('P16 activation: '+json.dumps(changed),True)
        for scope,field in (('nash','solve_calls'),('nash','checkpoints'),('nash','iterations'),('borrowed','seen_hits')):
            changed=counts(True);changed[scope][field]+=1
            with self.assertRaises(ValueError):gate.compare_counts(counts(False),changed)
        for text in ('','P16 activation: '+json.dumps(counts(False)),('P16 activation: '+json.dumps(counts(True))+'\n')*2):
            with self.assertRaises(ValueError):gate.validate_counts(text,True)

class CompleteGateTests(unittest.TestCase):
    def run_gate(self,fault=None):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE)
            row.update(baseline_sha=gate.SOURCE_SHA,candidate_sha=gate.SOURCE_SHA)
            if fault=='source':row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            for arm in ('baseline','candidate'):
                fixtures.write_json(root/'ci-results'/f'{arm}-build-receipt.json',{'status':'success','reused':fault=='cached','selection':gate.MODE})
                names=(*gate.COMMON_TESTS,gate.PUBLIC_TEST,*gate.borrowed.ON_TESTS[2:])
                if fault=='regression':names=names[:-1]
                (root/'ci-results'/f'{arm}-build.log').write_text('\n'.join('test '+name+' ... ok' for name in names))
            probe=root/'candidate'/gate.PROBE_PATH;probe.parent.mkdir(parents=True);probe.write_bytes(Path(gate.__file__).with_name('nash_scratch_probe.rs').read_bytes())
            if fault=='fresh':(root/'target-p16-dense-baseline-observer').mkdir()
            calls=[]
            def fake_run(argv,cwd,env,stem,timeout,commands,save):
                calls.append(argv);label=Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-p16-')
                storage,arm,kind=label.split('-');on=arm=='candidate';observer=kind=='observer'
                flags,expected=gate.actual_features(arm) if '--release' in argv else gate.variant_features(arm,storage=='compact',observer)
                self.assertNotIn('LAB_ENGINE_STATS',env);stdout='';stderr=''
                if argv[0]=='cargo':
                    self.assertEqual(argv[argv.index('--features')+1],flags)
                    packages={'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json',)}
                    if argv[1]=='test':
                        self.assertNotIn('--release',argv);self.assertEqual(env['CARGO_PROFILE_TEST_OPT_LEVEL'],'0');profile='debug'
                        target=argv[argv.index('--test')+1];packages['lab-search']+=('test-integration-test-'+target+'.json',)
                        if target=='nash_scratch':
                            stdout=test_output(on,observer)
                            if fault=='allocator' and on:stdout=stdout.replace('"actual": 11','"actual": 12',1)
                            for variable,bad in (('LAB_P16_MATRIX_RECORDS',False),('LAB_P16_MALFORMED_RECORDS',True)):
                                rows=records(bad)
                                if fault=='nash-bits' and on and not bad:rows[0]['output']['value']=1
                                if fault=='panic' and on and bad:rows[0]['got']={'Err':'wrong'}
                                keys.jsonl(env[variable],rows)
                        else:
                            stdout=keys.test_output((gate.PUBLIC_TEST,));rows=keys.records()
                            if fault=='search-output' and on:rows[0]['analysis']='changed'
                            if fault=='restoration' and on:rows[0]['after']['debug']='changed'
                            keys.jsonl(env['LAB_P13_RECORDS'],rows)
                        if fault=='test' and on:stdout=stdout.replace(' ... ok',' ... ignored',1)
                    else:
                        profile='release';packages['lab-search']+=('example-'+gate.PROBE+'.json',)
                        binary=Path(env['CARGO_TARGET_DIR'])/'release/examples'/gate.PROBE;binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'fake observer')
                    for package,kinds in packages.items():
                        for name in kinds:
                            if fault=='fingerprint' and name=='test-integration-test-nash_scratch.json':continue
                            features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                            if fault=='feature' and on and package=='lab-search':features.remove(ci.NASH_FEATURE)
                            if fault=='rogue' and on:features.append(ci.MATRIX_FEATURE)
                            fixtures.write_json(Path(env['CARGO_TARGET_DIR'])/profile/'.fingerprint'/(package+'-mock')/name,{'features':json.dumps(features)})
                else:
                    value=outputs.valid_output()
                    if fault=='work' and on:value['stats']['nodes']+=1
                    stdout=json.dumps(value)+'\n';value=counts(on)
                    if fault=='activation' and on:value=counts(False)
                    if fault=='work-count' and on:value['nash']['iterations']+=1
                    if fault=='P13-work' and on:value['borrowed']['seen_hits']+=1
                    stderr='P16 activation: '+json.dumps(value)+'\n'
                stem.with_suffix('.stdout').write_text(stdout,encoding='utf-8',newline='\n');stem.with_suffix('.stderr').write_text(stderr,encoding='utf-8',newline='\n')
                commands.append({'argv':argv,'returncode':0});save()
            cases=[dict(name=n,scenario='engine/jobs/'+n+'.json',position='initial') for n in ('coaching','sand')]
            with patch.object(gate,'verify_declarations',return_value={'pinned':True}),patch.object(gate.process,'environment',side_effect=lambda t:dict(CARGO_TARGET_DIR=str(t))),patch.object(gate.process,'_run',side_effect=fake_run),patch.object(gate.bench,'load_cases',return_value=(cases,{})):
                if fault:
                    with self.assertRaises((ValueError,KeyError)):gate.validate(root)
                    receipt=json.loads((root/'ci-results/nash-scratch-validation/receipt.json').read_text());self.assertEqual(receipt['status'],'failed')
                else:
                    receipt=gate.validate(root);self.assertEqual(receipt['status'],'success');self.assertEqual(len(receipt['variants']),6)
                    self.assertEqual(len(calls),18);self.assertEqual(sum('--release' in argv for argv in calls),2)
                    self.assertTrue(receipt['actual_public_search_activated']);self.assertTrue(receipt['R1_P13_activation_preserved'])

    def test_complete_gate(self):self.run_gate()
    def test_fail_closed_mutations(self):
        for fault in ('source','cached','regression','fresh','allocator','nash-bits','panic','search-output','restoration',
                      'test','fingerprint','feature','rogue','work','activation','work-count','P13-work'):
            with self.subTest(fault=fault):self.run_gate(fault)

if __name__=='__main__':unittest.main()
