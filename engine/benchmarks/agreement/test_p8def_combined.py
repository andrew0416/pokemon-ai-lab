"""Combined-controller regressions. Synthetic records are not engine verdicts."""
import contextlib
import copy
import gzip
import hashlib
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import p8def_combined_contract as c
import test_combined
import test_p8def
import test_run

controller = test_run.controller


def variants():
    return {'schema': 1, 'variants': [
        {'id': 'base', 'sha': c.SOURCE_SHA, 'features': sorted(c.BASE_FEATURES), 'compare_to': None},
        {'id': c.CANDIDATE, 'sha': c.SOURCE_SHA, 'features': sorted(c.FEATURES),
         'compare_to': 'base', 'comparison_contract': c.STATE_CONTRACT}]}


def summary():
    fixture = test_combined.CombinedContractTests()
    fixture.setUp()
    original = fixture.summary()
    value = copy.deepcopy(original)
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        value[name] = {key: copy.deepcopy(original[name]['combined-on']) for key in ('base', c.CANDIDATE)}
    comparison = copy.deepcopy(original['comparisons']['combined-on'])
    comparison.update(baseline='base', comparison_contract=c.STATE_CONTRACT,
                      raw_turn_different_ids=['turn/fixture/sample'],
                      raw_turn_differences=[{'id': 'turn/fixture/sample',
                          'baseline_sha256': 'a' * 64, 'candidate_sha256': 'b' * 64,
                          'baseline_stdout_bytes': 200, 'candidate_stdout_bytes': 150}])
    value['comparisons'] = {c.CANDIDATE: comparison}
    return value


def plan():
    jobs = []
    for kind, count in c.previous.COUNTS.items():
        special = list(c.previous.ORACLES if kind == 'oracle' else c.previous.SCOPED if kind == 'turn' else [])
        for index in range(count):
            key = special[index] if index < len(special) else f'{kind}/synthetic/{index}'
            job = {'id': key, 'kind': kind, 'input_id': key,
                   'args': ['input', 'deep' if index < 8 else 'mixed', '1', 'expect', 'median', '1']}
            if key in c.previous.ORACLES:
                job['oracle_contract'] = c.previous.ORACLES[key]
            if key in c.previous.SCOPED:
                job['args'] += ['--before', 'requested-before']
            jobs.append(job)
    return {'archive_sha256': c.previous.ARCHIVE_SHA256, 'jobs': jobs}


def record(rows, raw):
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'stdout_bytes': len(raw),
            'turn_state_sha256': c.turn_state_digest(rows)}


class CombinedConfigurationTests(unittest.TestCase):
    def test_exact_two_same_frozen_source_variants_and_feature_closure(self):
        value = variants()
        c.validate_variants(value)
        baseline, candidate = value['variants']
        base_core, base_search = controller.expected_features(baseline)
        core, search = controller.expected_features(candidate)
        self.assertEqual(core - base_core, {'experiment-replay-action-keys', 'experiment-slot-diff',
                                            'experiment-stats-off-cost'})
        self.assertEqual(search, base_search)
        self.assertTrue({'experiment-leaf-ending-observer', 'experiment-prepared-turn-observe'} <= core)
        c.validate_variants(controller.read(Path(__file__).with_name('variants.json')))

    def test_missing_extra_duplicate_flags_wrong_source_p11_and_independent_variants_rejected(self):
        mutations = [lambda v: v['variants'].pop(),
                     lambda v: v['variants'].reverse(),
                     lambda v: v['variants'].append(copy.deepcopy(v['variants'][1])),
                     lambda v: v['variants'][1].update(id='p8e'),
                     lambda v: v['variants'][1].update(compare_to='p8d'),
                     lambda v: v['variants'][1].update(comparison_contract='exact-v1')]
        for index in (0, 1):
            mutations += [lambda v, i=index: v['variants'][i].update(sha='a' * 40),
                          lambda v, i=index: v['variants'][i]['features'].pop(),
                          lambda v, i=index: v['variants'][i]['features'].append(v['variants'][i]['features'][0]),
                          lambda v, i=index: v['variants'][i]['features'].append('lab-engine/experiment-inline-runstart')]
        for mutate in mutations:
            value = variants()
            mutate(value)
            with self.subTest(value=value), self.assertRaises(ValueError):
                c.validate_variants(value)

    def test_frozen_plan_denominator_special_contracts_and_controls_cannot_shrink(self):
        value = plan()
        c.validate_plan(value)
        for mutate in [lambda p: p['jobs'].pop(),
                       lambda p: p.update(archive_sha256='f' * 64),
                       lambda p: p['jobs'][0].pop('oracle_contract'),
                       lambda p: p['jobs'][-1].update(id=p['jobs'][0]['id']),
                       lambda p: p['jobs'][-c.previous.COUNTS['search']]['args'].__setitem__(1, 'mixed')]:
            bad = copy.deepcopy(value)
            mutate(bad)
            with self.assertRaises(ValueError):
                c.validate_plan(bad)


class CombinedComparisonTests(unittest.TestCase):
    def test_only_outcome_instruction_text_can_differ_and_search_remains_exact(self):
        before, after = test_p8def.outcome(), test_p8def.outcome()
        after['instructions'] = '[SetLastMove]'
        original, candidate = record([before], b'raw Switch'), record([after], b'raw SetLastMove')
        variant = variants()['variants'][1]
        self.assertTrue(controller.comparison_equal(variant, 'turn', candidate, original))
        self.assertFalse(controller.comparison_equal(variant, 'search', candidate, original))
        baseline = variants()['variants'][0]
        self.assertFalse(controller.comparison_equal(baseline, 'turn', candidate, original))
        for key in ('sha256', 'stdout_bytes', 'turn_state_sha256'):
            bad = dict(candidate)
            del bad[key]
            with self.assertRaises(ValueError):
                controller.comparison_equal(variant, 'turn', bad, original)
        for features in [sorted(c.BASE_FEATURES | {c.independent.FEATURES['p8e']}),
                         sorted(c.FEATURES) + ['lab-engine/experiment-inline-runstart']]:
            with self.assertRaises(ValueError):
                controller.comparison_equal(dict(variant, features=features), 'turn', candidate, original)

    def test_new_fields_hidden_state_probabilities_order_errors_and_nonoutcome_instructions_remain_exact(self):
        original = test_p8def.outcome()
        digest = c.turn_state_digest([original])
        changes = [('probability_bits', 124), ('position', 1), ('outcome', 1), ('suspension', 'pending'),
                   ('party_order', '[1,0]'), ('hidden', {'known': False}), ('new_future_field', 1)]
        changes += [('state', {**original['state'], key: value}) for key, value in
                    [('debug', 'different full state'), ('key_hash', 101), ('position_hash', 201),
                     ('future_hidden_payload', 'new')]]
        for key, value in changes:
            changed = copy.deepcopy(original)
            changed[key] = value
            self.assertNotEqual(c.turn_state_digest([changed]), digest, key)
        for kind in ('error', 'position', 'complete'):
            self.assertNotEqual(c.turn_state_digest([original, {'kind': kind, 'instructions': 'a'}]),
                                c.turn_state_digest([original, {'kind': kind, 'instructions': 'b'}]))
        self.assertNotEqual(c.turn_state_digest([original, {'kind': 'complete', 'errors': 0}]),
                            c.turn_state_digest([{'kind': 'complete', 'errors': 0}, original]))

    def test_apply_reverse_and_incremental_hash_evidence_remains_required(self):
        for key in ('instructions', 'state', 'input_restored', 'incremental_hash_checked', 'hidden'):
            value = test_p8def.outcome()
            del value[key]
            with self.assertRaises(ValueError):
                c.turn_state_digest([value])
        for key in ('input_restored', 'incremental_hash_checked'):
            value = test_p8def.outcome()
            value[key] = False
            with self.assertRaises(ValueError):
                c.turn_state_digest([value])

    def test_raw_stream_is_retained_while_combined_turn_semantics_agree(self):
        streams = [test_run.turn_output(), test_run.turn_output()]
        streams[1][3]['instructions'] = '[SetLastMove]'
        with tempfile.TemporaryDirectory() as temporary:
            records = []
            for index, rows in enumerate(streams):
                directory = Path(temporary) / str(index)
                directory.mkdir()
                payload = b''.join((json.dumps(row) + '\n').encode() for row in rows)
                def child(command, stdout, stderr, limit, **kwargs):
                    stdout.write_bytes(payload)
                    stderr.write_text('diagnostics preserved', encoding='utf-8')
                    return 0, False
                job = {'id': 'turn/synthetic', 'kind': 'turn', 'input_id': 'synthetic', 'args': [], 'timeout': 1}
                with patch.object(controller, 'bounded', side_effect=child):
                    value = controller.run_case(job, {'turn': Path('inert')}, directory)
                self.assertTrue(value['complete'] and value['successful'])
                self.assertEqual(value['stdout_bytes'], len(payload))
                self.assertEqual(value['sha256'], hashlib.sha256(payload).hexdigest())
                self.assertEqual(gzip.decompress((directory / value['output_file']).read_bytes()), payload)
                records.append(value)
            self.assertNotEqual(records[0]['sha256'], records[1]['sha256'])
            self.assertTrue(controller.comparison_equal(variants()['variants'][1], 'turn', *records))


class CombinedSummaryTests(unittest.TestCase):
    def test_full_success_preserves_both_activations_and_raw_representation_ledger(self):
        c.validate_summary(summary())

    def test_wrong_denominator_equal_errors_omissions_or_independent_results_cannot_pass(self):
        for mutate in [lambda s: s.update(expected_cases=6183),
                       lambda s: s['comparisons'][c.CANDIDATE].update(equal_success=6183),
                       lambda s: s['comparisons'][c.CANDIDATE].update(equal_error=1),
                       lambda s: s['comparisons'][c.CANDIDATE].update(uncompared=1),
                       lambda s: s['comparisons'].update(p8d=s['comparisons'][c.CANDIDATE]),
                       lambda s: s['oracle']['base'].update(match=3055),
                       lambda s: s['oracle_contracts'][c.CANDIDATE][-1]['checks'].update(direct_oracle_histories=1),
                       lambda s: s['scoped_turn_coverage'][c.CANDIDATE][0].update(expected_choice_rejections=0)]:
            value = summary()
            mutate(value)
            with self.assertRaises(ValueError):
                c.validate_summary(value)

    def test_each_sides_activation_must_have_real_prepared_leaf_and_nonleaf_evidence(self):
        for side in ('base', c.CANDIDATE):
            for mutate in [lambda a: a.update(passed=False), lambda a: a.update(leaf_required=False),
                           lambda a: a.pop('prepared_nonleaf'),
                           lambda a: a['prepared'].update(passed=False),
                           lambda a: a['prepared_nonleaf']['probe']['controls'][0]['off'].update(signature='different')]:
                value = summary()
                mutate(value['activation'][side])
                with self.assertRaises(ValueError):
                    c.validate_summary(value)

    def test_raw_differences_cannot_be_hidden_duplicated_or_lose_bytes_and_digests(self):
        for mutate in [lambda r: r.pop('raw_turn_different_ids'),
                       lambda r: r.update(raw_turn_different_ids=['search/x']),
                       lambda r: r['raw_turn_different_ids'].append(r['raw_turn_different_ids'][0]),
                       lambda r: r.pop('raw_turn_differences'),
                       lambda r: r['raw_turn_differences'][0].update(id='turn/different'),
                       lambda r: r['raw_turn_differences'][0].update(candidate_sha256='not-a-sha'),
                       lambda r: r['raw_turn_differences'][0].update(candidate_sha256='a' * 64),
                       lambda r: r['raw_turn_differences'][0].update(candidate_stdout_bytes=True)]:
            value = summary()
            mutate(value['comparisons'][c.CANDIDATE])
            with self.assertRaises(ValueError):
                c.validate_summary(value)

    def test_runner_two_variant_full_summary_and_new_contract_are_actually_routed(self):
        # Exercise all comparator denominators without running an engine or
        # claiming these synthetic records are fresh broad accuracy evidence.
        data, evidence = plan(), summary()
        special = {row['id']: row for row in evidence['oracle_contracts']['base']}
        scoped = {row['id']: row for row in evidence['scoped_turn_coverage']['base']}
        rows = []
        for job in data['jobs']:
            row = {'id': job['id'], 'kind': job['kind'], 'status': 'ok', 'complete': True,
                   'successful': True, 'sha256': 'a' * 64, 'stdout_bytes': 200}
            if job['kind'] == 'turn':
                row['turn_state_sha256'] = 'c' * 64
            elif job['kind'] == 'oracle':
                row.update(status='match', verdict={'status': 'match', 'engineMs': 1})
            if job['id'] in special:
                row.update(oracle_contract=job['oracle_contract'], verdict=copy.deepcopy(special[job['id']]))
            if job['id'] in scoped:
                coverage = copy.deepcopy(scoped[job['id']])
                for key in ('id', 'status', 'success_scope'):
                    del coverage[key]
                row.update(success_scope='requested', coverage=coverage)
            rows.append(row)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            controller.write(root / 'variants.json', variants())
            controller.write(root / 'case-plan.json', data)
            for side in ('base', c.CANDIDATE):
                cases = copy.deepcopy(rows)
                if side == c.CANDIDATE:
                    changed = next(row for row in cases if row['kind'] == 'turn')
                    changed.update(sha256='b' * 64, stdout_bytes=150)
                controller.write(root / side / 'results.json', {
                    'variant': side, 'expected': 6184, 'observed': 6184, 'cases': cases,
                    'plan_sha256': controller.sha(root / 'case-plan.json'),
                    'activation': evidence['activation'][side]})
            args = SimpleNamespace(results=root, out=root / 'summary.json')
            with patch.object(controller, 'HERE', root), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(controller.compare(args), 0)
            result = controller.read(args.out)
            c.validate_summary(result)
            self.assertEqual(set(result['comparisons']), {c.CANDIDATE})
            comparison = result['comparisons'][c.CANDIDATE]
            self.assertEqual(comparison['equal_success'], 6184)
            self.assertEqual(comparison['raw_turn_different_ids'], [changed['id']])
            self.assertEqual(comparison['raw_turn_differences'][0]['candidate_stdout_bytes'], 150)
            # Internal routing must reject missing detailed activation even if
            # every output still agrees and both top-level passed bits say true.
            path = root / 'base' / 'results.json'
            baseline = controller.read(path)
            baseline['activation'] = {'passed': True}
            controller.write(path, baseline)
            with patch.object(controller, 'HERE', root), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(controller.compare(args), 1)
            self.assertFalse(controller.read(args.out)['complete'])
            self.assertTrue(controller.read(args.out)['validation_errors'])


if __name__ == '__main__':
    unittest.main()
