"""P10 gate tests use inert binaries and mocked processes; no Rust runs here."""
from collections import Counter
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import compact_probe as gate


def output_rows():
    rows = []
    for kind, count in gate.KINDS.items():
        for index in range(count):
            row = dict.fromkeys(gate.FIELDS[kind])
            row['kind'] = kind
            if kind == 'complete':
                row.update(schema_version=1, registry_count=112, singleton_patterns=8,
                           fixture_configurations=10, scope='test data')
            rows.append(row)
    return rows


def jsonl(rows):
    return b''.join((json.dumps(row, sort_keys=True) + '\n').encode() for row in rows)


def layout(compact=False):
    value = {key: 8 for key in gate.LAYOUT_NUMBERS}
    value.update(kind='layout', schema_version=1, scope='static test layout',
                 volatiles=64 if compact else 896)
    return value


class OutputTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.path = Path(folder.name)/'probe.stdout'

    def test_exact_56_records_and_complete_contract(self):
        self.path.write_bytes(jsonl(output_rows()))
        result = gate.validate_output(self.path)
        self.assertEqual(result['records'], 56)
        self.assertEqual(result['kind_counts'], gate.KINDS)
        self.assertEqual(result['sha256'], gate.sha(self.path))

    def test_rejects_partial_extra_malformed_and_wrong_completion_records(self):
        rows = output_rows()
        malformed = copy.deepcopy(rows)
        malformed[0].pop('records')
        wrong_counts = copy.deepcopy(rows)
        wrong_counts[0] = copy.deepcopy(rows[8])
        false_registry = copy.deepcopy(rows)
        false_registry[-1]['registry_count'] = True
        duplicate_key = jsonl(rows).replace(b'"kind": "registry-singletons"',
            b'"kind": "registry-singletons", "kind": "registry-singletons"', 1)
        cases = [jsonl(rows[:-1]), jsonl(rows + [rows[-1]]), jsonl(rows)[:-1],
                 jsonl(rows).replace(b'\n', b'\r\n'), jsonl(malformed), jsonl(wrong_counts),
                 jsonl(false_registry), duplicate_key, jsonl(rows).replace(b'null', b'NaN', 1),
                 jsonl(rows[1:] + rows[:1])]
        for raw in cases:
            with self.subTest(raw_start=raw[:100]):
                self.path.write_bytes(raw)
                with self.assertRaises(ValueError):
                    gate.validate_output(self.path)

    def test_layout_is_separate_strict_positive_integer_record(self):
        self.path.write_bytes(jsonl([layout(True)]))
        self.assertEqual(gate.validate_layout(self.path)['volatiles'], 64)
        for field, value in [('volatiles', 0), ('state1', True), ('kind', 'complete'), ('schema_version', True)]:
            changed = layout()
            changed[field] = value
            self.path.write_bytes(jsonl([changed]))
            with self.assertRaises(ValueError):
                gate.validate_layout(self.path)

    def test_off_guard_must_execute_with_unfiltered_passing_core_suite(self):
        good = f'test {gate.OFF_GUARD} ... ok\ntest another ... ok\n' \
               'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'
        self.path.write_text(good)
        self.assertEqual(gate.validate_off_guard(self.path)['guard_passes'], 1)
        for text in [good.replace(gate.OFF_GUARD, 'wrong_guard'),
                     good.replace('0 filtered out', '1 filtered out'),
                     good.replace('2 passed', '3 passed'),
                     good + f'test {gate.OFF_GUARD} ... ok\n']:
            self.path.write_text(text)
            with self.assertRaises(ValueError):
                gate.validate_off_guard(self.path)


class FreshGateTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name).resolve()
        self.results = self.root/'ci-results'
        self.results.mkdir()
        self.out = self.results/'compact-validation'
        (self.results/'request.json').write_text(json.dumps({'candidate_feature': 'compact-volatiles',
            'baseline_sha': 'a'*40, 'candidate_sha': 'b'*40}))
        (self.results/'provenance.json').write_text(json.dumps({'rustc': 'mock rustc', 'cargo': 'mock cargo'}))
        for tree in ('baseline', 'candidate'):
            probe = self.root/tree/gate.PROBE_SOURCE
            probe.parent.mkdir(parents=True)
            probe.write_bytes(Path(gate.__file__).with_suffix('.rs').read_bytes())
            (self.root/tree/'engine/Cargo.lock').write_text('immutable lock\n')
        self.calls = []
        self.wrong_feature = False
        self.missing_fingerprint = False
        self.nonzero_probe = False
        self.byte_mismatch = False
        self.layout_mismatch = False
        self.guard_missing = False
        self.mutate_source = False
        self.timeout = False
        env = patch.dict(os.environ, {'RUSTFLAGS': '-Ctarget-cpu=x86-64'}, clear=True)
        env.start()
        self.addCleanup(env.stop)
        for item in (patch.object(gate.platform, 'system', return_value='Linux'),
                     patch.object(gate.platform, 'machine', return_value='x86_64'),
                     patch.object(gate.ci, 'verify_prepared', side_effect=self.verify),
                     patch.object(gate.os, 'access', return_value=True)):
            item.start()
            self.addCleanup(item.stop)

    def verify(self, workspace):
        self.assertEqual(workspace, self.root)
        (self.results/'prepared-source-verification.json').write_text('{"status":"success"}')

    def fingerprint(self, target, package, name, features):
        path = target/'release/.fingerprint'/(package+'-mock')/name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({'features': json.dumps(features)}))

    def process_class(self):
        owner = self
        class Process:
            def __init__(self, argv, *, cwd, env, stdout, stderr, start_new_session):
                owner.calls.append((list(argv), Path(cwd), dict(env)))
                owner.assertTrue(start_new_session)
                self.returncode = 0
                self.pid = 987654
                self.waited = False
                target = Path(env['CARGO_TARGET_DIR'])
                compact = target.name == 'target-compact-probe-candidate'
                features = [gate.HURT] + ([gate.COMPACT] if compact else [])
                if owner.wrong_feature and compact:
                    features.append('experiment-prepared-turn-observe')
                if argv[0] == 'cargo':
                    owner.assertIn('--locked', argv)
                    owner.assertIn('--release', argv)
                    owner.assertEqual(env['RUSTFLAGS'], '-Ctarget-cpu=x86-64')
                    owner.assertEqual(env['CARGO_INCREMENTAL'], '0')
                    owner.fingerprint(target, 'lab-engine', 'lib-lab_engine.json', features)
                    if argv[1] == 'test':
                        owner.assertEqual(argv[-2:], ['--', '--test-threads=1'])
                        owner.fingerprint(target, 'lab-engine', 'test-lib-lab_engine.json', features)
                        guard = 'wrong_guard' if owner.guard_missing else gate.OFF_GUARD
                        stdout.write((f'test {guard} ... ok\n'
                                      'test result: ok. 1 passed; 0 failed; 0 ignored; '
                                      '0 measured; 0 filtered out;\n').encode())
                    else:
                        owner.fingerprint(target, 'lab-scenario', 'lib-lab_scenario.json', [])
                        if not owner.missing_fingerprint:
                            owner.fingerprint(target, 'lab-scenario', 'example-ci_compact_probe.json', [])
                        binary = target/'release/examples'/gate.PROBE_EXAMPLE
                        binary.parent.mkdir(parents=True, exist_ok=True)
                        binary.write_bytes(b'inert never executed')
                        binary.chmod(0o755)
                elif argv[-1] == '--layout':
                    value = layout(compact)
                    if owner.layout_mismatch and target.name == 'target-compact-off':
                        value['slot'] = 999
                    stdout.write(jsonl([value]))
                else:
                    owner.assertEqual(argv[-1], str(owner.root/'baseline/engine'))
                    rows = output_rows()
                    if owner.byte_mismatch and compact:
                        rows[0]['pattern'] = 999
                    if owner.nonzero_probe:
                        self.returncode = 7
                        rows = rows[:2]
                        stderr.write(b'intentional mock probe failure\n')
                    stdout.write(jsonl(rows))
                    if owner.mutate_source:
                        (owner.root/'candidate/engine/Cargo.lock').write_text('changed lock')
                stdout.flush()
                stderr.flush()

            def wait(self, timeout=None):
                if owner.timeout and not self.waited:
                    self.waited = True
                    self.returncode = -9
                    raise gate.subprocess.TimeoutExpired('mock', timeout)
                return self.returncode
        return Process

    def run_gate(self):
        with patch.object(gate.subprocess, 'Popen', self.process_class()):
            return gate.validate_compact(self.root, self.out)

    def assert_failed_receipt(self):
        receipt = json.loads((self.out/'receipt.json').read_text())
        self.assertEqual(receipt['status'], 'failed')
        self.assertFalse(receipt['performance_measurement'])
        return receipt

    def test_three_fresh_targets_exact_bytes_layout_and_real_fingerprints(self):
        result = self.run_gate()
        self.assertEqual(result['status'], 'success')
        self.assertTrue(result['complete_jsonl_byte_equal'])
        self.assertTrue(result['source_inputs_unchanged'])
        self.assertEqual(len(result['commands']), 10)
        targets = Counter(Path(env['CARGO_TARGET_DIR']).name for _, _, env in self.calls)
        self.assertEqual(targets, {'target-compact-probe-baseline': 3,
                                  'target-compact-probe-candidate': 3, 'target-compact-off': 4})
        self.assertFalse((self.root/'target-baseline').exists())
        self.assertFalse((self.root/'target-candidate').exists())
        self.assertEqual(result['variants']['candidate-off']['off_api_guard']['guard_passes'], 1)
        for variant, value in result['variants'].items():
            self.assertEqual(len(value['compiler_feature_evidence']['fingerprints']),
                             4 if variant == 'candidate-off' else 3)
            self.assertTrue((self.out/variant/'probe.stdout').is_file())
            self.assertTrue((self.out/variant/'layout.stdout').is_file())
        self.assertTrue(all('stdout_sha256' in command and 'stderr_sha256' in command
                            for command in result['commands']))

    def test_preexisting_any_validation_target_is_refused_before_cargo(self):
        (self.root/'target-compact-off').mkdir()
        with self.assertRaisesRegex(ValueError, 'must be fresh'):
            self.run_gate()
        self.assertFalse(self.calls)
        self.assert_failed_receipt()

    def test_probe_failure_preserves_partial_stdout_stderr_and_exit_code(self):
        self.nonzero_probe = True
        with self.assertRaisesRegex(RuntimeError, r'failed \(7\)'):
            self.run_gate()
        receipt = self.assert_failed_receipt()
        self.assertEqual(receipt['commands'][-1]['returncode'], 7)
        self.assertEqual(len((self.out/'baseline/probe.stdout').read_bytes().splitlines()), 2)
        self.assertIn('mock probe failure', (self.out/'baseline/probe.stderr').read_text())

    def test_actual_feature_mismatch_fails_before_candidate_execution(self):
        self.wrong_feature = True
        with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
            self.run_gate()
        self.assert_failed_receipt()
        self.assertFalse((self.out/'candidate-on/probe.stdout').exists())

    def test_missing_compiler_evidence_is_fatal(self):
        self.missing_fingerprint = True
        with self.assertRaisesRegex(ValueError, 'missing compiled'):
            self.run_gate()
        self.assert_failed_receipt()

    def test_byte_difference_is_not_hidden_by_successful_exit_or_counts(self):
        self.byte_mismatch = True
        with self.assertRaisesRegex(ValueError, 'byte-identical'):
            self.run_gate()
        self.assert_failed_receipt()
        self.assertTrue((self.out/'candidate-off/probe.stdout').is_file())

    def test_default_off_layout_difference_is_fatal(self):
        self.layout_mismatch = True
        with self.assertRaisesRegex(ValueError, 'Default-off layout differs'):
            self.run_gate()
        self.assert_failed_receipt()

    def test_missing_api_guard_does_not_count_as_successful_core_gate(self):
        self.guard_missing = True
        with self.assertRaisesRegex(ValueError, 'guard must execute'):
            self.run_gate()
        self.assert_failed_receipt()
        self.assertFalse((self.out/'candidate-off/build.stdout').exists())

    def test_changed_source_inputs_cannot_produce_success_receipt(self):
        self.mutate_source = True
        with self.assertRaisesRegex(ValueError, 'changed during'):
            self.run_gate()
        self.assert_failed_receipt()

    def test_timeout_kills_group_and_preserves_failed_command(self):
        self.timeout = True
        with patch.object(gate.os, 'killpg', create=True) as kill:
            with self.assertRaisesRegex(RuntimeError, 'timed out'):
                self.run_gate()
        kill.assert_called_once_with(987654, gate.KILL_SIGNAL)
        receipt = self.assert_failed_receipt()
        self.assertEqual(receipt['commands'][-1]['returncode'], -9)

    def test_output_collision_and_non_generic_environment_are_refused(self):
        with patch.dict(os.environ, {'RUSTFLAGS': '-Ctarget-cpu=native'}):
            with self.assertRaisesRegex(ValueError, 'generic'):
                self.run_gate()
        self.assertFalse(self.calls)
        self.assert_failed_receipt()
        with self.assertRaises(FileExistsError):
            self.run_gate()


if __name__ == '__main__':
    unittest.main()
