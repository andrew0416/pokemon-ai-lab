import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci


class RequestTests(unittest.TestCase):
    def env(self, root, **changes):
        return {'BASELINE_SHA': 'a'*40, 'CANDIDATE_SHA': '', 'GITHUB_SHA': 'b'*40,
                'CANDIDATE_FEATURE': 'none',
                'SUITE': 'smoke', 'THREADS': '1', 'PAIRS': '2', 'GITHUB_RUN_ID': '123',
                'GITHUB_RUN_ATTEMPT': '1', 'GITHUB_OUTPUT': str(root/'outputs'), **changes}

    def test_blank_candidate_pins_workflow_commit(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with patch.dict(os.environ, self.env(root), clear=True):
                ci.refs(root)
            metadata = json.loads((root/'ci-results/request.json').read_text())
            self.assertEqual(metadata['candidate_sha'], 'b'*40)
            self.assertEqual(metadata['candidate_feature'], 'none')
            self.assertEqual(metadata['feature_args'], {'baseline': [], 'candidate': []})
            self.assertIn('candidate_sha='+'b'*40, (root/'outputs').read_text())

    def test_rejects_unsafe_ref_and_unbalanced_or_unbounded_request(self):
        for change in ({'BASELINE_SHA': 'main; touch injected'}, {'PAIRS': '3'},
                       {'PAIRS': '22'}, {'THREADS': '16'}, {'SUITE': '../other'},
                       {'CANDIDATE_FEATURE': ''}, {'CANDIDATE_FEATURE': '--all-features'},
                       {'CANDIDATE_FEATURE': 'experiment-hurt-readers'}):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                with patch.dict(os.environ, self.env(root, **change), clear=True):
                    with self.assertRaises(ValueError):
                        ci.refs(root)
                self.assertFalse((root/'outputs').exists())

    def test_explicit_feature_is_recorded_only_for_candidate(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with patch.dict(os.environ, self.env(root, CANDIDATE_FEATURE='hurt-readers'), clear=True):
                ci.refs(root)
            metadata = json.loads((root/'ci-results/request.json').read_text())
            self.assertEqual(metadata['candidate_feature'], 'hurt-readers')
            self.assertEqual(metadata['feature_args']['baseline'], [])
            self.assertEqual(metadata['feature_args']['candidate'],
                             ['--features', 'lab-engine/experiment-hurt-readers'])

    def test_rejects_output_directory_reuse(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'ci-results').mkdir()
            with patch.dict(os.environ, self.env(root), clear=True):
                with self.assertRaises(FileExistsError):
                    ci.refs(root)


class FeatureTests(unittest.TestCase):
    def test_feature_is_routed_to_every_candidate_test_and_build_only(self):
        expected = ['--features', 'lab-engine/experiment-hurt-readers']
        for suite, expected_count in (('smoke', 3), ('narrow', 2)):
            baseline = ci.build_commands(suite, 'hurt-readers', 'baseline')
            candidate = ci.build_commands(suite, 'hurt-readers', 'candidate')
            self.assertEqual(len(candidate), expected_count)
            self.assertTrue(all('--features' not in argv for argv in baseline))
            self.assertEqual(candidate, [argv + expected for argv in baseline])
            self.assertEqual(ci.build_commands(suite, 'none', 'candidate'), baseline)
        regression = ci.build_commands('narrow', 'hurt-readers', 'candidate')[0]
        self.assertEqual(regression[:10], ['cargo', 'test', '--locked', '--release',
                                         '-p', 'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search'])
        self.assertEqual(regression.count('--timings'), 1)
        self.assertEqual(regression[-2:], expected)
        self.assertNotIn('--lib', regression)
        with self.assertRaises(ValueError):
            ci.feature_args('unknown', 'candidate')

    def test_requires_empty_declaration_and_rejects_default_activation(self):
        with tempfile.TemporaryDirectory() as folder:
            manifest = Path(folder)/'Cargo.toml'
            for body in ('[package]\nname="lab-engine"\n',
                         '[features]\nexperiment-hurt-readers=["dependency"]\n',
                         '[features]\nexperiment-hurt-readers=[]\ndefault=["experiment-hurt-readers"]\n',
                         '[features]\nexperiment-hurt-readers=[]\ndefault=["alias"]\nalias=["experiment-hurt-readers"]\n'):
                with self.subTest(body=body):
                    manifest.write_text(body, encoding='utf-8')
                    with self.assertRaises(ValueError):
                        ci.verify_feature_declaration(manifest, 'hurt-readers')
            manifest.write_text('[features]\nexperiment-hurt-readers=[]\n', encoding='utf-8')
            self.assertTrue(ci.verify_feature_declaration(manifest, 'hurt-readers')['declared_empty'])
            manifest.write_text('[package]\nname="lab-engine"\n', encoding='utf-8')
            self.assertFalse(ci.verify_feature_declaration(manifest, 'none')['declared_empty'])

    def make_fingerprints(self, root, label, *, active, missing=None):
        directory = root/('target-' + label)/'release/.fingerprint/lab-engine-testhash'
        directory.mkdir(parents=True, exist_ok=True)
        features = [ci.EXPERIMENT_FEATURE] if active else []
        for name in ('lib-lab_engine.json', 'test-lib-lab_engine.json'):
            if name != missing:
                # Current Cargo writes a JSON-encoded string; retain compatibility with
                # a parsed list, while refusing unparseable or absent feature evidence.
                value = json.dumps(features) if name == 'lib-lab_engine.json' else features
                (directory/name).write_text(json.dumps({'features': value}), encoding='utf-8')

    def test_preserves_actual_fingerprints_and_checks_both_build_kinds(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root/'ci-results').mkdir()
            for label, active in (('baseline', False), ('candidate', True)):
                self.make_fingerprints(root, label, active=active)
                evidence = ci.preserve_fingerprints(root, label, 'hurt-readers')
                self.assertEqual(evidence['expected_active'], active)
                self.assertEqual(len(evidence['fingerprints']), 2)
                for item in evidence['fingerprints']:
                    artifact = root/'ci-results'/item['artifact_path']
                    self.assertTrue(artifact.is_file())
                    self.assertEqual(ci.sha(artifact), item['sha256'])
                    self.assertEqual(ci.EXPERIMENT_FEATURE in item['features'], active)

    def test_refuses_missing_or_incorrect_compiler_activation_and_keeps_evidence(self):
        cases = [('baseline', True, None), ('candidate', False, None),
                 ('candidate', True, 'test-lib-lab_engine.json')]
        for label, active, missing in cases:
            with self.subTest(label=label, active=active, missing=missing), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                (root/'ci-results').mkdir()
                self.make_fingerprints(root, label, active=active, missing=missing)
                with self.assertRaises(ValueError):
                    ci.preserve_fingerprints(root, label, 'hurt-readers')
                self.assertTrue((root/'ci-results'/f'{label}-features.json').is_file())

    def test_build_records_commands_timeout_and_compiler_evidence(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            result = root/'ci-results'
            result.mkdir()
            request = {'suite': 'narrow', 'candidate_feature': 'hurt-readers',
                       'feature_args': {label: ci.feature_args('hurt-readers', label)
                                        for label in ('baseline', 'candidate')}}
            (result/'request.json').write_text(json.dumps(request), encoding='utf-8')
            (result/'provenance.json').write_text('{}', encoding='utf-8')
            commands, timeouts = [], []
            owner = self

            class FakeProcess:
                returncode = 0

                def __init__(self, argv, *, cwd, env, **kwargs):
                    label = cwd.parent.name
                    commands.append((label, argv))
                    owner.assertEqual(env['CARGO_TARGET_DIR'], str(root/('target-' + label)))
                    owner.make_fingerprints(root, label, active='--features' in argv)

                def wait(self, timeout):
                    timeouts.append(timeout)
                    return 0

            with patch.object(ci.subprocess, 'Popen', FakeProcess):
                ci.build(root)
            self.assertEqual([label for label, _ in commands],
                             ['baseline', 'baseline', 'candidate', 'candidate'])
            self.assertEqual(timeouts, [2700]*4)
            plan = json.loads((result/'test-plan.json').read_text())
            provenance = json.loads((result/'provenance.json').read_text())
            self.assertEqual(provenance['build_plan'], plan)
            self.assertEqual(set(provenance['compiler_feature_evidence']), {'baseline', 'candidate'})
            self.assertEqual(plan['commands_by_version']['candidate'],
                             [argv for label, argv in commands if label == 'candidate'])


if __name__ == '__main__':
    unittest.main()
