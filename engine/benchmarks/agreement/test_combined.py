"""Meaningful failure injections for combined feature and activation contracts."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch

def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

controller = load('run')
contract = load('combined_contract')


def activation_probe():
    controls = []
    for key in contract.ACTIVATION_IDS:
        leaf = {'batches': 16, 'visits': 32, 'materialized_outcomes': 16, 'emitted_instructions': 48}
        controls.append({'id': key, 'equal': True, 'passed': True,
                         'on': {'prepared_requested': True, 'restored': True, 'successful': True,
                                'signature': 'result-bits|nodes32|turns16|all-work-stats',
                                'validation_counts': [17, 40, 17], 'leaf': dict(leaf)},
                         'off': {'prepared_requested': False, 'restored': True, 'successful': True,
                                 'signature': 'result-bits|nodes32|turns16|all-work-stats',
                                 'validation_counts': [32, 64, 32], 'leaf': dict(leaf)}})
    return {'schema': 1, 'suite': contract.ACTIVATION_SUITE,
            'config': dict(contract.ACTIVATION_CONFIG),
            'features': dict.fromkeys(contract.ACTIVATION_FEATURES, True),
            'controls': controls, 'passed': True}


def activation_receipt():
    return {'passed': True, 'timeout': False, 'returncode': 0,
            'binary_sha256': 'a' * 64, 'stdout_sha256': 'b' * 64, 'probe': activation_probe()}


class CombinedContractTests(unittest.TestCase):
    def setUp(self):
        self.variants = {'variants': [
            {'id': 'combined-off', 'sha': 'a' * 40, 'features': [], 'compare_to': None},
            {'id': 'combined-on', 'sha': 'a' * 40, 'features': sorted(contract.FEATURES), 'compare_to': 'combined-off'}]}
        self.on = self.variants['variants'][1]
        self.jobs = []
        self.records = []
        for mode, count in [('mixed', 8), ('deep', 9)]:
            for index in range(count):
                key = f'{mode}/{index}'
                meta = self.metadata(prepared=True, count=0 if mode == 'mixed' else 1)
                self.jobs.append({'id': key, 'kind': 'search', 'args': ['input.json', mode, '1', 'expect', 'median', '1']})
                self.records.append({'id': key, 'kind': 'search', 'successful': True, 'sha256': 'same',
                                     'stderr': json.dumps(meta)})

    @staticmethod
    def metadata(*, prepared=True, count=1, leaf=5):
        return {'phase': 'search-complete', 'requested_threads': 1, 'factored': False,
                'leaf_observer_compiled': True, 'leaf': {'batches': leaf, 'visits': leaf * 2},
                'prepared_compiled': True, 'prepared_observer_compiled': True,
                'prepared_requested': prepared, 'prepared': {'parent_checks': count}}

    def off(self, job, binaries, result_dir):
        self.assertTrue(job['id'].startswith('prepared-off-control/deep/'))
        self.assertEqual(job['args'][-2:], ['--prepared', 'off'])
        return {'complete': True, 'successful': True, 'sha256': 'same',
                'stderr': json.dumps(self.metadata(prepared=False, count=1))}

    def activate(self, child=None):
        with patch.object(controller, 'run_case', side_effect=child or self.off), \
             patch.object(controller, 'run_nonleaf_activation', return_value=activation_receipt()):
            return controller.activation_checks(self.on, self.records, self.jobs, {}, Path('.'))

    def test_two_same_source_variants_and_both_feature_closures(self):
        contract.validate_variants(self.variants)
        core, search = controller.expected_features(self.on)
        self.assertEqual(core, {'experiment-hurt-readers', 'experiment-compact-volatiles',
                               'experiment-leaf-ending-states', 'experiment-leaf-ending-observer',
                               'experiment-prepared-turn', 'experiment-prepared-turn-observe'})
        self.assertTrue({'experiment-leaf-ending-observer', 'experiment-prepared-turn-observe'} <= search)
        self.assertEqual(controller.expected_features(self.variants['variants'][0])[0], set())

    def test_unknown_or_duplicate_feature_fails(self):
        for features in [['lab-search/unknown'], self.on['features'] + self.on['features'][:1]]:
            with self.assertRaises(ValueError):
                controller.expected_features({'features': features})

    def test_wrong_source_or_missing_observer_fails_configuration(self):
        for mutate in [lambda d: d['variants'][1].update(sha='b' * 40),
                       lambda d: d['variants'][1]['features'].pop(),
                       lambda d: d['variants'][0]['features'].append('lab-engine/experiment-hurt-readers')]:
            value = copy.deepcopy(self.variants)
            mutate(value)
            with self.assertRaises(ValueError):
                contract.validate_variants(value)

    def test_both_paths_pass_and_depth_one_zero_is_allowed(self):
        result = self.activate()
        self.assertTrue(result['passed'])
        self.assertTrue(result['leaf']['passed'])
        self.assertTrue(result['prepared']['passed'])
        self.assertEqual(result['prepared']['expected_control_ids'], [f'deep/{i}' for i in range(8)])

    def test_missing_preselected_deep_case_cannot_be_replaced_by_next_success(self):
        self.records = [r for r in self.records if r['id'] != 'deep/0']
        result = self.activate()
        self.assertFalse(result['passed'])
        self.assertNotIn('deep/8', [r['id'] for r in result['prepared']['controls']])

    def test_leaf_nonactivation_fails_even_when_prepared_controls_pass(self):
        for record in self.records:
            value = json.loads(record['stderr'])
            value['leaf'] = {'batches': 0, 'visits': 0}
            record['stderr'] = json.dumps(value)
        result = self.activate()
        self.assertFalse(result['passed'])
        self.assertFalse(result['prepared']['passed'])

    def test_deep_counts_must_be_equal_even_when_leaf_visits_exist(self):
        def equal_counts(job, binaries, result_dir):
            value = self.off(job, binaries, result_dir)
            value['stderr'] = json.dumps(self.metadata(prepared=False, count=8))
            return value
        result = self.activate(equal_counts)
        self.assertFalse(result['passed'])
        self.assertTrue(result['leaf']['passed'])

    def test_one_control_output_difference_fails(self):
        def mismatch(job, binaries, result_dir):
            value = self.off(job, binaries, result_dir)
            if job['id'].endswith('deep/3'):
                value['sha256'] = 'different'
            return value
        self.assertFalse(self.activate(mismatch)['passed'])

    def test_control_toggle_or_observer_flag_missing_fails(self):
        def missing(job, binaries, result_dir):
            value = self.off(job, binaries, result_dir)
            meta = json.loads(value['stderr'])
            del meta['prepared_requested']
            value['stderr'] = json.dumps(meta)
            return value
        self.assertFalse(self.activate(missing)['passed'])

    def test_plan_denominator_cannot_shrink(self):
        with self.assertRaisesRegex(ValueError, 'denominator'):
            contract.validate_plan({'jobs': []})

    def summary(self):
        oracle_rows = []
        for key, kind in contract.ORACLES.items():
            gender = kind == 'gender-mixture-v1'
            oracle_rows.append({'id': key, 'contract': kind, 'status': 'match',
                                'matching_positions': 16 if gender else 2,
                                'checks': {'all_states_restored': True,
                                           'distinct_assignments': 16, 'expected_assignments': 16,
                                           'direct_oracle_histories': 2, 'metamorphic_histories': 0}})
        scoped_rows = [{'id': key, 'expected_choice_rejections': count, 'status': 'ok',
                        'success_scope': 'requested', 'all_positions_status': 'error',
                        'scopes': {'requested': {'errors': 0}, 'global': {'errors': 0}}}
                       for key, count in contract.SCOPED.items()]
        return {'complete': True, 'all_requested_successful': True, 'activation_passed': True,
                'validation_errors': [], 'expected_cases': 6184,
                'comparisons': {'combined-on': {'baseline': 'combined-off', 'equal_success': 6184,
                    'equal_error': 0, 'different': 0, 'uncompared': 0,
                    'by_kind': {kind: {'equal_success': count} for kind, count in contract.COUNTS.items()}}},
                'oracle': {name: {'match': 3056} for name in ('combined-off', 'combined-on')},
                'oracle_contracts': {name: copy.deepcopy(oracle_rows) for name in ('combined-off', 'combined-on')},
                'scoped_turn_coverage': {name: copy.deepcopy(scoped_rows) for name in ('combined-off', 'combined-on')},
                'activation': {'combined-on': self.activate()}}

    def test_full_success_summary_passes(self):
        contract.validate_summary(self.summary())

    def test_equal_error_cannot_pass_summary_even_if_complete_bit_is_true(self):
        summary = self.summary()
        summary['comparisons']['combined-on']['equal_error'] = 1
        with self.assertRaises(ValueError):
            contract.validate_summary(summary)

    def test_additional_rejections_cannot_be_hidden_or_widened(self):
        for count in (0, 2):
            summary = self.summary()
            summary['scoped_turn_coverage']['combined-on'][0]['expected_choice_rejections'] = count
            with self.assertRaises(ValueError):
                contract.validate_summary(summary)

    def test_special_oracle_history_cannot_be_removed(self):
        summary = self.summary()
        summary['oracle_contracts']['combined-on'][-1]['checks']['direct_oracle_histories'] = 1
        with self.assertRaises(ValueError):
            contract.validate_summary(summary)

    def test_missing_combined_observer_result_cannot_pass_summary(self):
        summary = self.summary()
        del summary['activation']['combined-on']['prepared']
        with self.assertRaises(ValueError):
            contract.validate_summary(summary)


class ActivationProbeContractTests(unittest.TestCase):
    def reject(self, mutate):
        value = activation_probe()
        mutate(value)
        with self.assertRaises(ValueError):
            contract.validate_activation_probe(value)

    def test_fixed_eight_successful_controls_pass(self):
        contract.validate_activation_probe(activation_probe())

    def test_missing_duplicate_extra_reordered_or_unknown_control_fails(self):
        for mutate in [lambda d: d['controls'].pop(),
                       lambda d: d['controls'].__setitem__(1, copy.deepcopy(d['controls'][0])),
                       lambda d: d['controls'].append(copy.deepcopy(d['controls'][0])),
                       lambda d: d['controls'].reverse(),
                       lambda d: d['controls'][0].update(id='different')]:
            with self.subTest(mutate=mutate):
                self.reject(mutate)

    def test_wrong_schema_suite_or_workload_fails(self):
        for key, value in [('schema', True), ('schema', 2), ('suite', 'other')]:
            self.reject(lambda d, k=key, v=value: d.update({k: v}))
        for key, value in [('depth', 1), ('depth', 2.0), ('threads', True),
                           ('rolls', 'Full'), ('factored', 0), ('fixture', 'posthoc-picked')]:
            self.reject(lambda d, k=key, v=value: d['config'].update({k: v}))

    def test_missing_false_or_nonboolean_feature_fails(self):
        for key in contract.ACTIVATION_FEATURES:
            self.reject(lambda d, k=key: d['features'].pop(k))
            for value in (False, 1, 'true'):
                self.reject(lambda d, k=key, v=value: d['features'].update({k: v}))

    def test_forged_success_bit_cannot_hide_output_or_work_difference(self):
        self.reject(lambda d: d['controls'][0]['off'].update(signature='different-work'))
        self.reject(lambda d: d['controls'][0]['off']['leaf'].update(visits=33))
        for side in ('on', 'off'):
            for field in ('restored', 'successful'):
                self.reject(lambda d, s=side, f=field: d['controls'][0][s].update({f: False}))
            self.reject(lambda d, s=side: d['controls'][0][s].update(signature=''))

    def test_missing_toggle_and_repeated_same_run_cannot_pass(self):
        for side in ('on', 'off'):
            self.reject(lambda d, s=side: d['controls'][0][s].pop('prepared_requested'))
            self.reject(lambda d, s=side: d['controls'][0][s].update(prepared_requested=(s != 'on')))
        self.reject(lambda d: d['controls'][0].update(off=copy.deepcopy(d['controls'][0]['on'])))

    def test_all_three_validators_must_reduce_in_every_control(self):
        for row in range(8):
            for index in range(3):
                self.reject(lambda d, r=row, i=index: d['controls'][r]['on']['validation_counts'].__setitem__(
                    i, d['controls'][r]['off']['validation_counts'][i]))
        for value in ([1, 2], [1, True, 3], [1, -1, 3], [1, 0, 3], [1, 2.0, 3]):
            self.reject(lambda d, v=value: d['controls'][0]['on'].update(validation_counts=v))

    def test_p9_and_nonleaf_work_required_on_both_sides(self):
        for side in ('on', 'off'):
            for key in ('batches', 'visits', 'materialized_outcomes'):
                self.reject(lambda d, s=side, k=key: d['controls'][0][s]['leaf'].update({k: 0}))
            self.reject(lambda d, s=side: d['controls'][0][s]['leaf'].update(batches=True))
            self.reject(lambda d, s=side: d['controls'][0][s]['leaf'].pop('emitted_instructions'))

    def test_forged_row_or_global_pass_bits_not_accepted(self):
        self.reject(lambda d: d.update(passed=1))
        self.reject(lambda d: d['controls'][0].update(passed=False))
        self.reject(lambda d: d['controls'][0].update(equal=False))

    def test_missing_or_failed_separate_gate_cannot_pass_summary(self):
        base = CombinedContractTests()
        base.setUp()
        for mutate in [lambda a: a.pop('prepared_nonleaf'),
                       lambda a: a['prepared_nonleaf'].update(passed=False),
                       lambda a: a['prepared_nonleaf'].update(timeout=True),
                       lambda a: a['prepared_nonleaf'].update(returncode=1),
                       lambda a: a['prepared_nonleaf'].pop('binary_sha256'),
                       lambda a: a['prepared_nonleaf']['probe']['controls'].pop(),
                       lambda a: a['prepared']['controls'][0].update(leaf_active_both=False),
                       lambda a: a['prepared']['controls'][0]['off_metadata'].update(prepared_requested=True)]:
            summary = base.summary()
            mutate(summary['activation']['combined-on'])
            with self.assertRaises(ValueError):
                contract.validate_summary(summary)

    def test_separate_process_failure_or_forged_json_preserves_failed_receipt(self):
        for fault in ('none', 'nonzero', 'timeout', 'invalid', 'duplicate-key', 'forged-pass'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / 'activation.exe'
                binary.write_bytes(b'inert identity for mocked execution')
                def fake_bounded(command, stdout, stderr, timeout, **kwargs):
                    self.assertEqual(command, [str(binary)])
                    self.assertEqual(kwargs['env']['LAB_ENGINE_FACTORED'], '0')
                    value = activation_probe()
                    if fault == 'forged-pass':
                        value['controls'][0]['on']['validation_counts'] = [32, 64, 32]
                    text = json.dumps(value)
                    if fault == 'invalid': text = '{broken'
                    if fault == 'duplicate-key': text = text[:-1] + ', "passed": true}'
                    stdout.write_text(text, encoding='utf8')
                    stderr.write_text('retained diagnostics', encoding='utf8')
                    return (1 if fault == 'nonzero' else 0, fault == 'timeout')
                with patch.object(controller, 'bounded', side_effect=fake_bounded):
                    receipt = controller.run_nonleaf_activation({'activation': binary}, root)
                self.assertEqual(receipt['passed'], fault == 'none')
                self.assertTrue((root / 'prepared-nonleaf-activation.json').is_file())
                self.assertTrue((root / 'prepared-nonleaf-activation.stdout').is_file())
                self.assertTrue((root / 'prepared-nonleaf-activation.stderr').is_file())

    def test_standalone_prepared_keeps_original_reduction_contract(self):
        base = CombinedContractTests()
        base.setUp()
        variant = {'features': ['lab-search/experiment-prepared-turn-observe']}
        # Preserve its existing first-eight-successful selection and any-reduction rule.
        records = [r for r in base.records if r['id'].startswith('deep/')]
        def off(job, binaries, result_dir):
            result = base.off(job, binaries, result_dir)
            result['stderr'] = json.dumps(base.metadata(prepared=False, count=8))
            return result
        with patch.object(controller, 'run_case', side_effect=off), \
             patch.object(controller, 'run_nonleaf_activation') as probe:
            result = controller.activation_checks(variant, records, base.jobs, {}, Path('.'))
        self.assertTrue(result['passed'])
        self.assertNotIn('prepared_nonleaf', result)
        probe.assert_not_called()
        self.assertFalse(controller.uses_combined_activation(variant))

    def test_combined_precedence_rejects_forged_flags_or_duplicate_observers(self):
        base = CombinedContractTests()
        base.setUp()
        for flag, value in [('prepared_requested', True), ('prepared_compiled', False),
                            ('prepared_observer_compiled', False), ('leaf_observer_compiled', False),
                            ('requested_threads', 2), ('factored', True)]:
            def wrong(job, binaries, result_dir, k=flag, v=value):
                result = base.off(job, binaries, result_dir)
                meta = json.loads(result['stderr'])
                meta[k] = v
                result['stderr'] = json.dumps(meta)
                return result
            self.assertFalse(base.activate(wrong)['passed'])
        base.records[-9]['stderr'] += '\n' + base.records[-9]['stderr']
        self.assertFalse(base.activate()['passed'])


if __name__ == '__main__':
    unittest.main()
