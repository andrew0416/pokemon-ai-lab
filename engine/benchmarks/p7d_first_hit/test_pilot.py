"""Synthetic controller tests: real receipt/verification and full pipeline boundaries."""
import copy,json,subprocess,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import pilot_contract as h
import pilot_build as b
import pilot_run as r
import test_joint as joint_fixture
import sys
sys.path.insert(0,str(h.HERE.parent/'turn_distribution'))
import test_contract as metric_fixture

def fake_bound():
    value=json.loads((h.HERE/'source-binding.json').read_text())
    value['source_sha']='1'*40
    return value

class BuildProtocolTests(unittest.TestCase):
    def produce(self,workspace):
        (workspace/h.RESULTS).mkdir();bound=fake_bound()
        def cargo(argv,**kwargs):
            arm=Path(kwargs['env']['CARGO_TARGET_DIR']).name.removeprefix('target-p7d-')
            expected=[row for row in h.commands(arm,bound) if row[1]==argv]
            self.assertEqual(len(expected),1);label,_,names=expected[0]
            if names:
                text='\n'.join('test '+name+' ... ok' for name in names)+'\n\ntest result: ok. '+str(len(names))+' passed; 0 failed; 0 ignored; 0 measured; '+('7' if label=='core' or label.startswith('regress_') else '0')+' filtered out; finished in 0.01s\n'
            else:text='Finished release\n'
            kwargs['stdout'].write(text.encode())
            target=h.target(workspace,arm)/'release';target.mkdir(parents=True,exist_ok=True)
            for kind in h.bin_kinds(arm):(target/h.BINS[kind]).write_bytes(('synthetic '+arm+' '+kind).encode())
            engine=({'test-lib-lab_engine.json'} if arm=='edge' else {'lib-lab_engine.json'})|({'test-lib-lab_engine.json'} if arm=='on' else set())
            packages={'lab-engine':engine}
            if arm!='edge':packages['lab-scenario']={'lib-lab_scenario.json'}|{p+h.BINS[k]+'.json' for k in h.bin_kinds(arm) for p in ('bin-','test-bin-')}
            if arm=='on':packages['lab-scenario'].update('test-integration-test-'+name+'.json' for name in ('factored','bundle_scenario_04','bundle_scenario_06'))
            for package,names in packages.items():
                directory=target/'.fingerprint'/(package+'-synthetic');directory.mkdir(parents=True,exist_ok=True)
                for name in names:
                    (directory/name).write_text(json.dumps({'features':json.dumps(h.features(arm) if package=='lab-engine' else []),'rustflags':['-Ctarget-cpu=x86-64']}))
            return subprocess.CompletedProcess(argv,0)
        env=lambda workspace,arm:{'CARGO_TARGET_DIR':str(h.target(workspace,arm))}
        with patch.object(h,'binding',return_value=bound),patch.object(h,'verify_sources',return_value={}),patch.object(h,'environment',side_effect=env),patch.object(b.subprocess,'run',side_effect=cargo):b.build(workspace)
        return bound

    def test_actual_producer_verifier_names_features_and_count(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace=Path(temp);bound=self.produce(workspace);value=b.verify_build(workspace,bound)
            self.assertEqual(value['fresh_named_test_executions'],85);self.assertEqual(len(value['commands']),23)
            for row in value['commands']:self.assertEqual(row['log'],h.command_log_name(row['arm'],row['label']))
    def test_old_filename_bug_mixed_source_features_and_stale_logs_rejected(self):
        for edit in ('filename','source','features','log','missing','binary'):
            with self.subTest(edit=edit),tempfile.TemporaryDirectory() as temp:
                workspace=Path(temp);bound=self.produce(workspace);path=workspace/h.RESULTS/'build-receipt.json';v=json.loads(path.read_text())
                if edit=='filename':v['commands'][0]['log']='reference-joint_tests_build.log'
                if edit=='source':v['arms']['off']['source_sha']=h.REFERENCE
                if edit=='features':v['arms']['on']['features']=h.features('off')
                if edit=='log':(workspace/h.RESULTS/v['commands'][0]['log']).write_text('stale')
                if edit=='missing':v['commands'].pop()
                if edit=='binary':Path(v['arms']['on']['binaries']['joint']['path']).write_bytes(b'changed')
                path.write_text(json.dumps(v))
                with self.assertRaises(ValueError):b.verify_build(workspace,bound)
    def test_feature_isolation_and_p1e_edge(self):
        self.assertEqual(h.features('on'),h.features('off')+[h.FEATURE])
        self.assertEqual(h.features('reference'),h.features('off'))
        self.assertNotIn(h.s.p1.FEATURE,h.features('edge'));self.assertIn(h.FEATURE,h.features('edge'))
        self.assertFalse(any('observer' in f for a in h.BUILD_ARMS for f in h.features(a)))
    def test_unbound_source_refuses_execution(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);v=fake_bound();v['source_sha']='UNBOUND';(root/'source-binding.json').write_text(json.dumps(v))
            with patch.object(h,'HERE',root),self.assertRaisesRegex(ValueError,'unbound'):h.binding()

class PipelineTests(BuildProtocolTests):
    # Do not duplicate parent tests in discovery.
    test_actual_producer_verifier_names_features_and_count=None
    test_old_filename_bug_mixed_source_features_and_stale_logs_rejected=None
    test_feature_isolation_and_p1e_edge=None
    test_unbound_source_refuses_execution=None
    def accuracy_fixture(self,workspace,mode):
        bound=self.produce(workspace)
        fixture=joint_fixture.JointExportTests();raw=fixture.write(workspace/'fixture')
        plan=workspace/'plan.json';plan.write_bytes(raw)
        scenario=workspace/'controller/case.json';scenario.parent.mkdir();scenario.write_bytes(b'{}')
        case={'id':'opening-0000','joint_seed':1,'scenario':'case.json','scenario_sha256':h.c.sha(scenario)}
        calls=[]
        def child(argv,cwd,env,stem,**kwargs):
            arm=next(a for a in h.ARMS if str(h.target(workspace,a)) in argv[0]);calls.append((arm,'describe' if '--describe' in argv else 'export'))
            stem.with_suffix('.stderr').write_bytes(b'')
            if '--describe' in argv:
                stem.with_suffix('.stdout').write_bytes(b'wrong' if mode=='description' and arm=='reference' else raw)
            elif mode=='timeout' and arm=='reference':
                stem.with_suffix('.stdout').write_bytes(b'');return {'status':'timeout','wall_seconds':300.1}
            else:
                output=Path(argv[argv.index('--output-dir')+1])
                rows=[(0,(10,)*12,.6),(0,(20,)*12,.4)] if mode=='mismatch' and arm=='on' else None
                if mode=='off_roundoff' and arm=='off':rows=[(0,(10,)*12,.5+1e-14),(0,(20,)*12,.5-1e-14)]
                fixture.write(output,rows)
                stem.with_suffix('.stdout').write_bytes((output/'manifest.json').read_bytes())
            return {'status':'ok','wall_seconds':.01}
        with patch.object(h,'binding',return_value=bound),patch.object(h,'verify_sources',return_value={}),patch.object(h,'environment',return_value={}),patch.object(h,'pilot',return_value=({},[(case,plan)])),patch.object(h,'CASE_IDS',[case['id']]),patch.object(r.c,'description',return_value={}),patch.object(r.os,'sched_getaffinity',return_value={0},create=True),patch.object(r.tail_process,'run',side_effect=child):
            code=r.correctness(workspace)
        return code,json.loads((workspace/h.RESULTS/'correctness.json').read_text()),calls
    def test_complete_three_arm_joint_reference(self):
        with tempfile.TemporaryDirectory() as temp:
            code,value,calls=self.accuracy_fixture(Path(temp),'match')
            self.assertEqual(code,0);self.assertTrue(value['full_state_agreement'])
            self.assertEqual([x for x in calls if x[1]=='export'],[(a,'export') for a in h.ARMS])
            self.assertEqual(set(value['cases'][0]['comparisons']),{'reference_vs_off','reference_vs_on'})
    def test_mismatch_cannot_pass_or_authorize_timing(self):
        with tempfile.TemporaryDirectory() as temp:
            code,v,_=self.accuracy_fixture(Path(temp),'mismatch')
            self.assertEqual(code,1);self.assertFalse(v['full_state_agreement']);self.assertEqual(v['cases'][0]['status'],'mismatch')
    def test_feature_off_requires_payload_byte_identity(self):
        with tempfile.TemporaryDirectory() as temp:
            code,v,_=self.accuracy_fixture(Path(temp),'off_roundoff')
            self.assertEqual(code,1);self.assertFalse(v['full_state_agreement']);self.assertIn('payload bytes',v['cases'][0]['error'])
    def test_timeout_preserves_other_arms_and_cannot_pass(self):
        with tempfile.TemporaryDirectory() as temp:
            code,v,calls=self.accuracy_fixture(Path(temp),'timeout')
            self.assertEqual(code,1);self.assertFalse(v['full_state_agreement'])
            self.assertEqual([x for x in calls if x[1]=='export'],[(a,'export') for a in h.ARMS])
    def test_changed_plan_prevents_affected_export(self):
        with tempfile.TemporaryDirectory() as temp:
            code,v,calls=self.accuracy_fixture(Path(temp),'description')
            self.assertEqual(code,1);self.assertNotIn(('reference','export'),calls)
            self.assertIn(('off','export'),calls);self.assertIn(('on','export'),calls)

class TimingTests(unittest.TestCase):
    def rows(self):
        bound=fake_bound();rows=[]
        for item in r.timing_schedule():
            value=metric_fixture.value();value['reference']['kernel_ns']=200 if item['arm']=='off' else 100
            for sample in value['samples']:sample['kernel_ns']=999999 if item['arm']=='on' else 1
            rows.append(dict(item,status='ok',source_sha=bound['source_sha'],value=value))
        return bound,rows
    def test_schedule_is_three_abba_blocks_after_discarded_warmups(self):
        rows=r.timing_schedule();self.assertEqual(len(rows),70);self.assertEqual(sum(x['phase']=='warmup' for x in rows),10)
        for i in range(10,70,4):self.assertEqual([x['arm'] for x in rows[i:i+4]],['off','on','on','off'])
    def test_full_kernel_only_ratios_ignore_sample_time(self):
        bound,rows=self.rows()
        with tempfile.TemporaryDirectory() as temp:
            plan=Path(temp)/'plan.json';plan.write_bytes((json.dumps(metric_fixture.plan())+'\n').encode())
            plans={case:(metric_fixture.CASE,plan) for case in h.TIMED_IDS}
            v=r.timing_summary(rows,bound,plans)
            self.assertTrue(v['complete'])
            for row in v['by_case'].values():self.assertEqual(row['median_on_over_off'],.5)
    def test_mixed_reordered_and_partial_rows_fail_closed(self):
        for edit in ('mixed','order','partial'):
            with self.subTest(edit=edit):
                bound,rows=self.rows()
                if edit=='mixed':rows[0]['source_sha']=h.REFERENCE
                if edit=='order':rows[-1]['arm']='on'
                if edit=='partial':rows[-1]['status']='timeout'
                if edit=='partial':
                    v=r.timing_summary(rows,bound,{});self.assertFalse(v['complete']);self.assertEqual(v['by_case'],{});self.assertIsNone(v['timeout_ratio'])
                else:
                    with self.assertRaises(ValueError):r.timing_summary(rows,bound,{})
    def test_inherited_joint_rejects_hidden_state_hp_and_probability_drift(self):
        fixture=joint_fixture.JointExportTests()
        with self.assertRaises(ValueError):fixture.compare(b_entries=['State { hidden: 2, hp: 0 }\nNone'])
        with self.assertRaises(ValueError):fixture.compare(b_rows=[(0,(10,)*11+(11,),.5),(0,(20,)*12,.5)])
        with self.assertRaises(ValueError):fixture.compare(b_rows=[(0,(10,)*12,.6),(0,(20,)*12,.4)])
    def test_terminal_limits_and_phase_admission(self):
        self.assertEqual(r.tail_process.terminal_reason(None,300.01,1,0,300,h.RSS),'timeout')
        self.assertEqual(r.tail_process.terminal_reason(None,1,h.RSS+1,0,300,h.RSS),'rss_limit')
        with patch.object(r.time,'monotonic',return_value=100):
            self.assertFalse(r.has_budget(399));self.assertTrue(r.has_budget(400))
if __name__=='__main__':unittest.main()
