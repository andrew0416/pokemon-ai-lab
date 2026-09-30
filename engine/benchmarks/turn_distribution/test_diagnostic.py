"""Synthetic single-case diagnosis guards; no engine or performance execution."""
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
import contract as c
import diagnostic as d
import diagnostic_process as p
import test_contract as fixture

class DiagnosticTests(unittest.TestCase):
    def exercise(self,fault=None):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);out=root/'turn-distribution-results';out.mkdir()
            binary=root/'target-distribution/release/lab-distribution-bench';binary.parent.mkdir(parents=True);binary.write_bytes(b'fake')
            scenario=root/'scenario.json';scenario.write_bytes(b'{}')
            original=root/'original.json';raw=(json.dumps(fixture.plan())+'\n').encode();original.write_bytes(raw)
            case=dict(fixture.CASE,id=d.CASE_ID,scenario='scenario.json',scenario_sha256=c.sha(scenario))
            receipt=dict(status='success',cached_regression_reused=fault=='cached',source_sha=c.SOURCE_SHA,
                corpus_sha256=c.CORPUS_SHA,features=list(c.CORE_FEATURES),binary=str(binary),binary_sha256=c.sha(binary))
            (out/'build-receipt.json').write_text(json.dumps(receipt))
            calls=[]
            def describe(argv,cwd,env,stem,**kwargs):
                calls.append('describe');self.assertNotIn('LAB_ENGINE_STATS',env);self.assertEqual(argv[-1],'--describe')
                data=raw.replace(b'complete state',b'changed state') if fault=='plan' else raw
                stem.with_suffix('.stdout').write_bytes(data);stem.with_suffix('.stderr').write_bytes(b'')
                return {'status':'ok','returncode':0}
            def measure(argv,cwd,env,stem,**kwargs):
                calls.append('measure');self.assertEqual(env['LAB_ENGINE_STATS'],'1');self.assertEqual(kwargs['cpu'],1)
                self.assertEqual(Path(argv[argv.index('--plan')+1]).read_bytes(),raw)
                self.assertEqual(argv[-1],','.join(map(str,case['sample_seeds'])))
                value=fixture.value()
                if fault=='schema':value['all_state_restored']=False
                stem.with_suffix('.stdout').write_bytes(b'' if fault=='timeout' else (json.dumps(value)+'\n').encode())
                stem.with_suffix('.stderr').write_bytes(b'lab-engine: factored stage 1 groups (0 splits, 0 expansions), 1 runs -> 1 components -> 1 compacted, 0 finished; 0.1 ms\n')
                return {'status':'timeout' if fault=='timeout' else 'ok','returncode':-9 if fault=='timeout' else 0,'timeout_seconds':300,'rss_limit_bytes':6*1024**3}
            with (patch.object(d,'PLAN_SHA',c.sha(original)),patch.object(d,'fixed_case',return_value=(case,original)),
                patch.object(d.ci,'verify_source'),patch.object(d.ci,'environment',return_value={}),
                patch.object(d.c,'safe_file',return_value=scenario),patch.object(d,'os',SimpleNamespace(sched_getaffinity=lambda _: {1,2})),
                patch.object(d.process_run,'run',side_effect=describe),patch.object(d.diagnostic_process,'run',side_effect=measure)):
                code=d.run(root)
            result=json.loads((out/'diagnostic-summary.json').read_text())
            self.assertEqual(code,0 if fault is None else 1);self.assertEqual(result['diagnostic_complete'],fault is None)
            self.assertTrue(result['no_speed_comparison']);self.assertTrue(result['does_not_repair_full500_metrics'])
            self.assertEqual(result['actual_requested_cases'],1);self.assertFalse(result['timing_comparable'])
            self.assertEqual(result['stats_environment_enabled'],fault not in ('cached','plan'))
            self.assertIsNone(result['official_full500_metrics'])
            self.assertEqual(calls,[] if fault=='cached' else ['describe'] if fault=='plan' else ['describe','measure'])
            if fault=='timeout':
                self.assertEqual(result['status'],'timeout');self.assertEqual(result['measurement']['stdout_bytes'],0)
                self.assertGreater(result['measurement']['stderr_bytes'],0);self.assertEqual(len(result['factored_stage_lines']),1)
    def test_exact_description_before_instrumented_child(self):self.exercise()
    def test_changed_plan_blocks_diagnostic_child(self):self.exercise('plan')
    def test_timeout_preserves_stage_progress_without_comparison(self):self.exercise('timeout')
    def test_invalid_result_and_cached_receipt_fail_closed(self):
        self.exercise('schema');self.exercise('cached')
    def test_actual_pinned_plan_corpus_and_core6(self):
        controller=Path(__file__).resolve().parents[3]
        manifest=c.corpus(controller);case=manifest['cases'][429]
        self.assertEqual(case['id'],d.CASE_ID);self.assertEqual(c.sha(controller/d.PLAN_PATH),d.PLAN_SHA)
        c.description(c.line(controller/d.PLAN_PATH),case)
        self.assertEqual(len(c.CORE_FEATURES),6)
    def test_diagnostic_bound_300_and_rss_6gib(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);proc=SimpleNamespace(pid=123,returncode=None)
            usage=SimpleNamespace(ru_maxrss=100,ru_utime=299.,ru_stime=1.)
            statuses=[(0,0,None),(123,9,usage)];clock=iter([0,301,302])
            fakeos=SimpleNamespace(name='posix',WNOHANG=1,wait4=lambda *args:statuses.pop(0),
                sched_getaffinity=lambda _: {1,2},sched_setaffinity=lambda *args:None,
                waitstatus_to_exitcode=lambda status:-9,killpg=lambda *args:None)
            with (patch.object(p,'os',fakeos),patch.object(p.subprocess,'Popen',return_value=proc),
                patch.object(p,'rss_bytes',return_value=(100,100)),patch.object(p.time,'sleep'),
                patch.object(p.time,'monotonic',side_effect=lambda:next(clock))):
                result=p.run(['binary'],root,{'LAB_ENGINE_STATS':'1'},root/'case',cpu=1)
            self.assertEqual(result['status'],'timeout');self.assertEqual(result['timeout_seconds'],300)
            self.assertEqual(result['rss_limit_bytes'],6*1024**3)
            with self.assertRaises(ValueError):p.run([],root,{},root/'wrong',cpu=1,timeout_seconds=60)
if __name__=='__main__':unittest.main()
