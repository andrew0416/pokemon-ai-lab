"""Synthetic fail-closed controller tests; these are not engine accuracy evidence."""
import itertools
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import build_cache
import ci
import matrix_pass_through as gate
import test_p8def_ci as fixtures
import test_run as outputs


def counts(on):
    return dict(a_calls=8 if on else 10, a_passthrough=6 if on else 0, a_fallback=2 if on else 10,
                b_calls=3, b_passthrough=2 if on else 0, b_fallback=1 if on else 3)


def records(private=False):
    if private:
        result = []
        pool = [0,0x80000000,0x7f800000,0xff800000,1,0x80000001,0x3f800000,0xbf800000]
        nan = [0x7fc00001,0x7fa00023,0xffc00456]
        for n,m in itertools.product(range(4),range(4)):
            for mask in range(1 << (n*m)):
                values = [pool[i % len(pool)] if mask & (1 << i) == 0 else nan[i % len(nan)] for i in range(n*m)]
                for slots in (1,2):
                    result.append(dict(slots=slots,rows=n,cols=m,input=values,result={'ok':dict(ours='[]',theirs='[]',values=[],omitted_ours=0,omitted_theirs=0)}))
        return result
    result = []
    def make(kind):
        row = {key:0 for key in gate.PUBLIC_FIELDS[kind]}
        row.update(kind=kind,stats=[0]*7)
        for key in ('before','after','ending','reversed'):
            if key in row:
                row[key] = dict(debug='State { full payload }',full_hash=10,position_hash=20)
        return row
    for slots,chance,threads,dominance,lazy in itertools.product((1,2),('Expect','Worst'),(1,2),(False,True),(False,True)):
        row=make('toy');row.update(slots=slots,chance=chance,threads=threads,dominance=dominance,lazy=lazy);result.append(row)
    for kind in ('depth3','terminal','unsupported'):
        for slots in (1,2):
            row=make(kind);row['slots']=slots;result.append(row)
    for kind in ('successful-resume','successful-replacement'):
        result.append(make(kind))
    for slots,lazy in itertools.product((1,2),(False,True)):
        row=make('nan-evaluator');row.update(slots=slots,lazy=lazy,value_bits=0x7fc00035);result.append(row)
    for name,index in [('aa-power-construct',i) for i in range(4)]+[('ability-change-fails',0),('eject-button-uturn',0),('eject-button-uturn',1)]:
        row=make('fixture');row.update(name=name,index=index,instructions='[Set...]',suspension='None');result.append(row)
    return result


def jsonl(path, rows):
    Path(path).write_text(''.join(json.dumps(row,sort_keys=True)+'\n' for row in rows),encoding='utf-8',newline='\n')


def test_output(names,lib=False,on=False,observer=False):
    text=''.join('test '+name+' ... ok\n' for name in names)
    if lib:
        text+='running 1 test\ntest '+gate.LIB_PREFIX+'isolated_allocator_proves_a_reconstruction_and_b_choice_clone_removal ... '
        text+=f'P14 ALLOC candidate={str(on).lower()} a_reference=5 a_actual={0 if on else 5} b_reference={2 if on else 7} b_actual={0 if on else 7}\nok\n'
        text+='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out; finished in 0.00s\n'
        text+='P14 MATRIX CASES 1378\n'
    elif observer:
        text+='P14_PUBLIC_ACTIVATION '+json.dumps(counts(on))+'\n'
    return text+f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'


class RoutingTests(unittest.TestCase):
    def test_search_only_runtime_and_observers_absent_from_timing(self):
        base=ci.feature_args('p8d-vs-p8def','baseline')
        self.assertEqual(ci.feature_args(gate.MODE,'baseline'),base)
        self.assertEqual(ci.feature_args(gate.MODE,'candidate'),['--features',base[1]+',lab-search/'+ci.MATRIX_FEATURE])
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            self.assertFalse(expected['lab-engine'][ci.MATRIX_FEATURE])
            self.assertEqual(expected['lab-search'][ci.MATRIX_FEATURE],arm=='candidate')
            for package in expected:
                for flag in (ci.MATRIX_OBSERVER,ci.BORROWED_FEATURE,ci.BORROWED_OBSERVER,ci.NEW_FEATURES['slot-diff'],ci.NEW_FEATURES['stats-off-cost']):
                    self.assertFalse(expected[package][flag])
            commands=ci.build_commands('narrow',gate.MODE,arm)
            self.assertIn('--release',commands[0]);self.assertNotIn('observer',' '.join(commands[1]))
        self.assertEqual(set(ci.injected_sources(gate.MODE)),{'engine/search/examples/ci_bench.rs','engine/scenario/examples/ci_compact_probe.rs',gate.PROBE_PATH})

    def test_exact_closure_rejects_observer_p11_p12_p13_and_missing_common_feature(self):
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            for package in expected:
                features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                for rogue in ('experiment-inline-runstart','experiment-volatile-hash-update',ci.BORROWED_FEATURE,ci.MATRIX_OBSERVER,'unknown'):
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
                            with self.assertRaisesRegex(ValueError,'P14 requires frozen'):ci.refs(root)
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
                self.assertEqual(recipe['source']['matrix_pass_through_probe_sha256'],ci.sha(source/gate.PROBE_PATH))
                self.assertEqual(recipe['identity']['build_driver']['matrix_pass_through_probe.rs'],ci.sha(source/gate.PROBE_PATH))
                (source/gate.PROBE_PATH).write_text('changed',encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'P14 observer probe'):build_cache.make_recipe(root,'baseline')

    def test_manifest_requires_search_only_default_off_independent_observer_and_pins(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            runtime,observer=ci.MATRIX_FEATURE,ci.MATRIX_OBSERVER
            manifests={
                'core':'[features]\ndefault=[]\n',
                'scenario':'[features]\ndefault=[]\n',
                'search':f'[features]\ndefault=[]\n{runtime}=[]\n{observer}=[]\n[[test]]\nname="matrix_pass_through"\npath="tests/matrix_pass_through.rs"\n'}
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


class EvidenceTests(unittest.TestCase):
    def test_record_schemas_inputs_nan_payloads_and_state_restoration(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'records.jsonl'
            for private,count in ((False,51),(True,1378)):
                jsonl(path,records(private));self.assertEqual(gate.validate_records(path,private=private)['records'],count)
            row=records();row[0]['after']['debug']='changed';jsonl(path,row)
            with self.assertRaises(ValueError):gate.validate_records(path)
            row=records();next(r for r in row if r['kind']=='nan-evaluator')['value_bits']=0;jsonl(path,row)
            with self.assertRaises(ValueError):gate.validate_records(path)
            row=records(True);row[-1]['input'][-1]=0;jsonl(path,row)
            with self.assertRaises(ValueError):gate.validate_records(path,private=True)
            row=records(True);row.pop();jsonl(path,row)
            with self.assertRaises(ValueError):gate.validate_records(path,private=True)

    def test_missing_ignored_filtered_allocator_or_corpus_is_rejected(self):
        for on in (False,True):
            text=test_output(gate.COMMON_TESTS,lib=True,on=on)
            gate.validate_tests(text,gate.COMMON_TESTS,lib=True,on=on)
            prefix='test '+gate.LIB_PREFIX+'isolated_allocator_proves_a_reconstruction_and_b_choice_clone_removal ... '
            gate.validate_tests(text.replace(prefix+'P14 ALLOC','P14 ALLOC'),gate.COMMON_TESTS,lib=True,on=on)
            for broken in (text.replace('test '+gate.COMMON_TESTS[0]+' ... ok','test '+gate.COMMON_TESTS[0]+' ... ignored'),text.replace('0 filtered out','1 filtered out'),text.replace('P14 ALLOC','WRONG ALLOC'),text.replace('P14 MATRIX CASES 1378','P14 MATRIX CASES 1377'),text.replace(prefix+'P14 ALLOC','unexpected P14 ALLOC')):
                with self.assertRaises(ValueError):gate.validate_tests(broken,gate.COMMON_TESTS,lib=True,on=on)

    def test_a_calls_avoided_by_b_are_accounted_without_false_equality(self):
        before,after=counts(False),counts(True)
        for on,row in ((False,before),(True,after)):gate.validate_counts('P14 activation: '+json.dumps(row),on)
        gate.compare_counts(before,after)
        self.assertNotEqual(before['a_calls'],after['a_calls'])
        for field in ('a_calls','b_calls','b_passthrough'):
            changed=dict(after);changed[field]+=1
            with self.assertRaises(ValueError):gate.compare_counts(before,changed)
        for text,on in (('',True),('P14 activation: '+json.dumps(before),True),('P14 activation: '+json.dumps(after),False)):
            with self.assertRaises(ValueError):gate.validate_counts(text,on)


class CompleteGateTests(unittest.TestCase):
    def run_gate(self,fault=None):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE)
            row.update(baseline_sha=gate.SOURCE_SHA,candidate_sha=gate.SOURCE_SHA)
            if fault=='source':row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            for arm in ('baseline','candidate'):
                fixtures.write_json(root/'ci-results'/f'{arm}-build-receipt.json',{'status':'success','reused':fault=='cached','selection':gate.MODE})
                (root/'ci-results'/f'{arm}-build.log').write_text(test_output((gate.PUBLIC_TEST,)),encoding='utf-8')
            probe=root/'candidate'/gate.PROBE_PATH;probe.parent.mkdir(parents=True);probe.write_bytes(Path(gate.__file__).with_name('matrix_pass_through_probe.rs').read_bytes())
            if fault=='fresh':(root/'target-p14-dense-baseline-plain').mkdir()
            calls=[]
            def fake_environment(target):return {'CARGO_TARGET_DIR':str(target),'RUSTFLAGS':'-Ctarget-cpu=x86-64'}
            def fake_run(argv,cwd,env,stem,timeout,commands,save):
                calls.append((argv,dict(env)));label=Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                _,storage,arm,kind=label.split('-');on=arm=='candidate';compact=storage=='compact';observer=kind=='observer'
                flags,expected=gate.variant_features(arm,compact,observer)
                stdout='';stderr=''
                if argv[0]=='cargo':
                    self.assertEqual(argv[argv.index('--features')+1],flags)
                    packages={'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json',)}
                    if argv[1]=='test':
                        self.assertNotIn('--release',argv);self.assertEqual(env['CARGO_PROFILE_TEST_OPT_LEVEL'],'0');profile='debug'
                        if '--lib' in argv:
                            stdout=test_output(gate.COMMON_TESTS,lib=True,on=on)
                            packages['lab-search']+=('test-lib-lab_search.json',)
                            rows=records(True)
                            if fault=='matrix' and on:rows[-1]['result']['ok']['values']=[0]
                            jsonl(env['LAB_P14_UNIT_RECORDS'],rows)
                        else:
                            stdout=test_output((gate.PUBLIC_TEST,),on=on,observer=observer)
                            packages['lab-search']+=('test-integration-test-matrix_pass_through.json',)
                            rows=records()
                            if fault=='difference' and on:rows[0]['matrix_bits']=42
                            if fault=='state' and on:rows[0]['before'].pop('debug')
                            jsonl(env['LAB_P14_RECORDS'],rows)
                            if fault=='test' and on:stdout=stdout.replace(' ... ok',' ... ignored')
                    else:
                        self.assertIn('--release',argv);profile='release';packages['lab-search']+=('example-'+gate.PROBE+'.json',)
                        binary=Path(env['CARGO_TARGET_DIR'])/'release/examples'/gate.PROBE;binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'fake observer')
                    for package,kinds in packages.items():
                        for name in kinds:
                            features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                            if fault=='feature' and on and package=='lab-search':features.remove(ci.MATRIX_FEATURE)
                            fixtures.write_json(Path(env['CARGO_TARGET_DIR'])/profile/'.fingerprint'/(package+'-mock')/name,{'features':json.dumps(features)})
                else:
                    value=outputs.valid_output()
                    if fault=='work' and on:value['stats']['nodes']+=1
                    stdout=json.dumps(value)+'\n';value=counts(on)
                    if fault=='activation' and on:value=counts(False)
                    if fault=='equation' and on:value['a_calls']+=1;value['a_fallback']+=1
                    stderr='P14 activation: '+json.dumps(value)+'\n'
                stem.with_suffix('.stdout').write_text(stdout,encoding='utf-8',newline='\n');stem.with_suffix('.stderr').write_text(stderr,encoding='utf-8',newline='\n')
                commands.append({'argv':argv,'returncode':0});save()
            cases=[dict(name=name,scenario='engine/jobs/'+name+'.json',position='initial') for name in ('coaching','sand')]
            with patch.object(gate,'verify_declarations',return_value={'pinned':True}),patch.object(gate.process,'environment',side_effect=fake_environment),patch.object(gate.process,'_run',side_effect=fake_run),patch.object(gate.bench,'load_cases',return_value=(cases,{})):
                if fault:
                    with self.assertRaises((ValueError,KeyError)):gate.validate(root)
                    receipt=json.loads((root/'ci-results/matrix-pass-through-validation/receipt.json').read_text())
                    self.assertEqual(receipt['status'],'failed');self.assertIn('error',receipt)
                else:
                    receipt=gate.validate(root)
                    self.assertEqual(receipt['status'],'success');self.assertEqual(len(receipt['variants']),8)
                    self.assertTrue(receipt['actual_public_search_activated']);self.assertTrue(receipt['private_jsonl_byte_equal'])
                    self.assertEqual(len(calls),18);self.assertEqual(sum('--release' in argv for argv,env in calls),2)

    def test_complete_gate_and_actual_feature_evidence(self):self.run_gate()
    def test_source_cache_missing_coverage_feature_and_output_failures_preserve_receipts(self):
        for fault in ('source','cached','fresh','difference','state','test','feature','work','activation','equation','matrix'):
            with self.subTest(fault=fault):self.run_gate(fault)


if __name__ == '__main__':unittest.main()
