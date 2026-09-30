"""Synthetic fail-closed schema and resource/summary tests; no engine execution."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import contract as c
import ci
import run_distribution as run

CASE={'id':'opening-0000','joint_seed':7,'sample_seeds':[10,20,30,40,50]}
def plan():
    return dict(schema=1,kind='frozen-opening-selection',joint_seed=7,rng='fixed',position_order='fixed',
        joint_policy='fixed',ruleset='CHAMPIONS_MC',position_count=1,position_index=0,position_probability=1,
        position_probability_bits=4607182418800017408,position_total_mass=1,eligible_joint_counts=[1,1],
        joint_indices=[0,0],full_state_debug='complete state',party_order_debug='order',choices_debug='actions',
        choices=[[dict(move_index=0,target=0,gimmick='None') for _ in range(2)] for _ in range(2)])
def value():
    metric=dict(coverage=1,tv=0,outside_reference_mass=0,unique_states=1)
    return dict(schema=1,status='ok',description=plan(),metric_schema='full State plus Suspension',
        reference=dict(method='factored-full-exact',kernel_ns=100,metric_prepare_ns=50,components=1,total_mass=1,
            tv_bound=0,flat_count_upper_bound=1,full_support_materialized=False,suspended_components=0),
        non_hp_state_posthoc_top32=dict(projection='non_hp_state',support_size=1,retained_states=1,retained_mass=1,
            omitted_mass=0,renormalized_tv=0,method='posthoc',timing_speedup_claim=False),
        samples=[dict(count=count,seed=seed,kernel_ns=50,metric_ns=30,raw_outcomes=1,sample_total_mass=1,
            unique_full_states=1,full_state=copy.deepcopy(metric),non_hp_state=copy.deepcopy(metric))
            for count in c.SAMPLE_COUNTS for seed in CASE['sample_seeds']],all_state_restored=True,
        all_sample_outcomes_in_reference=True,timing_scope='API only',suspension_scope='first pause',
        sample_policy='seed prefixes',probability_policy='1e-9 tolerance and normalization',execution_policy='no warmup')
def records():
    return [dict(id=f'opening-{i:04d}',description={'status':'ok'},
        measurement={'status':'ok' if i<c.COUNT else 'not_selected','process':{'wall_seconds':2}},
        **({'value':value()} if i<c.COUNT else {})) for i in range(500)]

class ContractTests(unittest.TestCase):
    def test_complete_result(self):c.result(value(),plan(),CASE)
    def test_missing_fields_errors_and_raw_unsupported_states_fail(self):
        mutations=[lambda v:v.pop('probability_policy'),lambda v:v.update(status='error'),
            lambda v:v.update(all_state_restored=False),lambda v:v.update(all_sample_outcomes_in_reference=False),
            lambda v:v['samples'].pop(),lambda v:v['samples'].reverse(),
            lambda v:v['reference'].update(tv_bound=.01),lambda v:v['reference'].update(total_mass=.9),
            lambda v:v['reference'].update(full_support_materialized=True),
            lambda v:v['samples'][0].update(seed=CASE['sample_seeds'][1]),
            lambda v:v['samples'][0].update(kernel_ns=True),lambda v:v['samples'][0].update(kernel_ns=float('inf')),
            lambda v:v['samples'][0]['full_state'].update(outside_reference_mass=.01),
            lambda v:v['samples'][0]['non_hp_state'].update(tv=.1),
            lambda v:v['non_hp_state_posthoc_top32'].update(timing_speedup_claim=True),
            lambda v:v['non_hp_state_posthoc_top32'].update(retained_mass=.5),
            lambda v:v['description'].update(full_state_debug='changed')]
        for change in mutations:
            data=value();change(data)
            with self.subTest(mutation=change),self.assertRaises(ValueError):c.result(data,plan(),CASE)
    def test_description_seed_position_move_and_mega_contract(self):
        p=plan();p['choices'][0][0]['gimmick']='Mega';c.description(p,CASE)
        for change in (lambda v:v.update(joint_seed=8),lambda v:v.update(position_count=0),
                       lambda v:v.update(joint_indices=[1,0]),lambda v:v['choices'][0][0].update(gimmick='Tera'),
                       lambda v:v['choices'][0][0].update(move_index=4),lambda v:v.pop('full_state_debug')):
            p=plan();change(p)
            with self.assertRaises(ValueError):c.description(p,CASE)
    def test_json_duplicate_nonfinite_and_incomplete_lines(self):
        for raw in ('{"a":1,"a":2}','{"a":NaN}','{"a":Infinity}'):
            with self.assertRaises(ValueError):c.strict_json(raw)
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)/'out'
            for raw in (b'{}',b'{}\r\n',b'{}\n{}\n'):
                p.write_bytes(raw)
                with self.assertRaises(ValueError):c.line(p)
    def test_actual_frozen_corpus_and_ordered_prefix(self):
        controller=Path(__file__).resolve().parents[3]
        doc=c.corpus(controller)
        self.assertEqual([r['id'] for r in doc['cases'][:16]],[f'opening-{i:04d}' for i in range(16)])
    def test_safe_paths_cannot_escape_controller(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for path in ('../secret','/absolute','C:/outside','a\\b'):
                with self.assertRaises(ValueError):c.safe_file(root,path)

class SummaryTests(unittest.TestCase):
    def test_only_complete500_has_full500_metrics(self):
        s=run.summary(records());self.assertTrue(s['complete']);self.assertEqual(s['requested_cases'],500)
        self.assertEqual(s['requested_descriptions'],500);self.assertEqual(s['corpus_cases'],500)
        self.assertEqual(s['official_metrics']['samples']['16']['kernel_ns_opening_mean']['n'],500)
        self.assertEqual(s['process_measurement_wall_seconds_sum'],1000)
        partial=records()
        for row in partial[16:]:row['measurement']={'status':'not_selected'}
        self.assertFalse(run.summary(partial)['complete'])
        self.assertIsNone(run.summary(partial)['official_metrics'])
    def test_any_plan_or_measurement_failure_forbids_complete_metrics(self):
        for where,index,status in (('description',499,'timeout'),('measurement',499,'rss_limit'),
                                   ('measurement',0,'nonzero_exit'),('measurement',0,'pending')):
            data=records();data[index][where]['status']=status;s=run.summary(data)
            self.assertFalse(s['complete']);self.assertIsNone(s['official_metrics'])
    def test_no_case_reordering_or_omission(self):
        data=records();data.reverse()
        with self.assertRaises(ValueError):run.summary(data)
        data=records();data.pop()
        with self.assertRaises(ValueError):run.summary(data)

class FeatureTests(unittest.TestCase):
    def test_actual_core6_scenario0_and_missing_or_rogue_fingerprints(self):
        for fault in (None,'observer','missing','search'):
            with tempfile.TemporaryDirectory() as directory:
                root=Path(directory);fp=root/'target-distribution/release/.fingerprint'
                for package,names,features in (('lab-engine',['lib-lab_engine.json'],list(c.CORE_FEATURES)),
                    ('lab-scenario',['lib-lab_scenario.json','test-bin-lab-distribution-bench.json','bin-lab-distribution-bench.json'],[])):
                    path=fp/(package+'-test');path.mkdir(parents=True)
                    for name in names:
                        if fault=='missing' and name.startswith('test-bin'):continue
                        actual=features+(['experiment-nash-scratch-observer'] if fault=='observer' else [])
                        (path/name).write_text(json.dumps({'features':json.dumps(actual)}))
                if fault=='search':(fp/'lab-search-rogue').mkdir()
                if fault:
                    with self.assertRaises(ValueError):ci.fingerprints(root)
                else:self.assertEqual(len(ci.fingerprints(root)),4)

if __name__=='__main__':unittest.main()
