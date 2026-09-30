"""P11 feature isolation and fresh proof gates; all Cargo processes are inert mocks."""
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci
import build_cache
import compact_probe
import inline_runstart as gate
import test_p8def_ci as fixtures
import test_p8def_combined_ci as combined

MODE='inline-runstart'


def proof_rows():
    ending={key: [] for key in ('instructions','suspension','end_state')}
    ending.update(probability_bits=1,instructions_debug='exact',suspension_debug='None',
                  end_state_debug='full',end_position_hash=5)
    rows=[]
    for i,name in enumerate(sorted(gate.CASES)):
        rows.append({'kind':'enumeration','case':name,'slots':1 if i%2 else 2,'factored':bool(i%2),
            'rolls':'Full' if i==1 else 'Median','before_state':[],'before_state_debug':'full',
            'before_position_hash':3,'before_party_order':[],'decision':'Turn','position_probability_bits':1,
            'result':{'error_debug':'expected','error_display':'expected'} if i==0 else
                     {'ok':[{'outcome':copy.deepcopy(ending),'party_order':[]}]} })
    for i in range(9):
        rows.append({'kind':'sample','seed':(0,7,42)[i%3],'sample_count':4,'outcomes':[copy.deepcopy(ending)]})
    rows.append({'kind':'coverage','positions':len(gate.CASES),'outcomes':len(gate.CASES)-1,
                 'errors':1,'successes':len(gate.CASES)-1,'suspended':1,'resumed':1,'sample_calls':9})
    return rows


def output_tests(names,filtered=0):
    return ''.join('test '+name+' ... ok\n' for name in names)+f'test result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out;\n'


def allocator_output():
    return ('test '+gate.ALLOCATION_TEST+' ... ok\n'
            +'test '+gate.ALLOCATION_TEST+' ... '
            +''.join(f'P11 allocation proof: len={n} original={int(n>0)} capture={int(n>4)} clone={int(n>4)}\n' for n in gate.LENGTHS)
            +'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'*2)


class P11RoutingTests(unittest.TestCase):
    def test_common5_and_candidate_only_p11_full_regressions(self):
        common=ci.feature_args('p8d-vs-p8def','baseline')
        self.assertEqual(ci.feature_args(MODE,'baseline'),common)
        self.assertEqual(ci.feature_args(MODE,'candidate'),['--features',common[1]+',lab-engine/'+ci.INLINE_FEATURE])
        for arm in ('baseline','candidate'):
            commands=ci.build_commands('narrow',MODE,arm)
            self.assertEqual(commands[0][:10],['cargo','test','--locked','--release','-p','lab-engine','-p','lab-scenario','-p','lab-search'])
            self.assertNotIn('observe',' '.join(commands[1]))
            with self.assertRaises(ValueError):ci.build_commands('smoke',MODE,arm)
        prepared=ci.prepared_validation_command(MODE)
        self.assertIn(ci.INLINE_FEATURE,prepared[-1])
        self.assertNotIn('experiment-slot-diff',prepared[-1])
        self.assertNotIn('experiment-stats-off-cost',prepared[-1])

    def test_every_timing_feature_flip_and_future_feature_is_rejected(self):
        for arm in ('baseline','candidate'):
            packages,expected,_=ci.fingerprint_expectations(MODE,arm)
            with tempfile.TemporaryDirectory() as d:
                root=Path(d);fixtures.request(root,MODE)
                for package,names in packages.items():
                    for name in names:
                        for flag in (*ci.ALL_EXPERIMENT_FEATURES,'experiment-unrequested'):
                            fixtures.fingerprints(root,arm,packages,expected,(package,name,flag))
                            with self.assertRaises(ValueError):ci.preserve_fingerprints(root,arm,MODE)
                fixtures.fingerprints(root,arm,packages,expected)
                self.assertEqual(len(ci.preserve_fingerprints(root,arm,MODE)['fingerprints']),5)

    def test_same_sha_and_no_cache_bypass(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);row=fixtures.request(root,MODE)
            for arm in ('baseline','candidate'):
                with patch.dict(os.environ,{'BUILD_CACHE_ENABLED':'1','GITHUB_REPOSITORY':build_cache.TRUSTED_REPOSITORY,
                                            arm.upper()+'_CACHE_HIT':'true'}),patch.object(build_cache,'_current_plan') as calls:
                    self.assertFalse(build_cache.restore(root,arm));calls.assert_not_called()
            row['candidate_sha']='b'*40;fixtures.write_json(root/'ci-results/request.json',row)
            with patch.object(ci.subprocess,'Popen') as calls,self.assertRaises(ValueError):ci.build(root)
            calls.assert_not_called()

    def test_p11_cannot_request_slot_diff_instruction_exemption(self):
        with self.assertRaisesRegex(ValueError,'only valid'):
            compact_probe.slot_diff_semantic_output(Path('unused'),MODE)


class P11ProofTests(unittest.TestCase):
    def setup_proof(self,root):
        fixtures.request(root,MODE)
        for arm in ('baseline','candidate'):
            fixtures.write_json(root/f'ci-results/{arm}-build-receipt.json',{'status':'success','reused':False,'selection':MODE})
            (root/f'ci-results/{arm}-build.log').write_text(output_tests((*gate.CORE_TESTS,gate.SCENARIO_TEST)))

    def mock_child(self,root,fault=None):
        owner=self
        def child(argv,cwd,env,stem,timeout,commands,save):
            label=Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
            commands.append({'argv':argv,'label':label})
            owner.assertNotIn('LAB_P11_ALLOCATION_CHILD',env)
            owner.assertNotIn('LAB_ENGINE_STATS',env)
            owner.assertEqual(cwd,root/'candidate/engine')
            flags=argv[argv.index('--features')+1].split(',')
            names={f.rsplit('/',1)[-1] for f in flags}
            core={name:name in names for name in ci.ALL_EXPERIMENT_FEATURES}
            if ci.INLINE_OBSERVER in names:core[ci.INLINE_FEATURE]=True
            if fault=='extra-feature':core[ci.NEW_FEATURES['slot-diff']]=True
            if label=='inline-runstart-core-observer':
                packages={'lab-engine':('lib-lab_engine.json','test-lib-lab_engine.json','test-integration-test-inline_runstart_allocations.json')}
                expected={'lab-engine':core}
                text=allocator_output() if '--test' in argv else output_tests(gate.CORE_TESTS,109)
                if fault=='allocator' and '--test' in argv:text=text.replace('len=4 original=1 capture=0','len=4 original=1 capture=1')
                if fault=='zero-tests' and '--lib' in argv:text=output_tests((),112)
            else:
                packages={'lab-engine':('lib-lab_engine.json',),'lab-scenario':('lib-lab_scenario.json','test-integration-test-inline_runstart.json')}
                expected={'lab-engine':core,'lab-scenario':{name:('lab-scenario/'+name) in flags for name in ci.ALL_EXPERIMENT_FEATURES}}
                rows=proof_rows()
                if fault=='raw-difference' and label.endswith('candidate'):
                    rows[1]['result']['ok'][0]['outcome']['instructions_debug']='wrong'
                if fault=='missing-state':rows[1].pop('before_state')
                Path(env['LAB_P11_RECORDS']).write_text(''.join(json.dumps(row)+'\n' for row in rows),encoding='utf-8',newline='\n')
                text=output_tests((gate.SCENARIO_TEST,))+f'P11 deterministic records: {len(rows)}\n'
                if label.endswith('observer') and fault!='missing-activation':
                    text+='P11 activation: Counts { captures: 8, inline_snapshots: 8, spilled_snapshots: 0, empty_snapshots: 0, snapshot_entries: 24 }\n'
            fixtures.fingerprints(root,label,packages,expected)
            stem.parent.mkdir(parents=True,exist_ok=True)
            stem.with_suffix('.stdout').write_text(text,encoding='utf-8')
            stem.with_suffix('.stderr').write_text('',encoding='utf-8')
        return child

    def run_proof(self,root,fault=None):
        with patch.dict(os.environ,{'RUSTFLAGS':'-Ctarget-cpu=x86-64','LAB_P11_ALLOCATION_CHILD':'bad','LAB_ENGINE_STATS':'1'},clear=True), \
             patch.object(compact_probe.platform,'system',return_value='Linux'), \
             patch.object(compact_probe.platform,'machine',return_value='x86_64'), \
             patch.object(gate,'verify_declarations',return_value={}), \
             patch.object(compact_probe,'_run',side_effect=self.mock_child(root,fault)):
            return gate.validate(root)

    def test_fresh_core_allocator_and_three_scenario_builds_prove_exact_output(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.setup_proof(root)
            receipt=self.run_proof(root)
            self.assertEqual(receipt['status'],'success')
            self.assertTrue(receipt['complete_jsonl_byte_equal'])
            self.assertEqual(len(receipt['commands']),5)
            for arm in ('baseline','candidate','observer'):
                flags=receipt['scenario'][arm]['compiler_feature_evidence']['expected_by_package']['lab-engine']
                self.assertTrue(flags[ci.NEW_FEATURES['replay-action-keys']])
                self.assertFalse(flags[ci.NEW_FEATURES['slot-diff']])
                self.assertFalse(flags[ci.NEW_FEATURES['stats-off-cost']])
                self.assertEqual(flags[ci.INLINE_FEATURE],arm!='baseline')
                self.assertEqual(flags[ci.INLINE_OBSERVER],arm=='observer')

    def test_changed_output_skipped_tests_allocator_or_observer_cannot_pass(self):
        for fault in ('raw-difference','zero-tests','allocator','extra-feature','missing-state','missing-activation'):
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as d:
                root=Path(d);self.setup_proof(root)
                with self.assertRaises(ValueError):self.run_proof(root,fault)
                self.assertEqual(json.loads((root/'ci-results/inline-runstart-validation/receipt.json').read_text())['status'],'failed')

    def test_full_regression_cannot_be_reused_or_omit_p11_named_tests(self):
        for fault in ('reused','missing'):
            with tempfile.TemporaryDirectory() as d:
                root=Path(d);self.setup_proof(root)
                if fault=='reused':
                    fixtures.write_json(root/'ci-results/baseline-build-receipt.json',{'status':'success','reused':True,'selection':MODE})
                else:(root/'ci-results/candidate-build.log').write_text(output_tests(gate.CORE_TESTS))
                with self.assertRaises(ValueError):gate.validate_full_regressions(root/'ci-results')

    def test_preexisting_proof_target_refuses_execution(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);self.setup_proof(root)
            (root/'target-inline-runstart-records-baseline').mkdir()
            with self.assertRaisesRegex(ValueError,'fresh'):self.run_proof(root)


if __name__=='__main__':unittest.main()
