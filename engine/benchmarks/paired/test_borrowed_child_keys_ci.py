"""P13 controller fail-closed tests; Rust runs are inert mocks, never benchmarks."""
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
import borrowed_child_keys as gate
import test_p8def_ci as fixtures
import test_run as outputs


def records(private=False):
    if private:
        result=[]
        for capacity in (0,1,2):
            row={k:0 for k in ('broken','capacity','nodes','omitted','state','stats','tt_len','turns','unsupported','values')}
            row.update(capacity=capacity,state='State { full payload }');result.append(row)
        for reverse in (False,True):
            row={k:0 for k in ('broken','nodes','omitted','result','reverse','state','turns','unsupported')}
            row.update(reverse=reverse,state='State { full payload }');result.append(row)
        return result
    result=[]
    def make(kind):
        row={k:0 for k in gate.PUBLIC_FIELDS[kind]};row.update(kind=kind,stats=[0]*7)
        for key in ('before','after','ending','reversed'):
            if key in row:row[key]={'debug':'State { raw payload }','full_hash':10,'position_hash':20}
        return row
    for slots,chance,threads,tt,factored in itertools.product((1,2),('Expect','Worst'),(1,2),(False,True),(False,True)):
        row=make('toy');row.update(slots=slots,chance=chance,threads=threads,tt=tt,factored=factored);result.append(row)
    for kind in ('depth3','terminal','unsupported'):
        for slots in (1,2):
            row=make(kind);row['slots']=slots;result.append(row)
    for kind in ('successful-resume','successful-replacement'):result.append(make(kind))
    for name,index in [('aa-power-construct',i) for i in range(4)]+[('ability-change-fails',0),('eject-button-uturn',0),('eject-button-uturn',1)]:
        row=make('fixture');row.update(name=name,index=index,instructions='[Set...]',suspension='None');result.append(row)
    return result


def jsonl(path,rows):
    Path(path).write_text(''.join(json.dumps(row,sort_keys=True)+'\n' for row in rows),encoding='utf-8',newline='\n')


def test_output(names,lib=False,on=False,compact=False):
    text=''.join('test '+name+' ... ok\n' for name in names)
    if lib:
        old=2 if compact else 1
        text+=f'allocator old={old} actual={0 if on else old} candidate={str(on).lower()}\n'
        text+='test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out; finished in 0.00s\n'
    return text+f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'


class RoutingTests(unittest.TestCase):
    def test_common5_exact_search_flag_forwarding_and_observers_off(self):
        base=ci.feature_args('p8d-vs-p8def','baseline')
        self.assertEqual(ci.feature_args(gate.MODE,'baseline'),base)
        self.assertEqual(ci.feature_args(gate.MODE,'candidate'),['--features',base[1]+',lab-search/'+ci.BORROWED_FEATURE])
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            for package in ('lab-engine','lab-search'):
                self.assertEqual(expected[package][ci.BORROWED_FEATURE],arm=='candidate')
                self.assertFalse(expected[package][ci.BORROWED_OBSERVER])
                self.assertFalse(expected[package][ci.NEW_FEATURES['slot-diff']])
                self.assertFalse(expected[package][ci.NEW_FEATURES['stats-off-cost']])
            command=ci.build_commands('narrow',gate.MODE,arm)
            self.assertIn('--release',command[0]);self.assertNotIn('observer',' '.join(command[1]))
        injected=ci.injected_sources(gate.MODE)
        self.assertEqual(set(injected),{'engine/search/examples/ci_bench.rs','engine/scenario/examples/ci_compact_probe.rs',gate.PROBE_PATH})

    def test_timing_feature_changes_and_p11_p12_unknown_are_rejected(self):
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            for package in expected:
                features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                for rogue in ('experiment-inline-runstart','experiment-volatile-hash-update','unknown',ci.BORROWED_OBSERVER):
                    with self.subTest(arm=arm,package=package,rogue=rogue),self.assertRaises(ValueError):
                        ci.validate_strict_feature_closure(package,features+[rogue],expected[package])
                for feature in (ci.BORROWED_FEATURE,ci.NEW_FEATURES['replay-action-keys']):
                    changed=set(features)^{feature}
                    with self.assertRaises(ValueError):ci.validate_strict_feature_closure(package,changed,expected[package])

    def test_refs_require_frozen_source_one_thread_ten_pairs(self):
        for change in ({},{'THREADS':'2'},{'PAIRS':'2'},{'BASELINE_SHA':'a'*40,'CANDIDATE_SHA':'a'*40}):
            with tempfile.TemporaryDirectory() as directory:
                root=Path(directory)
                env={'BASELINE_SHA':gate.SOURCE_SHA,'CANDIDATE_SHA':gate.SOURCE_SHA,'GITHUB_SHA':'c'*40,'CANDIDATE_FEATURE':gate.MODE,'SUITE':'narrow','THREADS':'1','PAIRS':'10','GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(root/'output'),**change}
                with patch.dict(os.environ,env,clear=True):
                    if change:
                        with self.assertRaisesRegex(ValueError,'P13 requires frozen'):ci.refs(root)
                    else:
                        ci.refs(root);self.assertEqual(json.loads((root/'ci-results/request.json').read_text())['pairs'],10)

    def test_default_alias_and_different_sha_fail_before_build(self):
        for feature in (ci.BORROWED_FEATURE,ci.BORROWED_OBSERVER):
            with self.assertRaises(ValueError):ci.reject_default_experiments(Path('Cargo.toml'),{'default':['alias'],'alias':[feature]})
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE);row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            with patch.object(ci,'output') as calls,self.assertRaises(ValueError):ci.prepare(root)
            calls.assert_not_called()

    def test_cache_cannot_reuse_accuracy_results(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixtures.request(root,gate.MODE)
            with patch.dict(os.environ,{'BUILD_CACHE_ENABLED':'1','BASELINE_CACHE_HIT':'true','GITHUB_REPOSITORY':build_cache.TRUSTED_REPOSITORY}),patch.object(build_cache,'_current_plan') as calls:
                self.assertFalse(build_cache.restore(root,'baseline'))
            calls.assert_not_called()

    def test_cache_recipe_binds_every_injected_probe_and_rejects_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixtures.request(root,gate.MODE);source=root/'baseline'
            injected=ci.injected_sources(gate.MODE)
            for name,path in injected.items():
                target=source/name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(path.read_bytes())
            (source/'engine/Cargo.lock').write_text('fixed lock',encoding='utf-8')
            def git_output(argv,**kwargs):
                return ('\0'.join(injected)+'\0').encode() if '--others' in argv else b'engine/Cargo.lock\0'
            def command(argv,*args,**kwargs):return 'a'*40 if 'rev-parse' in argv else ''
            with patch.object(build_cache,'_command',side_effect=command),patch.object(build_cache.subprocess,'check_output',side_effect=git_output),patch.object(build_cache,'_runtime_identity',return_value={}),patch.object(build_cache,'_cargo_configuration',return_value={}),patch.object(gate.bench,'load_cases',return_value=([],[])):
                recipe=build_cache.make_recipe(root,'baseline')
                self.assertEqual(recipe['source']['borrowed_child_keys_probe_sha256'],ci.sha(source/gate.PROBE_PATH))
                self.assertEqual(recipe['identity']['build_driver']['borrowed_child_keys_probe.rs'],ci.sha(source/gate.PROBE_PATH))
                (source/gate.PROBE_PATH).write_text('changed',encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'P13 observer probe'):build_cache.make_recipe(root,'baseline')

    def test_declaration_pins_forwarding_default_off_and_independent_observer(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for name in gate.SOURCE_FILES:
                path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text('test fixture',encoding='utf-8')
            runtime,observer=ci.BORROWED_FEATURE,ci.BORROWED_OBSERVER
            core=root/'engine/core/Cargo.toml';core.write_text(f'[features]\ndefault=[]\n{runtime}=[]\n',encoding='utf-8')
            search=root/'engine/search/Cargo.toml';search.write_text(f'[features]\ndefault=[]\n{runtime}=["lab-engine/{runtime}"]\n{observer}=[]\n[[test]]\nname="borrowed_child_keys"\npath="tests/borrowed_child_keys.rs"\n',encoding='utf-8')
            scenario=root/'engine/scenario/Cargo.toml';scenario.parent.mkdir(parents=True);scenario.write_text('[features]\ndefault=[]\n',encoding='utf-8')
            pins={name:ci.sha(root/name) for name in gate.SOURCE_FILES}
            with patch.object(gate,'SOURCE_FILES',pins):
                self.assertTrue(gate.verify_declarations(root)['default_off'])
                original=search.read_text(encoding='utf-8');search.write_text(original.replace(f'{observer}=[]',f'{observer}=["{runtime}"]'),encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'independent observer'):gate.verify_declarations(root)
                search.write_text(original,encoding='utf-8');core.write_text(core.read_text(encoding='utf-8').replace('default=[]',f'default=["{runtime}"]'),encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'default'):gate.verify_declarations(root)

    def test_dense_observer_axes_keep_all_other_common_features(self):
        for arm,compact,observer in itertools.product(('baseline','candidate'),(False,True),(False,True)):
            flags,expected=gate.variant_features(arm,compact,observer)
            self.assertEqual(expected['lab-engine'][ci.COMPACT_FEATURE],compact)
            self.assertEqual(expected['lab-search'][ci.BORROWED_OBSERVER],observer)
            self.assertTrue(expected['lab-engine'][ci.NEW_FEATURES['replay-action-keys']])
            self.assertEqual(('lab-search/'+ci.BORROWED_OBSERVER) in flags.split(','),observer)


class RecordTests(unittest.TestCase):
    def test_exact_record_counts_and_state_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'records.jsonl'
            for private,count in ((False,47),(True,5)):
                jsonl(path,records(private));self.assertEqual(gate.validate_records(path,private=private)['records'],count)
            cases=[]
            row=records();row.pop();cases.append(row)
            row=records();row[0]['before'].pop('debug');cases.append(row)
            row=records();row[0]['after']['debug']='changed';cases.append(row)
            row=records();row[0]['stats'][0]=True;cases.append(row)
            row=records();row[-1]['index']=99;cases.append(row)
            for row in cases:
                jsonl(path,row)
                with self.assertRaises(ValueError):gate.validate_records(path)

    def test_missing_named_filtered_ignored_allocator_fail(self):
        for on,compact in itertools.product((False,True),(False,True)):
            names=gate.COMMON_TESTS+(gate.ON_TESTS if on else ())
            text=test_output(names,lib=True,on=on,compact=compact)
            gate.validate_tests(text,names,lib=True,on=on,compact=compact)
            for broken in (text.replace('test '+names[0]+' ... ok','test '+names[0]+' ... ignored'),text.replace('0 filtered out','1 filtered out'),text.replace('allocator old=','allocator wrong=')):
                with self.assertRaises(ValueError):gate.validate_tests(broken,names,lib=True,on=on,compact=compact)

    def test_activation_requires_owned_vs_borrowed_and_work_equations(self):
        before=dict(key_captures=8,job_captures=3,borrowed_queries=0,seen_hits=1,seen_collisions=0,seen_links=0)
        after=dict(key_captures=0,job_captures=3,borrowed_queries=8,seen_hits=1,seen_collisions=0,seen_links=3)
        for on,row in ((False,before),(True,after)):gate.validate_counts('P13 activation: '+json.dumps(row),on)
        gate.compare_counts(before,after)
        for field in ('borrowed_queries','job_captures','seen_hits'):
            changed=dict(after);changed[field]+=1
            with self.assertRaises(ValueError):gate.compare_counts(before,changed)
        with self.assertRaises(ValueError):gate.validate_counts('',True)
        with self.assertRaises(ValueError):gate.validate_counts('P13 activation: '+json.dumps(before),True)


class CompleteGateTests(unittest.TestCase):
    def run_gate(self,fault=None):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE)
            row.update(baseline_sha=gate.SOURCE_SHA,candidate_sha=gate.SOURCE_SHA)
            if fault=='source':row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            for arm in ('baseline','candidate'):
                fixtures.write_json(root/'ci-results'/f'{arm}-build-receipt.json',{'status':'success','reused':fault=='cached','selection':gate.MODE})
                names=(gate.PUBLIC_TEST,)+(gate.ON_TESTS[2:] if arm=='candidate' else ())
                (root/'ci-results'/f'{arm}-build.log').write_text(test_output(names),encoding='utf-8')
            probe=root/'candidate'/gate.PROBE_PATH;probe.parent.mkdir(parents=True);probe.write_bytes(Path(gate.__file__).with_name('borrowed_child_keys_probe.rs').read_bytes())
            if fault=='fresh':(root/'target-p13-dense-baseline-plain').mkdir()
            calls=[]
            def fake_environment(target):return {'CARGO_TARGET_DIR':str(target),'RUSTFLAGS':'-Ctarget-cpu=x86-64'}
            def fake_run(argv,cwd,env,stem,timeout,commands,save):
                calls.append((argv,dict(env)));label=Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                _,storage,arm,kind=label.split('-');on=arm=='candidate';compact=storage=='compact';observer=kind=='observer'
                flags,expected=gate.variant_features(arm,compact,observer)
                stdout='';stderr='';is_cargo=argv[0]=='cargo'
                if is_cargo:
                    self.assertEqual(argv[argv.index('--features')+1],flags)
                    packages={'lab-engine':('lib-lab_engine.json',),'lab-search':('lib-lab_search.json',)}
                    if argv[1]=='test':
                        self.assertNotIn('--release',argv);self.assertEqual(env['CARGO_PROFILE_TEST_OPT_LEVEL'],'0')
                        profile='debug'
                        if '--lib' in argv:
                            names=gate.COMMON_TESTS+(gate.ON_TESTS if on else ())
                            stdout=test_output(names,lib=True,on=on,compact=compact)
                            packages['lab-search']+=('test-lib-lab_search.json',)
                            jsonl(env['LAB_P13_UNIT_RECORDS'],records(True))
                        else:
                            stdout=test_output((gate.PUBLIC_TEST,));packages['lab-search']+=('test-integration-test-borrowed_child_keys.json',)
                            rows=records()
                            if fault=='difference' and on:rows[0]['matrix_bits']=42
                            if fault=='state' and on:rows[0]['before'].pop('debug')
                            jsonl(env['LAB_P13_RECORDS'],rows)
                            if fault=='test' and on:stdout=stdout.replace(' ... ok',' ... ignored')
                    else:
                        self.assertIn('--release',argv);profile='release';packages['lab-search']+=('example-'+gate.PROBE+'.json',)
                        binary=Path(env['CARGO_TARGET_DIR'])/'release/examples'/gate.PROBE;binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'fake observer')
                    for package,kinds in packages.items():
                        for name in kinds:
                            features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                            if fault=='feature' and on and package=='lab-search':features.remove(ci.BORROWED_FEATURE)
                            fixtures.write_json(Path(env['CARGO_TARGET_DIR'])/profile/'.fingerprint'/(package+'-mock')/name,{'features':json.dumps(features)})
                else:
                    value=outputs.valid_output()
                    if fault=='work' and on:value['stats']['nodes']+=1
                    stdout=json.dumps(value)+'\n'
                    counts=dict(key_captures=0 if on else 8,job_captures=3,borrowed_queries=8 if on else 0,seen_hits=1,seen_collisions=0,seen_links=3 if on else 0)
                    if fault=='activation' and on:counts['borrowed_queries']=0
                    if fault=='equation' and on:counts['job_captures']=4
                    stderr='P13 activation: '+json.dumps(counts)+'\n'
                stem.with_suffix('.stdout').write_text(stdout,encoding='utf-8',newline='\n');stem.with_suffix('.stderr').write_text(stderr,encoding='utf-8',newline='\n')
                commands.append({'argv':argv,'returncode':0});save()
            cases=[{'name':name,'scenario':'engine/jobs/'+name+'.json','position':'initial'} for name in ('coaching','sand')]
            with patch.object(gate,'verify_declarations',return_value={'pinned':True}),patch.object(gate.process,'environment',side_effect=fake_environment),patch.object(gate.process,'_run',side_effect=fake_run),patch.object(gate.bench,'load_cases',return_value=(cases,{})):
                if fault:
                    with self.assertRaises((ValueError,KeyError)):gate.validate(root)
                    receipt=json.loads((root/'ci-results/borrowed-child-keys-validation/receipt.json').read_text())
                    self.assertEqual(receipt['status'],'failed');self.assertIn('error',receipt)
                else:
                    receipt=gate.validate(root);self.assertEqual(receipt['status'],'success')
                    self.assertEqual(len(receipt['variants']),8);self.assertTrue(receipt['actual_public_search_activated'])
                    self.assertEqual(len(calls),18);self.assertEqual(sum('--release' in a for a,e in calls),2)
                    self.assertEqual(receipt['bounded_profile'],'debug opt0')
                    self.assertEqual(receipt['public_activation_profile'],'release opt3')

    def test_complete_fresh_matrix_public_proofs_and_feature_fingerprints(self):self.run_gate()
    def test_failure_receipts_are_preserved_for_source_cache_records_features_activation(self):
        for fault in ('source','cached','fresh','difference','state','test','feature','work','activation','equation'):
            with self.subTest(fault=fault):self.run_gate(fault)


if __name__=='__main__':unittest.main()
