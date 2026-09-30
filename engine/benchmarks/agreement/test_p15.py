"""Synthetic fail-closed P15 controller checks, never engine accuracy claims."""
import contextlib
import copy
import io
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import p15_contract as c
import test_p8def_combined as old

controller = old.controller
def plan():
    value=old.plan()
    controls=[job for job in value['jobs'] if job['kind']=='search'
              and job['args'][1]=='deep' and job['args'][5]=='1'][:8]
    for job,key in zip(controls,c.CONTROL_IDS):job['id']=key
    return value


def variants():
    return {'schema': 1, 'variants': [
        {'id': 'base', 'sha': c.SOURCE_SHA, 'features': sorted(c.BASE_FEATURES), 'compare_to': None},
        {'id': c.CANDIDATE, 'sha': c.SOURCE_SHA, 'features': sorted(c.FEATURES),
         'compare_to': 'base', 'comparison_contract': 'exact-v1'}]}


def summary():
    value = old.summary()
    for name in ('oracle', 'oracle_contracts', 'scoped_turn_coverage', 'activation'):
        value[name][c.CANDIDATE] = value[name].pop(old.c.CANDIDATE)
    comp = value['comparisons'].pop(old.c.CANDIDATE)
    comp.update(comparison_contract='exact-v1', raw_turn_different_ids=[], raw_turn_differences=[])
    value['comparisons'][c.CANDIDATE] = comp
    for side in ('base',c.CANDIDATE):
        prepared=value['activation'][side]['prepared']
        prepared['expected_control_ids']=list(c.CONTROL_IDS)
        for row,key in zip(prepared['controls'],c.CONTROL_IDS):row['id']=key
    activation=value['activation'][c.CANDIDATE]
    prepared=activation['prepared']
    prepared.update(purpose='prepared-leaf-sharing',contract=c.PRECEDENCE_CONTRACT)
    for row in prepared['controls']:
        row.update(on_stdout_sha256='a'*64,off_stdout_sha256='a'*64,on_stdout_bytes=400,off_stdout_bytes=400,
                   on_parent_checks=1,off_parent_checks=2)
        for side,count in (('on',1),('off',2)):
            row[side+'_metadata']['prepared']=dict.fromkeys(c.VALIDATORS,count)
            row[side+'_metadata']['leaf'].update(materialized_outcomes=2,emitted_instructions=0)
    return value


class P15Tests(unittest.TestCase):
    def test_versioned_precedence_rejects_zero_activation_more_work_or_fake_evidence(self):
        c.validate_prepared_leaf_precedence(summary()['activation'][c.CANDIDATE]['prepared'])
        def no_reduction(p):
            for row in p['controls']:
                row['on_parent_checks']=row['off_parent_checks']
                row['on_metadata']['prepared']=copy.deepcopy(row['off_metadata']['prepared'])
        changes=[no_reduction, lambda p:p['controls'].pop(),
                 lambda p:p['controls'].__setitem__(1,copy.deepcopy(p['controls'][0])),
                 lambda p:p['controls'].reverse(),
                 lambda p:p.update(expected_control_ids=['other/'+str(i) for i in range(8)]),
                 lambda p:p['controls'][0].update(on_stdout_sha256='b'*64),
                 lambda p:p['controls'][0].update(on_stdout_bytes=401),
                 lambda p:p['controls'][0]['on_metadata']['leaf'].update(visits=99),
                 lambda p:p['controls'][0]['on_metadata']['prepared'].update(side_checks=99),
                 lambda p:p['controls'][0]['on_metadata']['prepared'].update(support_checks=0),
                 lambda p:p['controls'][0]['on_metadata'].update(prepared_observer_compiled=False),
                 lambda p:p['controls'][0]['on_metadata'].update(prepared_requested=False),
                 lambda p:p['controls'][0]['on_metadata']['leaf'].pop('materialized_outcomes'),
                 lambda p:p.update(contract='legacy-allowed')]
        for change in changes:
            prepared=summary()['activation'][c.CANDIDATE]['prepared'];change(prepared)
            with self.assertRaises(ValueError):c.validate_prepared_leaf_precedence(prepared)

    def test_actual_activation_dispatch_preserves_old_contract_and_checks_new_candidate(self):
        fixture=old.test_combined.CombinedContractTests();fixture.setUp()
        jobs=fixture.jobs
        mapping={f'deep/{i}':key for i,key in enumerate(c.CONTROL_IDS)}
        for row in jobs+fixture.records:row['id']=mapping.get(row['id'],row['id'])
        def metadata(requested,count):
            value=fixture.metadata(prepared=requested,count=count)
            value['prepared']={'parent_checks':count,'side_checks':count*2,'support_checks':count}
            value['leaf'].update(materialized_outcomes=2,emitted_instructions=0)
            return value
        for candidate,on_count,off_count,expected in ((True,2,5,True),(True,2,2,False),(True,6,5,False),(False,2,2,True)):
            variant=variants()['variants'][int(candidate)]
            rows=[dict(row,sha256='a'*64,stdout_bytes=400,stderr=json.dumps(metadata(True,on_count))) for row in fixture.records]
            def off(job,binaries,result_dir):
                self.assertEqual(job['args'][-2:],['--prepared','off'])
                return dict(complete=True,successful=True,sha256='a'*64,stdout_bytes=400,stderr=json.dumps(metadata(False,off_count)))
            with patch.object(controller,'run_case',side_effect=off),patch.object(controller,'run_nonleaf_activation',return_value=old.test_combined.activation_receipt()):
                actual=controller.activation_checks(variant,rows,jobs,{},Path('.'))
            self.assertEqual(actual['passed'],expected)
            if candidate and expected:
                c.validate_candidate_activation(actual)
                self.assertEqual(actual['prepared']['contract'],c.PRECEDENCE_CONTRACT)
            if not candidate:
                c.previous.validate_precedence(actual['prepared'])
                self.assertNotIn('contract',actual['prepared'])

    def test_real_variants_exact_feature_difference_and_contract_selection(self):
        v = json.loads(Path(__file__).with_name('p15-variants.json').read_text())
        c.validate_variants(v)
        self.assertIs(controller.experiment_contract(v), c)
        core0, search0 = controller.expected_features(v['variants'][0])
        core1, search1 = controller.expected_features(v['variants'][1])
        self.assertEqual(core1 - core0, {'experiment-prepared-leaf'})
        self.assertEqual(search1 - search0, {'experiment-prepared-leaf'})
        self.assertIn('experiment-borrowed-child-keys',core0)
        self.assertIn('experiment-borrowed-child-keys',search0)
        self.assertNotIn('experiment-slot-diff', core1)
        self.assertNotIn('experiment-stats-off-cost', core1)
        with patch.dict(os.environ, {'AGREEMENT_VARIANTS_FILE': 'p15-variants.json'}):
            self.assertEqual(controller.variant(c.CANDIDATE), v['variants'][1])

    def test_wrong_source_baseline_features_duplicates_or_relaxed_contract_rejected(self):
        mutations = [lambda v: v['variants'].reverse(), lambda v: v['variants'].pop(),
                     lambda v: v['variants'][1].update(compare_to='p8d'),
                     lambda v: v['variants'][1].update(comparison_contract=old.c.STATE_CONTRACT)]
        for index in (0, 1):
            mutations += [lambda v, i=index: v['variants'][i].update(sha=old.c.SOURCE_SHA),
                          lambda v, i=index: v['variants'][i]['features'].pop(),
                          lambda v, i=index: v['variants'][i]['features'].append(v['variants'][i]['features'][0])]
            for flag in ('slot-diff', 'stats-off-cost', 'prepared-leaf-observer', 'inline-runstart', 'volatile-hash-update', 'matrix-pass-through'):
                mutations += [lambda v, i=index, f=flag: v['variants'][i]['features'].append('lab-engine/experiment-' + f)]
        for mutation in mutations:
            value = variants(); mutation(value)
            with self.assertRaises(ValueError): c.validate_variants(value)

    def test_raw_instructions_and_every_other_output_bit_remain_exact(self):
        variant = variants()['variants'][1]
        original = {'sha256': 'a'*64, 'stdout_bytes': 120, 'turn_state_sha256': 'c'*64}
        for kind in ('turn', 'search'):
            self.assertTrue(controller.comparison_equal(variant, kind, original, original))
            for changed in (dict(original, sha256='b'*64), dict(original, stdout_bytes=121)):
                self.assertFalse(controller.comparison_equal(variant, kind, changed, original))
            for changed in ({'sha256': 'a'*64}, dict(original, stdout_bytes=True), dict(original, sha256='invalid')):
                with self.assertRaises(ValueError): controller.comparison_equal(variant, kind, changed, original)

    def test_no_denominator_error_omission_raw_difference_or_activation_relaxation(self):
        c.validate_plan(plan()); c.validate_summary(summary())
        mutations = [lambda s: s.update(expected_cases=6183),
                     lambda s: s['comparisons'][c.CANDIDATE].update(equal_error=1),
                     lambda s: s['comparisons'][c.CANDIDATE].update(uncompared=1),
                     lambda s: s['comparisons'][c.CANDIDATE].update(raw_turn_different_ids=['turn/x']),
                     lambda s: s['comparisons'][c.CANDIDATE].update(raw_turn_differences=[{}]),
                     lambda s: s['oracle']['base'].update(match=3055)]
        for side in ('base', c.CANDIDATE):
            mutations += [lambda s, v=side: s['activation'][v].pop('prepared_nonleaf'),
                          lambda s, v=side: s['activation'][v].update(leaf_required=False)]
        for mutation in mutations:
            value = summary(); mutation(value)
            with self.assertRaises(ValueError): c.validate_summary(value)

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
            self.assertEqual(comparison['raw_turn_different_ids'], [])
            self.assertEqual(comparison['raw_turn_differences'], [])
            candidate_path = root / c.CANDIDATE / 'results.json'
            candidate = controller.read(candidate_path)
            next(row for row in candidate['cases'] if row['kind'] == 'turn')['sha256'] = 'b' * 64
            controller.write(candidate_path, candidate)
            with patch.object(controller, 'HERE', root), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(controller.compare(args), 1)
            self.assertEqual(controller.read(args.out)['comparisons'][c.CANDIDATE]['different'], 1)
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
