"""Synthetic protocol checks; no Cargo, Git, network or engine execution."""
import copy,json,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
import shared as s
import adoption,tail

class Protocol(unittest.TestCase):
    def test_bounded_tail_attempts_on_after_timeout_and_never_adopts(self):
        calls=[]
        def execute(arm):
            calls.append(arm)
            return {'status':'incomplete' if arm=='original' else 'complete'}
        rows=tail.run_arms(execute)
        self.assertEqual(calls,['original','on'])
        self.assertEqual(tail.result_verdict(rows),'inconclusive')
        self.assertFalse(tail.CONTRACT['adoption_if_incomplete'])
        self.assertEqual(tail.TIMEOUTS,{'original':1200,'on':300})
        self.assertEqual(tail.CONTRACT['job_timeout_minutes'],35)
        rows['original']['status']='invalid_complete_output'
        self.assertEqual(tail.result_verdict(rows),'failed')
        rows['original']['status']='complete'
        self.assertEqual(tail.result_verdict(rows),'complete_reference')
        with self.assertRaises(ValueError):tail.result_verdict({'on':rows['on']})

    def test_named_gate_rejects_ignored_and_accepts_expected_panic(self):
        text='test a::ordinary ... ok\ntest a::guard - should panic ... ok\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
        self.assertEqual(s.old_ci.named_test_proof(text,['a::ordinary','a::guard'],0)['passed'],2)
        for value in (text.replace('a::guard - should panic ... ok','a::guard ... ignored'),text.replace('2 passed; 0 failed','1 passed; 1 failed'),text.replace('0 filtered out','1 filtered out')):
            with self.assertRaises(ValueError):s.old_ci.named_test_proof(value,['a::ordinary','a::guard'],0)

    def test_actual_tail_fingerprints_reject_observer_and_native(self):
        with tempfile.TemporaryDirectory() as temp:
            workspace=Path(temp);root=workspace/'target-p1e2-tail-on/release/.fingerprint'
            packages={'lab-engine':{'lib-lab_engine.json'},'lab-scenario':{'lib-lab_scenario.json','bin-lab-joint-export.json','test-bin-lab-joint-export.json'}}
            paths=[]
            for package,names in packages.items():
                folder=root/(package+'-synthetic');folder.mkdir(parents=True)
                for name in names:
                    path=folder/name;value={'features':json.dumps(s.p1.features(True) if package=='lab-engine' else []),'rustflags':['-Ctarget-cpu=x86-64']}
                    path.write_text(json.dumps(value));paths.append(path)
            self.assertEqual(len(tail.fingerprints(workspace,'on')),4)
            core=root/'lab-engine-synthetic/lib-lab_engine.json';good=core.read_bytes();value=json.loads(good)
            value['features']=json.dumps(s.p1.features(True)+['unexpected-observer']);core.write_text(json.dumps(value))
            with self.assertRaises(ValueError):tail.fingerprints(workspace,'on')
            core.write_bytes(good);value=json.loads(good);value['rustflags']=['-Ctarget-cpu=native'];core.write_text(json.dumps(value))
            with self.assertRaises(ValueError):tail.fingerprints(workspace,'on')

    def test_actual_broad_executable_must_be_fresh_release_test(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);target=root/'target';binary=target/'release/deps/lazy_ko_damage_expanded-abc';binary.parent.mkdir(parents=True);binary.write_bytes(b'synthetic')
            artifact={'reason':'compiler-artifact','target':{'name':s.BROAD_TARGET},'profile':{'test':True,'opt_level':'3'},'features':[],'executable':str(binary)}
            log=root/'build.log';log.write_text(json.dumps(artifact)+'\n')
            self.assertEqual(adoption.compiled_executable(log,target)['sha256'],s.c.sha(binary))
            for value in (dict(artifact,features=['observer']),dict(artifact,profile={'test':False,'opt_level':'3'}),dict(artifact,executable=str(root/'outside'))):
                log.write_text(json.dumps(value)+'\n')
                with self.assertRaises(ValueError):adoption.compiled_executable(log,target)
            log.write_text((json.dumps(artifact)+'\n')*2)
            with self.assertRaises(ValueError):adoption.compiled_executable(log,target)

    def test_original_registration_and_additions_are_exact(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);baseline=b'[package]\nname="lab-scenario"\n';manifest=root/s.MANIFEST;manifest.parent.mkdir(parents=True)
            pins={s.MANIFEST:'irrelevant',s.BROAD_FILE:'',tail.SOURCE:''}
            for name in (s.PUBLIC,s.BROAD_FILE,tail.SOURCE,s.p1.BENCHMARK):
                path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(name)
                if name in pins:pins[name]=s.c.sha(path)
            bound={'test_only_file_sha256':pins,'inherited_p1e_file_sha256':{s.PUBLIC:s.c.sha(root/s.PUBLIC)}}
            names=s.original_test_files(bound)
            expected=baseline+(s.old_ci.TEST_REGISTRATION+s.REGISTRATION).encode();manifest.write_bytes(expected)
            def command(argv,cwd):
                return {('git','rev-parse','HEAD'):s.ORIGINAL,('git','rev-parse','HEAD^'):s.c.CANONICAL_SOURCE,
                  ('git','diff','--name-only','HEAD'):s.MANIFEST,('git','ls-files','--others','--exclude-standard'):'\n'.join(names)}[tuple(argv)]
            with patch.object(s.base_ci,'command',side_effect=command),patch('subprocess.check_output',return_value=baseline),patch.object(s.p1,'BENCHMARK_SHA',s.c.sha(root/s.p1.BENCHMARK)):
                self.assertTrue(s.verify_original(root,bound)['runtime_unchanged'])
                manifest.write_bytes(expected+b'# unrelated manifest edit\n')
                with self.assertRaises(ValueError):s.verify_original(root,bound)
                manifest.write_bytes(expected);(root/tail.SOURCE).write_text('different exporter')
                with self.assertRaises(ValueError):s.verify_original(root,bound)

    def test_broad_compile_does_not_execute_and_tail_tests_are_exact(self):
        for arm in s.ARMS:
            command=adoption.broad_command(arm)
            self.assertIn('--no-run',command);self.assertIn('--message-format=json',command)
            self.assertIn('lab-engine/experiment-lazy-ko-damage',command[command.index('--features')+1] if arm=='on' else 'lab-engine/experiment-lazy-ko-damage')
        self.assertEqual(len(tail.TESTS),6)
        self.assertEqual(len(set(tail.TESTS)),6)
        self.assertEqual(s.BROAD_TESTS,['p1e2_corpus_contract','p1e2_full_state_corpus'])

    def test_terminal_bound_checks_even_if_child_exited_before_poll(self):
        import accuracy_process,tail_process
        for process in (accuracy_process,tail_process):
            self.assertEqual(process.terminal_reason(None,300.01,100,0,300,1000),'timeout')
            self.assertEqual(process.terminal_reason(None,299.99,100,0,300,1000),'ok')
            self.assertEqual(process.terminal_reason(None,1,1001,0,300,1000),'rss_limit')
            self.assertEqual(process.terminal_reason(None,1,100,1,300,1000),'nonzero_exit')
            self.assertEqual(process.terminal_reason('timeout',1,100,0,300,1000),'timeout')
    def test_inconclusive_writes_visible_non_adoption_summary(self):
        import os
        with tempfile.TemporaryDirectory() as temp:
            summary=Path(temp)/'summary.md'
            with patch.dict(os.environ,{'GITHUB_STEP_SUMMARY':str(summary)}),patch('builtins.print') as output:
                tail.emit_verdict({'status':'inconclusive'})
            self.assertIn('::warning::',output.call_args.args[0]);self.assertIn('Adoption is not authorized',summary.read_text())

if __name__=='__main__':unittest.main()