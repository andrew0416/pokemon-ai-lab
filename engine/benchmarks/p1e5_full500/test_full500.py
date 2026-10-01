"""Synthetic producer/verifier/driver regression; never launch Cargo or an engine."""
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import copy,json,tempfile,unittest
import full500 as f
import test_contract as fixture

class BuildFixture:
    def __init__(self,root):self.root=root;self.folder=root/f.RESULTS;self.folder.mkdir();self.calls=[]
    def cargo(self,argv,*,cwd,env,stdout,stderr,timeout):
        rows=[row for row in f.command_plan() if row[1]==argv];assert len(rows)==1;label,_,tests=rows[0]
        assert cwd==self.root/'source/engine' and timeout==600 and stderr==f.subprocess.STDOUT
        self.calls.append(label)
        for package,names in {'lab-engine':['lib-lab_engine.json'],'lab-scenario':['lib-lab_scenario.json','bin-lab-distribution-bench.json','test-bin-lab-distribution-bench.json']}.items():
            directory=self.root/f.TARGET/'release/.fingerprint'/(package+'-synthetic');directory.mkdir(parents=True,exist_ok=True)
            for name in names:(directory/name).write_text(json.dumps({'features':json.dumps(f.FEATURES if package=='lab-engine' else []),'rustflags':['-Ctarget-cpu=x86-64']}))
        binary=self.root/f.TARGET/'release/lab-distribution-bench';binary.write_bytes(b'synthetic never executed')
        if tests:stdout.write((''.join('test '+name+' ... ok\n' for name in tests)+'\ntest result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n').encode())
        else:stdout.write(b'synthetic build\n')
        return SimpleNamespace(returncode=0)
    def build(self):
        with patch.object(f,'contracts',return_value=(f.s.binding(),None,None)),patch.object(f.s,'verify_source'),patch.object(f,'environment',return_value={}),patch.object(f.subprocess,'run',side_effect=self.cargo):f.build(self.root)
        assert self.calls==[row[0] for row in f.command_plan()]
        return f.verify_build(self.root)

class Protocol(unittest.TestCase):
    def test_real_build_verify_contract_and_hash_command_feature_rejections(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture=BuildFixture(Path(temp));receipt=fixture.build();self.assertEqual(receipt['fresh_named_test_executions'],7)
            path=fixture.folder/'build-receipt.json';baseline=path.read_bytes()
            for change in (lambda d:d['commands'][0].update(log='wrong-name.log'),lambda d:d['commands'][0].update(argv=['cargo','test']),lambda d:d.update(cached_regression_reused=True),lambda d:d['commands'].pop()):
                value=json.loads(baseline);change(value);path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):f.verify_build(fixture.root)
            path.write_bytes(baseline)
            fp=fixture.root/f.TARGET/'release/.fingerprint/lab-engine-synthetic/lib-lab_engine.json';fp.write_text(json.dumps({'features':json.dumps(f.FEATURES+['observer']),'rustflags':['-Ctarget-cpu=x86-64']}))
            with self.assertRaises(ValueError):f.verify_build(fixture.root)
    def test_fresh_named_tests_cannot_be_replaced_with_summary_only(self):
        with tempfile.TemporaryDirectory() as temp:
            fixture=BuildFixture(Path(temp));fixture.build();path=fixture.folder/'benchmark_tests.log'
            path.write_text('test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
            receipt=json.loads((fixture.folder/'build-receipt.json').read_bytes());receipt['commands'][0]['log_sha256']=f.c.sha(path);f.base_ci.write(fixture.folder/'build-receipt.json',receipt)
            with self.assertRaisesRegex(ValueError,'Named tests'):f.verify_build(fixture.root)
    def test_actual_gate_and_all500_plan_hash_contract(self):
        root=Path(__file__).resolve().parents[4]
        bound,manifest,plans=f.contracts(root)
        self.assertEqual(bound['source_sha'],f.SOURCE);self.assertEqual(len(manifest['cases']),500)
        self.assertEqual(plans['plans'][429]['sha256'],f.s.p1.PLAN_SHA['opening-0429'])
    def test_terminal_exit_after_deadline_is_not_success(self):
        self.assertEqual(f.bounded_process.terminal_reason(None,60.1,100,0,60,1000),'timeout')
        self.assertEqual(f.bounded_process.terminal_reason(None,.2,1001,0,60,1000),'rss_limit')
        self.assertEqual(f.bounded_process.terminal_reason(None,.2,100,0,60,1000),'ok')
    def test_full500_summary_rejects_omissions_failures_and_mixed_claims(self):
        rows=[dict(row,source_sha=f.SOURCE) for row in fixture.records()];good=f.summary(rows);self.assertTrue(good['candidate_full500_complete'])
        self.assertFalse(good['mixed_source_ledger']);self.assertFalse(good['repairs_original_full500']);self.assertFalse(good['adoption_approved'])
        self.assertEqual(set(good['official_metrics']['samples']),{'16','64','256'})
        for fault in ('timeout','rss_limit','pending','not_run_whole_budget'):
            changed=copy.deepcopy(rows);changed[429]['measurement']['status']=fault
            self.assertIsNone(f.summary(changed)['official_metrics']);self.assertFalse(f.summary(changed)['complete'])
        with self.assertRaises(ValueError):f.summary(rows[:-1])
        with self.assertRaises(ValueError):f.summary(list(reversed(rows)))
        mixed=copy.deepcopy(rows);mixed[429]['source_sha']=f.s.ORIGINAL
        with self.assertRaisesRegex(ValueError,'Mixed-source'):f.summary(mixed)
    def exercise(self,bad_plan=False):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);built=BuildFixture(root).build();scenario=root/'scenario.json';scenario.write_bytes(b'{}')
            cases=[dict(fixture.CASE,id=f'opening-{i:04d}',scenario='scenario.json',scenario_sha256=f.c.sha(scenario)) for i in range(500)]
            raw=(json.dumps(fixture.plan())+'\n').encode();import hashlib
            plans={'plans':[{'id':case['id'],'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw)} for case in cases]}
            calls=[];saved={}
            def child(argv,cwd,env,stem,**kw):
                described='--describe' in argv;calls.append((stem.parent.name,described));self.assertEqual(env,{})
                if not described:
                    self.assertEqual(sum(flag for _,flag in calls),500);self.assertEqual(argv[-1],'10,20,30,40,50')
                value=fixture.plan() if described else fixture.value()
                if bad_plan and described and stem.parent.name=='opening-0003':value['choices_debug']='same valid schema but changed original choice bytes'
                stem.with_suffix('.stdout').write_bytes((json.dumps(value)+'\n').encode());stem.with_suffix('.stderr').write_bytes(b'')
                return {'status':'ok','returncode':0,'wall_seconds':.01}
            with ExitStack() as stack:
                stack.enter_context(patch.object(f,'contracts',return_value=(f.s.binding(),{'cases':cases},plans)))
                stack.enter_context(patch.object(f.s,'verify_source'));stack.enter_context(patch.object(f,'environment',return_value={}))
                stack.enter_context(patch.object(f.c,'safe_file',return_value=scenario));stack.enter_context(patch.object(f.bounded_process,'run',side_effect=child))
                stack.enter_context(patch.object(f.base_ci,'write',side_effect=lambda path,value:saved.update({path.name:value})))
                stack.enter_context(patch.object(f.os,'sched_getaffinity',return_value={0,1},create=True))
                result=f.run(root)
            self.assertEqual(result,1 if bad_plan else 0)
            self.assertEqual(len(calls),500 if bad_plan else 1000)
            self.assertEqual(saved['summary.json']['complete'],not bad_plan)
            self.assertEqual({row['source_sha'] for row in saved['records.json']['cases']},{f.SOURCE})
            if bad_plan:self.assertIsNone(saved['summary.json']['official_metrics'])
    def test_build_verify_all500_driver_end_to_end(self):self.exercise()
    def test_changed_original_plan_blocks_all_measurements(self):self.exercise(True)
    def test_whole_measurement_budget_stops_before_next_child(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);BuildFixture(root).build()
            cases=[dict(fixture.CASE,id=f'opening-{i:04d}') for i in range(500)];saved={};ticks=iter([0])
            with ExitStack() as stack:
                stack.enter_context(patch.object(f,'contracts',return_value=(f.s.binding(),{'cases':cases},{'plans':[{}]*500})))
                stack.enter_context(patch.object(f.s,'verify_source'));stack.enter_context(patch.object(f,'environment',return_value={}))
                stack.enter_context(patch.object(f.os,'sched_getaffinity',return_value={0,1},create=True))
                stack.enter_context(patch.object(f.time,'monotonic',side_effect=lambda:next(ticks,1201)))
                stack.enter_context(patch.object(f.base_ci,'write',side_effect=lambda path,value:saved.update({path.name:value})))
                child=stack.enter_context(patch.object(f.bounded_process,'run',side_effect=AssertionError('No child after whole budget')))
                self.assertEqual(f.run(root),1)
            self.assertFalse(child.called);self.assertIsNone(saved['summary.json']['official_metrics'])
            self.assertEqual(saved['summary.json']['description_status_counts'],{'not_run_whole_budget':500})
if __name__=='__main__':unittest.main()
