"""Synthetic wire evidence and actual build->verify receipt regressions; no engine runs."""
import copy,json,subprocess,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import oracle_shared as h
import oracle_ci as driver
import oracle_compare as o
import test_joint as fixtures

class OracleWireTests(unittest.TestCase):
    def exports(self,root,rows=None,candidate_rows=None,edit=None):
        fixture=fixtures.JointExportTests()
        plan=fixture.write(root/'oracle',rows)
        fixture.write(root/'candidate',candidate_rows if candidate_rows is not None else rows)
        path=root/'oracle/manifest.json';prior=json.loads(path.read_text())
        keys=['schema','status','dictionary','joint','selection','party_lengths','hp_unit_count','record_bytes','dictionary_entries','unique_joint_rows','joint_mass','tv_bound','joint_bytes','dictionary_bytes','selection_bytes','payload_bytes_excluding_manifest','all_state_restored','all_lazy_tags_clear','actual_eq_hash_dictionary','debug_injective_on_observed_keys']
        m={key:prior[key] for key in keys}
        m.update(kind='exact-stream-oracle-export',**o.TEXT,suspended_dictionary_entries=0,max_export_bytes_including_manifest=o.joint.MAX_BYTES,
          limits=o.LIMITS.copy(),scratch_bytes_written=1024,total_replays=1,instruction_rows=2,caller_state_immutable=True,oracle_ns=1,adoption_approved=False,full500_complete=False,
          stage_reports=[{'stage':0,'input_rows':1,'input_dictionary_entries':1,'replays':1,'next_rows':0,'next_dictionary_entries':0,'finished_dictionary_entries':m['dictionary_entries'],'active_mass':0.,'finished_mass':1.}])
        if edit:edit(m)
        path.write_text(json.dumps(m)+'\n',encoding='utf-8');return plan
    def test_complete_distinct_methods_and_tolerated_roundoff(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);plan=self.exports(root,candidate_rows=[(0,(10,)*12,.5+1e-14),(0,(20,)*12,.5-1e-14)])
            proof=o.compare_exports(root/'oracle',root/'candidate',plan)
            self.assertTrue(proof['passed']);self.assertEqual(proof['unique_joint_rows'],2)
            self.assertNotEqual(proof['oracle_method'],proof['candidate_method']);self.assertIsNone(proof['speed_ratio'])
    def test_missing_mixed_approximate_and_resource_claims_fail_closed(self):
        edits=[lambda m:m.update(kind='exact-joint-hp-export'),lambda m:m.update(method=o.joint.TEXT_CONTRACT['method']),
          lambda m:m.update(status='incomplete'),lambda m:m.update(caller_state_immutable=False),lambda m:m.update(tv_bound=.1),
          lambda m:m.update(scratch_bytes_written=4294967297),lambda m:m.update(components=1),lambda m:m.update(total_replays=2),
          lambda m:m['stage_reports'][0].update(input_rows=2),lambda m:m['stage_reports'][0].update(next_dictionary_entries=1)]
        for edit in edits:
            with self.subTest(edit=edit),tempfile.TemporaryDirectory() as temp:
                root=Path(temp);plan=self.exports(root,edit=edit)
                with self.assertRaises(ValueError):o.read_oracle(root/'oracle',plan)
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);plan=self.exports(root);(root/'oracle/manifest.json').unlink()
            with self.assertRaises(ValueError):o.read_oracle(root/'oracle',plan)
    def test_full_support_last_reserve_and_hidden_state_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);plan=self.exports(root,candidate_rows=[(0,(10,)*11+(11,),.5),(0,(20,)*12,.5)])
            with self.assertRaisesRegex(ValueError,'support'):o.compare_exports(root/'oracle',root/'candidate',plan)
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);plan=self.exports(root);p=root/'candidate/dictionary.json';p.write_bytes(p.read_bytes().replace(b'hidden: 1',b'hidden: 2'))
            with self.assertRaisesRegex(ValueError,'dictionary changed'):o.compare_exports(root/'oracle',root/'candidate',plan)
    def test_mass_probability_truncation_and_plan_rejected(self):
        for rows in [[(0,(10,)*12,.6),(0,(20,)*12,.4)],[(0,(10,)*12,.4),(0,(20,)*12,.4)]]:
            with tempfile.TemporaryDirectory() as temp:
                root=Path(temp);plan=self.exports(root,candidate_rows=rows)
                with self.assertRaises(ValueError):o.compare_exports(root/'oracle',root/'candidate',plan)
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);plan=self.exports(root)
            with self.assertRaises(ValueError):o.read_oracle(root/'oracle',b'other')
            path=root/'oracle/joint.bin';path.write_bytes(path.read_bytes()[:-1])
            with self.assertRaises(ValueError):o.read_oracle(root/'oracle',plan)
    def test_accumulated_tv_not_hidden_by_per_key_tolerance(self):
        rows=[(0,(i,)+(10,)*11,1/4000) for i in range(4000)]
        shifted=[(a,hp,p+(9e-13 if i<2000 else -9e-13)) for i,(a,hp,p) in enumerate(rows)]
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);plan=self.exports(root,rows,shifted)
            with self.assertRaisesRegex(ValueError,'TV'):o.compare_exports(root/'oracle',root/'candidate',plan)

class ReceiptProtocolTests(unittest.TestCase):
    def produce(self,workspace):
        (workspace/h.RESULTS).mkdir();bound=h.binding()
        def cargo(argv,**kwargs):
            arm='oracle' if Path(kwargs['cwd']).parent.name=='oracle' else 'candidate'
            pair=[(label,cmd) for label,cmd in h.commands(arm) if cmd==argv];self.assertEqual(len(pair),1);label=pair[0][0]
            if label in h.SUITES:
                names=h.SUITES[label];filtered=7 if label=='oracle_core' else 0
                log='\n'.join('test '+name+' ... ok' for name in names)+'\n\ntest result: ok. '+str(len(names))+' passed; 0 failed; 0 ignored; 0 measured; '+str(filtered)+' filtered out; finished in 0.01s\n'
                kwargs['stdout'].write(log.encode())
            else:kwargs['stdout'].write(b'Finished release\n')
            target=h.target_root(workspace,arm)/'release';target.mkdir(parents=True,exist_ok=True)
            (target/h.TARGETS[arm]).write_bytes(('synthetic '+arm).encode())
            packages={'lab-engine':{'lib-lab_engine.json'}|({'test-lib-lab_engine.json'} if arm=='oracle' else set()),'lab-scenario':{'lib-lab_scenario.json','bin-'+h.TARGETS[arm]+'.json','test-bin-'+h.TARGETS[arm]+'.json'}}
            for package,names in packages.items():
                directory=target/'.fingerprint'/(package+'-synthetic');directory.mkdir(parents=True,exist_ok=True)
                for name in names:
                    feature=h.features(arm) if package=='lab-engine' else ([h.FEATURE] if arm=='oracle' else [])
                    (directory/name).write_text(json.dumps({'features':json.dumps(feature),'rustflags':['-Ctarget-cpu=x86-64']}))
            return subprocess.CompletedProcess(argv,0)
        with patch.object(h,'verify_sources',return_value={}),patch.object(h,'environment',return_value={}),patch.object(driver.subprocess,'run',side_effect=cargo):driver.build(workspace)
        return bound
    def test_actual_producer_to_verifier_and_run_boundary(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace=Path(temp);bound=self.produce(workspace);proof=driver.verify_build(workspace,bound)
            self.assertEqual(proof['fresh_named_test_executions'],17)
            self.assertEqual(len(proof['commands']),5)
            for row in proof['commands']:self.assertEqual(row['log'],h.command_log_name(row['arm'],row['label']))
    def test_original_filename_mismatch_and_stale_source_rejected(self):
        for mutation in ('filename','source','log','features'):
            with self.subTest(mutation=mutation),tempfile.TemporaryDirectory() as temp:
                workspace=Path(temp);bound=self.produce(workspace);path=workspace/h.RESULTS/'build-receipt.json';value=json.loads(path.read_text())
                if mutation=='filename':value['commands'][0]['log']='oracle-oracle_core_build.log'
                if mutation=='source':value['oracle_source_sha']='0'*40
                if mutation=='log':(workspace/h.RESULTS/value['commands'][0]['log']).write_text('stale')
                if mutation=='features':value['arms']['oracle']['features']=h.features('candidate')
                path.write_text(json.dumps(value))
                with self.assertRaises(ValueError):driver.verify_build(workspace,bound)
    def test_candidate_still_attempted_after_incomplete_oracle(self):
        called=[]
        def execute(arm):called.append(arm);return {'status':'incomplete' if arm=='oracle' else 'complete'}
        rows=driver.run_arms(execute);self.assertEqual(called,list(h.ARMS));self.assertEqual(driver.verdict(rows),'inconclusive')
        rows['oracle']['status']='invalid_complete_output';self.assertEqual(driver.verdict(rows),'failed')
    def test_registration_preserves_all_original_bytes(self):
        for name,(anchor,insert,append) in h.REGISTRATIONS.items():
            old=('before\n'+anchor+'after\n').encode();actual=h.registration_bytes(name,old)
            self.assertEqual(actual.replace(insert.encode(),b'',1)[:-len(append)] if append else actual.replace(insert.encode(),b'',1),old)
            with self.assertRaises(ValueError):h.registration_bytes(name,b'wrong anchor')
    def test_full_driver_incomplete_and_mismatch_verdicts(self):
        import shutil
        for outcome in ('match','timeout','mismatch'):
            with self.subTest(outcome=outcome),tempfile.TemporaryDirectory() as temp:
                workspace=Path(temp);bound=self.produce(workspace)
                fixtures_root=workspace/'fixtures';fixtures_root.mkdir()
                rows=[(0,(10,)*12,.6),(0,(20,)*12,.4)] if outcome=='mismatch' else None
                plan_bytes=OracleWireTests().exports(fixtures_root,candidate_rows=rows)
                plan=workspace/'plan.json';plan.write_bytes(plan_bytes);bound=dict(bound,plan_sha256=h.c.sha(plan))
                scenario=workspace/'controller/case.json';scenario.parent.mkdir();scenario.write_bytes(b'{}')
                case={'id':'opening-0429','joint_seed':18220721905388468017,'scenario':'case.json'};calls=[]
                def describe(argv,cwd,env,stem,**kwargs):
                    stem.with_suffix('.stdout').write_bytes(plan_bytes);stem.with_suffix('.stderr').write_bytes(b'')
                    return {'status':'ok','wall_seconds':.01}
                def child(argv,cwd,env,stem,**kwargs):
                    arm='oracle' if '--out' in argv else 'candidate';calls.append(arm)
                    output=Path(argv[argv.index('--out' if arm=='oracle' else '--output-dir')+1])
                    stem.with_suffix('.stderr').write_bytes(b'synthetic progress\n')
                    if arm=='oracle' and outcome=='timeout':
                        stem.with_suffix('.stdout').write_bytes(b'');return {'status':'timeout','wall_seconds':1200.1}
                    shutil.copytree(fixtures_root/arm,output)
                    stem.with_suffix('.stdout').write_bytes((output/'manifest.json').read_bytes());return {'status':'ok','wall_seconds':.01}
                with patch.object(h,'binding',return_value=bound),patch.object(h,'verify_sources',return_value={}),patch.object(h,'environment',return_value={}),patch.object(h.s.p1,'fixed_case',return_value=(case,plan)),patch.object(driver.os,'sched_getaffinity',return_value={0},create=True),patch.object(driver.describe_process,'run',side_effect=describe),patch.object(driver.tail_process,'run',side_effect=child),patch.object(driver,'emit_verdict'):
                    code=driver.run(workspace)
                value=json.loads((workspace/h.RESULTS/'oracle-summary.json').read_text())
                self.assertEqual(calls,list(h.ARMS));self.assertEqual(code,1 if outcome=='mismatch' else 0)
                self.assertEqual(value['status'],{'match':'complete_reference','timeout':'inconclusive','mismatch':'failed'}[outcome])
                self.assertEqual(value['exact_reference_complete'],outcome=='match');self.assertFalse(value['adoption_approved'])
if __name__=='__main__':unittest.main()
