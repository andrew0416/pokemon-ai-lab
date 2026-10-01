"""Mocked sequencing, bounds and raw-output preservation; no engine run."""
import copy
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import common as p
import run as r
import test_contract as fixture

class ExecutionTests(unittest.TestCase):
    def test_pair_order_balanced(self):
        orders = [r.control_order(repeat, case) for repeat in range(3) for case in range(2)]
        for position in range(3):
            for arm in ('original', 'off', 'on'):
                self.assertEqual(sum(order[position] == arm for order in orders), 2)

    def exercise_run(self, fault=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            out = root / r.ci.RESULTS
            out.mkdir()
            plan = root / 'plan.json'
            plan.write_bytes((json.dumps(fixture.plan()) + '\n').encode())
            bound = {'source_sha': 'a' * 40}
            receipt = {'public_agreement': {'passed': True}}
            calls = []
            def execute(workspace, proof, case, arm, directory, cpu, tail=False):
                calls.append((case, arm, tail))
                if tail and arm == 'original':
                    return {'status': 'timeout', 'description': {'status': 'ok'},
                            'measurement': {'process': {'timeout_seconds': 300, 'wall_seconds': 300.1}}}
                value = fixture.value()
                if fault == 'control' and not tail and arm == 'on':
                    value['samples'][0]['full_state'].update(coverage=0.5, tv=0.5)
                return {'status': 'ok', 'result': value, 'description': {'status': 'ok'},
                        'measurement': {'process': {'timeout_seconds': 300 if tail else 60, 'wall_seconds': 2.0}}}
            with (patch.object(r, 'binding', return_value=bound), patch.object(r.ci, 'verify_source'), patch.object(r.ci, 'verify_original'),
                  patch.object(r, 'verify_build', side_effect=ValueError('gate failed') if fault == 'gate' else None, return_value=receipt),
                  patch.object(r, 'execute', side_effect=execute), patch.object(r, 'fixed_case', return_value=(fixture.CASE, plan)),
                  patch.object(r, 'os', SimpleNamespace(sched_getaffinity=lambda _: {1, 2}))):
                result = r.run(root)
            summary = json.loads((out / 'comparison-summary.json').read_text())
            self.assertEqual(result, 1 if fault else 0, summary.get('error'))
            self.assertFalse(summary['full500_complete'])
            self.assertIsNone(summary['official_full500_metrics'])
            if fault == 'gate':
                self.assertEqual(calls, [])
            elif fault == 'control':
                self.assertEqual(len(calls), 3)
                self.assertEqual(summary['status'], 'validation_or_execution_error')
            else:
                self.assertEqual(len(calls), 20)
                self.assertEqual(calls[-2:], [('opening-0429', 'original', True), ('opening-0429', 'on', True)])
                self.assertEqual(summary['status'], 'completed_censored_tail')
                self.assertFalse(summary['tail_reference_complete_in_both_arms'])

    def test_timeout_off_still_runs_on_and_is_censored(self):
        self.exercise_run()

    def test_failed_correctness_or_control_prevents_tail(self):
        self.exercise_run('gate')
        self.exercise_run('control')

    def test_exact_plan_and_no_observer_env_before_measurement(self):
        for fault in (None, 'plan', 'timeout', 'observer'):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                binary = root / 'target-p1e-on/release/lab-distribution-bench'
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b'fake')
                scenario = root / 'scenario.json'
                scenario.write_bytes(b'{}')
                original = root / 'original.json'
                raw = (json.dumps(fixture.plan()) + '\n').encode()
                original.write_bytes(raw)
                case = dict(fixture.CASE, scenario='scenario.json', scenario_sha256=p.c.sha(scenario))
                receipt = {'arms': {'on': {'binary': str(binary), 'binary_sha256': p.c.sha(binary)}}}
                calls = []
                def describe(argv, cwd, env, stem, **kwargs):
                    calls.append('describe')
                    self.assertFalse(any(k.startswith('LAB_') for k in env))
                    stem.with_suffix('.stdout').write_bytes(raw.replace(b'complete state', b'changed state') if fault == 'plan' else raw)
                    stem.with_suffix('.stderr').write_bytes(b'')
                    return {'status': 'ok', 'returncode': 0}
                def measure(argv, cwd, env, stem, **kwargs):
                    calls.append('measure')
                    self.assertFalse(any(k.startswith('LAB_') for k in env))
                    self.assertEqual(kwargs['cpu'], 1)
                    self.assertEqual(Path(argv[argv.index('--plan') + 1]).read_bytes(), raw)
                    self.assertEqual(argv[-1], ','.join(map(str, case['sample_seeds'])))
                    stem.with_suffix('.stdout').write_bytes(b'' if fault == 'timeout' else (json.dumps(fixture.value()) + '\n').encode())
                    stem.with_suffix('.stderr').write_bytes(b'P17_FRONTIER {}\n' if fault == 'observer' else b'')
                    return {'status': 'timeout' if fault == 'timeout' else 'ok', 'returncode': -9 if fault == 'timeout' else 0}
                with (patch.object(r, 'fixed_case', return_value=(case, original)), patch.object(r, 'PLAN_SHA', {'opening-0429': p.c.sha(original)}),
                      patch.object(r.ci, 'environment', return_value={}), patch.object(r.c, 'safe_file', return_value=scenario),
                      patch.object(r.process_run, 'run', side_effect=describe), patch.object(r.diagnostic_process, 'run', side_effect=measure)):
                    row = r.execute(root, receipt, 'opening-0429', 'on', root / 'output', 1, tail=True)
                self.assertEqual(calls, ['describe'] if fault == 'plan' else ['describe', 'measure'])
                self.assertEqual(row['status'], 'validation_or_execution_error' if fault in ('plan', 'observer') else 'timeout' if fault else 'ok')
                self.assertTrue((root / 'output/receipt.json').is_file())
                if fault == 'timeout':
                    self.assertEqual(row['measurement']['stdout_bytes'], 0)

    def test_60_and_300_second_resource_runners(self):
        for module, limit in ((r.process_run, 60), (r.diagnostic_process, 300)):
            for mode in ('ok', 'timeout', 'rss', 'nonzero'):
                with tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    child = SimpleNamespace(pid=123, returncode=None)
                    usage = SimpleNamespace(ru_maxrss=100, ru_utime=0.1, ru_stime=0.01)
                    statuses = [(0, 0, None), (123, 9 if mode in ('timeout', 'rss') else 256 if mode == 'nonzero' else 0, usage)]
                    clock = iter([0, limit + 1 if mode == 'timeout' else 1, limit + 2 if mode == 'timeout' else 2])
                    fakeos = SimpleNamespace(name='posix', WNOHANG=1, wait4=lambda *args: statuses.pop(0),
                        sched_getaffinity=lambda _: {1, 2}, sched_setaffinity=lambda *args: None,
                        waitstatus_to_exitcode=lambda status: -9 if status == 9 else 1 if status == 256 else 0, killpg=lambda *args: None)
                    with (patch.object(module, 'os', fakeos), patch.object(module.subprocess, 'Popen', return_value=child),
                          patch.object(module, 'rss_bytes', return_value=(7 * 1024**3 if mode == 'rss' else 100, 100)),
                          patch.object(module.time, 'sleep'), patch.object(module.time, 'monotonic', side_effect=lambda: next(clock))):
                        value = module.run(['fake'], root, {}, root / 'raw', cpu=1)
                    self.assertEqual(value['timeout_seconds'], limit)
                    self.assertEqual(value['rss_limit_bytes'], 6 * 1024**3)
                    self.assertEqual(value['status'], {'ok': 'ok', 'timeout': 'timeout', 'rss': 'rss_limit', 'nonzero': 'nonzero_exit'}[mode])
                    self.assertEqual(value['cpu_affinity'], [1])

if __name__ == '__main__':
    unittest.main()
