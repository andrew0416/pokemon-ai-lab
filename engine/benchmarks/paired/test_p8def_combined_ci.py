"""Common4 versus common7 contracts; mocked Cargo never runs the engine."""
import copy
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci
import build_cache
import compact_probe
import dependency_target
import test_combined_ci as old
import test_p8def_ci as independent
import test_compact_probe as probe_fixture
import test_prepared_ci as prepared_fixture

MODE = 'p8def-combined'


def request(root):
    return independent.request(root, MODE)


class CombinedRoutingTests(unittest.TestCase):
    def test_exact_common_four_vs_all_seven_and_full_regressions(self):
        common = ci.feature_args('all-optimizations', 'candidate')[1].split(',')
        self.assertEqual(ci.feature_args(MODE, 'baseline'), ['--features', ','.join(common)])
        self.assertEqual(set(ci.feature_args(MODE, 'candidate')[1].split(',')),
                         set(common) | {'lab-engine/' + f for f in ci.NEW_FEATURES.values()})
        for arm in ('baseline', 'candidate'):
            commands = ci.build_commands('narrow', MODE, arm)
            self.assertEqual(commands[0][:10], ['cargo', 'test', '--locked', '--release', '-p',
                                               'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search'])
            self.assertEqual(commands[0][-2:], ['--', '--test-threads=1'])
            for command in commands:
                self.assertNotIn('observe', ' '.join(command))
                self.assertNotIn('p11', ' '.join(command))
            with self.assertRaises(ValueError):
                ci.build_commands('smoke', MODE, arm)

    def test_different_sha_and_tampered_request_are_refused_before_build(self):
        for fault in ('sha', 'flags'):
            with tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                data = request(root)
                if fault == 'sha':
                    data['candidate_sha'] = 'b'*40
                else:
                    data['feature_args']['candidate'] = ci.feature_args('stats-off-cost', 'candidate')
                independent.write_json(root/'ci-results/request.json', data)
                for operation in (ci.prepare, ci.build):
                    with patch.object(ci, 'output') as calls, patch.object(ci.subprocess, 'Popen') as processes:
                        with self.assertRaises(ValueError):
                            operation(root)
                        calls.assert_not_called()
                        processes.assert_not_called()

    def test_each_actual_timing_flag_and_unknown_p11_is_fail_closed(self):
        for arm in ('baseline', 'candidate'):
            packages, expected, _ = ci.fingerprint_expectations(MODE, arm)
            with tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root)
                for package, kinds in packages.items():
                    for kind in kinds:
                        for flag in (*ci.ALL_EXPERIMENT_FEATURES, 'experiment-p11-evaluator', 'unexpected-generic'):
                            with self.subTest(arm=arm, package=package, kind=kind, flag=flag):
                                independent.fingerprints(root, arm, packages, expected, (package, kind, flag))
                                with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                                    ci.preserve_fingerprints(root, arm, MODE)
                independent.fingerprints(root, arm, packages, expected)
                self.assertEqual(len(ci.preserve_fingerprints(root, arm, MODE)['fingerprints']), 5)

    def test_each_new_declaration_is_required_before_injection(self):
        helper = old.CombinedPrepareTests()
        for missing in (None, *ci.NEW_FEATURES.values()):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                helper.make_workspace(root)
                request(root)
                for arm in ('baseline', 'candidate'):
                    core = root/arm/'engine/core/Cargo.toml'
                    text = core.read_text()
                    for mode in ci.NEW_MODES:
                        runtime, observer = ci.NEW_FEATURES[mode], ci.NEW_OBSERVERS[mode]
                        if runtime != missing:
                            text += f'{runtime}=[]\n'
                        text += f'{observer}=["{runtime}"]\n'
                    core.write_text(text)
                    scenario = root/arm/'engine/scenario/Cargo.toml'
                    text = scenario.read_text() + '[features]\n'
                    for mode in ('replay-action-keys', 'slot-diff'):
                        observer = ci.NEW_OBSERVERS[mode]
                        text += f'{observer}=["lab-engine/{observer}"]\n'
                    for mode in ('replay-action-keys', 'slot-diff'):
                        observer, target = ci.NEW_OBSERVERS[mode], ci.NEW_OBSERVER_TARGETS[mode]['target']
                        text += f'[[test]]\nname="{target}"\npath="tests/{target}.rs"\nrequired-features=["{observer}"]\n'
                    scenario.write_text(text)
                with patch.object(ci, 'output', side_effect=helper.host_output):
                    if missing:
                        with self.assertRaises(ValueError):
                            ci.prepare(root)
                        self.assertFalse((root/'candidate'/ci.COMPACT_PROBE_PATH).exists())
                    else:
                        ci.prepare(root)
                        value = json.loads((root/'ci-results/provenance.json').read_text())
                        for arm in ('baseline', 'candidate'):
                            self.assertEqual(set(value['sources'][arm]['combined_candidate_declarations']), set(ci.NEW_MODES))

    def test_cache_hit_cannot_skip_either_complete_regression_arm(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            request(root)
            for arm in ('baseline', 'candidate'):
                with patch.dict(os.environ, {'BUILD_CACHE_ENABLED': '1', arm.upper() + '_CACHE_HIT': 'true',
                                            'GITHUB_REPOSITORY': build_cache.TRUSTED_REPOSITORY}), \
                     patch.object(build_cache, '_current_plan') as plan:
                    self.assertFalse(build_cache.restore(root, arm))
                plan.assert_not_called()
                self.assertEqual(json.loads((root/f'ci-results/cache-{arm}.json').read_text())['status'],
                                 'fresh-regressions-required')


class CombinedObserverTests(unittest.TestCase):
    def process(self, root, calls, fault=None):
        owner = self
        class Process:
            returncode = 0
            def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                label = Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                calls.append(label)
                if label in ('baseline', 'candidate'):
                    packages, expected, _ = ci.fingerprint_expectations(MODE, label)
                    owner.assertNotIn('observe', ' '.join(argv))
                elif label.startswith('prepared-'):
                    packages = {'lab-engine': ('lib-lab_engine.json',),
                                'lab-search': ('lib-lab_search.json', 'test-integration-test-prepared_turn.json')}
                    _, expected, _ = ci.fingerprint_expectations(MODE, 'candidate')
                    if label == 'prepared-validation':
                        for package in expected:
                            expected[package] = {name: name in (ci.PREPARED_FEATURE, ci.PREPARED_OBSERVER_FEATURE)
                                                 or package == 'lab-engine' and name == ci.EXPERIMENT_FEATURE
                                                 for name in ci.ALL_EXPERIMENT_FEATURES}
                    else:
                        for package in expected:
                            expected[package][ci.PREPARED_OBSERVER_FEATURE] = True
                            expected[package][ci.OBSERVER_FEATURE] = True
                    stdout.write(prepared_fixture.test_output())
                else:
                    mode = next(m for m in ci.NEW_MODES if label == MODE + '-' + m + '-observer-validation')
                    spec = ci.NEW_OBSERVER_TARGETS[mode]
                    runtime_flags = {'lab-engine/' + f for f in ci.NEW_FEATURES.values()}
                    flags = set(argv[argv.index('--features') + 1].split(','))
                    owner.assertTrue(runtime_flags <= flags)
                    owner.assertEqual({f for f in flags if 'observe' in f},
                                      {spec['package'] + '/' + ci.NEW_OBSERVERS[mode]})
                    _, values, _ = ci.fingerprint_expectations(MODE, 'candidate')
                    core = values['lab-engine']
                    core[ci.NEW_OBSERVERS[mode]] = True
                    if fault == 'missing-runtime':
                        core[ci.NEW_FEATURES[next(m for m in ci.NEW_MODES if m != mode)]] = False
                    if fault == 'extra-observer':
                        core[ci.NEW_OBSERVERS[next(m for m in ci.NEW_MODES if m != mode)]] = True
                    if fault == 'p11':
                        core['experiment-p11-evaluator'] = True
                    expected = {'lab-engine': core}
                    packages = {'lab-engine': ('test-lib-lab_engine.json',)}
                    if spec['package'] == 'lab-scenario':
                        packages = {'lab-engine': ('lib-lab_engine.json',),
                                    'lab-scenario': ('lib-lab_scenario.json', 'test-integration-test-' + spec['target'] + '.json')}
                        expected['lab-scenario'] = {name: name == ci.NEW_OBSERVERS[mode] for name in ci.ALL_EXPERIMENT_FEATURES}
                        if spec.get('additional_package'):
                            packages['lab-engine'] += ('test-integration-test-' + spec['target'] + '.json',)
                    elif fault == 'missing-test-fingerprint':
                        packages = {'lab-engine': ('lib-lab_engine.json',)}
                    for name in ci.NEW_OBSERVER_TESTS[mode]:
                        if fault != 'zero':
                            stdout.write(f'test {name} ... ok\n')
                    counts = spec.get('test_counts', (len(ci.NEW_OBSERVER_TESTS[mode]),))
                    for count in counts:
                        stdout.write(f'test result: ok. {0 if fault == "zero" else count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n')
                independent.fingerprints(root, label, packages, expected)
            def wait(self, timeout):
                return 0
        return Process

    def test_full_build_executes_two_regressions_prepared_controls_and_each_combined_observer(self):
        with tempfile.TemporaryDirectory() as folder:
            root, calls = Path(folder), []
            request(root)
            with patch.dict(os.environ, {'BUILD_CACHE_ENABLED': '0'}, clear=True), \
                 patch.object(dependency_target, 'prepare_target', return_value=False), \
                 patch.object(ci.subprocess, 'Popen', self.process(root, calls)):
                ci.build(root)
            value = json.loads((root/'ci-results/provenance.json').read_text())
            self.assertEqual(calls.count('baseline'), 2)
            self.assertEqual(calls.count('candidate'), 2)
            self.assertEqual(calls.count('prepared-validation'), 1)
            self.assertEqual(calls.count('prepared-combined-validation'), 1)
            self.assertEqual(set(value['combined_new_observer_validation']), set(ci.NEW_MODES))
            for mode in ci.NEW_MODES:
                receipt = value['combined_new_observer_validation'][mode]
                self.assertEqual(receipt['status'], 'ok')
                self.assertEqual(receipt['runtime_selection'], MODE)
                self.assertFalse(receipt['cached_results_reused'])

    def test_each_observer_requires_other_runtime_features_and_no_extra_observers(self):
        for mode in ci.NEW_MODES:
            faults = ('missing-runtime', 'extra-observer', 'p11', 'zero')
            if mode == 'stats-off-cost':
                faults += ('missing-test-fingerprint',)
            for fault in faults:
                with self.subTest(mode=mode, fault=fault), tempfile.TemporaryDirectory() as folder:
                    root = Path(folder)
                    request(root)
                    with patch.object(ci.subprocess, 'Popen', self.process(root, [], fault)):
                        with self.assertRaises(ValueError):
                            ci.validate_new_observer(root, mode, runtime_selection=MODE)
                    self.assertFalse((root/'target-candidate').exists())


class CombinedRepresentationTests(independent.SlotRepresentationTests):
    def digest(self, root, rows, mode=MODE):
        return super().digest(root, rows, mode)

    def test_combined_probe_uses_all_seven_and_only_ending_instruction_exemption(self):
        for fault in (None, 'state', 'instructions-missing', 'observer'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                request(root)
                out = root/'ci-results/compact-validation'
                out.mkdir()
                receipt = {'commands': []}
                def child(argv, cwd, env, stem, timeout, commands, save):
                    stem.parent.mkdir(parents=True, exist_ok=True)
                    target = Path(env['CARGO_TARGET_DIR'])
                    label = target.name.removeprefix('target-')
                    arm = 'baseline' if label.endswith('-baseline') else 'candidate'
                    if argv[0] == 'cargo':
                        _, expected, _ = ci.fingerprint_expectations(MODE, arm)
                        core = expected['lab-engine']
                        if arm == 'candidate' and fault == 'observer':
                            core[ci.NEW_OBSERVERS['slot-diff']] = True
                        independent.fingerprints(root, label,
                            {'lab-engine': ('lib-lab_engine.json',),
                             'lab-scenario': ('lib-lab_scenario.json', 'example-ci_compact_probe.json')},
                            {'lab-engine': core, 'lab-scenario': {f: False for f in ci.ALL_EXPERIMENT_FEATURES}})
                        flags = set(argv[argv.index('--features') + 1].split(','))
                        self.assertEqual(flags, {'lab-engine/' + f for f, active in expected['lab-engine'].items() if active}
                                         - ({'lab-engine/' + ci.NEW_OBSERVERS['slot-diff']} if fault == 'observer' else set()))
                        binary = target/'release/examples/ci_compact_probe'
                        binary.parent.mkdir(parents=True)
                        binary.write_bytes(b'inert')
                    elif '--layout' in argv:
                        Path(str(stem)+'.stdout').write_bytes(probe_fixture.jsonl([probe_fixture.layout(True)]))
                    else:
                        rows = independent.representation_rows()
                        ending = next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'][0]
                        if arm == 'candidate':
                            ending['instructions'] = '[SetSlot]'
                            if fault == 'state':
                                ending['state'] = {'full': 'wrong'}
                            elif fault == 'instructions-missing':
                                del ending['instructions']
                        Path(str(stem)+'.stdout').write_bytes(probe_fixture.jsonl(rows))
                with patch.dict(os.environ, {'RUSTFLAGS': '-Ctarget-cpu=x86-64'}, clear=True), \
                     patch.object(compact_probe.platform, 'system', return_value='Linux'), \
                     patch.object(compact_probe.platform, 'machine', return_value='x86_64'), \
                     patch.object(compact_probe.os, 'access', return_value=True), \
                     patch.object(compact_probe, '_run', side_effect=child):
                    if fault:
                        with self.assertRaises(ValueError):
                            compact_probe.validate_independent_candidate(root, out, MODE, receipt, lambda: None)
                    else:
                        compact_probe.validate_independent_candidate(root, out, MODE, receipt, lambda: None)
                        compared = receipt['independent_candidate_comparison']
                        self.assertEqual(compared['status'], 'success')
                        self.assertTrue(compared['semantic_equal'])
                        self.assertFalse(compared['complete_jsonl_byte_equal'])
                        self.assertEqual(compared['excluded_path'], 'fixture.records[].endings[].instructions')


if __name__ == '__main__':
    unittest.main()
