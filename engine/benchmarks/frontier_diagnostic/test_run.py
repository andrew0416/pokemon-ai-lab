import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import run as r
from common import c,ENVIRONMENT,features
from test_observer_trace import event,stream

class Runner(unittest.TestCase):
    def test_control_mismatch_or_failure_never_starts_diagnostic(self):
        for fault in (None,'mismatch','timeout'):
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as tmp:
                workspace=Path(tmp);folder=workspace/'p17-diagnostic-results';folder.mkdir()
                receipt={'status':'success','cached_regression_reused':False,'source_sha':'a'*40,'corpus_sha256':c.CORPUS_SHA,
                    'arms':{arm:{'features':features(arm=='on'),'compiler_features':[]} for arm in ('off','on')}}
                (folder/'build-receipt.json').write_text(json.dumps(receipt));calls=[]
                def execute(w,b,case,arm,*args,**kwargs):
                    calls.append((case,arm));return {'status':'timeout' if fault=='timeout' and arm=='on' else 'ok',
                        'semantic_result':{'meaning':2 if fault=='mismatch' and arm=='on' else 1},'semantic_sha256':'mock'}
                with patch.object(r,'binding',return_value={'source_sha':'a'*40}),patch.object(r.ci,'verify_source'),patch.object(r.ci,'fingerprints',return_value=[]),\
                    patch.object(r.os,'sched_getaffinity',return_value={1},create=True),patch.object(r,'execute',side_effect=execute):
                    self.assertEqual(r.run(workspace),0 if fault is None else 1)
                self.assertEqual(len(calls),3 if fault is None else 2)
                self.assertEqual(calls[:2],[('opening-0000','off'),('opening-0000','on')])
    def test_timeout_keeps_raw_process_and_observer_parse_error(self):
        for malformed in (False,True):
            with self.subTest(malformed=malformed),tempfile.TemporaryDirectory() as tmp:
                workspace=Path(tmp);(workspace/'controller').mkdir();scenario=workspace/'controller/scenario.json';scenario.write_text('{}')
                plan=workspace/'original.json';plan.write_bytes(b'{}\n');binary=workspace/'target-p17-on/release/lab-distribution-bench';binary.parent.mkdir(parents=True);binary.write_bytes(b'not executable')
                case={'scenario':'scenario.json','scenario_sha256':c.sha(scenario),'joint_seed':4,'sample_seeds':[1,2,3,4,5]}
                receipt={'arms':{'on':{'binary':str(binary),'binary_sha256':c.sha(binary)}}};calls=[]
                def child(argv,cwd,env,stem,cpu):
                    calls.append(dict(env));measurement='--describe' not in argv
                    stem.with_suffix('.stdout').write_bytes(b'' if measurement else plan.read_bytes())
                    stderr=(b'P17_FRONTIER {bad}\n' if malformed else stream([event(),event('progress',2)])) if measurement else b''
                    stem.with_suffix('.stderr').write_bytes(stderr)
                    return {'status':'timeout' if measurement else 'ok','returncode':-9 if measurement else 0}
                with patch.object(r,'fixed_case',return_value=(case,plan)),patch.object(r.ci,'environment',return_value={}),\
                    patch.dict(r.PLAN_SHA,{'opening-0429':c.sha(plan)}),patch.object(c,'description'),\
                    patch.object(r.process_run,'run',side_effect=child),patch.object(r.diagnostic_process,'run',side_effect=child):
                    result=r.execute(workspace,receipt,'opening-0429','on',workspace/'evidence',0,diagnostic=True)
                self.assertEqual(calls,[{},ENVIRONMENT]);self.assertEqual(result['measurement']['process']['status'],'timeout')
                self.assertEqual(result['status'],'validation_or_execution_error' if malformed else 'timeout')
                self.assertTrue((workspace/'evidence/measurement.stderr').exists())
    def test_existing_build_receipt_cannot_bypass_tests(self):
        with tempfile.TemporaryDirectory() as tmp:
            workspace=Path(tmp);folder=workspace/'p17-diagnostic-results';folder.mkdir();(folder/'build-receipt.json').write_text('{}')
            with patch.object(r.ci.subprocess,'run') as child,self.assertRaises(ValueError):r.ci.build(workspace)
            child.assert_not_called()
if __name__=='__main__':unittest.main()
