"""Combined feature routing, activation, and fail-closed gate contracts.

Cargo and engine execution are mocked. These tests exercise the real controller
and evidence writers; they do not measure engine speed or prove engine equality.
"""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci
import build_cache
import compact_probe
import test_build_cache as cache_fixtures
import test_compact_probe as compact_fixtures
import test_prepared_ci as prepared_fixtures


MODE = 'all-optimizations'
HURT = 'experiment-hurt-readers'
PREPARED = 'experiment-prepared-turn'
LEAF = 'experiment-leaf-ending-states'
COMPACT = 'experiment-compact-volatiles'
POBS = 'experiment-prepared-turn-observe'
LOBS = 'experiment-leaf-ending-observer'
TIMING_FLAGS = ('lab-engine/experiment-hurt-readers,lab-search/experiment-prepared-turn,'
                'lab-search/experiment-leaf-ending-states,lab-engine/experiment-compact-volatiles')
COMBINED_VALIDATION_FLAGS = TIMING_FLAGS + ',lab-search/' + POBS + ',lab-search/' + LOBS


def request(root):
    result = root/'ci-results'
    result.mkdir(exist_ok=True)
    value = {'suite': 'narrow', 'baseline_sha': 'a'*40, 'candidate_sha': 'a'*40,
             'candidate_feature': MODE,
             'feature_args': {'baseline': [], 'candidate': ['--features', TIMING_FLAGS]}}
    (result/'request.json').write_text(json.dumps(value), encoding='utf-8')
    (result/'provenance.json').write_text('{}', encoding='utf-8')
    return value


def fingerprint_files(root, label, wrong=None, missing=None):
    if label == 'baseline':
        values = {'lab-engine': [], 'lab-search': []}
    elif label == 'candidate':
        values = {'lab-engine': [HURT, PREPARED, LEAF, COMPACT], 'lab-search': [PREPARED, LEAF]}
    elif label == 'prepared-validation':
        values = {'lab-engine': [HURT, PREPARED, POBS], 'lab-search': [PREPARED, POBS]}
    elif label == 'prepared-combined-validation':
        values = {'lab-engine': [HURT, PREPARED, LEAF, COMPACT, POBS, LOBS],
                  'lab-search': [PREPARED, LEAF, POBS, LOBS]}
    else:
        raise AssertionError(label)
    kinds = {'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json'),
             'lab-search': ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json')}
    if label.startswith('prepared-'):
        kinds = {'lab-engine': ('lib-lab_engine.json',),
                 'lab-search': ('lib-lab_search.json', 'test-integration-test-prepared_turn.json')}
    for package, names in kinds.items():
        for name in names:
            if (package, name) == missing:
                continue
            features = values[package]
            if wrong and wrong[:2] == (package, name):
                features = wrong[2]
            path = root/('target-' + label)/'release/.fingerprint'/(package+'-mock')/name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({'features': json.dumps(features)}), encoding='utf-8')


class CombinedRoutingTests(unittest.TestCase):
    def test_request_records_same_source_off_vs_all_on(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            values = {'BASELINE_SHA': 'a'*40, 'CANDIDATE_SHA': 'a'*40,
                      'GITHUB_SHA': 'c'*40, 'CANDIDATE_FEATURE': MODE,
                      'SUITE': 'narrow', 'THREADS': '1', 'PAIRS': '10',
                      'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1',
                      'GITHUB_OUTPUT': str(root/'output.txt')}
            with patch.dict(os.environ, values, clear=True):
                ci.refs(root)
            value = json.loads((root/'ci-results/request.json').read_text())
            self.assertEqual(value['feature_args'], {'baseline': [], 'candidate': ['--features', TIMING_FLAGS]})
            self.assertEqual((value['suite'], value['threads'], value['pairs']), ('narrow', 1, 10))

    def test_request_refuses_different_source_or_partial_regressions(self):
        for changes in ({'CANDIDATE_SHA': 'b'*40}, {'SUITE': 'smoke'}):
            with self.subTest(changes=changes), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                values = {'BASELINE_SHA': 'a'*40, 'CANDIDATE_SHA': 'a'*40,
                          'GITHUB_SHA': 'c'*40, 'CANDIDATE_FEATURE': MODE,
                          'SUITE': 'narrow', 'THREADS': '1', 'PAIRS': '10',
                          'GITHUB_RUN_ID': '123', 'GITHUB_RUN_ATTEMPT': '1',
                          'GITHUB_OUTPUT': str(root/'output.txt'), **changes}
                with patch.dict(os.environ, values, clear=True), self.assertRaises(ValueError):
                    ci.refs(root)
                self.assertFalse((root/'output.txt').exists())

    def test_full_regressions_and_timing_have_all_features_and_no_observers(self):
        for label in ('baseline', 'candidate'):
            commands = ci.build_commands('narrow', MODE, label)
            self.assertEqual(len(commands), 2)
            self.assertEqual(commands[0][:11], ['cargo', 'test', '--locked', '--release', '-p',
                                              'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search', '--timings'])
            self.assertEqual(commands[0][-2:], ['--', '--test-threads=1'])
            for command in commands:
                self.assertNotIn('observe', ' '.join(command))
                if label == 'candidate':
                    self.assertEqual(command[command.index('--features') + 1], TIMING_FLAGS)
                else:
                    self.assertNotIn('--features', command)
            with self.assertRaises(ValueError):
                ci.build_commands('smoke', MODE, label)
        self.assertIn(ci.COMPACT_PROBE_PATH, ci.injected_sources(MODE))

    def test_combined_validation_uses_both_observers_only_in_separate_target(self):
        self.assertEqual(ci.prepared_validation_command(MODE)[-2:], ['--features', COMBINED_VALIDATION_FLAGS])
        self.assertEqual(ci.prepared_validation_command()[-2:],
                         ['--features', 'lab-engine/' + HURT + ',lab-search/' + POBS])


class CombinedPrepareTests(unittest.TestCase):
    def make_workspace(self, root, missing=None):
        request(root)
        core = ('[features]\nexperiment-hurt-readers=[]\nexperiment-compact-volatiles=[]\n'
                'experiment-leaf-ending-states=[]\nexperiment-leaf-ending-observer=["experiment-leaf-ending-states"]\n'
                'experiment-prepared-turn=[]\nexperiment-prepared-turn-observe=["experiment-prepared-turn"]\n')
        search = ('[features]\ndefault=["cli"]\ncli=[]\n'
                  'experiment-leaf-ending-states=["lab-engine/experiment-leaf-ending-states"]\n'
                  'experiment-leaf-ending-observer=["experiment-leaf-ending-states","lab-engine/experiment-leaf-ending-observer"]\n'
                  'experiment-prepared-turn=["lab-engine/experiment-prepared-turn"]\n'
                  'experiment-prepared-turn-observe=["experiment-prepared-turn","lab-engine/experiment-prepared-turn-observe"]\n')
        if missing:
            core = '\n'.join(line for line in core.splitlines() if not line.startswith(missing+'=')) + '\n'
        for label in ('baseline', 'candidate'):
            for name, text in {'Cargo.toml': '[workspace]\n', 'Cargo.lock': '# same lock\n',
                               'core/Cargo.toml': core, 'search/Cargo.toml': search,
                               'scenario/Cargo.toml': '[package]\nname="lab-scenario"\n',
                               'py/Cargo.toml': '[package]\nname="lab-py"\n'}.items():
                path = root/label/'engine'/name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding='utf-8')

    def host_output(self, argv, cwd=None):
        if argv == ['git', 'rev-parse', 'HEAD']:
            return 'a'*40
        if argv == ['git', 'status', '--porcelain']:
            return ''
        if argv in (['rustc', '-Vv'], ['cargo', '-V']):
            return 'pinned fixture toolchain'
        raise AssertionError(argv)

    def test_preparation_requires_all_three_declaration_contracts_and_identical_injections(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.make_workspace(root)
            with patch.object(ci, 'output', side_effect=self.host_output):
                ci.prepare(root)
            provenance = json.loads((root/'ci-results/provenance.json').read_text())
            for label in ('baseline', 'candidate'):
                self.assertTrue({'leaf_declarations', 'prepared_declarations', 'compact_declarations'}
                                <= set(provenance['sources'][label]))
                for name, source in ci.injected_sources(MODE).items():
                    self.assertEqual((root/label/name).read_bytes(), source.read_bytes())

    def test_no_missing_combined_feature_can_reach_harness_injection(self):
        for missing in (HURT, PREPARED, LEAF, COMPACT):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                self.make_workspace(root, missing)
                with patch.object(ci, 'output', side_effect=self.host_output), self.assertRaises(ValueError):
                    ci.prepare(root)
                for label in ('baseline', 'candidate'):
                    self.assertFalse((root/label/'engine/search/examples/ci_bench.rs').exists())

    def test_tampered_request_cannot_compare_different_sources(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            value = request(root)
            value['candidate_sha'] = 'b'*40
            (root/'ci-results/request.json').write_text(json.dumps(value))
            with patch.object(ci, 'output') as output, self.assertRaisesRegex(ValueError, 'same source SHA'):
                ci.prepare(root)
            output.assert_not_called()


class CombinedFingerprintTests(unittest.TestCase):
    def test_both_builds_preserve_all_five_compiler_records(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            for label in ('baseline', 'candidate'):
                fingerprint_files(root, label)
                value = ci.preserve_fingerprints(root, label, MODE)
                self.assertEqual(len(value['fingerprints']), 5)
                self.assertEqual(set(value['expected_by_package']['lab-engine']),
                                 {HURT, PREPARED, LEAF, COMPACT, POBS, LOBS})

    def test_each_feature_bit_or_observer_leak_in_any_timing_artifact_is_fatal(self):
        cases = []
        for label in ('baseline', 'candidate'):
            for package, kinds, active in (
                    ('lab-engine', ('lib-lab_engine.json', 'test-lib-lab_engine.json'),
                     [HURT, PREPARED, LEAF, COMPACT] if label == 'candidate' else []),
                    ('lab-search', ('lib-lab_search.json', 'test-lib-lab_search.json', 'example-ci_bench.json'),
                     [PREPARED, LEAF] if label == 'candidate' else [])):
                for kind in kinds:
                    for feature in (HURT, PREPARED, LEAF, COMPACT, POBS, LOBS):
                        wrong = sorted(set(active) ^ {feature})
                        cases.append((label, package, kind, feature, wrong))
        for label, package, kind, feature, wrong in cases:
            with self.subTest(label=label, package=package, kind=kind, feature=feature), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root)
                fingerprint_files(root, label, wrong=(package, kind, wrong))
                with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                    ci.preserve_fingerprints(root, label, MODE)
                self.assertTrue((root/'ci-results'/(label+'-features.json')).is_file())

    def test_missing_timing_harness_fingerprint_is_fatal(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            fingerprint_files(root, 'candidate', missing=('lab-search', 'example-ci_bench.json'))
            with self.assertRaisesRegex(ValueError, 'missing compiled'):
                ci.preserve_fingerprints(root, 'candidate', MODE)


class CombinedBuildGateTests(unittest.TestCase):
    def fake_process(self, root, calls, combined_text=None, combined_wrong=None):
        owner = self
        class Process:
            returncode = 0
            def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                label = Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                calls.append((label, list(argv)))
                owner.assertEqual(cwd, root/('candidate' if label.startswith('prepared-') else label)/'engine')
                if label.startswith('prepared-'):
                    flags = (COMBINED_VALIDATION_FLAGS if label == 'prepared-combined-validation'
                             else 'lab-engine/' + HURT + ',lab-search/' + POBS)
                    owner.assertEqual(argv[-2:], ['--features', flags])
                    wrong = combined_wrong if label == 'prepared-combined-validation' else None
                    fingerprint_files(root, label, wrong=wrong)
                    text = (combined_text if combined_text is not None and label == 'prepared-combined-validation'
                            else prepared_fixtures.test_output())
                    stdout.write(text)
                else:
                    owner.assertNotIn('observe', ' '.join(argv))
                    fingerprint_files(root, label)
            def wait(self, timeout):
                owner.assertEqual(timeout, ci.COMMAND_TIMEOUT_SECONDS)
                return 0
        return Process

    def test_build_runs_standalone_and_combined_nine_tests_then_keeps_timing_targets_clean(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            calls = []
            with patch.dict(os.environ, {}, clear=True), patch.object(ci.subprocess, 'Popen', self.fake_process(root, calls)):
                ci.build(root)
            self.assertEqual([label for label, _ in calls],
                             ['baseline', 'baseline', 'candidate', 'candidate',
                              'prepared-validation', 'prepared-combined-validation'])
            provenance = json.loads((root/'ci-results/provenance.json').read_text())
            self.assertEqual(provenance['prepared_validation']['status'], 'ok')
            combined = provenance['combined_prepared_validation']
            self.assertEqual(combined['status'], 'ok')
            self.assertEqual(len(combined['passed_tests']), 9)
            self.assertEqual(combined['selection'], MODE)
            for label in ('baseline', 'candidate'):
                self.assertEqual(ci.preserve_fingerprints(root, label, MODE),
                                 provenance['compiler_feature_evidence'][label])

    def test_combined_validation_requires_nine_real_tests_and_both_actual_observers(self):
        cases = [('zero tests', 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n', None),
                 ('leaf observer missing', None,
                  ('lab-search', 'test-integration-test-prepared_turn.json', [PREPARED, LEAF, POBS])),
                 ('compact missing', None, ('lab-engine', 'lib-lab_engine.json', [HURT, PREPARED, LEAF, POBS, LOBS]))]
        for title, text, wrong in cases:
            with self.subTest(title=title), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root)
                with patch.object(ci.subprocess, 'Popen', self.fake_process(root, [], text, wrong)):
                    with self.assertRaises(ValueError):
                        ci.validate_prepared_turn(root, MODE)
                receipt = json.loads((root/'ci-results/prepared-combined-validation.json').read_text())
                self.assertEqual(receipt['status'], 'failed')
                self.assertFalse((root/'target-candidate').exists())


class CombinedIndependentCompactTests(unittest.TestCase):
    def test_combined_mode_keeps_fresh_isolated_three_variant_compact_contract(self):
        fixture = compact_fixtures.FreshGateTests('test_three_fresh_targets_exact_bytes_layout_and_real_fingerprints')
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        path = fixture.results/'request.json'
        value = json.loads(path.read_text())
        value['candidate_feature'] = MODE
        path.write_text(json.dumps(value))
        result = fixture.run_gate()
        self.assertEqual(result['status'], 'success')
        self.assertEqual(result['selection'], MODE)
        self.assertEqual(len(result['commands']), 10)
        self.assertTrue(result['complete_jsonl_byte_equal'])
        for entry in result['variants'].values():
            self.assertNotIn(LEAF, entry['features'])
            self.assertNotIn(PREPARED, entry['features'])
        self.assertFalse((fixture.root/'target-candidate').exists())

    def test_combined_cache_identity_binds_injected_compact_probe_and_gate(self):
        fixture = cache_fixtures.CacheRecipeInputTests('test_compact_probe_identity_is_mode_scoped_and_cannot_use_stale_cache')
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        request_path = fixture.root/'ci-results/request.json'
        value = json.loads(request_path.read_text())
        value['candidate_feature'] = MODE
        request_path.write_text(json.dumps(value))
        for name in build_cache.COMPACT_CONTROLLER_FILES:
            (fixture.controller/name).write_bytes(name.encode() + b' frozen fixture\n')
        probe = fixture.root/'baseline'/build_cache.COMPACT_PROBE_PATH
        probe.parent.mkdir(parents=True)
        probe.write_bytes((fixture.controller/'compact_probe.rs').read_bytes())
        fixture.untracked.append(build_cache.COMPACT_PROBE_PATH)
        recipe = build_cache.make_recipe(fixture.root, 'baseline')
        self.assertEqual(recipe['source']['compact_probe_sha256'], cache_fixtures.digest(probe))
        self.assertIn('compact_probe.py', recipe['identity']['build_driver'])
        self.assertFalse(recipe['fingerprint_spec']['hurt_active'])
        self.assertEqual(recipe['selection'], MODE)
        original_key = build_cache.recipe_key(recipe)
        gate = fixture.controller/'compact_probe.py'
        gate.write_bytes(gate.read_bytes()+b'changed\n')
        self.assertNotEqual(original_key, build_cache.recipe_key(build_cache.make_recipe(fixture.root, 'baseline')))
        probe.write_bytes(b'wrong injected bytes')
        with self.assertRaisesRegex(ValueError, 'Injected compact probe differs'):
            build_cache.make_recipe(fixture.root, 'baseline')


if __name__ == '__main__':
    unittest.main()
