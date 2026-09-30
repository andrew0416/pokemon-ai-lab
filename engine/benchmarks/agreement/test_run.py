"""Correctness controller regressions using tiny JSON and inert mocked children."""
import contextlib
import copy
import gzip
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


spec = importlib.util.spec_from_file_location('agreement_controller_under_test',
                                              Path(__file__).with_name('run.py'))
controller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(controller)


def search_output(status='ok'):
    return {'schema_version': 1, 'stage': 'search', 'status': status,
            'input': {'canonical_before': {'turn': 2}}, 'config': {'mode': 'mixed'},
            'result': {'value_bits': 4607182418800017408}, 'stats': {'cells': 1},
            'state_restored': True, 'state_after': 'complete logical state'}


def turn_output():
    return [
        {'kind': 'case', 'schema_version': 1, 'rolls': 'Median', 'factored': False},
        {'kind': 'loaded', 'slots': 2, 'state': {'logical': 'loaded'}},
        {'kind': 'position', 'position': 0, 'setup_probability_bits': 4607182418800017408},
        {'kind': 'outcome', 'position': 0, 'outcome': 0,
         'probability_bits': 4607182418800017408, 'instructions': '[SetHp]',
         'state': {'debug': 'after', 'key_hash': 1, 'position_hash': 2}, 'hidden': {'status': 'ok'},
         'suspension': 'None', 'party_order': '[0,1]',
         'input_restored': True, 'incremental_hash_checked': True},
        {'kind': 'position-complete', 'position': 0, 'outcomes': 1,
         'enumeration_restored': True, 'all_outcomes_reversed': True},
        {'kind': 'complete', 'schema_version': 1, 'status': 'ok',
         'positions': 1, 'outcomes': 1, 'errors': 0, 'hidden_diagnostics': 0,
         'enumerations_restored': 1, 'outcome_rollbacks': 1,
         'pinned_status': {'status': 'ok'}},
    ]


class OracleSemanticsTests(unittest.TestCase):
    def test_only_top_level_engine_time_is_excluded_and_input_is_not_mutated(self):
        original = {'status': 'match', 'engineMs': 12, 'scenario': 'same/scenario.json',
                    'report': 'same/report.json.gz', 'tv': 0.0,
                    'diagnostic': {'engineMs': 7}, 'elapsedMs': 23}
        saved = copy.deepcopy(original)
        result = controller.semantic_oracle(original)
        self.assertEqual(result, {key: value for key, value in original.items() if key != 'engineMs'})
        self.assertEqual(original, saved)
        changed = copy.deepcopy(original)
        changed['engineMs'] = 123456
        self.assertEqual(result, controller.semantic_oracle(changed))
        for key, value in [('tv', 1e-12), ('status', 'mismatch'),
                           ('elapsedMs', 24), ('diagnostic', {'engineMs': 8})]:
            changed = copy.deepcopy(original)
            changed[key] = value
            self.assertNotEqual(result, controller.semantic_oracle(changed), key)


class ProbeContractTests(unittest.TestCase):
    def test_search_success_and_structured_error_are_distinct(self):
        result = controller.validate_probe('search', iter([search_output()]))
        self.assertEqual(result['records'], 1)
        self.assertEqual(result['status'], 'ok')
        self.assertTrue(result['complete'])
        self.assertTrue(result['successful'])
        error = {'schema_version': 1, 'stage': 'setup', 'status': 'error',
                 'error': {'kind': 'Unsupported', 'reason': 'explicit unsupported fixture'}}
        result = controller.validate_probe('search', [error])
        self.assertTrue(result['complete'])
        self.assertFalse(result['successful'])
        self.assertEqual(result['status'], 'error')

    def test_search_missing_extra_schema_and_restore_fail_closed(self):
        missing = search_output()
        del missing['stats']
        cases = [[], [search_output(), search_output()], [missing],
                 [dict(search_output(), schema_version=2)],
                 [dict(search_output(), state_restored=False)],
                 [dict(search_output(), state_restored=None)]]
        for rows in cases:
            with self.subTest(rows=rows):
                with self.assertRaises(ValueError):
                    controller.validate_probe('search', rows)

    def test_turn_requires_complete_counts_and_restoration(self):
        result = controller.validate_probe('turn', iter(turn_output()))
        self.assertEqual(result['records'], 6)
        self.assertEqual(result['last'], turn_output()[-1])
        self.assertTrue(result['complete'])
        self.assertTrue(result['successful'])
        cases = [turn_output()[:-1], turn_output()+[turn_output()[-1]],
                 turn_output()[1:], list(reversed(turn_output()))]
        for key in ('positions', 'outcomes', 'errors', 'outcome_rollbacks'):
            rows = turn_output()
            rows[-1][key] += 1
            cases.append(rows)
        for row, field in ((3, 'input_restored'), (3, 'incremental_hash_checked'),
                           (4, 'enumeration_restored'), (4, 'all_outcomes_reversed')):
            rows = turn_output()
            rows[row][field] = False
            cases.append(rows)
        for rows in cases:
            with self.subTest(last=rows[-1] if rows else None):
                with self.assertRaises(ValueError):
                    controller.validate_probe('turn', rows)

    def test_turn_error_or_empty_cannot_be_a_success(self):
        case = turn_output()[0]
        complete = dict(turn_output()[-1], status='error', positions=0, outcomes=0,
                        errors=1, enumerations_restored=0, outcome_rollbacks=0)
        error = {'kind': 'error', 'stage': 'load', 'category': 'load-error',
                 'message': 'bad input', 'position': None}
        result = controller.validate_probe('turn', [case, error, complete])
        self.assertTrue(result['complete'])
        self.assertFalse(result['successful'])
        self.assertEqual(result['status'], 'error')
        result = controller.validate_probe('turn', [case, dict(complete, status='empty', errors=0)])
        self.assertTrue(result['complete'])
        self.assertFalse(result['successful'])


def contract_output(contract="gender-mixture-v1"):
    branches = [{"gender_assignment": sex, "setup_probability": 0.5, "normalized_weight": 0.5,
                 "state_restored": True, "distribution": {"canonical-state": 1.0}}
                for sex in ("M", "F")]
    checks = {"expected_assignments": 2, "distinct_assignments": 2,
              "uniform_weights": True, "matching_probability_mass": 1.0,
              "all_states_restored": True}
    value = {"schema_version": 1, "contract": contract, "status": "match",
             "scope": "complete-weighted-gender-mixture", "generated_positions": 2,
             "matching_positions": 2, "comparison": {"matches": True},
             "checks": checks, "branches": branches, "engine_distribution": {"canonical-state": 1.0}}
    if contract == "hidden-redirect-order-v1":
        value["scope"] = "both-hidden-histories-with-independent-oracles"
        checks.update(direct_oracle_histories=2, metamorphic_histories=0)
        for slot, branch in enumerate(branches):
            branch.update(recipient_slot=slot, validation="direct-oracle")
    return value


def scoped_turn_output():
    rows = turn_output()
    rows[0].update(schema_version=2, scenario="bb-choicelock-struggle.json", scope_policy="oracle-before-with-additional-observations")
    rows[1].update(before={"turn": 2}, recorded_turn_choices={"p1": "move harden", "p2": "move harden"})
    for row in rows[2:5]:
        row["scope"] = "requested"
    rows[2]["canonical_state"] = {"turn": 2}
    rows[3]["hidden"] = {"status": "ok"}
    scopes = {scope: {"positions": 0, "outcomes": 0, "errors": 0, "hidden_diagnostics": 0,
                     "enumerations_restored": 0, "outcome_rollbacks": 0,
                     "position_completions": 0, "empty_positions": 0, "status": "empty"}
              for scope in ("requested", "additional", "global")}
    scopes["requested"].update(positions=1, outcomes=1, enumerations_restored=1,
                               outcome_rollbacks=1, position_completions=1, status="ok")
    scopes["additional"].update(positions=1, errors=1, status="error")
    rows.insert(-1, {"kind": "position", "position": 1, "scope": "additional", "canonical_state": {"turn": 3}})
    rows.insert(-1, {"kind": "error", "position": 1, "scope": "additional", "stage": "decision",
                     "category": "invalid", "message": '\"move harden\": empty slot', "input_restored": True,
                     "expected_choice_rejection": True, "contract": "bb-choicelock-struggle"})
    rows[-1].update(schema_version=2, positions=2, errors=1, success_scope="requested",
                     all_positions_status="error", scopes=scopes, expected_choice_rejections=1,
                     unexpected_additional_errors=0, selection={"mode": "canonical-before", "matched_parents": 1,
                                                                "additional_parents": 1, "unclassified_parents": 0})
    return rows


class SpecialContractTests(unittest.TestCase):
    def test_only_four_provenanced_oracle_fixtures_select_special_semantics(self):
        self.assertEqual(controller.ORACLE_CONTRACTS, {
            "rr-attract-undecided-gender": "gender-mixture-v1",
            "rr-cute-charm-undecided-gender": "gender-mixture-v1",
            "rr-rivalry-undecided-gender": "gender-mixture-v1",
            "ss-redirect-tie-hidden-order": "hidden-redirect-order-v1"})
        root = controller.HERE.parents[1] / "oracle"
        for stem in controller.ORACLE_CONTRACTS:
            report = controller.read(root / "expected" / (stem + ".turn.json"))
            self.assertEqual(Path(report["scenario"]).stem, stem)
            self.assertTrue((root / "scenarios" / (stem + ".json")).is_file())
        alternate = controller.read(controller.HERE / "data/contracts/ss-redirect-tie-hidden-order-rod-a.turn.json")
        original = controller.read(root / "expected/ss-redirect-tie-hidden-order.turn.json")
        self.assertEqual(alternate["before"], original["before"])
        self.assertEqual(alternate["showdownCommit"], original["showdownCommit"])
        self.assertTrue(alternate["exact"])
        derived = controller.read(controller.HERE / "data/contracts/ss-redirect-tie-hidden-order-rod-a.json")
        source = controller.read(root / "scenarios/ss-redirect-tie-hidden-order.json")
        self.assertEqual(derived.pop("seed"), [2, 2, 3, 4])
        self.assertEqual(derived, source)
        self.assertNotEqual(alternate["outcomes"][0]["state"], original["outcomes"][0]["state"])

    def test_complete_special_contracts_pass_but_ambiguous_does_not(self):
        for contract in set(controller.ORACLE_CONTRACTS.values()):
            value = contract_output(contract)
            controller.validate_oracle_contract(value, contract)
            value["status"] = "ambiguous"
            with self.assertRaises(ValueError):
                controller.validate_oracle_contract(value, contract)

    def test_oracle_contract_missing_branch_weight_identity_or_restore_fails(self):
        for change in (lambda v: v.update(contract="other"),
                       lambda v: v["branches"].pop(),
                       lambda v: v["branches"][0].update(state_restored=False),
                       lambda v: v["branches"][0].update(normalized_weight=0.75),
                       lambda v: v["branches"][0].update(distribution={"a": float("nan")}),
                       lambda v: v["branches"][0].update(gender_assignment="F"),
                       lambda v: v["comparison"].update(matches=False)):
            value = contract_output()
            change(value)
            with self.assertRaises(ValueError):
                controller.validate_oracle_contract(value, "gender-mixture-v1")
        value = contract_output("hidden-redirect-order-v1")
        value["checks"].update(direct_oracle_histories=1, metamorphic_histories=1)
        with self.assertRaises(ValueError):
            controller.validate_oracle_contract(value, "hidden-redirect-order-v1")

    def test_requested_scope_success_keeps_additional_error_visible(self):
        result = controller.validate_probe("turn", scoped_turn_output())
        self.assertTrue(result["successful"])
        self.assertEqual(result["success_scope"], "requested")
        self.assertEqual(result["last"]["errors"], 1)
        self.assertEqual(result["coverage"]["all_positions_status"], "error")
        self.assertEqual(result["coverage"]["expected_choice_rejections"], 1)

    def test_unknown_additional_error_is_complete_but_not_successful(self):
        rows = scoped_turn_output()
        rows[-2].update(expected_choice_rejection=False, contract=None, message="unknown new engine error")
        rows[-1].update(status="error", expected_choice_rejections=0, unexpected_additional_errors=1)
        result = controller.validate_probe("turn", rows)
        self.assertTrue(result["complete"])
        self.assertFalse(result["successful"])

    def test_scope_misclassification_missing_records_and_fake_expected_errors_fail_closed(self):
        changes = [lambda r: r[2].update(canonical_state={"turn": 99}),
                   lambda r: r[-2].update(message="different error"),
                   lambda r: r[-2].update(contract="unknown-fixture"),
                   lambda r: r[-2].update(input_restored=False),
                   lambda r: r[-1]["scopes"]["additional"].update(errors=0),
                   lambda r: r[-1].update(all_positions_status="ok"),
                   lambda r: r[-1].update(expected_choice_rejections=0),
                   lambda r: r.pop(3), lambda r: r.pop(4), lambda r: r.pop()]
        for change in changes:
            rows = scoped_turn_output()
            change(rows)
            with self.assertRaises(ValueError):
                controller.validate_probe("turn", rows)


class ExtractionAndTimeoutTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def archive(self, members):
        path = self.root/'input.tar.gz'
        with tarfile.open(path, 'w:gz') as tar:
            for name, kind in members:
                info = tarfile.TarInfo(name)
                if kind == 'file':
                    info.size = 5
                    tar.addfile(info, io.BytesIO(b'bytes'))
                else:
                    info.type = tarfile.SYMTYPE if kind == 'symlink' else tarfile.LNKTYPE
                    info.linkname = '../outside'
                    tar.addfile(info)
        return path

    def test_regular_members_keep_bytes_and_destination_cannot_be_reused(self):
        archive = self.archive([('positions/a.json.gz', 'file')])
        destination = self.root/'out'
        controller.extract_checked(archive, destination)
        self.assertEqual((destination/'positions/a.json.gz').read_bytes(), b'bytes')
        with self.assertRaises(FileExistsError):
            controller.extract_checked(archive, destination)

    def test_traversal_absolute_links_and_duplicate_members_are_refused(self):
        cases = [[('../outside', 'file')], [(str(self.root/'outside'), 'file')],
                 [('link', 'symlink')], [('link', 'hardlink')],
                 [('same', 'file'), ('same', 'file')]]
        for index, members in enumerate(cases):
            with self.subTest(members=members):
                with self.assertRaises((ValueError, FileExistsError)):
                    controller.extract_checked(self.archive(members), self.root/f'out-{index}')
                self.assertFalse((self.root/'outside').exists())

    def test_timeout_kills_linux_process_group_and_waits(self):
        child = Mock(pid=32100, returncode=-9)
        child.wait.side_effect = [subprocess.TimeoutExpired('inert', 1), -9]
        fake_os = SimpleNamespace(name='posix', killpg=Mock())
        with patch.object(controller.subprocess, 'Popen', return_value=child) as start, \
             patch.object(controller, 'os', fake_os), \
             patch.object(controller, 'signal', SimpleNamespace(SIGKILL=9)):
            result = controller.bounded(['inert'], self.root/'out', self.root/'err', 1)
        self.assertEqual(result, (-9, True))
        fake_os.killpg.assert_called_once_with(32100, 9)
        self.assertEqual(child.wait.call_count, 2)
        self.assertTrue(start.call_args.kwargs['start_new_session'])


class CaseAndComparisonTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.controller_dir = self.root/'controller'
        self.controller_dir.mkdir()
        controller.write(self.controller_dir/'variants.json', {'variants': [
            {'id': 'baseline', 'compare_to': None}, {'id': 'p8g', 'compare_to': 'baseline'}]})
        self.here = patch.object(controller, 'HERE', self.controller_dir)
        self.here.start()
        self.addCleanup(self.here.stop)
        self.job = {'id': 'search/one', 'kind': 'search', 'input_id': 'one',
                    'args': ['same-input'], 'timeout': 1, 'factored': 0}
        self.plan_path = self.root/'case-plan.json'
        controller.write(self.plan_path, {'jobs': [self.job]})
        self.args = SimpleNamespace(results=self.root/'results', out=self.root/'comparison.json',
                                    plan=self.plan_path)

    def case(self, rows=None, *, variant='baseline', code=0, timeout=False, kind='search'):
        directory = self.root/'raw'/variant
        directory.mkdir(parents=True)
        job = dict(self.job, kind=kind)
        controller.write(self.plan_path, {'jobs': [job]})
        payload = b''.join((json.dumps(row)+'\n').encode() for row in
                           (rows if rows is not None else [search_output()]))
        def child(command, stdout, stderr, limit, **kwargs):
            Path(stdout).write_bytes(payload)
            Path(stderr).write_text('preserved child diagnostic', encoding='utf-8')
            return code, timeout
        with patch.object(controller, 'bounded', side_effect=child):
            return controller.run_case(job, {kind: Path('inert-never-executed')}, directory)

    def save_results(self, rows):
        plan = controller.read(self.plan_path)
        primary = plan['jobs'][0]
        add_oracle_anchor = primary['kind'] != 'oracle'
        plan['jobs'] = [primary]
        if add_oracle_anchor:
            plan['jobs'].append(dict(primary, id='oracle/anchor', kind='oracle', input_id='anchor'))
        controller.write(self.plan_path, plan)
        controller.write(self.args.results/'case-plan.json', plan)
        for variant, records in rows.items():
            records = copy.deepcopy(records)
            if add_oracle_anchor:
                records.append({'id': 'oracle/anchor', 'kind': 'oracle', 'input_id': 'anchor',
                                'status': 'match', 'complete': True, 'successful': True,
                                'verdict': {'status': 'match', 'engineMs': 1, 'tv': 0.0},
                                'sha256': 'same-oracle-output'})
            controller.write(self.args.results/variant/'results.json', {
                'schema': 1, 'variant': variant, 'plan_sha256': controller.sha(self.plan_path),
                'expected': len(plan['jobs']), 'observed': len(records),
                'activation': {'passed': True}, 'cases': records})

    def compare(self):
        with contextlib.redirect_stdout(io.StringIO()):
            code = controller.compare(self.args)
        return code, controller.read(self.args.out)

    def test_valid_fresh_cases_are_equal_success(self):
        first = self.case()
        second = self.case(variant='p8g')
        self.save_results({'baseline': [first], 'p8g': [second]})
        code, result = self.compare()
        self.assertEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['by_kind']['search']['equal_success'], 1)
        self.assertTrue(result['all_requested_successful'])

    def test_equal_structured_errors_are_reported_without_success(self):
        error = {'schema_version': 1, 'stage': 'load', 'status': 'error', 'error': 'same refusal'}
        self.save_results({'baseline': [self.case([error])],
                           'p8g': [self.case([error], variant='p8g')]})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['equal_error'], 1)
        self.assertEqual(result['comparisons']['p8g']['by_kind']['search']['equal_success'], 0)
        self.assertFalse(result['all_requested_successful'])

    def test_timeout_retains_raw_partial_output_and_is_uncompared(self):
        first = self.case()
        second = self.case(variant='p8g', code=-9, timeout=True)
        self.assertFalse(second['complete'])
        self.assertEqual(second['status'], 'timeout')
        self.assertIn('preserved child diagnostic', second['stderr'])
        raw = self.root/'raw/p8g'/second['output_file']
        self.assertEqual(json.loads(gzip.decompress(raw.read_bytes())), search_output())
        self.save_results({'baseline': [first], 'p8g': [second]})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['uncompared'], 1)

    def test_partial_turn_output_is_invalid_even_with_zero_exit(self):
        row = self.case(turn_output()[:-1], kind='turn')
        self.assertFalse(row['complete'])
        self.assertEqual(row['status'], 'invalid-output')

    def test_full_stream_difference_is_visible_even_when_last_record_is_identical(self):
        before = turn_output()
        after = copy.deepcopy(before)
        after[3]['instructions'] = '[DifferentInstruction]'
        self.save_results({'baseline': [self.case(before, kind='turn')],
                           'p8g': [self.case(after, variant='p8g', kind='turn')]})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['different'], 1)

    def test_oracle_time_only_difference_is_equal_but_probability_difference_is_not(self):
        verdict = {'status': 'match', 'engineMs': 1, 'tv': 0.0, 'maxSharedDiff': 0.0,
                   'scenario': 'same', 'report': 'same', 'onlyEngine': 0, 'onlyOracle': 0}
        first = self.case([verdict], kind='oracle')
        second = self.case([dict(verdict, engineMs=100)], variant='p8g', kind='oracle')
        self.save_results({'baseline': [first], 'p8g': [second]})
        code, result = self.compare()
        self.assertEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['equal_success'], 1)
        second['verdict']['maxSharedDiff'] = 1e-12
        self.save_results({'baseline': [first], 'p8g': [second]})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['different'], 1)

    def test_missing_id_is_kept_in_denominator_and_written_before_failure(self):
        self.save_results({'baseline': [self.case()], 'p8g': []})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['uncompared'], 1)
        self.assertFalse(result['differential_complete'])

    def test_duplicate_case_identity_cannot_collapse_into_success(self):
        first = self.case()
        second = self.case(variant='p8g')
        self.save_results({'baseline': [first], 'p8g': [second, copy.deepcopy(second)]})
        try:
            code, result = self.compare()
        except (ValueError, AssertionError):
            return
        self.assertNotEqual(code, 0)
        self.assertFalse(result['all_requested_successful'])

    def test_equal_oracle_engine_errors_are_not_a_supported_success(self):
        error = {'status': 'engine-error', 'engineMs': 7, 'error': 'same unsupported history'}
        self.save_results({'baseline': [self.case([error], code=1, kind='oracle')],
                           'p8g': [self.case([error], variant='p8g', code=1, kind='oracle')]})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertEqual(result['comparisons']['p8g']['equal_error'], 1)
        self.assertFalse(result['oracle_all_match'])
        self.assertFalse(result['all_requested_successful'])

    def test_success_text_with_nonzero_exit_is_not_completed(self):
        row = self.case(code=1)
        self.assertFalse(row['complete'])
        oracle = self.case([{'status': 'match', 'engineMs': 1}],
                           variant='other', code=1, kind='oracle')
        self.assertFalse(oracle['complete'])

    def test_missing_or_conflicting_frozen_plan_fails_closed(self):
        first, second = self.case(), self.case(variant='p8g')
        self.save_results({'baseline': [first], 'p8g': [second]})
        (self.args.results/'case-plan.json').unlink()
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertTrue(result['validation_errors'])
        self.assertFalse(result['complete'])
        self.save_results({'baseline': [first], 'p8g': [second]})
        controller.write(self.args.results/'conflict/case-plan.json', {'jobs': []})
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertTrue(result['validation_errors'])

    def test_equal_outputs_without_activation_evidence_cannot_pass_final_gate(self):
        self.save_results({'baseline': [self.case()], 'p8g': [self.case(variant='p8g')]})
        path = self.args.results/'p8g/results.json'
        result = controller.read(path)
        result['activation'] = {'passed': False}
        controller.write(path, result)
        code, result = self.compare()
        self.assertNotEqual(code, 0)
        self.assertTrue(result['differential_complete'])
        self.assertFalse(result['activation_passed'])


if __name__ == '__main__':
    unittest.main()
