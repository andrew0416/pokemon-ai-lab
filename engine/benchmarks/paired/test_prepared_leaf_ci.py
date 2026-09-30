"""Synthetic CI failure injections, not engine correctness/performance results."""
import itertools
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import build_cache
import ci
import prepared_leaf as gate
import test_p8def_ci as fixtures
import test_run as outputs


def counts(on):
    return {'prepared_leaf':dict(requests=6 if on else 0,declined=0,batches=6 if on else 0,errors=0),
            'validation':[4,8,4] if on else [10,20,10],
            'leaf':dict(batches=10,visits=20,materialized_outcomes=0,emitted_instructions=0),
            'borrowed':dict(key_captures=0,job_captures=5,borrowed_queries=20,seen_hits=1,seen_collisions=0,seen_links=5)}


class RoutingTests(unittest.TestCase):
    def test_R1_in_both_arms_candidate_adds_only_P15(self):
        r1=ci.feature_args(ci.BORROWED_MODE,'candidate')
        self.assertEqual(ci.feature_args(gate.MODE,'baseline'),r1)
        self.assertEqual(ci.feature_args(gate.MODE,'candidate'),['--features',r1[1]+',lab-search/'+ci.PL_FEATURE])
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            for package in expected:
                self.assertTrue(expected[package][ci.BORROWED_FEATURE])
                self.assertEqual(expected[package][ci.PL_FEATURE],arm=='candidate')
                for flag in (ci.PL_OBSERVER,ci.BORROWED_OBSERVER,ci.MATRIX_FEATURE,ci.MATRIX_OBSERVER,ci.NEW_FEATURES['slot-diff'],ci.NEW_FEATURES['stats-off-cost']):
                    self.assertFalse(expected[package][flag])
            commands=ci.build_commands('narrow',gate.MODE,arm)
            self.assertIn('--release',commands[0]);self.assertNotIn('observer',' '.join(commands[1]))
        self.assertEqual(set(ci.injected_sources(gate.MODE)),{'engine/search/examples/ci_bench.rs','engine/scenario/examples/ci_compact_probe.rs',gate.PROBE_PATH})

    def test_closure_rejects_missing_P13_extra_runtime_and_observers(self):
        for arm in ('baseline','candidate'):
            _,expected,_=ci.fingerprint_expectations(gate.MODE,arm)
            for package in expected:
                features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                for rogue in ('experiment-inline-runstart','experiment-volatile-hash-update',ci.MATRIX_FEATURE,ci.PL_OBSERVER,ci.BORROWED_OBSERVER,'unknown'):
                    with self.subTest(package=package,rogue=rogue),self.assertRaises(ValueError):ci.validate_strict_feature_closure(package,features+[rogue],expected[package])
                with self.assertRaises(ValueError):ci.validate_strict_feature_closure(package,[f for f in features if f!=ci.BORROWED_FEATURE],expected[package])

    def test_refs_freeze_same_source_one_thread_ten_pairs(self):
        with patch.object(gate,'SOURCE_SHA','d'*40):
            for change in ({},{'THREADS':'2'},{'PAIRS':'2'},{'BASELINE_SHA':'a'*40,'CANDIDATE_SHA':'a'*40}):
                with tempfile.TemporaryDirectory() as directory:
                    root=Path(directory)
                    env={'BASELINE_SHA':gate.SOURCE_SHA,'CANDIDATE_SHA':gate.SOURCE_SHA,'GITHUB_SHA':'c'*40,'CANDIDATE_FEATURE':gate.MODE,'SUITE':'narrow','THREADS':'1','PAIRS':'10','GITHUB_RUN_ID':'123','GITHUB_RUN_ATTEMPT':'1','GITHUB_OUTPUT':str(root/'output'),**change}
                    with patch.dict(os.environ,env,clear=True):
                        if change:
                            with self.assertRaisesRegex(ValueError,'P15 requires frozen'):ci.refs(root)
                        else:ci.refs(root)

    def test_default_alias_and_source_mismatch_fail_before_work(self):
        for flag in (ci.PL_FEATURE,ci.PL_OBSERVER):
            with self.assertRaises(ValueError):ci.reject_default_experiments(Path('Cargo.toml'),{'default':['alias'],'alias':[flag]})
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE);row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            with patch.object(ci,'output') as calls,self.assertRaises(ValueError):ci.prepare(root)
            calls.assert_not_called()

    def test_old_binary_cache_never_substitutes_for_new_regressions(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);fixtures.request(root,gate.MODE)
            with patch.dict(os.environ,{'BUILD_CACHE_ENABLED':'1','BASELINE_CACHE_HIT':'true','GITHUB_REPOSITORY':build_cache.TRUSTED_REPOSITORY}),patch.object(build_cache,'_current_plan') as calls:
                self.assertFalse(build_cache.restore(root,'baseline'))
            calls.assert_not_called()

    def test_cache_identity_binds_new_probe_and_rejects_mutation(self):
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
                self.assertEqual(recipe['source']['prepared_leaf_probe_sha256'],ci.sha(source/gate.PROBE_PATH))
                self.assertEqual(recipe['identity']['build_driver']['prepared_leaf_probe.rs'],ci.sha(source/gate.PROBE_PATH))
                (source/gate.PROBE_PATH).write_text('changed',encoding='utf-8')
                with self.assertRaisesRegex(ValueError,'P15 observer probe'):build_cache.make_recipe(root,'baseline')

    def test_observer_and_storage_axes_preserve_all_R1_flags(self):
        for arm,compact,observer in itertools.product(('baseline','candidate'),(False,True),(False,True)):
            flags,expected=gate.variant_features(arm,compact,observer)
            self.assertEqual(expected['lab-engine'][ci.COMPACT_FEATURE],compact)
            for package in expected:
                self.assertTrue(expected[package][ci.BORROWED_FEATURE])
                self.assertEqual(expected[package][ci.PL_FEATURE],arm=='candidate')
                for flag in (ci.PL_OBSERVER,ci.PREPARED_OBSERVER_FEATURE,ci.OBSERVER_FEATURE):self.assertEqual(expected[package][flag],observer)
            self.assertFalse(expected['lab-engine'][ci.BORROWED_OBSERVER])
            self.assertEqual(expected['lab-search'][ci.BORROWED_OBSERVER],observer)
            self.assertEqual(('lab-search/'+ci.PL_OBSERVER) in flags.split(','),observer)

    def test_declaration_binding_and_independent_observer_are_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            manifests={
                'core':{ci.PL_FEATURE:[ci.PREPARED_FEATURE,ci.LEAF_FEATURE],ci.PL_OBSERVER:[ci.PREPARED_OBSERVER_FEATURE,ci.OBSERVER_FEATURE],ci.BORROWED_FEATURE:[]},
                'search':{ci.PL_FEATURE:[ci.PREPARED_FEATURE,ci.LEAF_FEATURE,'lab-engine/'+ci.PL_FEATURE],ci.PL_OBSERVER:[ci.PREPARED_OBSERVER_FEATURE,ci.OBSERVER_FEATURE,'lab-engine/'+ci.PL_OBSERVER],ci.BORROWED_FEATURE:['lab-engine/'+ci.BORROWED_FEATURE],ci.BORROWED_OBSERVER:[]},
                'scenario':{}}
            for name,features in manifests.items():
                value='[features]\ndefault=[]\n'+''.join(k+'='+json.dumps(v)+'\n' for k,v in features.items())
                if name=='search':value+='[[test]]\nname="prepared_leaf"\npath="tests/prepared_leaf.rs"\n'
                path=root/f'engine/{name}/Cargo.toml';path.parent.mkdir(parents=True);path.write_text(value,encoding='utf-8')
            path=root/'engine/search/Cargo.toml';pins={'engine/search/Cargo.toml':ci.sha(path)}
            with patch.object(gate,'SOURCE_SHA','d'*40),patch.object(gate,'SOURCE_FILES',pins):
                self.assertTrue(gate.verify_declarations(root)['default_off'])
                original=path.read_text(encoding='utf-8')
                for changed in (original.replace('default=[]',f'default=["{ci.PL_FEATURE}"]'),original.replace(ci.PL_OBSERVER+'=[',ci.PL_OBSERVER+'=["'+ci.PL_FEATURE+'",'),original+'# modified\n'):
                    path.write_text(changed,encoding='utf-8')
                    with self.assertRaises(ValueError):gate.verify_declarations(root)


class CounterTests(unittest.TestCase):
    def test_actual_sharing_preserves_P9_and_P13_work_and_reduces_all_validators(self):
        before,after=counts(False),counts(True)
        for on,row in ((False,before),(True,after)):gate.validate_counts('P15 activation: '+json.dumps(row),on)
        gate.compare_counts(before,after)
        for scope,key in (('leaf','visits'),('borrowed','job_captures')):
            value=counts(True);value[scope][key]+=1
            with self.assertRaises(ValueError):gate.compare_counts(before,value)
        for value in ([10,20,10],[4,20,4],[11,8,4]):
            changed=counts(True);changed['validation']=value
            with self.assertRaises(ValueError):gate.compare_counts(before,changed)

    def test_missing_zero_fake_or_unbalanced_observer_is_rejected(self):
        for scope,key,replacement in (('prepared_leaf','batches',0),('prepared_leaf','requests',99),('borrowed','borrowed_queries',0),('borrowed','key_captures',1),('leaf','visits',0)):
            value=counts(True);value[scope][key]=replacement
            with self.assertRaises(ValueError):gate.validate_counts('P15 activation: '+json.dumps(value),True)
        for text,on in (('',True),('P15 activation: '+json.dumps(counts(False)),True),('P15 activation: '+json.dumps(counts(True)),False)):
            with self.assertRaises(ValueError):gate.validate_counts(text,on)


def bounded(on):
    return {'ordinary':[16,32,16],'prepared':[1,8,1] if on else [16,32,16],
            'p15':dict(requests=16 if on else 0,batches=16 if on else 0,declined=0,errors=0),
            'leaf':dict(batches=16,visits=16,materialized_outcomes=0,emitted_instructions=0)}


def test_output(names,*,on=False,observer=False,filtered=0):
    text='\n'.join('test '+name+' ... ok' for name in names)+'\n'
    if gate.PUBLIC_TEST in names:text+='P15 PUBLIC CASES 54\n'
    if observer:text+='P15_ACTIVATION '+json.dumps(bounded(on))+'\n'
    return text+f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out; finished in 0.10s\n'


def records():
    identities=[]
    for slots in (1,2):
        for side,chance,mode,depth in itertools.product(('One','Two'),('Expect','Worst'),('exact','mixed'),(1,2)):
            identities.append((f'toy-{slots}-{side}-{chance}-{mode}-{depth}',slots,mode))
        for kind in ('full','factored','parallel','extremes','pessimistic'):
            identities.append((f'fallback-{slots}-{kind}',slots,'mixed'))
        identities += [(f'budget-{slots}-{n}',slots,'mixed') for n in (0,1)]
        identities += [(f'{kind}-{slots}',slots,'mixed') for kind in ('terminal','unsupported')]
        identities.append((f'deep-{slots}',slots,'deep'))
    identities += [('successful-resume',2,'nash'),('successful-replacement',1,'nash')]
    rows=[]
    for identity,slots,mode in identities:
        state=dict(debug='complete State debug',hash=1,position_hash=2)
        eq=dict(rows=[1065353216],cols=[1065353216],value=0,exploitability=0,iterations=16)
        ok={'exact':dict(debug='complete Analysis debug',value=0,lines=[0]),
            'mixed':dict(debug='complete Mixed debug',matrix=[0],equilibrium=eq,maximin=0),
            'deep':dict(debug='complete Deep debug',matrix=[0],equilibrium=eq,shallow=eq),
            'nash':dict(value=0)}[mode]
        rows.append(dict(id=identity,slots=slots,mode=mode,signature=dict(before=state,after=copy.deepcopy(state),
                    suspension='None',result={'ok':ok},stats=[1,2,3,4,5,6,7])))
    return rows


def jsonl(path,rows):
    Path(path).write_text(''.join(json.dumps(row,sort_keys=True)+'\n' for row in rows),encoding='utf-8',newline='\n')


class EvidenceParserTests(unittest.TestCase):
    def test_exact_names_markers_and_conditional_observer(self):
        for on,observer in itertools.product((False,True),(False,True)):
            names=(gate.PUBLIC_TEST,)+(gate.ON_TESTS if on else ())+((gate.OBS_TEST,) if observer else ())
            text=test_output(names,on=on,observer=observer)
            self.assertEqual(gate.validate_tests(text,names)['passed'],len(names))
            self.assertEqual(gate.validate_bounded_activation(text,on,observer),bounded(on) if observer else None)
            for changed in (text.replace(' ... ok',' ... ignored',1),text.replace('P15 PUBLIC CASES 54','P15 PUBLIC CASES 53'),text+text,text.replace('0 filtered out','1 filtered out')):
                with self.assertRaises(ValueError):gate.validate_tests(changed,names)
        for on in (False,True):
            for change in ('absent','twice','not-reduced','leaf-zero','invalid-accounting'):
                value=bounded(on)
                if change=='not-reduced':value['prepared']=[16,32,16] if on else [1,8,1]
                if change=='leaf-zero':value['leaf']['visits']=0
                if change=='invalid-accounting':value['p15']['requests']+=1
                line='P15_ACTIVATION '+json.dumps(value)+'\n'
                text='' if change=='absent' else line*2 if change=='twice' else line
                with self.subTest(on=on,change=change),self.assertRaises(ValueError):gate.validate_bounded_activation(text,on,True)
                if change=='not-reduced':
                    with self.assertRaises(ValueError):gate.validate_bounded_activation(text,on,False)
        gate.validate_tests(test_output(gate.PRIVATE_TESTS,filtered=10),gate.PRIVATE_TESTS,filtered=True)
        gate.validate_tests(test_output(gate.CORE_TESTS,filtered=100),gate.CORE_TESTS,filtered=True)

    def test_public_schema_case_order_and_full_bits_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'records.jsonl';jsonl(path,records())
            self.assertEqual(gate.validate_records(path)['records'],54)
            changes=[lambda r:r.pop(),lambda r:r.append(r[0]),lambda r:r.reverse(),
                     lambda r:r[0].pop('signature'),lambda r:r[-2].update(slots=1),
                     lambda r:r[0]['signature']['after'].update(hash=9),
                     lambda r:r[0]['signature']['before'].pop('debug'),
                     lambda r:r[0]['signature'].update(stats=[0]),
                     lambda r:r[0]['signature']['result']['ok'].update(value=True),
                     lambda r:r[2]['signature']['result']['ok']['equilibrium'].pop('cols'),
                     lambda r:r[2]['signature']['result']['ok'].update(matrix=[-1]),
                     lambda r:r[-1]['signature'].update(result={'error':'failed'})]
            for index,change in enumerate(changes):
                rows=records();change(rows);jsonl(path,rows)
                with self.subTest(change=index),self.assertRaises(ValueError):gate.validate_records(path)
            jsonl(path,records());path.write_bytes(path.read_bytes().replace(b'\n',b'\r\n'))
            with self.assertRaises(ValueError):gate.validate_records(path)


class CompleteGateTests(unittest.TestCase):
    def run_gate(self,fault=None):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);row=fixtures.request(root,gate.MODE)
            row.update(baseline_sha=gate.SOURCE_SHA,candidate_sha=gate.SOURCE_SHA)
            if fault=='source':row['candidate_sha']='b'*40
            fixtures.write_json(root/'ci-results/request.json',row)
            for arm in ('baseline','candidate'):
                fixtures.write_json(root/'ci-results'/f'{arm}-build-receipt.json',{'status':'success','reused':fault=='cached','selection':gate.MODE})
                names=(gate.PUBLIC_TEST,)+(gate.ON_TESTS+gate.PRIVATE_TESTS if arm=='candidate' else ())
                if fault=='release-proof' and arm=='candidate':names=names[:-1]
                (root/'ci-results'/f'{arm}-build.log').write_text(test_output(names),encoding='utf-8')
            probe=root/'candidate'/gate.PROBE_PATH;probe.parent.mkdir(parents=True);probe.write_bytes(Path(gate.__file__).with_name('prepared_leaf_probe.rs').read_bytes())
            if fault=='fresh':(root/'target-p15-dense-baseline-observer').mkdir()
            calls=[]
            def fake_environment(target):return {'CARGO_TARGET_DIR':str(target),'RUSTFLAGS':'-Ctarget-cpu=x86-64'}
            def fake_run(argv,cwd,env,stem,timeout,commands,save):
                calls.append((argv,dict(env)));label=Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                _,storage,arm,kind=label.split('-');on=arm=='candidate';compact=storage=='compact';observer=kind=='observer'
                flags,expected=gate.variant_features(arm,compact,observer)
                stdout='';stderr=''
                if argv[0]=='cargo':
                    core=argv[argv.index('-p')+1]=='lab-engine'
                    required_flags=','.join('lab-engine/'+name for name,active in expected['lab-engine'].items() if active) if core else flags
                    self.assertEqual(argv[argv.index('--features')+1],required_flags)
                    packages={'lab-engine':('lib-lab_engine.json',)}
                    if not core:packages['lab-search']=('lib-lab_search.json',)
                    if argv[1]=='test':
                        self.assertNotIn('--release',argv);self.assertEqual(env['CARGO_PROFILE_TEST_OPT_LEVEL'],'0');profile='debug'
                        if core:
                            stdout=test_output(gate.CORE_TESTS,filtered=100)
                            packages['lab-engine']+=('test-lib-lab_engine.json',)
                            if fault=='core-test' and on:stdout=stdout.replace(' ... ok',' ... ignored',1)
                        elif '--lib' in argv:
                            stdout=test_output(gate.PRIVATE_TESTS,filtered=100)
                            packages['lab-search']+=('test-lib-lab_search.json',)
                            if fault=='private-test':stdout=stdout.replace(' ... ok',' ... ignored',1)
                        else:
                            names=(gate.PUBLIC_TEST,)+(gate.ON_TESTS if on else ())+((gate.OBS_TEST,) if observer else ())
                            stdout=test_output(names,on=on,observer=observer)
                            packages['lab-search']+=('test-integration-test-prepared_leaf.json',)
                            rows=records()
                            if fault=='difference' and on:rows[0]['signature']['stats'][0]+=1
                            if fault=='state' and on:rows[0]['signature']['before'].pop('debug')
                            jsonl(env['LAB_P15_RECORDS'],rows)
                            if fault=='test' and on:stdout=stdout.replace(' ... ok',' ... ignored')
                            if fault=='bounded' and on and observer:stdout=stdout.replace('P15_ACTIVATION','MISSING_ACTIVATION')
                    else:
                        self.assertIn('--release',argv);profile='release';packages['lab-search']+=('example-'+gate.PROBE+'.json',)
                        binary=Path(env['CARGO_TARGET_DIR'])/'release/examples'/gate.PROBE;binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'fake observer')
                    for package,kinds in packages.items():
                        for name in kinds:
                            features=[k for k,v in expected[package].items() if v]+sorted(ci.PACKAGE_BASE_FEATURES[package])
                            if fault=='feature' and on and package=='lab-search':features.remove(ci.PL_FEATURE)
                            if fault=='P13-feature' and on and package=='lab-engine':features.remove(ci.BORROWED_FEATURE)
                            fixtures.write_json(Path(env['CARGO_TARGET_DIR'])/profile/'.fingerprint'/(package+'-mock')/name,{'features':json.dumps(features)})
                else:
                    value=outputs.valid_output()
                    if fault=='work' and on:value['stats']['nodes']+=1
                    stdout=json.dumps(value)+'\n';value=counts(on)
                    if fault=='activation' and on:value=counts(False)
                    if fault=='validation' and on:value['validation']=[10,20,10]
                    if fault=='P13-work' and on:value['borrowed']['borrowed_queries']+=1
                    if fault=='leaf-work' and on:value['leaf']['visits']+=1
                    stderr='P15 activation: '+json.dumps(value)+'\n'
                stem.with_suffix('.stdout').write_text(stdout,encoding='utf-8',newline='\n');stem.with_suffix('.stderr').write_text(stderr,encoding='utf-8',newline='\n')
                commands.append({'argv':argv,'returncode':0});save()
            cases=[dict(name=name,scenario='engine/jobs/'+name+'.json',position='initial') for name in ('coaching','sand')]
            with patch.object(gate,'verify_declarations',return_value={'pinned':True}),patch.object(gate.process,'environment',side_effect=fake_environment),patch.object(gate.process,'_run',side_effect=fake_run),patch.object(gate.bench,'load_cases',return_value=(cases,{})):
                if fault:
                    with self.assertRaises((ValueError,KeyError)):gate.validate(root)
                    receipt=json.loads((root/'ci-results/prepared-leaf-validation/receipt.json').read_text())
                    self.assertEqual(receipt['status'],'failed');self.assertIn('error',receipt)
                else:
                    receipt=gate.validate(root)
                    self.assertEqual(receipt['status'],'success');self.assertEqual(len(receipt['variants']),6)
                    self.assertTrue(receipt['actual_prepared_leaf_activation']);self.assertTrue(receipt['complete_jsonl_byte_equal'])
                    self.assertTrue(receipt['R1_P13_activation_preserved'])
                    self.assertEqual(len(calls),17);self.assertEqual(sum('--release' in argv for argv,env in calls),2)

    def test_complete_gate_and_actual_feature_evidence(self):self.run_gate()
    def test_source_cache_named_proof_features_outputs_and_counters_fail_with_receipts(self):
        for fault in ('source','cached','release-proof','fresh','difference','state','test','private-test','core-test','bounded','feature','P13-feature','work','activation','validation','P13-work','leaf-work'):
            with self.subTest(fault=fault):self.run_gate(fault)


if __name__=='__main__':unittest.main()
