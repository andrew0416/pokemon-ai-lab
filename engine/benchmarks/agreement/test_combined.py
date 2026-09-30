"""Meaningful failure injections for combined feature and activation contracts."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

controller = load('run')
contract = load('combined_contract')


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
        return {'phase': 'search-complete', 'requested_threads': 1,
                'leaf_observer_compiled': True, 'leaf': {'batches': leaf, 'visits': leaf * 2},
                'prepared_compiled': True, 'prepared_observer_compiled': True,
                'prepared_requested': prepared, 'prepared': {'parent_checks': count}}

    def off(self, job, binaries, result_dir):
        self.assertTrue(job['id'].startswith('prepared-off-control/deep/'))
        self.assertEqual(job['args'][-2:], ['--prepared', 'off'])
        return {'complete': True, 'successful': True, 'sha256': 'same',
                'stderr': json.dumps(self.metadata(prepared=False, count=8))}

    def activate(self, child=None):
        with patch.object(controller, 'run_case', side_effect=child or self.off):
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
        self.assertTrue(result['prepared']['passed'])

    def test_prepared_nonactivation_fails_even_when_leaf_visits_exist(self):
        def equal_counts(job, binaries, result_dir):
            value = self.off(job, binaries, result_dir)
            value['stderr'] = json.dumps(self.metadata(prepared=False, count=1))
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
                'activation': {'combined-on': {'leaf_required': True, 'prepared_required': True,
                                               'leaf': {'passed': True}, 'prepared': {'passed': True}}}}

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


if __name__ == '__main__':
    unittest.main()
