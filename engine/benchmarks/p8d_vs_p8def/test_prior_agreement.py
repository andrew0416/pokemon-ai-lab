"""No network or engine execution: strict pins, reusable evidence and tamper cases."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import prior_agreement as gate


def remote_fixture(label):
    run = {'id': gate.EXPECTED_RUNS[label], 'head_sha': gate.EXPECTED_HEADS[label],
           'repository': {'full_name': gate.REPO}, 'head_repository': {'full_name': gate.REPO},
           'event': 'workflow_dispatch', 'run_attempt': 1, 'status': 'completed',
           'path': '.github/workflows/engine-benchmark.yml',
           'conclusion': 'failure' if label == 'd' else 'success'}
    rows = []
    for job_id, name in gate.JOBS[label].items():
        rows.append({'id': job_id, 'name': name if name.startswith(('agreement_', 'speed ')) else
                     f'agreement_evaluate ({name}, {gate.SOURCE}, exact-pinned-features)',
                     'run_id': run['id'], 'head_sha': run['head_sha'], 'run_attempt': 1,
                     'status': 'completed', 'conclusion': 'success'})
    if label == 'd':
        rows.append({'id': 109782315543, 'name': 'speed stats-off-cost', 'run_id': run['id'],
                     'head_sha': run['head_sha'], 'run_attempt': 1,
                     'status': 'completed', 'conclusion': 'failure'})
    artifact = {'id': gate.ARTIFACT_IDS[label], 'name': 'agreement-summary', 'expired': False,
                'digest': gate.ARTIFACT_DIGESTS[label], 'workflow_run': {'id': run['id'],
                    'head_sha': run['head_sha'], 'repository_id': gate.REPO_ID,
                    'head_repository_id': gate.REPO_ID}}
    commit = {'sha': run['head_sha'], 'tree': {'sha': 'a' * 40}}
    tree = {'sha': 'a' * 40, 'truncated': False, 'tree': [
        {'path': 'engine/benchmarks/agreement', 'type': 'tree',
         'sha': gate.load_pins()['controllers'][label]['agreement_tree']}]}
    return {'run': run, 'jobs': {'total_count': len(rows), 'jobs': rows},
            'artifact': artifact, 'commit': commit, 'tree': tree}


def summaries():
    gate.contracts()  # Make the unchanged agreement fixtures importable.
    import test_combined
    fixture = test_combined.CombinedContractTests()
    fixture.setUp()
    original = fixture.summary()
    values = {}
    ids = [f'turn/synthetic/{index:04}' for index in range(1505)]
    for label, variants in [('d', ('base', 'p8d', 'p8e', 'p8f')), ('def', ('base', 'p8def_combined'))]:
        value = copy.deepcopy(original)
        for field in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
            value[field] = {name: copy.deepcopy(original[field]['combined-on']) for name in variants}
        value['comparisons'] = {}
        for name in variants[1:]:
            semantic = name in ('p8e', 'p8def_combined')
            row = copy.deepcopy(original['comparisons']['combined-on'])
            row.update(baseline='base', comparison_contract='state-equivalent-slot-diff-v1' if semantic else 'exact-v1',
                       raw_turn_different_ids=list(ids) if semantic else [])
            if label == 'def':
                row['raw_turn_differences'] = [{'id': key, 'baseline_sha256': 'a' * 64,
                    'candidate_sha256': 'b' * 64, 'baseline_stdout_bytes': 200,
                    'candidate_stdout_bytes': 100} for key in ids]
            value['comparisons'][name] = row
        values[label] = value
    return values


class RemoteTests(unittest.TestCase):
    def test_exact_relevant_jobs_pass_despite_original_unrelated_f_failure(self):
        for label in gate.EXPECTED_RUNS:
            value = remote_fixture(label)
            receipt = gate.validate_remote(label, value['run'], value['jobs'])
            self.assertEqual(receipt['required_successful_job_ids'], sorted(gate.JOBS[label]))
        document = {'schema': 1, 'evidence': {label: remote_fixture(label) for label in gate.EXPECTED_RUNS}}
        self.assertEqual(set(gate.validate_remote_document(document)), {'d', 'def'})

    def test_run_repository_head_event_attempt_status_and_workflow_are_pinned(self):
        for label in gate.EXPECTED_RUNS:
            for key, value in [('id', 1), ('head_sha', 'b' * 40), ('event', 'push'),
                               ('run_attempt', 2), ('run_attempt', True), ('status', 'in_progress'),
                               ('path', '.github/workflows/other.yml'), ('conclusion', 'cancelled'),
                               ('repository', {'full_name': 'other/repo'}),
                               ('head_repository', {'full_name': 'fork/repo'})]:
                data = remote_fixture(label)
                data['run'][key] = value
                with self.subTest(label=label, key=key), self.assertRaises(ValueError):
                    gate.validate_remote(label, data['run'], data['jobs'])

    def test_required_job_failure_or_reidentification_is_rejected(self):
        for label in gate.EXPECTED_RUNS:
            for job_index in range(len(gate.JOBS[label])):
                for key, value in [('conclusion', 'failure'), ('status', 'queued'), ('run_attempt', 2),
                                   ('run_id', 1), ('head_sha', 'b' * 40), ('name', 'unrelated')]:
                    data = remote_fixture(label)
                    data['jobs']['jobs'][job_index][key] = value
                    with self.assertRaises(ValueError):
                        gate.validate_remote(label, data['run'], data['jobs'])

    def test_partial_duplicate_or_missing_required_job_listing_rejected(self):
        for mutate in [lambda d: d['jobs'].pop(0),
                       lambda d: d['jobs'].__setitem__(0, copy.deepcopy(d['jobs'][1])),
                       lambda d: d.update(total_count=100),
                       lambda d: d['jobs'][0].update(id=123)]:
            data = remote_fixture('d')
            mutate(data['jobs'])
            with self.assertRaises(ValueError):
                gate.validate_remote('d', data['run'], data['jobs'])

    def test_artifact_id_archive_digest_owner_and_expiration_are_pinned(self):
        for label in gate.EXPECTED_RUNS:
            data = remote_fixture(label)
            gate.validate_artifact(label, data['artifact'])
            for key, value in [('id', 1), ('name', 'other'), ('expired', True), ('digest', 'sha256:' + 'c' * 64)]:
                bad = copy.deepcopy(data['artifact'])
                bad[key] = value
                with self.assertRaises(ValueError):
                    gate.validate_artifact(label, bad)
            for key in ('id', 'head_sha', 'repository_id', 'head_repository_id'):
                bad = copy.deepcopy(data['artifact'])
                bad['workflow_run'][key] = 'wrong'
                with self.assertRaises(ValueError):
                    gate.validate_artifact(label, bad)

    def test_prior_immutable_controller_tree_cannot_be_truncated_or_replaced(self):
        for label in gate.EXPECTED_RUNS:
            data = remote_fixture(label)
            gate.validate_controller_tree(label, data['commit'], data['tree'])
            for mutate in [lambda d: d['tree'].update(truncated=True),
                           lambda d: d['tree'].update(sha='b' * 40),
                           lambda d: d['tree']['tree'][0].update(sha='b' * 40),
                           lambda d: d['tree']['tree'].append(copy.deepcopy(d['tree']['tree'][0])),
                           lambda d: d['commit'].update(sha='b' * 40),
                           lambda d: d['tree']['tree'].clear()]:
                bad = copy.deepcopy(data)
                mutate(bad)
                with self.assertRaises(ValueError):
                    gate.validate_controller_tree(label, bad['commit'], bad['tree'])

    def test_remote_document_requires_both_evidence_records(self):
        with self.assertRaises(ValueError):
            gate.validate_remote_document({'schema': 1, 'evidence': {'d': remote_fixture('d')}})


class SourceTests(unittest.TestCase):
    def test_actual_frozen_controller_and_actual_direct_feature_mode_pass(self):
        value = gate.validate_source()
        self.assertEqual(value['source_sha'], gate.SOURCE)
        self.assertEqual(value['shared_frozen_file_count'], 15)
        self.assertEqual(value['timing_features'], {label: sorted(flags) for label, flags in gate.RUNTIME_FEATURES.items()})

    def test_timing_feature_missing_duplicate_observer_or_p11_is_rejected(self):
        for label in ('baseline', 'candidate'):
            for action in ('remove', 'duplicate', 'observer', 'p11'):
                def reader(mode, arm):
                    self.assertEqual(mode, gate.MODE)
                    flags = sorted(gate.RUNTIME_FEATURES[arm])
                    if arm == label:
                        if action == 'remove': flags.pop()
                        elif action == 'duplicate': flags.append(flags[0])
                        elif action == 'observer': flags.append('lab-engine/experiment-slot-diff-observer')
                        else: flags.append('lab-engine/experiment-inline-runstart')
                    return ['--features', ','.join(flags)]
                with self.assertRaises(ValueError):
                    gate.validate_source(feature_reader=reader)

    def test_pinned_manifest_or_frozen_probe_cannot_change(self):
        with patch.object(gate, 'PINS_SHA256', '0' * 64), self.assertRaises(ValueError):
            gate.validate_source()
        original = Path.read_bytes
        def changed(path):
            raw = original(path)
            return raw + b'changed' if path.name == 'turn_probe.rs' else raw
        with patch.object(Path, 'read_bytes', changed), self.assertRaises(ValueError):
            gate.validate_source()

    def test_source_revision_or_common_baseline_change_is_rejected(self):
        with patch.object(gate, 'SOURCE', 'b' * 40), self.assertRaises(ValueError):
            gate.validate_source()
        pins = gate.load_pins()
        pins['controllers']['d']['variants']['variants'][0]['features'].pop()
        with patch.object(gate, 'load_pins', return_value=pins), self.assertRaises(ValueError):
            gate.validate_source()


class SummaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.values = summaries()

    def test_exact_and_semantic_summaries_both_pass_their_original_contracts(self):
        for label, value in self.values.items():
            gate.validate_summary_data(label, value)

    def test_success_flags_cannot_hide_denominator_errors_omissions_or_weakened_contract(self):
        for label, key in [('d', 'p8d'), ('def', 'p8def_combined')]:
            for mutate in [lambda s: s.update(expected_cases=6183),
                           lambda s: s['comparisons'][key].update(equal_success=6183),
                           lambda s: s['comparisons'][key].update(equal_error=1),
                           lambda s: s['comparisons'][key].update(different=1),
                           lambda s: s['comparisons'][key].update(uncompared=1),
                           lambda s: s['comparisons'][key].update(comparison_contract='unknown'),
                           lambda s: s['oracle']['base'].update(match=3055),
                           lambda s: s['activation']['base'].update(passed=False)]:
                bad = copy.deepcopy(self.values[label])
                mutate(bad)
                with self.assertRaises(ValueError):
                    gate.validate_summary_data(label, bad)

    def test_1505_raw_p8e_evidence_rows_are_required(self):
        for field in ('raw_turn_different_ids', 'raw_turn_differences'):
            bad = copy.deepcopy(self.values['def'])
            bad['comparisons']['p8def_combined'][field].pop()
            with self.assertRaises(ValueError):
                gate.validate_summary_data('def', bad)
        bad = copy.deepcopy(self.values['def'])
        bad['comparisons']['p8def_combined']['raw_turn_differences'][0]['candidate_sha256'] = 'bad'
        with self.assertRaises(ValueError):
            gate.validate_summary_data('def', bad)

    def test_wrong_summary_bytes_fail_before_any_contract_can_accept(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'summary.json'
            path.write_text('{"complete":true}', encoding='utf8')
            with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                gate.validate_summaries(path, path)

    def test_receipt_preserves_ledger_and_labels_shared_baseline_inference(self):
        with tempfile.TemporaryDirectory() as temporary:
            paths = {}
            for label, value in self.values.items():
                paths[label] = Path(temporary) / (label + '.json')
                paths[label].write_text(json.dumps(value), encoding='utf8')
            hashes = {label: gate.sha(path.read_bytes()) for label, path in paths.items()}
            with patch.object(gate, 'SUMMARY_SHA256', hashes):
                value = gate.validate_summaries(paths['d'], paths['def'])
                self.assertFalse(value['fresh_direct_broad_execution'])
                self.assertTrue(value['fresh_regressions_observers_probes_and_timing_still_required'])
                self.assertEqual(len(value['raw_turn_differences']), 1505)
                # Even two independently valid semantic summaries cannot silently
                # substitute a different ledger under the common baseline claim.
                changed = copy.deepcopy(self.values['d'])
                changed['comparisons']['p8e']['raw_turn_different_ids'][0] = 'turn/other'
                paths['d'].write_text(json.dumps(changed), encoding='utf8')
                hashes['d'] = gate.sha(paths['d'].read_bytes())
                with self.assertRaisesRegex(ValueError, 'identities changed'):
                    gate.validate_summaries(paths['d'], paths['def'])

    def test_duplicate_keys_nonfinite_json_and_overwrite_are_rejected(self):
        for raw in ('{"id":1,"id":2}', '{"value":NaN}', '{"value":Infinity}'):
            with self.assertRaises(ValueError):
                gate.strict_json(raw)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'receipt.json'
            gate.write_new(path, {'first': True})
            with self.assertRaises(FileExistsError):
                gate.write_new(path, {'second': True})
            self.assertEqual(json.loads(path.read_text()), {'first': True})


if __name__ == '__main__':
    unittest.main()
