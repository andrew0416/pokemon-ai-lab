"""Synthetic R1/P14 controller tests; not engine correctness/performance evidence."""
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
import r1_matrix_pass_through as gate
import test_p8def_ci as fixtures
import test_run as outputs
import test_matrix_pass_through_ci as matrix
import test_borrowed_child_keys_ci as keys

def counts(on):
    return {'matrix':matrix.counts(on),'borrowed':dict(key_captures=0,job_captures=5,
        borrowed_queries=20,seen_hits=1,seen_collisions=0,seen_links=5)}

class RoutingTests(unittest.TestCase):
    def test_old_validation_parsers_and_distinct_allocator_axes_are_preserved(self):
        self.assertIs(gate.validate_tests,gate.previous.validate_tests)
        self.assertIs(gate.validate_records,gate.previous.validate_records)
        self.assertEqual(len(gate.PROOF_VARIANTS),6)
        for arm in ('baseline','candidate'):
            _,debug=gate.variant_features(arm,True,True)
            _,actual=gate.actual_features(arm)
            self.assertFalse(debug['lab-search'][ci.BORROWED_OBSERVER])
            self.assertTrue(actual['lab-search'][ci.BORROWED_OBSERVER])
            self.assertTrue(actual['lab-search'][ci.MATRIX_OBSERVER])
        _,proof=gate.borrowed_proof_features()
        self.assertTrue(proof['lab-search'][ci.BORROWED_OBSERVER])
        self.assertTrue(proof['lab-search'][ci.MATRIX_FEATURE])
        self.assertTrue(proof['lab-search'][ci.MATRIX_OBSERVER])
    def test_search_only_runtime_and_observers_absent_from_timing(self):
        base=ci.feature_args(ci.BORROWED_MODE,'candidate')
        self.assertEqual(ci.feature_args(gate.MODE,'baseline'),base)
        self.assertEqual(ci.feature_args(gate.MODE,'candidate'),['--features',base[1]+',lab-search/'+ci.MATRIX_FEATURE])
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            self.assertFalse(expected['lab-engine'][ci.MATRIX_FEATURE])
            self.assertEqual(expected['lab-search'][ci.MATRIX_FEATURE],arm=='candidate')
            for package in expected:
                for flag in (ci.MATRIX_OBSERVER,ci.PL_FEATURE,ci.BORROWED_OBSERVER,ci.NEW_FEATURES['slot-diff'],ci.NEW_FEATURES['stats-off-cost']):
                    self.assertFalse(expected[package][flag])
            for package in expected:self.assertTrue(expected[package][ci.BORROWED_FEATURE])
            commands=ci.build_commands('narrow',gate.MODE,arm)
            self.assertIn('--release',commands[0]);self.assertNotIn('observer',' '.join(commands[1]))
        self.assertEqual(set(ci.injected_sources(gate.MODE)),{'engine/search/examples/ci_bench.rs','engine/scenario/examples/ci_compact_probe.rs',gate.PROBE_PATH})

    def test_exact_closure_rejects_observer_p11_p12_p15_and_missing_common_feature(self):
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            for package in expected:
                features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                for rogue in ('experiment-inline-runstart','experiment-volatile-hash-update',ci.PL_FEATURE,ci.MATRIX_OBSERVER,'unknown'):
                    with self.subTest(arm=arm,package=package,rogue=rogue),self.assertRaises(ValueError):
                        ci.validate_strict_feature_closure(package,features+[rogue],expected[package])
                with self.assertRaises(ValueError):ci.validate_strict_feature_closure(package,features[:-1],expected[package])

    def test_refs_bind_same_source_one_thread_ten_pairs(self):
        with patch.object(gate,'SOURCE_SHA','d'*40):
            for change in ({},{'THREADS':'2'},{'PAIRS':'2'},{'BASELINE_SHA':'a'*40,'CANDIDATE_SHA':'a'*40}):
                with tempfile.TemporaryDirectory() as directory:
                    root=Path(directory)
                    env={'BASELINE_SHA':gate.SOURCE_SHA,'CANDIDATE_SHA':gate.SOURCE_SHA,'GITHUB_SHA':'c'*40,'CANDIDATE_FEATURE':gate.MODE,'SUITE':'narrow','THREADS':'1','PAIRS':'10','GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(root/'output'),**change}
                    with patch.dict(os.environ,env,clear=True):
                        if change:
                            with self.assertRaisesRegex(ValueError,'R1/P14 requires frozen'):ci.refs(root)
                        else:ci.refs(root)

    def test_default_alias_and_source_mismatch_fail_before_build(self):
        for flag in (ci.MATRIX_FEATURE,ci.MATRIX_OBSERVER):
            with self.assertRaises(ValueError):ci.reject_default_experiments(Path('Cargo.toml'),{'default':['alias'],'alias':[flag]})
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE);row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            with patch.object(ci,'output') as calls,self.assertRaises(ValueError):ci.prepare(root)
            calls.assert_not_called()

    def test_cache_never_substitutes_old_accuracy_results(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixtures.request(root,gate.MODE)
            with patch.dict(os.environ,{'BUILD_CACHE_ENABLED':'1','BASELINE_CACHE_HIT':'true','GITHUB_REPOSITORY':build_cache.TRUSTED_REPOSITORY}),patch.object(build_cache,'_current_plan') as calls:
                self.assertFalse(build_cache.restore(root,'baseline'))
            calls.assert_not_called()

    def test_cache_binds_every_injected_probe(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixtures.request(root,gate.MODE);source=root/'baseline'
            injected=ci.injected_sources(gate.MODE)
            for name,path in injected.items():
                target=source/name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(path.read_bytes())
            (source/'engine/Cargo.lock').write_text('fixed lock',encoding='utf-8')
            def git_output(argv,**kwargs):return ('\0'.join(injected)+'\0').encode() if '--others' in argv else b'engine/Cargo.lock\0'
            def command(argv,*args,**kwargs):return 'a'*40 if 'rev-parse' in argv else ''
            with patch.object(build_cache,'_command',side_effect=command),patch.object(build_cache.subprocess,'check_output',side_effect=git_output),patch.object(build_cache,'_runtime_identity',return_value={}),patch.object(build_cache,'_cargo_configuration',return_value={}),patch.object(gate.bench,'load_cases',return_value=([],[])):
                recipe=build_cache.make_recipe(root,'baseline')
                self.assertEqual(recipe['source']['r1_matrix_probe_sha256'],ci.sha(source/gate.PROBE_PATH))
                self.assertEqual(recipe['identity']['build_driver']['r1_matrix_pass_through_probe.rs'],ci.sha(source/gate.PROBE_PATH))
                (source/gate.PROBE_PATH).write_text('changed',encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'R1/P14 observer probe'):build_cache.make_recipe(root,'baseline')

    def test_manifest_requires_search_only_default_off_independent_observer_and_pins(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            runtime,observer=ci.MATRIX_FEATURE,ci.MATRIX_OBSERVER
            manifests={
                'core':'[features]\ndefault=[]\nexperiment-borrowed-child-keys=[]\n',
                'scenario':'[features]\ndefault=[]\n',
                'search':f'[features]\ndefault=[]\n{runtime}=[]\n{observer}=[]\nexperiment-borrowed-child-keys=["lab-engine/experiment-borrowed-child-keys"]\nexperiment-borrowed-child-keys-observer=[]\n[[test]]\nname="matrix_pass_through"\npath="tests/matrix_pass_through.rs"\n'}
            for package,value in manifests.items():
                path=root/f'engine/{package}/Cargo.toml';path.parent.mkdir(parents=True);path.write_text(value,encoding='utf-8')
            path=root/'engine/search/Cargo.toml'
            pins={'engine/search/Cargo.toml':ci.sha(path)}
            with patch.object(gate,'SOURCE_SHA','d'*40),patch.object(gate,'SOURCE_FILES',pins):
                self.assertTrue(gate.verify_declarations(root)['default_off'])
                original=path.read_text(encoding='utf-8')
                for changed in (original.replace(f'{observer}=[]',f'{observer}=["{runtime}"]'),original.replace('default=[]',f'default=["{runtime}"]'),original+'# modified\n'):
                    path.write_text(changed,encoding='utf-8')
                    with self.assertRaises(ValueError):gate.verify_declarations(root)

    def test_eight_axes_change_only_storage_runtime_and_observer(self):
        for arm,compact,observer in itertools.product(('baseline','candidate'),(False,True),(False,True)):
            flags,expected=gate.variant_features(arm,compact,observer)
            self.assertEqual(expected['lab-engine'][ci.COMPACT_FEATURE],compact)
            self.assertFalse(expected['lab-engine'][ci.MATRIX_FEATURE])
            self.assertEqual(expected['lab-search'][ci.MATRIX_OBSERVER],observer)
            self.assertTrue(expected['lab-engine'][ci.NEW_FEATURES['replay-action-keys']])
            self.assertEqual(('lab-search/'+ci.MATRIX_OBSERVER) in flags.split(','),observer)


class JointObserverTests(unittest.TestCase):
    def test_both_R1_arms_activate_P13_and_only_candidate_activates_P14(self):
        for on in (False,True):self.assertEqual(gate.validate_counts('R1_P14 activation: '+json.dumps(counts(on)),on),counts(on))
        gate.compare_counts(counts(False),counts(True))
        for field,value in (('key_captures',1),('borrowed_queries',0),('job_captures',0),('seen_links',0)):
            changed=counts(True);changed['borrowed'][field]=value
            with self.assertRaises(ValueError):gate.validate_counts('R1_P14 activation: '+json.dumps(changed),True)
        for change in ('absent','twice','zero-P14','no-P13','missing-field'):
            value=counts(False) if change=='zero-P14' else counts(True)
            if change=='no-P13':value['borrowed']=None
            if change=='missing-field':value['borrowed'].pop('seen_hits')
            line='R1_P14 activation: '+json.dumps(value)+'\n'
            text='' if change=='absent' else line*2 if change=='twice' else line
            with self.subTest(change=change),self.assertRaises((ValueError,TypeError)):gate.validate_counts(text,True)
        for scope,field in (('borrowed','seen_hits'),('borrowed','job_captures'),('matrix','a_calls'),('matrix','b_calls')):
            changed=counts(True);changed[scope][field]+=1
            with self.assertRaises(ValueError):gate.compare_counts(counts(False),changed)


class CompleteGateTests(unittest.TestCase):
    def run_gate(self,fault=None):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE)
            row.update(baseline_sha=gate.SOURCE_SHA,candidate_sha=gate.SOURCE_SHA)
            if fault=='source':row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            for arm in ('baseline','candidate'):
                fixtures.write_json(root/'ci-results'/f'{arm}-build-receipt.json',{'status':'success','reused':fault=='cached','selection':gate.MODE})
                names=(gate.PUBLIC_TEST,gate.borrowed.PUBLIC_TEST)+gate.borrowed.ON_TESTS[2:]
                if fault=='P13-regression' and arm=='baseline':names=names[:-1]
                (root/'ci-results'/f'{arm}-build.log').write_text(matrix.test_output(names),encoding='utf-8')
            probe=root/'candidate'/gate.PROBE_PATH;probe.parent.mkdir(parents=True);probe.write_bytes(Path(gate.__file__).with_name('r1_matrix_pass_through_probe.rs').read_bytes())
            if fault=='fresh':(root/'target-r1-p14-dense-baseline-observer').mkdir()
            calls=[]
            def fake_environment(target):return {'CARGO_TARGET_DIR':str(target),'RUSTFLAGS':'-Ctarget-cpu=x86-64'}
            def fake_run(argv,cwd,env,stem,timeout,commands,save):
                calls.append((argv,dict(env)));label=Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                p13=label=='r1-p14-borrowed-proof'
                if p13:
                    on=True;compact=True;observer=False;flags,expected=gate.borrowed_proof_features()
                else:
                    storage,arm,kind=label.removeprefix('r1-p14-').split('-');on=arm=='candidate';compact=storage=='compact';observer=kind=='observer'
                    flags,expected=gate.actual_features(arm) if '--release' in argv else gate.variant_features(arm,compact,observer)
                stdout='';stderr=''
                if argv[0]=='cargo':
                    self.assertEqual(argv[argv.index('--features')+1],flags)
                    packages={'lab-engine':('lib-lab_engine.json',),'lab-search':() if p13 else ('lib-lab_search.json',)}
                    if argv[1]=='test':
                        self.assertNotIn('--release',argv);self.assertEqual(env['CARGO_PROFILE_TEST_OPT_LEVEL'],'0');profile='debug'
                        if p13:
                            stdout=keys.test_output(gate.borrowed.COMMON_TESTS+gate.borrowed.ON_TESTS,lib=True,on=True,compact=True)
                            packages['lab-search']+=('test-lib-lab_search.json',)
                            rows=keys.records(private=True)
                            if fault=='P13-private':rows.pop()
                            keys.jsonl(env['LAB_P13_UNIT_RECORDS'],rows)
                            if fault=='allocator-isolation':stdout+='\n'+gate.COMMON_TESTS[0]+'\n'
                        elif '--lib' in argv:
                            self.assertFalse(expected['lab-search'][ci.BORROWED_OBSERVER])
                            stdout=matrix.test_output(gate.COMMON_TESTS,lib=True,on=on)
                            packages['lab-search']+=('test-lib-lab_search.json',)
                            rows=matrix.records(True)
                            if fault=='matrix' and on:rows[-1]['result']['ok']['values']=[0]
                            matrix.jsonl(env['LAB_P14_UNIT_RECORDS'],rows)
                            if fault=='allocator' and on:stdout=stdout.replace('a_actual=0','a_actual=5')
                        else:
                            stdout=matrix.test_output((gate.PUBLIC_TEST,),on=on,observer=observer)
                            packages['lab-search']+=('test-integration-test-matrix_pass_through.json',)
                            rows=matrix.records()
                            if fault=='difference' and on:rows[0]['matrix_bits']=42
                            if fault=='state' and on:rows[0]['before'].pop('debug')
                            matrix.jsonl(env['LAB_P14_RECORDS'],rows)
                            if fault=='test' and on:stdout=stdout.replace(' ... ok',' ... ignored')
                    else:
                        self.assertIn('--release',argv);profile='release';packages['lab-search']+=('example-'+gate.PROBE+'.json',)
                        binary=Path(env['CARGO_TARGET_DIR'])/'release/examples'/gate.PROBE;binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'fake observer')
                    for package,kinds in packages.items():
                        for name in kinds:
                            if fault=='P13-fingerprint' and p13 and name=='test-lib-lab_search.json':continue
                            features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                            if fault=='feature' and on and package=='lab-search':features.remove(ci.MATRIX_FEATURE)
                            if fault=='P13-feature' and package=='lab-engine':features.remove(ci.BORROWED_FEATURE)
                            fixtures.write_json(Path(env['CARGO_TARGET_DIR'])/profile/'.fingerprint'/(package+'-mock')/name,{'features':json.dumps(features)})
                    if p13:
                        self.assertFalse(list((Path(env['CARGO_TARGET_DIR'])/'debug/.fingerprint').glob('lab-search-*/lib-lab_search.json')))
                else:
                    value=outputs.valid_output()
                    if fault=='work' and on:value['stats']['nodes']+=1
                    stdout=json.dumps(value)+'\n';value=counts(on)
                    if fault=='activation' and on:value=counts(False)
                    if fault=='equation' and on:value['matrix']['a_calls']+=1;value['matrix']['a_fallback']+=1
                    if fault=='P13-work' and on:value['borrowed']['seen_hits']+=1
                    stderr='R1_P14 activation: '+json.dumps(value)+'\n'
                stem.with_suffix('.stdout').write_text(stdout,encoding='utf-8',newline='\n');stem.with_suffix('.stderr').write_text(stderr,encoding='utf-8',newline='\n')
                commands.append({'argv':argv,'returncode':0});save()
            cases=[dict(name=name,scenario='engine/jobs/'+name+'.json',position='initial') for name in ('coaching','sand')]
            with patch.object(gate,'verify_declarations',return_value={'pinned':True}),patch.object(gate.process,'environment',side_effect=fake_environment),patch.object(gate.process,'_run',side_effect=fake_run),patch.object(gate.bench,'load_cases',return_value=(cases,{})):
                if fault:
                    with self.assertRaises((ValueError,KeyError)):gate.validate(root)
                    receipt=json.loads((root/'ci-results/r1-matrix-pass-through-validation/receipt.json').read_text())
                    self.assertEqual(receipt['status'],'failed');self.assertIn('error',receipt)
                else:
                    receipt=gate.validate(root)
                    self.assertEqual(receipt['status'],'success');self.assertEqual(len(receipt['variants']),6)
                    self.assertTrue(receipt['actual_public_search_activated']);self.assertTrue(receipt['private_jsonl_byte_equal'])
                    self.assertTrue(receipt['R1_P13_activation_preserved']);self.assertEqual(receipt['borrowed_proof']['records']['records'],5)
                    self.assertEqual(len(calls),17);self.assertEqual(sum('--release' in argv for argv,env in calls),2)

    def test_complete_gate_and_actual_feature_evidence(self):self.run_gate()
    def test_scope_allocator_features_work_and_activation_fail_closed(self):
        for fault in ('source','cached','fresh','P13-regression','difference','state','test','feature','P13-feature','work','activation','equation','matrix','allocator','P13-private','P13-fingerprint','allocator-isolation','P13-work'):
            with self.subTest(fault=fault):self.run_gate(fault)


if __name__=='__main__':unittest.main()
