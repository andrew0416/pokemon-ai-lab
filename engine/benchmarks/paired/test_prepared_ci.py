"""P8c CI gates: isolated activation, real compiler evidence, and observer-only tests."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci


TIMING_KINDS = {
    'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json'),
    'lab-search': ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json'),
}
VALIDATION_KINDS = {
    'lab-engine': ('lib-lab_engine.json',),
    'lab-search': ('lib-lab_search.json', 'test-integration-test-prepared_turn.json'),
}
BASE_FEATURES = 'lab-engine/experiment-hurt-readers'
CANDIDATE_FEATURES = BASE_FEATURES + ',lab-search/experiment-prepared-turn'
VALIDATION_FEATURES = BASE_FEATURES + ',lab-search/experiment-prepared-turn-observe'


def manifests(root):
    definitions = {
        'core': '[features]\nexperiment-hurt-readers=[]\n'
                'experiment-prepared-turn=[]\n'
                'experiment-prepared-turn-observe=["experiment-prepared-turn"]\n',
        'search': '[features]\ndefault=["cli"]\ncli=["scenario"]\nscenario=[]\n'
                  'experiment-prepared-turn=["lab-engine/experiment-prepared-turn"]\n'
                  'experiment-prepared-turn-observe=["experiment-prepared-turn",'
                  '"lab-engine/experiment-prepared-turn-observe"]\n',
    }
    for package, body in definitions.items():
        path = root/'engine'/package/'Cargo.toml'
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding='utf-8')
    return definitions


def fingerprints(root, label, *, missing=None, wrong=None):
    validating = label == 'prepared-validation'
    kinds = VALIDATION_KINDS if validating else TIMING_KINDS
    for package, names in kinds.items():
        directory = root/('target-' + label)/'release/.fingerprint'/(package + '-testhash')
        directory.mkdir(parents=True, exist_ok=True)
        for name in names:
            if (package, name) == missing:
                continue
            features = ['experiment-hurt-readers'] if package == 'lab-engine' else []
            if label != 'baseline':
                features.append('experiment-prepared-turn')
            if validating:
                features.append('experiment-prepared-turn-observe')
            if wrong and (package, name) == wrong[:2]:
                features = wrong[2]
            # Both formats supported by the existing Cargo evidence reader.
            encoded = features if name.startswith('test-') else json.dumps(features)
            (directory/name).write_text(json.dumps({'features': encoded}), encoding='utf-8')


def request(root, suite='narrow'):
    result = root/'ci-results'
    result.mkdir()
    data = {'suite': suite, 'baseline_sha': 'a'*40, 'candidate_sha': 'b'*40,
            'candidate_feature': 'prepared-turn',
            'feature_args': {'baseline': ['--features', BASE_FEATURES],
                             'candidate': ['--features', CANDIDATE_FEATURES]}}
    (result/'request.json').write_text(json.dumps(data), encoding='utf-8')
    (result/'provenance.json').write_text('{}', encoding='utf-8')
    return data


def test_output(names=ci.PREPARED_TESTS, summary=None):
    text = ''.join(f'test {name} ... ok\n' for name in names)
    return text + (summary or 'test result: ok. 9 passed; 0 failed; 0 ignored; '
                              '0 measured; 0 filtered out; finished in 0.01s\n')


class PreparedModeTests(unittest.TestCase):
    def test_request_pins_prepared_activation_for_both_builds(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            env = {'BASELINE_SHA': 'a'*40, 'CANDIDATE_SHA': 'b'*40, 'GITHUB_SHA': 'c'*40,
                   'CANDIDATE_FEATURE': 'prepared-turn', 'SUITE': 'narrow', 'THREADS': '1',
                   'PAIRS': '6', 'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1',
                   'GITHUB_OUTPUT': str(root/'outputs')}
            with patch.dict(os.environ, env, clear=True):
                ci.refs(root)
            recorded = json.loads((root/'ci-results/request.json').read_text())
            self.assertEqual(recorded['candidate_feature'], 'prepared-turn')
            self.assertEqual(recorded['feature_args'], {
                'baseline': ['--features', BASE_FEATURES],
                'candidate': ['--features', CANDIDATE_FEATURES]})

    def test_routes_timing_features_without_observers_and_preserves_old_modes(self):
        expected = {
            'none': ([], []),
            'hurt-readers': ([], ['--features', BASE_FEATURES]),
            'leaf-ending-states': (['--features', BASE_FEATURES],
                                  ['--features', BASE_FEATURES + ',lab-search/experiment-leaf-ending-states']),
            'prepared-turn': (['--features', BASE_FEATURES], ['--features', CANDIDATE_FEATURES]),
        }
        for suite in ('smoke', 'narrow'):
            plain = ci.build_commands(suite, 'none', 'baseline')
            for mode, routed in expected.items():
                for label, flags in zip(('baseline', 'candidate'), routed):
                    with self.subTest(suite=suite, mode=mode, label=label):
                        commands = ci.build_commands(suite, mode, label)
                        self.assertEqual(commands, [command + flags for command in plain])
                        self.assertNotIn('observe', ' '.join(sum(commands, [])))
        narrow = ci.build_commands('narrow', 'prepared-turn', 'candidate')[0]
        self.assertNotIn('--lib', narrow)
        self.assertEqual(narrow[4:10], ['-p', 'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search'])

    def test_exact_declarations_forwarding_and_recursive_default_guard(self):
        changes = [
            ('core', 'experiment-prepared-turn=[]', ''),
            ('core', 'experiment-prepared-turn=[]', 'experiment-prepared-turn=["other"]'),
            ('core', 'experiment-prepared-turn-observe=["experiment-prepared-turn"]', ''),
            ('search', 'experiment-prepared-turn=["lab-engine/experiment-prepared-turn"]',
             'experiment-prepared-turn=[]'),
            ('search', '"lab-engine/experiment-prepared-turn-observe"',
             '"lab-engine/experiment-prepared-turn"'),
            ('search', 'scenario=[]', 'scenario=["alias"]\nalias=["experiment-prepared-turn"]'),
            ('search', 'scenario=[]', 'scenario=["lab-engine/experiment-prepared-turn-observe"]'),
            ('core', '[features]', '[features]\ndefault=["experiment-prepared-turn-observe"]'),
        ]
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bodies = manifests(root)
            self.assertEqual(set(ci.verify_prepared_declarations(root)), {'core', 'search'})
            ci.verify_feature_declaration(root/'engine/core/Cargo.toml', 'prepared-turn')
            for package, before, after in changes:
                with self.subTest(package=package, after=after):
                    manifests(root)
                    (root/'engine'/package/'Cargo.toml').write_text(
                        bodies[package].replace(before, after), encoding='utf-8')
                    with self.assertRaises(ValueError):
                        ci.verify_prepared_declarations(root)
            manifests(root)
            core = root/'engine/core/Cargo.toml'
            core.write_text(bodies['core'].replace('experiment-hurt-readers=[]\n', ''), encoding='utf-8')
            with self.assertRaises(ValueError):
                ci.verify_feature_declaration(core, 'prepared-turn')

    def test_prepare_rejects_unmatched_build_inputs_before_harness_injection(self):
        for changed in (None, 'engine/core/Cargo.toml', 'engine/search/Cargo.toml',
                        'engine/Cargo.lock', 'engine/Cargo.toml', 'engine/.cargo/config.toml'):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                data = request(root)
                for label in ('baseline', 'candidate'):
                    manifests(root/label)
                    for name in ('engine/Cargo.lock', 'engine/Cargo.toml', 'engine/scenario/Cargo.toml',
                                 'engine/py/Cargo.toml', 'engine/.cargo/config.toml'):
                        path = root/label/name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_text('# identical\n', encoding='utf-8')
                if changed:
                    with (root/'candidate'/changed).open('a', encoding='utf-8') as stream:
                        stream.write('# changed\n')

                def fake_output(argv, cwd=None):
                    if argv == ['git', 'rev-parse', 'HEAD']:
                        return data[cwd.name + '_sha']
                    return '' if argv[:2] == ['git', 'status'] else 'test toolchain'

                with patch.object(ci, 'output', fake_output):
                    if changed:
                        with self.assertRaisesRegex(ValueError, 'strict source-only benchmark refused'):
                            ci.prepare(root)
                        self.assertFalse((root/'baseline/engine/search/examples/ci_bench.rs').exists())
                    else:
                        ci.prepare(root)
                        evidence = json.loads((root/'ci-results/provenance.json').read_text())
                        for label in ('baseline', 'candidate'):
                            self.assertIn('prepared_declarations', evidence['sources'][label])
                        self.assertEqual(ci.sha(root/'baseline/engine/search/examples/ci_bench.rs'),
                                         ci.sha(root/'candidate/engine/search/examples/ci_bench.rs'))

    def test_preserves_ten_timing_fingerprints_with_observers_off(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            all_evidence = []
            for label in ('baseline', 'candidate'):
                fingerprints(root, label)
                evidence = ci.preserve_fingerprints(root, label, 'prepared-turn')
                self.assertEqual(len(evidence['fingerprints']), 5)
                for item in evidence['fingerprints']:
                    self.assertEqual(ci.sha(root/'ci-results'/item['artifact_path']), item['sha256'])
                    self.assertEqual('experiment-prepared-turn' in item['features'], label == 'candidate')
                    self.assertNotIn('experiment-prepared-turn-observe', item['features'])
                all_evidence.extend(evidence['fingerprints'])
            self.assertEqual(len(all_evidence), 10)

    def test_refuses_every_missing_timing_build_kind(self):
        for label in ('baseline', 'candidate'):
            for package, names in TIMING_KINDS.items():
                for name in names:
                    with self.subTest(label=label, kind=name), tempfile.TemporaryDirectory() as folder:
                        root = Path(folder)
                        request(root)
                        fingerprints(root, label, missing=(package, name))
                        with self.assertRaisesRegex(ValueError, 'missing compiled'):
                            ci.preserve_fingerprints(root, label, 'prepared-turn')
                        self.assertTrue((root/'ci-results'/f'{label}-features.json').is_file())

    def test_refuses_wrong_candidate_baseline_observer_and_other_experiment_activation(self):
        cases = [
            ('baseline', 'lab-engine', 'lib-lab_engine.json', []),
            ('baseline', 'lab-search', 'lib-lab_search.json', ['experiment-prepared-turn']),
            ('candidate', 'lab-engine', 'test-lib-lab_engine.json', ['experiment-hurt-readers']),
            ('candidate', 'lab-search', 'example-ci_bench.json', []),
            ('candidate', 'lab-search', 'test-lib-lab_search.json',
             ['experiment-prepared-turn', 'experiment-prepared-turn-observe']),
            ('baseline', 'lab-engine', 'lib-lab_engine.json',
             ['experiment-hurt-readers', 'experiment-prepared-turn-observe']),
            ('candidate', 'lab-search', 'lib-lab_search.json',
             ['experiment-prepared-turn', 'experiment-leaf-ending-states']),
            ('candidate', 'lab-search', 'example-ci_bench.json',
             ['experiment-prepared-turn', 'experiment-leaf-ending-observer']),
        ]
        for label, package, name, features in cases:
            with self.subTest(label=label, kind=name, features=features), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root)
                fingerprints(root, label, wrong=(package, name, features))
                with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                    ci.preserve_fingerprints(root, label, 'prepared-turn')
                self.assertTrue((root/'ci-results'/f'{label}-features.json').is_file())

    def fake_process(self, root, calls, *, validation_text=None, validation_wrong=None):
        owner = self

        class FakeProcess:
            returncode = 0

            def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                label = Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                calls.append((label, argv))
                owner.assertEqual(env['CARGO_TARGET_DIR'], str(root/('target-' + label)))
                if label == 'prepared-validation':
                    owner.assertEqual(cwd, root/'candidate/engine')
                    owner.assertEqual(argv[-2:], ['--features', VALIDATION_FEATURES])
                    fingerprints(root, label, wrong=validation_wrong)
                    stdout.write(test_output() if validation_text is None else validation_text)
                else:
                    owner.assertEqual(cwd, root/label/'engine')
                    owner.assertNotIn('observe', ' '.join(argv))
                    fingerprints(root, label)

            def wait(self, timeout):
                owner.assertEqual(timeout, ci.COMMAND_TIMEOUT_SECONDS)
                return 0

        return FakeProcess

    def test_build_runs_nine_observer_tests_in_separate_target_after_timing_builds(self):
        for suite in ('smoke', 'narrow'):
            with self.subTest(suite=suite), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root, suite)
                calls = []
                with patch.object(ci.subprocess, 'Popen', self.fake_process(root, calls)):
                    ci.build(root)
                count = 3 if suite == 'smoke' else 2
                self.assertEqual([label for label, _ in calls],
                                 ['baseline']*count + ['candidate']*count + ['prepared-validation'])
                provenance = json.loads((root/'ci-results/provenance.json').read_text())
                receipt = json.loads((root/'ci-results/prepared-validation.json').read_text())
                self.assertEqual(provenance['prepared_validation'], receipt)
                self.assertEqual(receipt['status'], 'ok')
                self.assertEqual(len(receipt['passed_tests']), 9)
                self.assertEqual(len(receipt['compiler_feature_evidence']['fingerprints']), 3)
                # Observer validation never rewrites or contaminates the timed artifacts.
                for label in ('baseline', 'candidate'):
                    self.assertEqual(ci.preserve_fingerprints(root, label, 'prepared-turn'),
                                     provenance['compiler_feature_evidence'][label])

    def test_zero_filtered_or_missing_tests_cannot_pass_the_validation_gate(self):
        outputs = [
            'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n',
            test_output(ci.PREPARED_TESTS[:-1]),
            test_output(summary='test result: ok. 9 passed; 0 failed; 0 ignored; '
                                '0 measured; 1 filtered out;\n'),
        ]
        for text in outputs:
            with self.subTest(text=text[-100:]), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root)
                with patch.object(ci.subprocess, 'Popen', self.fake_process(root, [], validation_text=text)):
                    with self.assertRaisesRegex(ValueError, 'all nine named tests'):
                        ci.validate_prepared_turn(root)
                receipt = json.loads((root/'ci-results/prepared-validation.json').read_text())
                self.assertEqual(receipt['status'], 'failed')
                self.assertEqual(receipt['returncode'], 0)
                self.assertTrue((root/'ci-results/prepared-validation.log').is_file())

    def test_observer_validation_requires_actual_observer_compilation(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            wrong = ('lab-search', 'test-integration-test-prepared_turn.json', ['experiment-prepared-turn'])
            with patch.object(ci.subprocess, 'Popen', self.fake_process(root, [], validation_wrong=wrong)):
                with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                    ci.validate_prepared_turn(root)
            self.assertEqual(json.loads((root/'ci-results/prepared-validation.json').read_text())['status'],
                             'failed')
            self.assertTrue((root/'ci-results/prepared-validation-features.json').is_file())

    def test_reused_validation_target_is_rejected_without_running_cargo(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            (root/'target-prepared-validation').mkdir()
            with patch.object(ci.subprocess, 'Popen') as process:
                with self.assertRaisesRegex(ValueError, 'must be new'):
                    ci.validate_prepared_turn(root)
                process.assert_not_called()


if __name__ == '__main__':
    unittest.main()
