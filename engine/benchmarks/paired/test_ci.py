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

    def test_compact_request_records_direct_core_features_and_refuses_smoke(self):
        for suite in ('narrow', 'smoke'):
            with self.subTest(suite=suite), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                with patch.dict(os.environ, self.env(root, CANDIDATE_FEATURE='compact-volatiles', SUITE=suite), clear=True):
                    if suite == 'smoke':
                        with self.assertRaisesRegex(ValueError, 'full narrow'):
                            ci.refs(root)
                        self.assertFalse((root/'ci-results/request.json').exists())
                        continue
                    ci.refs(root)
                metadata = json.loads((root/'ci-results/request.json').read_text())
                self.assertEqual(metadata['feature_args'], {
                    'baseline': ['--features', 'lab-engine/experiment-hurt-readers'],
                    'candidate': ['--features', 'lab-engine/experiment-hurt-readers,lab-engine/experiment-compact-volatiles']})


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


class CompactModeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        (self.root/'ci-results').mkdir()
        self.controller = self.root/'controller'
        self.controller.mkdir()
        (self.controller/'harness.rs').write_bytes(b'// inert timing harness\n')
        (self.controller/'compact_probe.rs').write_bytes(b'// inert independent compact probe\n')
        self.core = ('[package]\nname="lab-engine"\n[features]\n'
                     'experiment-hurt-readers=[]\nexperiment-compact-volatiles=[]\n')
        self.search = '[package]\nname="lab-search"\n[features]\ndefault=["cli"]\ncli=[]\n'
        for label in ('baseline', 'candidate'):
            source = self.root/label/'engine'
            source.mkdir(parents=True)
            (source/'Cargo.lock').write_text('same lock', encoding='utf-8')
            (source/'Cargo.toml').write_text('[workspace]\n', encoding='utf-8')
            for package in ('core', 'search', 'scenario', 'py'):
                folder = source/package
                folder.mkdir()
                text = self.core if package == 'core' else self.search if package == 'search' else '[package]\nname="fixture"\n'
                (folder/'Cargo.toml').write_text(text, encoding='utf-8')
        self.request = {'suite': 'narrow', 'candidate_feature': 'compact-volatiles',
                        'baseline_sha': 'a'*40, 'candidate_sha': 'b'*40,
                        'feature_args': {label: ci.feature_args('compact-volatiles', label)
                                         for label in ('baseline', 'candidate')}}
        self.write_request()
        self.untracked = ['engine/scenario/examples/ci_compact_probe.rs', 'engine/search/examples/ci_bench.rs']

        def command(argv, cwd=None):
            if argv[0] in ('cargo', 'rustc'):
                return 'mock toolchain identity'
            if argv == ['git', 'rev-parse', 'HEAD']:
                return self.request[Path(cwd).name + '_sha']
            if argv[:2] == ['git', 'status']:
                return ''
            if argv == ['git', 'ls-files', '--others', '--exclude-standard']:
                return '\n'.join(self.untracked)
            raise AssertionError(f'Unexpected command: {argv}')

        for active in (patch.object(ci, '__file__', str(self.controller/'ci.py')),
                       patch.object(ci, 'output', side_effect=command)):
            active.start()
            self.addCleanup(active.stop)

    def write_request(self):
        (self.root/'ci-results/request.json').write_text(json.dumps(self.request), encoding='utf-8')

    def fingerprints(self, label, *, wrong=None, missing=None):
        core_features = ['experiment-hurt-readers']
        if label == 'candidate':
            core_features.append('experiment-compact-volatiles')
        for package, kinds, features in (
                ('lab-engine', ('lib-lab_engine.json', 'test-lib-lab_engine.json'), core_features),
                ('lab-search', ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json'), ['cli'])):
            folder = self.root/('target-'+label)/'release/.fingerprint'/(package+'-mock')
            folder.mkdir(parents=True, exist_ok=True)
            for kind in kinds:
                if (package, kind) == missing:
                    continue
                actual = wrong[2] if wrong and wrong[:2] == (package, kind) else features
                (folder/kind).write_text(json.dumps({'features': json.dumps(actual)}), encoding='utf-8')

    def test_compact_routes_core_features_only_and_serializes_full_narrow_tests(self):
        for label in ('baseline', 'candidate'):
            features = 'lab-engine/experiment-hurt-readers'
            if label == 'candidate':
                features += ',lab-engine/experiment-compact-volatiles'
            commands = ci.build_commands('narrow', 'compact-volatiles', label)
            self.assertEqual(len(commands), 2)
            test, build = commands
            self.assertEqual(test[:10], ['cargo', 'test', '--locked', '--release', '-p', 'lab-engine',
                                         '-p', 'lab-scenario', '-p', 'lab-search'])
            self.assertEqual(test[-4:], ['--features', features, '--', '--test-threads=1'])
            self.assertEqual(build[-2:], ['--features', features])
            self.assertNotIn('--', build)
            self.assertNotIn('--lib', test)
            self.assertNotIn('lab-search/experiment-', ' '.join(test + build))
            with self.assertRaisesRegex(ValueError, 'full narrow'):
                ci.build_commands('smoke', 'compact-volatiles', label)

    def test_compact_requires_empty_core_features_and_recursive_default_off(self):
        root = self.root/'baseline'
        self.assertEqual(ci.verify_compact_declarations(root)['core'], {'experiment-compact-volatiles': []})
        invalid = [self.core.replace('experiment-compact-volatiles=[]\n', ''),
                   self.core.replace('experiment-compact-volatiles=[]', 'experiment-compact-volatiles=["other"]'),
                   self.core.replace('experiment-hurt-readers=[]\n', ''),
                   self.core + 'default=["experiment-compact-volatiles"]\n',
                   self.core + 'default=["alias"]\nalias=["experiment-compact-volatiles"]\n']
        for text in invalid:
            with self.subTest(manifest=text):
                (root/'engine/core/Cargo.toml').write_text(text, encoding='utf-8')
                with self.assertRaises(ValueError):
                    ci.verify_compact_declarations(root)

    def test_compact_rejects_search_declaration_forwarding_and_other_default_experiments(self):
        root = self.root/'baseline'
        invalid = [self.search + 'experiment-compact-volatiles=[]\n',
                   self.search + 'alias=["lab-engine/experiment-compact-volatiles"]\n',
                   self.search.replace('default=["cli"]', 'default=["alias"]') +
                   'alias=["experiment-prepared-turn"]\nexperiment-prepared-turn=[]\n',
                   self.search.replace('default=["cli"]', 'default=["alias"]') +
                   'alias=["lab-engine/experiment-leaf-ending-observer"]\n']
        for text in invalid:
            with self.subTest(manifest=text):
                (root/'engine/search/Cargo.toml').write_text(text, encoding='utf-8')
                with self.assertRaises(ValueError):
                    ci.verify_compact_declarations(root)

    def test_prepare_injects_identical_probe_and_hashes_then_verifies_exact_allowed_paths(self):
        ci.prepare(self.root)
        metadata = json.loads((self.root/'ci-results/provenance.json').read_text())
        self.assertEqual(set(metadata['injected_source_sha256']), set(self.untracked))
        for label in ('baseline', 'candidate'):
            self.assertEqual((self.root/label/ci.COMPACT_PROBE_PATH).read_bytes(),
                             (self.controller/'compact_probe.rs').read_bytes())
            self.assertEqual(metadata['sources'][label]['compact_declarations']['search_forwarding'], False)
        self.assertEqual(ci.verify_prepared(self.root)['status'], 'success')
        self.untracked.append('engine/scenario/examples/unexpected.rs')
        with self.assertRaisesRegex(ValueError, 'unexpected untracked'):
            ci.verify_prepared(self.root)

    def test_noncompact_mode_never_authorizes_or_injects_compact_probe(self):
        self.request['candidate_feature'] = 'none'
        self.request['feature_args'] = {'baseline': [], 'candidate': []}
        self.write_request()
        ci.prepare(self.root)
        for label in ('baseline', 'candidate'):
            self.assertFalse((self.root/label/ci.COMPACT_PROBE_PATH).exists())
        self.assertEqual(set(ci.injected_sources('none')), {'engine/search/examples/ci_bench.rs'})
        with self.assertRaisesRegex(ValueError, 'unexpected untracked'):
            ci.verify_prepared(self.root)

    def test_prepare_refuses_existing_probe_and_manifest_mismatch_before_any_injection(self):
        reserved = self.root/'candidate'/ci.COMPACT_PROBE_PATH
        reserved.parent.mkdir(parents=True)
        reserved.write_bytes(b'existing reserved file')
        with self.assertRaisesRegex(ValueError, 'Reserved injected example'):
            ci.prepare(self.root)
        self.assertFalse((self.root/'baseline/engine/search/examples/ci_bench.rs').exists())
        reserved.unlink()
        manifest = self.root/'candidate/engine/scenario/Cargo.toml'
        manifest.write_text(manifest.read_text()+'# differing manifest bytes\n', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'differ in package_manifests'):
            ci.prepare(self.root)
        self.assertFalse((self.root/'baseline'/ci.COMPACT_PROBE_PATH).exists())

    def test_verify_prepared_rejects_controller_or_injected_probe_changes(self):
        ci.prepare(self.root)
        for path in (self.controller/'compact_probe.rs', self.root/'candidate'/ci.COMPACT_PROBE_PATH):
            original = path.read_bytes()
            try:
                path.write_bytes(original + b'changed\n')
                with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'changed after prepare'):
                    ci.verify_prepared(self.root)
                self.assertEqual(json.loads((self.root/'ci-results/prepared-source-verification.json').read_text())['status'], 'failed')
            finally:
                path.write_bytes(original)

    def test_compact_accepts_actual_core_and_search_fingerprints_for_both_labels(self):
        for label in ('baseline', 'candidate'):
            self.fingerprints(label)
            evidence = ci.preserve_fingerprints(self.root, label, 'compact-volatiles')
            self.assertEqual(len(evidence['fingerprints']), 5)
            self.assertTrue(evidence['expected_active'])
            self.assertEqual(evidence['expected_by_package']['lab-engine']['experiment-compact-volatiles'], label == 'candidate')
            self.assertFalse(evidence['expected_by_package']['lab-search']['experiment-compact-volatiles'])
            self.assertFalse(evidence['expected_by_package']['lab-search']['experiment-hurt-readers'])

    def test_compact_missing_any_required_fingerprint_is_refused(self):
        for package, names in (('lab-engine', ('lib-lab_engine.json', 'test-lib-lab_engine.json')),
                               ('lab-search', ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json'))):
            for name in names:
                with self.subTest(package=package, name=name):
                    self.fingerprints('candidate')
                    path = self.root/'target-candidate/release/.fingerprint'/(package+'-mock')/name
                    original = path.read_bytes()
                    path.unlink()
                    try:
                        with self.assertRaisesRegex(ValueError, 'missing compiled'):
                            ci.preserve_fingerprints(self.root, 'candidate', 'compact-volatiles')
                    finally:
                        path.write_bytes(original)

    def test_compact_wrong_activation_and_p9_p8c_observers_are_refused_in_real_fingerprints(self):
        hurt, compact = 'experiment-hurt-readers', 'experiment-compact-volatiles'
        cases = [('baseline', 'lab-engine', 'lib-lab_engine.json', [hurt, compact]),
                 ('candidate', 'lab-engine', 'test-lib-lab_engine.json', [hurt]),
                 ('baseline', 'lab-engine', 'test-lib-lab_engine.json', []),
                 ('candidate', 'lab-search', 'example-ci_bench.json', [compact]),
                 ('candidate', 'lab-search', 'test-lib-lab_search.json', [hurt])]
        for forbidden in ('experiment-leaf-ending-states', 'experiment-leaf-ending-observer',
                          'experiment-prepared-turn', 'experiment-prepared-turn-observe'):
            cases += [('candidate', 'lab-engine', 'lib-lab_engine.json', [hurt, compact, forbidden]),
                      ('baseline', 'lab-search', 'lib-lab_search.json', [forbidden])]
        for label, package, kind, features in cases:
            with self.subTest(label=label, package=package, kind=kind, features=features):
                self.fingerprints(label, wrong=(package, kind, features))
                with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                    ci.preserve_fingerprints(self.root, label, 'compact-volatiles')
                self.assertTrue((self.root/'ci-results'/(label+'-features.json')).is_file())

    def test_compact_build_runs_full_serial_regressions_without_prepared_observer_gate(self):
        ci.prepare(self.root)
        owner, calls = self, []

        class Process:
            returncode = 0

            def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                label = cwd.parent.name
                calls.append((label, argv))
                owner.assertEqual(env['CARGO_TARGET_DIR'], str(owner.root/('target-'+label)))
                owner.fingerprints(label)
                stdout.write('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')

            def wait(self, timeout):
                owner.assertEqual(timeout, ci.COMMAND_TIMEOUT_SECONDS)
                return 0

        with patch.dict(os.environ, {'BUILD_CACHE_ENABLED': '0', 'BUILD_CACHE_ALLOW_SAVE': '0'}, clear=True), \
                patch.object(ci.subprocess, 'Popen', Process), patch.object(ci, 'validate_prepared_turn') as prepared:
            ci.build(self.root)
        prepared.assert_not_called()
        self.assertEqual(calls, [(label, argv) for label in ('baseline', 'candidate')
                                 for argv in ci.build_commands('narrow', 'compact-volatiles', label)])
        for label in ('baseline', 'candidate'):
            receipt = json.loads((self.root/'ci-results'/(label+'-build-receipt.json')).read_text())
            self.assertEqual(receipt['status'], 'success')
            self.assertFalse(receipt['reused'])


if __name__ == '__main__':
    unittest.main()
