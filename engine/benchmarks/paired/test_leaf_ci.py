"""P9 mode guards: shared P8g, candidate-only P9, no timing observers."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci


class LeafModeTests(unittest.TestCase):
    def test_routes_common_base_and_candidate_feature_through_all_commands(self):
        for suite in ('smoke', 'narrow'):
            for label in ('baseline', 'candidate'):
                expected = 'lab-engine/experiment-hurt-readers'
                if label == 'candidate':
                    expected += ',lab-search/experiment-leaf-ending-states'
                commands = ci.build_commands(suite, 'leaf-ending-states', label)
                for command in commands:
                    self.assertEqual(command[-2:], ['--features', expected])
                    self.assertNotIn('observer', ' '.join(command))

    def manifests(self, root):
        definitions = {
            'core': '[features]\nexperiment-hurt-readers=[]\n'
                    'experiment-leaf-ending-states=[]\n'
                    'experiment-leaf-ending-observer=["experiment-leaf-ending-states"]\n',
            'search': '[features]\ndefault=["cli"]\ncli=[]\n'
                      'experiment-leaf-ending-states=["lab-engine/experiment-leaf-ending-states"]\n'
                      'experiment-leaf-ending-observer=["experiment-leaf-ending-states",'
                      '"lab-engine/experiment-leaf-ending-observer"]\n',
        }
        for package, body in definitions.items():
            path = root/'engine'/package/'Cargo.toml'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding='utf-8')
        return definitions

    def test_validates_exact_forwarding_and_rejects_default_observers(self):
        changes = [
            ('core', 'experiment-leaf-ending-states=[]', 'experiment-leaf-ending-states=["other"]'),
            ('core', 'experiment-leaf-ending-observer=["experiment-leaf-ending-states"]', ''),
            ('search', 'experiment-leaf-ending-states=["lab-engine/experiment-leaf-ending-states"]',
             'experiment-leaf-ending-states=[]'),
            ('search', 'cli=[]', 'cli=["alias"]\nalias=["experiment-leaf-ending-states"]'),
            ('search', 'cli=[]', 'cli=["lab-engine/experiment-leaf-ending-observer"]'),
            ('core', '[features]', '[features]\ndefault=["experiment-leaf-ending-observer"]'),
        ]
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bodies = self.manifests(root)
            ci.verify_feature_declaration(root/'engine/core/Cargo.toml', 'leaf-ending-states')
            self.assertEqual(set(ci.verify_leaf_declarations(root)), {'core', 'search'})
            for package, before, after in changes:
                with self.subTest(package=package, after=after):
                    self.manifests(root)
                    (root/'engine'/package/'Cargo.toml').write_text(
                        bodies[package].replace(before, after), encoding='utf-8')
                    with self.assertRaises(ValueError):
                        ci.verify_leaf_declarations(root)

    def test_prepare_keeps_exact_manifest_and_lock_equality(self):
        for changed in (None, 'engine/core/Cargo.toml', 'engine/search/Cargo.toml',
                        'engine/Cargo.toml', 'engine/Cargo.lock', 'engine/.cargo/config.toml'):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                result = root/'ci-results'
                result.mkdir()
                request = {'baseline_sha': 'a'*40, 'candidate_sha': 'b'*40,
                           'candidate_feature': 'leaf-ending-states',
                           'feature_args': {label: ci.feature_args('leaf-ending-states', label)
                                            for label in ('baseline', 'candidate')}}
                (result/'request.json').write_text(json.dumps(request), encoding='utf-8')
                for label in ('baseline', 'candidate'):
                    self.manifests(root/label)
                    for name in ('engine/Cargo.toml', 'engine/Cargo.lock',
                                 'engine/scenario/Cargo.toml', 'engine/py/Cargo.toml',
                                 'engine/.cargo/config.toml'):
                        path = root/label/name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_text('# identical fixture\n', encoding='utf-8')
                if changed:
                    with (root/'candidate'/changed).open('a', encoding='utf-8') as stream:
                        stream.write('\n# unauthorized difference\n')

                def fake_output(argv, cwd=None):
                    if argv == ['git', 'rev-parse', 'HEAD']:
                        return request[cwd.name + '_sha']
                    if argv[:2] == ['git', 'status']:
                        return ''
                    return 'test toolchain'

                with patch.object(ci, 'output', fake_output):
                    if changed:
                        with self.assertRaisesRegex(ValueError, 'strict source-only benchmark refused'):
                            ci.prepare(root)
                        self.assertFalse((root/'baseline/engine/search/examples/ci_bench.rs').exists())
                    else:
                        ci.prepare(root)
                        self.assertEqual(ci.sha(root/'baseline/engine/search/examples/ci_bench.rs'),
                                         ci.sha(root/'candidate/engine/search/examples/ci_bench.rs'))

    def fingerprints(self, root, label, *, wrong=None, missing=None):
        for package, names in {
            'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json'),
            'lab-search': ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json'),
        }.items():
            folder = root/('target-' + label)/'release/.fingerprint'/(package + '-testhash')
            folder.mkdir(parents=True, exist_ok=True)
            for name in names:
                if name == missing:
                    continue
                features = [ci.EXPERIMENT_FEATURE] if package == 'lab-engine' else []
                if label == 'candidate':
                    features.append(ci.LEAF_FEATURE)
                if wrong and name == wrong[0]:
                    features = wrong[1]
                (folder/name).write_text(json.dumps({'features': json.dumps(features)}), encoding='utf-8')

    def test_requires_library_test_and_harness_features_and_preserves_evidence(self):
        for label in ('baseline', 'candidate'):
            with self.subTest(label=label), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                (root/'ci-results').mkdir()
                self.fingerprints(root, label)
                evidence = ci.preserve_fingerprints(root, label, 'leaf-ending-states')
                self.assertEqual(len(evidence['fingerprints']), 5)
                for item in evidence['fingerprints']:
                    self.assertEqual(ci.sha(root/'ci-results'/item['artifact_path']), item['sha256'])

    def test_rejects_missing_and_incorrect_features_before_timing(self):
        cases = [
            ('baseline', ('lib-lab_engine.json', []), None),
            ('baseline', ('lib-lab_search.json', [ci.LEAF_FEATURE]), None),
            ('candidate', ('lib-lab_search.json', []), None),
            ('candidate', ('example-ci_bench.json', []), None),
            ('candidate', ('test-lib-lab_engine.json', [ci.EXPERIMENT_FEATURE, ci.LEAF_FEATURE, ci.OBSERVER_FEATURE]), None),
            ('baseline', ('example-ci_bench.json', [ci.OBSERVER_FEATURE]), None),
            ('candidate', None, 'lib-lab_search.json'),
            ('candidate', None, 'example-ci_bench.json'),
        ]
        for label, wrong, missing in cases:
            with self.subTest(label=label, wrong=wrong, missing=missing), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                (root/'ci-results').mkdir()
                self.fingerprints(root, label, wrong=wrong, missing=missing)
                with self.assertRaises(ValueError):
                    ci.preserve_fingerprints(root, label, 'leaf-ending-states')
                self.assertTrue((root/'ci-results'/f'{label}-features.json').is_file())


if __name__ == '__main__':
    unittest.main()
