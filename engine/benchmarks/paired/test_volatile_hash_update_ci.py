"""P12 fail-closed controller checks. No Cargo or engine process is run locally."""
import copy
import contextlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

import ci
import build_cache
import compact_probe
import volatile_hash_update as gate
import p12_logic as logic
import test_p8def_ci as fixtures
import test_build_cache as cache_fixtures

MODE='volatile-hash-update'

def output_tests(names):
    return ''.join('test '+n+' ... ok\n' for n in names)+f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'

def outcome(): return {'probability_bits':1,'instructions':'[]','suspension':'None','state':'full State','hash':3}

def records(name):
    if name=='turns.jsonl':
        rows=[]
        for case,rolls,factored in sorted(gate.TURN_CONFIGS):
            for i in range(2 if case=='single-hit' else 1):
                rows.append({'case':case,'position':i,'rolls':rolls,'factored':factored,
                             'before':'full State','outcomes':[outcome()]})
        rows[0]['outcomes']=[outcome() for _ in range(1096-len(rows)+1)]
        rows.append({'kind':'coverage','positions':10,'outcomes':1096})
    else:
        rows=[{'case':'aa-power-construct-faint','position':0,'before':'full State','result':{'error':'expected'}},
              {'case':'aa-power-construct-faint','position':1,'before':'full State','result':{'outcomes':[outcome()]}}]
        rows += [{'case':'sample','position':i,'seed':s,'outcomes':[outcome()]} for i in (0,1) for s in (0,7,42)]
        rows.append({'kind':'coverage','errors':1,'successes':1,'samples':6})
    return rows

def write_records(path,rows): path.write_text(''.join(json.dumps(r)+'\n' for r in rows),encoding='utf8',newline='\n')

def activation():
    return {'schema':1,'kind':'p12-real-instruction-proof','cases':gate.PROBE_CASES,
        'positions':6,'outcomes':20,'volatile_updates':50,'original_locations':150,'candidate_locations':50,
        'original_ranks':120,'candidate_ranks':50,'enumeration_locations':800,'enumeration_ranks':300,
        'full_state_and_hash_equal':True,'rollback_equal':True}

class Routing(unittest.TestCase):
    def test_cache_identity_accepts_only_exact_injections_and_binds_observer(self):
        fixture=cache_fixtures.CacheRecipeInputTests()
        fixture.setUp();self.addCleanup(fixture.doCleanups)
        extra=build_cache.COMPACT_CONTROLLER_FILES+('volatile_hash_update.py','p12_logic.py','p12-source.json','p12_observer_probe.rs')
        for name in extra:(fixture.controller/name).write_bytes(name.encode()+b' P12 controller\n')
        for relative,name in ((ci.COMPACT_PROBE_PATH,'compact_probe.rs'),(gate.PROBE_SOURCE,'p12_observer_probe.rs')):
            path=fixture.root/'baseline'/relative;path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes((fixture.controller/name).read_bytes());fixture.untracked.append(relative)
        request_path=fixture.root/'ci-results/request.json'
        request=cache_fixtures.read_json(request_path);request['candidate_feature']=MODE
        cache_fixtures.write_json(request_path,request)
        first=build_cache.make_recipe(fixture.root,'baseline')
        probe=fixture.root/'baseline'/gate.PROBE_SOURCE
        self.assertEqual(first['source']['p12_observer_probe_sha256'],build_cache.digest(probe))
        fixture.untracked.append('engine/scenario/examples/unexpected.rs')
        with self.assertRaisesRegex(ValueError,'unexpected untracked'):build_cache.make_recipe(fixture.root,'baseline')
        fixture.untracked.pop();original=probe.read_bytes();probe.write_bytes(original+b' mutated\n')
        with self.assertRaisesRegex(ValueError,'Injected P12 observer'):build_cache.make_recipe(fixture.root,'baseline')
        (fixture.controller/'p12_observer_probe.rs').write_bytes(probe.read_bytes())
        self.assertNotEqual(build_cache.recipe_key(first),build_cache.recipe_key(build_cache.make_recipe(fixture.root,'baseline')))
        probe.unlink()
        with self.assertRaisesRegex(ValueError,'Injected P12 observer'):build_cache.make_recipe(fixture.root,'baseline')

    def test_only_common5_plus_p12_and_full_fresh_regressions(self):
        base=ci.feature_args('p8d-vs-p8def','baseline')
        self.assertEqual(ci.feature_args(MODE,'baseline'),base)
        self.assertEqual(ci.feature_args(MODE,'candidate'),['--features',base[1]+',lab-engine/'+logic.RUNTIME])
        for arm in ('baseline','candidate'):
            cmd=ci.build_commands('narrow',MODE,arm)
            self.assertEqual(cmd[0][:10],['cargo','test','--locked','--release','-p','lab-engine','-p','lab-scenario','-p','lab-search'])
            self.assertNotIn('observe',' '.join(cmd[1]))
            with self.assertRaises(ValueError):ci.build_commands('smoke',MODE,arm)
        self.assertIn(logic.RUNTIME,ci.prepared_validation_command(MODE)[-1])
        self.assertEqual(set(ci.injected_sources(MODE)),{'engine/search/examples/ci_bench.rs',ci.COMPACT_PROBE_PATH,gate.PROBE_SOURCE})

    def test_timing_rejects_every_feature_flip_and_future_experiment(self):
        for arm in ('baseline','candidate'):
            packages,expected,_=ci.fingerprint_expectations(MODE,arm)
            with tempfile.TemporaryDirectory() as d:
                root=Path(d);fixtures.request(root,MODE)
                for package,names in packages.items():
                    for name in names:
                        for feature in (*ci.ALL_EXPERIMENT_FEATURES,'experiment-inline-runstart','experiment-borrowed-child-keys'):
                            fixtures.fingerprints(root,arm,packages,expected,(package,name,feature))
                            with self.assertRaises(ValueError):ci.preserve_fingerprints(root,arm,MODE)
                fixtures.fingerprints(root,arm,packages,expected)
                self.assertEqual(len(ci.preserve_fingerprints(root,arm,MODE)['fingerprints']),5)

    def test_same_source_and_no_build_cache_bypass(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);row=fixtures.request(root,MODE)
            for arm in ('baseline','candidate'):
                with patch.dict(os.environ,{'BUILD_CACHE_ENABLED':'1','GITHUB_REPOSITORY':build_cache.TRUSTED_REPOSITORY,
                        arm.upper()+'_CACHE_HIT':'true'}),patch.object(build_cache,'_current_plan') as calls:
                    self.assertFalse(build_cache.restore(root,arm));calls.assert_not_called()
            row['candidate_sha']='b'*40;fixtures.write_json(root/'ci-results/request.json',row)
            with patch.object(ci.subprocess,'Popen') as calls,self.assertRaises(ValueError):ci.build(root)
            calls.assert_not_called()
        with self.assertRaises(ValueError):compact_probe.slot_diff_semantic_output(Path('unused'),MODE)

    def test_nine_arm_logic_excludes_e_f_p11_p13_and_local_cargo(self):
        self.assertEqual(len(logic.ARMS),9)
        self.assertEqual(logic.ARMS['common5-off'],[ci.EXPERIMENT_FEATURE,ci.PREPARED_FEATURE,ci.LEAF_FEATURE,
                                                  ci.COMPACT_FEATURE,ci.NEW_FEATURES['replay-action-keys']])
        for features in logic.ARMS.values():
            self.assertFalse(set(features)&{'experiment-slot-diff','experiment-stats-off-cost',
                                            'experiment-inline-runstart','experiment-borrowed-child-keys'})
        with patch.dict(os.environ,{},clear=True),patch('sys.argv',['p12_logic']),patch.object(logic.subprocess,'run') as calls:
            with self.assertRaises(SystemExit):logic.main()
            calls.assert_not_called()

class Evidence(unittest.TestCase):
    def setup_wrapper(self,root):
        r=fixtures.request(root,MODE)
        r.update(baseline_sha=gate.SOURCE,candidate_sha=gate.SOURCE,suite='narrow',threads=1,pairs=10)
        fixtures.write_json(root/'ci-results/request.json',r)
        for arm in ('baseline','candidate'):
            names=logic.CORE_TESTS+logic.SCENARIO_TESTS
            if arm=='candidate':names=names+logic.UNIT_TESTS+[gate.COMPACT_UNIT]
            fixtures.write_json(root/f'ci-results/{arm}-build-receipt.json',{'status':'success','reused':False,'selection':MODE})
            (root/f'ci-results/{arm}-build.log').write_text(output_tests(names))
        probe=root/'candidate'/gate.PROBE_SOURCE;probe.parent.mkdir(parents=True)
        probe.write_bytes((gate.HERE/'p12_observer_probe.rs').read_bytes())

    def run_wrapper(self,root,fault=None):
        def child(argv,cwd,env,stem,timeout,commands,save):
            commands.append({'argv':argv});text=''
            self.assertEqual(cwd,root/'candidate/engine')
            self.assertNotIn('LAB_ENGINE_STATS',env);self.assertNotIn('LAB_P12_RECORDS',env)
            if argv[0]=='cargo':
                self.assertIn('ci_p12_observer',argv)
                flags=argv[argv.index('--features')+1].split(',')
                self.assertEqual(set(flags),{'lab-engine/'+f for f in logic.COMMON+[logic.RUNTIME]}|{'lab-scenario/'+logic.OBSERVER})
                core={f:f in logic.COMMON+[logic.RUNTIME,logic.OBSERVER] for f in ci.ALL_EXPERIMENT_FEATURES}
                if fault=='extra-feature':core[ci.NEW_FEATURES['slot-diff']]=True
                scenario={f:f==logic.OBSERVER for f in ci.ALL_EXPERIMENT_FEATURES}
                fixtures.fingerprints(root,'p12-real-observer',
                    {'lab-engine':('lib-lab_engine.json',),'lab-scenario':('lib-lab_scenario.json','example-ci_p12_observer.json')},
                    {'lab-engine':core,'lab-scenario':scenario})
                binary=root/'target-p12-real-observer/release/examples/ci_p12_observer'
                binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'mock observer binary')
            elif argv[0].endswith('ci_p12_observer'):
                row=activation()
                if fault=='no-real-reduction':row['candidate_locations']=row['original_locations']
                text=json.dumps(row)+'\n'
            else:
                self.assertTrue(argv[1].endswith('p12_logic.py'))
                self.assertNotIn('--local-logic',argv);self.assertNotIn('--arms',argv)
            stem.with_suffix('.stdout').write_text(text,encoding='utf8',newline='\n')
            stem.with_suffix('.stderr').write_text('',encoding='utf8')
        with patch.dict(os.environ,{'RUSTFLAGS':'-Ctarget-cpu=x86-64','GITHUB_ACTIONS':'true',
                'LAB_ENGINE_STATS':'1','LAB_P12_RECORDS':'bad'},clear=True),\
                patch.object(compact_probe.platform,'system',return_value='Linux'),\
                patch.object(compact_probe.platform,'machine',return_value='x86_64'),\
                patch.object(gate,'verify_declarations',return_value={'source':'pinned'}),\
                patch.object(gate,'validate_logic',return_value={'nine-arm':'mocked'}),\
                patch.object(compact_probe,'_run',side_effect=child):
            return gate.validate(root)

    def test_wrapper_requires_full_regression_and_separate_common5_real_observer(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.setup_wrapper(root);r=self.run_wrapper(root)
            self.assertEqual(r['status'],'success');self.assertEqual(len(r['commands']),3)
            core=r['observer_features']['expected_by_package']['lab-engine']
            self.assertTrue(core[logic.RUNTIME] and core[logic.OBSERVER])
            self.assertFalse(core[ci.NEW_FEATURES['slot-diff']] or core[ci.NEW_FEATURES['stats-off-cost']])
            self.assertIn('enumeration counters separate',r['activation']['scope'])

    def test_wrapper_cannot_accept_feature_leak_or_missing_real_query_reduction(self):
        for fault in ('extra-feature','no-real-reduction'):
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as d:
                root=Path(d);self.setup_wrapper(root)
                with self.assertRaises(ValueError):self.run_wrapper(root,fault)
                r=json.loads((root/'ci-results/volatile-hash-update-validation/receipt.json').read_text())
                self.assertEqual(r['status'],'failed')

    def test_wrapper_refuses_preexisting_target_or_wrong_pinned_request(self):
        for fault in ('target','source','threads','pairs'):
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as d:
                root=Path(d);self.setup_wrapper(root)
                if fault=='target':(root/'target-p12-logic').mkdir()
                else:
                    path=root/'ci-results/request.json';r=json.loads(path.read_text())
                    if fault=='source':r['candidate_sha']='f'*40
                    else:r[fault]=2
                    fixtures.write_json(path,r)
                with self.assertRaises(ValueError):self.run_wrapper(root)

    def run_fake_logic(self,root):
        engine=root/'candidate/engine';engine.mkdir(parents=True)
        output=root/'proof';target=root/'target';binaries={};calls=[]
        original_digest=logic.digest
        def digest(path):
            try: name=path.relative_to(engine.parent).as_posix()
            except ValueError: return original_digest(path)
            return gate.PINS['file_sha256'][name]
        def fake(argv,**kwargs):
            calls.append(argv)
            if argv[0]=='cargo':
                names=argv[argv.index('--features')+1].split(',') if '--features' in argv else []
                features=sorted({name.rsplit('/',1)[-1] for name in names})
                arm=next(arm for arm,flags in logic.ARMS.items() if set(flags)==set(features))
                package=argv[argv.index('-p')+1]
                if '--lib' in argv:
                    tests=['existing_unit']+(logic.UNIT_TESTS if logic.RUNTIME in features else [])
                    if logic.RUNTIME in features and logic.COMPACT in features:tests=tests+[gate.COMPACT_UNIT]
                    text=output_tests(tests)
                else:
                    target.mkdir(exist_ok=True);binary=target/(arm+'-'+package)
                    binary.write_bytes((arm+package).encode());binaries[str(binary)]=(arm,package,features)
                    def artifact(name,flags,executable=None):
                        return {'reason':'compiler-artifact','target':{'name':name},'features':flags,'executable':executable}
                    events=[artifact('lab_engine',features)]
                    test_flags=features
                    if package=='lab-scenario':
                        test_flags=[logic.OBSERVER] if logic.OBSERVER in features else []
                        events.append(artifact('lab_scenario',test_flags))
                    events.append(artifact('volatile_hash_update',test_flags,str(binary)))
                    text=''.join(json.dumps(event)+'\n' for event in events)
            else:
                arm,package,features=binaries[argv[0]]
                tests=list(logic.CORE_TESTS if package=='lab-engine' else logic.SCENARIO_TESTS)
                if package=='lab-engine' and logic.OBSERVER in features:tests.append(gate.OBSERVER_TEST)
                if package=='lab-scenario':
                    for name in ('turns.jsonl','errors-samples.jsonl'):
                        write_records(Path(kwargs['env']['LAB_P12_RECORDS'])/name,records(name))
                text=output_tests(tests)
            return SimpleNamespace(returncode=0,stdout=text)
        with patch.dict(os.environ,{'GITHUB_ACTIONS':'true'}),patch('sys.argv',['p12_logic','--engine',str(engine),
                '--output',str(output),'--target',str(target)]),patch.object(logic,'digest',side_effect=digest),\
                patch.object(logic.subprocess,'run',side_effect=fake),contextlib.redirect_stdout(io.StringIO()):
            logic.main()
        self.assertEqual(len(calls),45)
        return output,target

    def test_actual_logic_runner_and_receipt_validator_execute_all_nine_mocked_arms(self):
        with tempfile.TemporaryDirectory() as d:
            output,target=self.run_fake_logic(Path(d))
            proof=gate.validate_logic(output,target)
            self.assertEqual(len(proof['comparisons']),6)
            self.assertEqual(len(proof['counts']),27)
            for arm in logic.ARMS:
                self.assertIn(arm,proof['records'])

    def test_logic_receipt_cannot_hide_wrong_feature_source_skip_filter_or_binary(self):
        with tempfile.TemporaryDirectory() as d:
            output,target=self.run_fake_logic(Path(d));path=output/'receipt.json'
            original=json.loads(path.read_text())
            mutations=[lambda r:r.update(passed=False),lambda r:r.update(ci_only=False),
                lambda r:r['selected_arms'].pop(),lambda r:r['source_files'].pop(next(iter(r['source_files']))),
                lambda r:r['compiled_features']['common5-on/lab-engine'].append('experiment-slot-diff'),
                lambda r:r['commands'].pop(),lambda r:r['commands'][0].update(exit_code=1),
                lambda r:r['commands'][0]['command'].append('only_one_test'),
                lambda r:r['artifacts']['dense-off/lab-engine'].update(sha256='f'*64),
                lambda r:r['comparisons'].pop()]
            for mutation in mutations:
                r=copy.deepcopy(original);mutation(r);fixtures.write_json(path,r)
                with self.assertRaises(ValueError):gate.validate_logic(output,target)
            fixtures.write_json(path,original)
            record=output/'common5-on-records/turns.jsonl';rows=records('turns.jsonl')
            rows[0]['outcomes'][0]['instructions']='changed';write_records(record,rows)
            with self.assertRaises(ValueError):gate.validate_logic(output,target)

    def test_full_regression_requires_actual_named_off_and_on_tests(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            for arm in ('baseline','candidate'):
                names=logic.CORE_TESTS+logic.SCENARIO_TESTS
                if arm=='candidate': names=names+logic.UNIT_TESTS+[gate.COMPACT_UNIT]
                (root/(arm+'-build.log')).write_text(output_tests(names))
                fixtures.write_json(root/(arm+'-build-receipt.json'),{'status':'success','selection':MODE,'reused':False})
            gate.validate_full_regressions(root)
            for arm in ('baseline','candidate'):
                path=root/(arm+'-build-receipt.json');saved=path.read_bytes()
                row=json.loads(saved);row['reused']=True;fixtures.write_json(path,row)
                with self.assertRaises(ValueError):gate.validate_full_regressions(root)
                path.write_bytes(saved)
            path=root/'candidate-build.log';path.write_text(output_tests(logic.CORE_TESTS+logic.SCENARIO_TESTS))
            with self.assertRaises(ValueError):gate.validate_full_regressions(root)

    def test_named_tests_cannot_be_zero_filtered_skipped_or_repeated(self):
        valid=output_tests(logic.CORE_TESTS);gate.named_tests(valid,logic.CORE_TESTS)
        for text in ('',output_tests([]),valid.replace('0 filtered out','1 filtered out'),
                     valid.replace('0 ignored','1 ignored'),valid+valid,valid.replace(' ... ok',' ... ignored',1)):
            with self.assertRaises(ValueError):gate.named_tests(text,logic.CORE_TESTS)

    def test_exact_record_schema_and_coverage_fail_closed(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            for name in ('turns.jsonl','errors-samples.jsonl'):
                path=root/name;original=records(name);write_records(path,original)
                gate.validate_records(path,name)
                mutations=[lambda rows:rows.pop(),lambda rows:rows[-1].update(kind='partial'),
                    lambda rows:rows[0].pop('before'),lambda rows:rows[-1].update(extra=True)]
                if name=='turns.jsonl':
                    mutations += [lambda rows:rows[0]['outcomes'][0].pop('state'),
                        lambda rows:rows[0]['outcomes'][0].update(probability_bits=True),
                        lambda rows:rows[0].update(factored='false'),lambda rows:rows[-1].update(outcomes=1095)]
                else:
                    mutations += [lambda rows:rows[2].update(seed=99),lambda rows:rows[0]['result'].update(error='')]
                for mutation in mutations:
                    rows=copy.deepcopy(original);mutation(rows);write_records(path,rows)
                    with self.assertRaises(ValueError):gate.validate_records(path,name)

    def test_actual_scenario_queries_and_ranks_cannot_be_synthetic_zero_or_unreduced(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'proof';r=activation();write_records(path,[r]);gate.validate_activation(path)
            changes=[{'volatile_updates':0},{'candidate_locations':150},{'candidate_ranks':120},
                {'enumeration_locations':0},{'enumeration_ranks':0},{'full_state_and_hash_equal':False},
                {'rollback_equal':False},{'positions':True},{'cases':gate.PROBE_CASES[:-1]}]
            for change in changes:
                write_records(path,[dict(r,**change)])
                with self.assertRaises(ValueError):gate.validate_activation(path)

if __name__=='__main__':unittest.main()
