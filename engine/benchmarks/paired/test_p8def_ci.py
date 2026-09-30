"""Independent P8d/e/f feature isolation and fail-closed evidence contracts.

All Rust processes are inert mocks. These tests validate the controller, not
engine correctness or performance.
"""
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
import test_compact_probe as compact_fixtures
import test_prepared_ci as prepared_fixtures
import test_run as run_fixtures
import test_memory as memory_fixtures


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding='utf-8')


def request(root, mode):
    value = {'suite': 'narrow', 'baseline_sha': 'a'*40, 'candidate_sha': 'a'*40,
             'candidate_feature': mode,
             'feature_args': {arm: ci.feature_args(mode, arm) for arm in ('baseline', 'candidate')}}
    write_json(root/'ci-results/request.json', value)
    write_json(root/'ci-results/provenance.json', value)
    return value


def fingerprints(root, label, packages, expected, mutation=None):
    for package, names in packages.items():
        for name in names:
            features = [key for key, active in expected[package].items() if active]
            if set(ci.ALL_EXPERIMENT_FEATURES) <= set(expected[package]):
                features += sorted(ci.PACKAGE_BASE_FEATURES[package])
            if mutation and mutation[:2] == (package, name):
                features = sorted(set(features) ^ {mutation[2]})
            write_json(root/('target-' + label)/'release/.fingerprint'/(package+'-mock')/name,
                       {'features': json.dumps(features)})


class RoutingAndFingerprintTests(unittest.TestCase):
    def test_every_candidate_adds_exactly_one_flag_to_the_same_all_four_baseline(self):
        common = ci.feature_args('all-optimizations', 'candidate')[1].split(',')
        for mode in ci.NEW_MODES:
            self.assertEqual(ci.feature_args(mode, 'baseline'), ['--features', ','.join(common)])
            self.assertEqual(ci.feature_args(mode, 'candidate'),
                             ['--features', ','.join(common + ['lab-engine/' + ci.NEW_FEATURES[mode]])])
            for arm in ('baseline', 'candidate'):
                commands = ci.build_commands('narrow', mode, arm)
                self.assertEqual(commands[0][:10], ['cargo', 'test', '--locked', '--release', '-p',
                                                   'lab-engine', '-p', 'lab-scenario', '-p', 'lab-search'])
                self.assertEqual(commands[0][-2:], ['--', '--test-threads=1'])
                self.assertNotIn('observer', ' '.join(commands[1]))
                self.assertNotIn('observe', ' '.join(commands[1]))
                with self.assertRaises(ValueError):
                    ci.build_commands('smoke', mode, arm)

    def test_different_source_sha_is_refused_before_commands(self):
        for mode in ci.NEW_MODES:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                value = request(root, mode)
                value['candidate_sha'] = 'b'*40
                write_json(root/'ci-results/request.json', value)
                with patch.object(ci, 'output') as calls, self.assertRaisesRegex(ValueError, 'same source SHA'):
                    ci.prepare(root)
                calls.assert_not_called()

    def test_every_known_feature_flip_in_any_timing_artifact_is_rejected(self):
        for mode in ci.NEW_MODES:
            for arm in ('baseline', 'candidate'):
                packages, expected, _ = ci.fingerprint_expectations(mode, arm)
                self.assertEqual(set(expected['lab-engine']), set(ci.ALL_EXPERIMENT_FEATURES))
                self.assertEqual(set(expected['lab-search']), set(ci.ALL_EXPERIMENT_FEATURES))
                with tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    request(root, mode)
                    for package, names in packages.items():
                        for name in names:
                            for feature in ci.ALL_EXPERIMENT_FEATURES:
                                fingerprints(root, arm, packages, expected, (package, name, feature))
                                with self.subTest(mode=mode, arm=arm, package=package, kind=name, feature=feature):
                                    with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                                        ci.preserve_fingerprints(root, arm, mode)
                    fingerprints(root, arm, packages, expected)
                    self.assertEqual(len(ci.preserve_fingerprints(root, arm, mode)['fingerprints']), 5)

    def test_default_alias_cannot_enable_any_new_runtime_or_observer(self):
        for feature in (*ci.NEW_FEATURES.values(), *ci.NEW_OBSERVERS.values()):
            with self.subTest(feature=feature), self.assertRaisesRegex(ValueError, 'default'):
                ci.reject_default_experiments(Path('Cargo.toml'), {'default': ['alias'], 'alias': [feature]})

    def test_unknown_experiment_or_generic_feature_is_rejected(self):
        _, expected, _ = ci.fingerprint_expectations('slot-diff', 'candidate')
        for package, specification in expected.items():
            features = [name for name, active in specification.items() if active] + sorted(ci.PACKAGE_BASE_FEATURES[package])
            for rogue in ('experiment-unrequested-future', 'unrequested-generic'):
                with self.assertRaisesRegex(ValueError, 'actual compiled feature activation'):
                    ci.validate_strict_feature_closure(package, features + [rogue], specification)

    def test_manifest_requires_empty_runtime_and_exact_observer_dependency(self):
        for mode in ci.NEW_MODES:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                core = root/'engine/core/Cargo.toml'
                core.parent.mkdir(parents=True)
                feature, observer = ci.NEW_FEATURES[mode], ci.NEW_OBSERVERS[mode]
                core.write_text(f'[features]\n{feature}=[]\n{observer}=["{feature}"]\n')
                search = root/'engine/search/Cargo.toml'
                search.parent.mkdir(parents=True)
                search.write_text('[features]\ndefault=[]\n')
                scenario = root/'engine/scenario/Cargo.toml'
                scenario.parent.mkdir(parents=True)
                target = ci.NEW_OBSERVER_TARGETS[mode]
                scenario.write_text(f'[features]\n{observer}=["lab-engine/{observer}"]\n'
                                    + (f'[[test]]\nname="{target["target"]}"\npath="tests/{target["target"]}.rs"\n'
                                       f'required-features=["{observer}"]\n' if target['kind'] == 'test' else ''))
                ci.verify_new_declarations(root, mode)
                core.write_text(f'[features]\n{feature}=["unrequested"]\n{observer}=["{feature}"]\n')
                with self.assertRaises(ValueError):
                    ci.verify_new_declarations(root, mode)

    def test_prepared_diagnostics_keep_both_old_observers_and_one_new_runtime(self):
        for mode in ci.NEW_MODES:
            command = ci.prepared_validation_command(mode)
            flags = command[command.index('--features') + 1].split(',')
            self.assertEqual(set(flags), set(ci.feature_args(mode, 'candidate')[1].split(','))
                             | {'lab-search/' + ci.PREPARED_OBSERVER_FEATURE, 'lab-search/' + ci.OBSERVER_FEATURE})
            self.assertFalse(set(flags) & {'lab-engine/' + value for value in ci.NEW_OBSERVERS.values()})


class CacheAndObserverTests(unittest.TestCase):
    def test_exact_cache_hit_cannot_skip_new_regressions(self):
        for mode in ci.NEW_MODES:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                request(root, mode)
                with patch.dict(os.environ, {'BUILD_CACHE_ENABLED': '1', 'BASELINE_CACHE_HIT': 'true',
                                            'GITHUB_REPOSITORY': build_cache.TRUSTED_REPOSITORY}), \
                     patch.object(build_cache, '_current_plan') as plan:
                    self.assertFalse(build_cache.restore(root, 'baseline'))
                plan.assert_not_called()
                evidence = json.loads((root/'ci-results/cache-baseline.json').read_text())
                self.assertEqual(evidence['status'], 'fresh-regressions-required')
                self.assertFalse(evidence['reused'])

    def test_each_mode_and_arm_has_distinct_strict_recipe_features(self):
        keys = set()
        for mode in ci.NEW_MODES:
            for arm in ('baseline', 'candidate'):
                packages, expected, hurt = ci.fingerprint_expectations(mode, arm)
                recipe = {'selection': mode, 'label': arm, 'commands': ci.build_commands('narrow', mode, arm),
                          'fingerprint_spec': {'packages': packages, 'expected': expected, 'hurt_active': hurt}}
                keys.add(build_cache.recipe_key(recipe))
        self.assertEqual(len(keys), 6)

    def test_missing_observer_registration_fails_before_build(self):
        with patch.dict(ci.NEW_OBSERVER_TESTS, {'stats-off-cost': ()}):
            with self.assertRaisesRegex(ValueError, 'registration'):
                ci.new_observer_validation_commands('stats-off-cost')

    def test_fresh_named_observer_cannot_pass_with_zero_tests_or_wrong_features(self):
        mode = 'stats-off-cost'
        for fault in (None, 'zero', 'missing-observer'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                request(root, mode)
                owner = self
                class Process:
                    returncode = 0
                    def __init__(self, argv, *, cwd, env, stdout, **kwargs):
                        label = Path(env['CARGO_TARGET_DIR']).name.removeprefix('target-')
                        owner.assertEqual(label, mode+'-observer-validation')
                        owner.assertIn('--exact', argv)
                        _, expected, _ = ci.fingerprint_expectations(mode, 'candidate')
                        core = expected['lab-engine']
                        core[ci.NEW_OBSERVERS[mode]] = fault != 'missing-observer'
                        fingerprints(root, label, {'lab-engine': ('lib-lab_engine.json', 'test-lib-lab_engine.json')},
                                     {'lab-engine': core})
                        count = 0 if fault == 'zero' else 1
                        if count:
                            stdout.write(f'test {ci.NEW_OBSERVER_TESTS[mode][0]} ... ok\n')
                        stdout.write(f'test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 55 filtered out;\n')
                    def wait(self, timeout):
                        return 0
                with patch.object(ci.subprocess, 'Popen', Process):
                    if fault:
                        with self.assertRaises(ValueError):
                            ci.validate_new_observer(root, mode)
                    else:
                        self.assertEqual(ci.validate_new_observer(root, mode)['status'], 'ok')
                self.assertFalse((root/'target-candidate').exists())


def representation_rows():
    rows = compact_fixtures.output_rows()
    for row in rows:
        if row['kind'] == 'fixture':
            row['records'] = [{'before': {'full': 'input'}, 'endings': [
                {'instructions': '[Granular]', 'state': {'full': 'output'}, 'probability_bits': 99,
                 'canonical_hidden': 'hidden', 'suspension': 'none'},
                {'instructions': '[Other]', 'state': {'full': 'second'}, 'probability_bits': 100,
                 'canonical_hidden': 'hidden2', 'suspension': 'resume'}]}]
        if row['kind'] == 'state-instructions':
            row['instructions'] = '[ExplicitRollbackCase]'
    return rows


class SlotRepresentationTests(unittest.TestCase):
    def digest(self, root, rows, mode='slot-diff'):
        path = root/'output.jsonl'
        path.write_bytes(compact_fixtures.jsonl(rows))
        return compact_probe.slot_diff_semantic_output(path, mode)

    def test_only_fixture_ending_instruction_string_may_differ(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rows = representation_rows()
            before = self.digest(root, rows)
            for row in rows:
                if row['kind'] == 'fixture':
                    for ending in row['records'][0]['endings']:
                        ending['instructions'] = '[SetSlot]'
            self.assertEqual(self.digest(root, rows), before)
            self.assertEqual(before['removed_outcome_instruction_fields'], 20)

    def test_every_other_field_and_order_remains_part_of_comparison(self):
        mutations = [
            lambda rows: next(r for r in rows if r['kind'] == 'state-instructions').update(instructions='changed'),
            lambda rows: next(r for r in rows if r['kind'] == 'fixture')['records'][0].update(before={'full': 'wrong'}),
            lambda rows: next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'][0].update(state={'full': 'wrong'}),
            lambda rows: next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'][0].update(probability_bits=98),
            lambda rows: next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'][0].update(canonical_hidden='wrong'),
            lambda rows: next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'][0].update(suspension='wrong'),
            lambda rows: next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'].reverse(),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = representation_rows()
            before = self.digest(root, original)
            for mutate in mutations:
                changed = copy.deepcopy(original)
                mutate(changed)
                self.assertNotEqual(self.digest(root, changed), before)

    def test_exemption_refuses_other_modes_or_missing_representation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for mode in ('replay-action-keys', 'stats-off-cost', 'all-optimizations'):
                with self.assertRaisesRegex(ValueError, 'only valid'):
                    self.digest(root, representation_rows(), mode)
            rows = representation_rows()
            next(r for r in rows if r['kind'] == 'fixture')['records'][0]['endings'][0].pop('instructions')
            with self.assertRaisesRegex(ValueError, 'Missing explicit'):
                self.digest(root, rows)

    def test_new_probe_gate_enforces_exact_or_scoped_equivalence_and_actual_features(self):
        for mode, fault in [('replay-action-keys', None), ('slot-diff', None), ('stats-off-cost', None),
                            ('slot-diff', 'state'), ('replay-action-keys', 'instructions'),
                            ('slot-diff', 'observer-leak')]:
            with self.subTest(mode=mode, fault=fault), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                request(root, mode)
                out = root/'ci-results/compact-validation'
                out.mkdir()
                receipt = {'commands': []}
                def child(argv, cwd, env, stem, timeout, commands, save):
                    stem.parent.mkdir(parents=True, exist_ok=True)
                    target = Path(env['CARGO_TARGET_DIR'])
                    label = target.name.removeprefix('target-')
                    tree = 'baseline' if label.endswith('-baseline') else 'candidate'
                    commands.append({'argv': argv})
                    if argv[0] == 'cargo':
                        _, expected, _ = ci.fingerprint_expectations(mode, tree)
                        core = expected['lab-engine']
                        if tree == 'candidate' and fault == 'observer-leak':
                            core[ci.NEW_OBSERVERS[mode]] = True
                        fingerprints(root, label, {'lab-engine': ('lib-lab_engine.json',),
                            'lab-scenario': ('lib-lab_scenario.json', 'example-ci_compact_probe.json')},
                            {'lab-engine': core, 'lab-scenario': {key: False for key in ci.ALL_EXPERIMENT_FEATURES}})
                        binary = target/'release/examples/ci_compact_probe'
                        binary.parent.mkdir(parents=True, exist_ok=True)
                        binary.write_bytes(b'inert')
                    elif '--layout' in argv:
                        Path(str(stem)+'.stdout').write_bytes(compact_fixtures.jsonl([compact_fixtures.layout(True)]))
                    else:
                        rows = representation_rows()
                        ending = next(row for row in rows if row['kind'] == 'fixture')['records'][0]['endings'][0]
                        if tree == 'candidate':
                            if mode == 'slot-diff' or fault == 'instructions':
                                ending['instructions'] = '[SetSlot]'
                            if fault == 'state':
                                ending['state'] = {'full': 'wrong'}
                        Path(str(stem)+'.stdout').write_bytes(compact_fixtures.jsonl(rows))
                with patch.dict(os.environ, {'RUSTFLAGS': '-Ctarget-cpu=x86-64'}, clear=True), \
                     patch.object(compact_probe.platform, 'system', return_value='Linux'), \
                     patch.object(compact_probe.platform, 'machine', return_value='x86_64'), \
                     patch.object(compact_probe.os, 'access', return_value=True), \
                     patch.object(compact_probe, '_run', side_effect=child):
                    if fault:
                        with self.assertRaises(ValueError):
                            compact_probe.validate_independent_candidate(root, out, mode, receipt, lambda: None)
                    else:
                        compact_probe.validate_independent_candidate(root, out, mode, receipt, lambda: None)
                        value = receipt['independent_candidate_comparison']
                        self.assertEqual(value['status'], 'success')
                        self.assertTrue(value['semantic_equal'])
                        self.assertEqual(value['complete_jsonl_byte_equal'], mode != 'slot-diff')
                self.assertFalse((root/'target-candidate').exists())


class StatsOffEnvironmentTests(unittest.TestCase):
    def test_timing_children_remove_stats_presence_including_empty_and_zero_values(self):
        for inherited in ('', '0', '1'):
            fixture = run_fixtures.ProcessTests('test_success_real_processes_raw_output_and_provenance')
            fixture.setUp()
            self.addCleanup(fixture.doCleanups)
            paths = [fixture.fake(name) for name in ('baseline', 'candidate')]
            for path in paths:
                path.write_text('import os\nassert "LAB_ENGINE_STATS" not in os.environ\n'
                                + path.read_text(encoding='utf-8'), encoding='utf-8')
            with patch.dict(os.environ, {'LAB_ENGINE_STATS': inherited}):
                code, result, _, out = fixture.run_fake_pair(*paths)
            self.assertEqual((code, result['status']), (0, 'ok'))
            manifest = json.loads((out/'manifest.json').read_text(encoding='utf-8'))
            self.assertIn('LAB_ENGINE_STATS', manifest['removed_environment_variable_names'])
            self.assertNotIn('LAB_ENGINE_STATS', manifest['environment'])

    def test_memory_children_remove_stats_presence_and_record_removal(self):
        fixture = memory_fixtures.MemoryPassTests('test_success_eight_fresh_processes_provenance_and_no_timing_fields')
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        paths = [fixture.child(name) for name in ('baseline', 'candidate')]
        for path in paths:
            path.write_text('import os\nassert "LAB_ENGINE_STATS" not in os.environ\n'
                            + path.read_text(encoding='utf-8'), encoding='utf-8')
        args = fixture.fixture(*paths)
        with patch.dict(os.environ, {'LAB_ENGINE_STATS': ''}):
            code, result, _, _ = fixture.invoke(args)
        self.assertEqual((code, result['status']), (0, 'ok'))
        manifest = json.loads((args.out_dir/'manifest.json').read_text(encoding='utf-8'))
        self.assertIn('LAB_ENGINE_STATS', manifest['removed_environment_variable_names'])
        self.assertNotIn('LAB_ENGINE_STATS', manifest['environment'])


if __name__ == '__main__':
    unittest.main()
