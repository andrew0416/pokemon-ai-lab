"""Synthetic protocol tests; no Rust execution or performance measurement."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import common as p
import ci
import test_contract as fixture

class ProtocolTests(unittest.TestCase):
    def test_actual_frozen_plans_and_corpus(self):
        workspace = p.HERE.parents[3]
        for case_id in p.PLAN_SHA:
            case, plan = p.fixed_case(workspace, case_id)
            self.assertEqual(case['id'], case_id)
            self.assertEqual(p.c.sha(plan), p.PLAN_SHA[case_id])
        self.assertEqual(len(p.features(False)), 6)
        self.assertEqual(p.features(True), p.features(False) + [p.FEATURE])

    def test_unbound_source_fails_closed(self):
        value = p.c.strict_json((p.HERE / 'source-binding.json').read_bytes())
        value['source_sha'] = 'UNBOUND_P1E_SOURCE_SHA'
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'bad.json'
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                p.binding(path)

    def test_factorization_layout_change_does_not_claim_distribution_equality(self):
        left = fixture.value()
        right = copy.deepcopy(left)
        right['reference'].update(components=3, flat_count_upper_bound=9, suspended_components=1)
        right['reference']['kernel_ns'] = 50
        right['samples'][0]['full_state']['coverage'] -= 1e-12
        proof = p.comparison(left, right, fixture.plan(), fixture.CASE)
        self.assertTrue(proof['passed'])
        self.assertFalse(proof['full_output_state_or_sampled_state_equality_proven'])
        pair = p.timed_pair({'status': 'ok', 'result': left}, {'status': 'ok', 'result': right}, fixture.plan(), fixture.CASE)
        self.assertEqual(pair['candidate_over_baseline'], 0.5)

    def test_plan_seeds_mass_and_derived_metric_drifts_fail(self):
        changes = (
            lambda v: v['description'].update(full_state_debug='changed'),
            lambda v: v['samples'][0].update(seed=999),
            lambda v: v['reference'].update(total_mass=0.5),
            lambda v: v['samples'][0]['full_state'].update(coverage=0.5, tv=0.5),
            lambda v: v['samples'][0].update(raw_outcomes=2),
        )
        for change in changes:
            right = fixture.value()
            change(right)
            with self.subTest(change=change), self.assertRaises(ValueError):
                p.comparison(fixture.value(), right, fixture.plan(), fixture.CASE)

    def test_censored_tail_has_no_kernel_ratio_or_full500_completion(self):
        complete = {'status': 'ok', 'result': fixture.value(),
                    'measurement': {'process': {'wall_seconds': 20, 'timeout_seconds': 300}}}
        timeout = {'status': 'timeout', 'measurement': {'process': {'wall_seconds': 300.1, 'timeout_seconds': 300}}}
        for off, on in ((timeout, complete), (complete, timeout), (timeout, timeout)):
            value = p.tail_comparison(off, on, fixture.plan(), fixture.CASE)
            self.assertIsNone(value['kernel_ratio'])
            self.assertFalse(value['full500_complete'])
            self.assertIsNone(value['official_full500_metrics'])
            self.assertTrue(value['censored'])
        value = p.tail_comparison(timeout, complete, fixture.plan(), fixture.CASE)
        self.assertEqual(value['whole_process_bound']['baseline_over_candidate_process_wall_lower_bound'], 15)
        with self.assertRaises(ValueError):
            p.tail_comparison({'status': 'rss_limit'}, complete, fixture.plan(), fixture.CASE)

    def test_control_summary_requires_all_exact_pairs(self):
        rows = [{'case_id': case, 'repeat': repeat, 'timing': {'baseline_reference_kernel_ns': 100,
                  'candidate_reference_kernel_ns': 80, 'candidate_over_baseline': 0.8}}
                for repeat in range(3) for case in p.CONTROL_CASES]
        result = p.control_summary(rows)
        self.assertFalse(result['full500_complete'])
        self.assertAlmostEqual(result['by_case']['opening-0000']['median_reduction_percent'], 20)
        for bad in (rows[:-1], rows[:-1] + [rows[0]]):
            with self.assertRaises(ValueError):
                p.control_summary(bad)

    def test_named_tests_cannot_be_missing_ignored_or_filtered(self):
        text = 'test bounded_case ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'
        self.assertEqual(ci.named_test_proof(text, ['bounded_case'], 0)['passed'], 1)
        for bad in (text.replace('1 passed', '0 passed'), text.replace('0 ignored', '1 ignored'),
                    text.replace('0 filtered', '1 filtered'), text.replace('bounded_case', 'other')):
            with self.assertRaises(ValueError):
                ci.named_test_proof(bad, ['bounded_case'], 0)

    def test_expected_panic_passes_are_named_but_ignored_or_failed_panics_reject(self):
        names = ['turn::lazy::p1e_dealt_damage_tests::dependent_damage_cannot_be_read_after_its_lazy_run_ended',
                 'turn::lazy::p1e_dealt_damage_tests::unresolved_damage_cannot_silently_enter_a_hashed_key']
        text = ''.join('test ' + name + ' - should panic ... ok\n' for name in names)
        text += 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'
        self.assertEqual(ci.named_test_proof(text, names, 0)['named_tests'], names)
        for bad in (text.replace('... ok', '... ignored', 1), text.replace('... ok', '... FAILED', 1)):
            with self.assertRaises(ValueError):
                ci.named_test_proof(bad, names, 0)

    def test_actual_fingerprints_must_match_core6_on_off_and_generic_target(self):
        bound = {'core_suites': [{'arms': ['on']}]}
        for arm in ('original', 'off', 'on'):
            for fault in (None, 'observer', 'avx', 'missing', 'search'):
                with tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    fp = root / ('target-p1e-' + arm) / 'release/.fingerprint'
                    sets = {'lab-engine': ['lib-lab_engine.json'] + (['test-lib-lab_engine.json'] if arm == 'on' else []),
                            'lab-scenario': ['lib-lab_scenario.json', 'bin-lab-distribution-bench.json',
                                             'test-bin-lab-distribution-bench.json', 'test-integration-test-factored.json',
                                             'test-integration-test-lazy_ko_damage.json']}
                    for package, names in sets.items():
                        folder = fp / (package + '-test')
                        folder.mkdir(parents=True)
                        for name in names:
                            if fault == 'missing' and name == 'test-integration-test-factored.json':
                                continue
                            flags = p.features(arm == 'on') if package == 'lab-engine' else []
                            if fault == 'observer':
                                flags += ['experiment-frontier-observer']
                            value = {'features': json.dumps(flags), 'rustflags': ['-Ctarget-cpu=native' if fault == 'avx' else '-Ctarget-cpu=x86-64']}
                            (folder / name).write_text(json.dumps(value))
                    if fault == 'search':
                        (fp / 'lab-search-rogue').mkdir()
                    if fault:
                        with self.assertRaises(ValueError):
                            ci.fingerprints(root, arm, bound)
                    else:
                        self.assertEqual(len(ci.fingerprints(root, arm, bound)), 7 if arm == 'on' else 6)

if __name__ == '__main__':
    unittest.main()
