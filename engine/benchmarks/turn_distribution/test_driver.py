"""Execute orchestration against synthetic child outputs, never an engine binary."""
import copy
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
import contract as c
import run_distribution as run
import test_contract as fixture

class DriverTests(unittest.TestCase):
    def exercise(self,fault=None):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);out=root/'turn-distribution-results';out.mkdir()
            binary=root/'target-distribution/release/lab-distribution-bench';binary.parent.mkdir(parents=True);binary.write_bytes(b'fake')
            scenario=root/'scenario.json';scenario.write_text('{}')
            cases=[dict(fixture.CASE,id=f'opening-{i:04d}',scenario='scenario.json',scenario_sha256=c.sha(scenario)) for i in range(500)]
            receipt=dict(status='success',cached_regression_reused=False,source_sha=c.SOURCE_SHA,corpus_sha256=c.CORPUS_SHA,
                features=list(c.CORE_FEATURES),binary=str(binary),binary_sha256=c.sha(binary))
            if fault=='cached':receipt['cached_regression_reused']=True
            (out/'build-receipt.json').write_text(json.dumps(receipt))
            calls=[]
            def child(argv,cwd,env,stem,**kwargs):
                describe='--describe' in argv;calls.append((stem.parent.name,describe))
                if not describe:
                    self.assertEqual(sum(d for _,d in calls),500)
                    plan=Path(argv[argv.index('--plan')+1]);self.assertEqual(c.line(plan),fixture.plan())
                status='timeout' if fault=='plan' and stem.parent.name=='opening-0499' and describe else 'rss_limit' if fault=='measure' and not describe and stem.parent.name=='opening-0000' else 'ok'
                value=fixture.plan() if describe else fixture.value()
                if fault=='metric' and not describe:value['samples'].pop()
                stem.with_suffix('.stdout').write_text(json.dumps(value)+'\n',encoding='utf8',newline='\n')
                stem.with_suffix('.stderr').write_bytes(b'')
                return {'status':status,'returncode':0 if status=='ok' else -9,'wall_seconds':1}
            fakeos=SimpleNamespace(sched_getaffinity=lambda _: {1,2})
            with (patch.object(c,'corpus',return_value={'cases':cases}),patch.object(c,'safe_file',return_value=scenario),
                  patch.object(run.ci,'verify_source'),patch.object(run.ci,'environment',return_value={}),
                  patch.object(run.process_run,'run',side_effect=child),patch.object(run,'os',fakeos)):
                if fault=='cached':
                    with self.assertRaises(ValueError):run.run(root)
                    self.assertEqual(calls,[]);return
                result=run.run(root)
            doc=json.loads((out/'summary.json').read_text());ledger=json.loads((out/'records.json').read_text())
            self.assertEqual(len(ledger['cases']),500)
            self.assertEqual(sum(d for _,d in calls),500)
            self.assertEqual(result,0 if fault is None else 1)
            self.assertEqual(doc['complete'],fault is None)
            self.assertEqual(sum(not d for _,d in calls),0 if fault=='plan' else 16)
            if fault:self.assertIsNone(doc['official_metrics'])
            else:
                self.assertEqual(doc['official_metrics']['reference_kernel_ns']['n'],16)
                self.assertEqual(doc['suspended_reference_case_count'],16 if fixture.value()['reference']['suspended_components'] else 0)
                self.assertIn('intermediate switch suspensions retained',doc['endpoint_scope'])
    def test_all500_plans_before_prefix16_measurement(self):self.exercise()
    def test_description_failure_blocks_all_measurement_without_omitting_case(self):self.exercise('plan')
    def test_measurement_resource_and_metric_failure_keep_denominators(self):
        self.exercise('measure');self.exercise('metric')
    def test_cached_receipt_cannot_bypass_fresh_build(self):self.exercise('cached')

if __name__=='__main__':unittest.main()
