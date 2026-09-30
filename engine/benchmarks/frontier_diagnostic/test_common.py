import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import ci
import common as p
from common import c
import test_contract as fixture

class Contracts(unittest.TestCase):
    def test_unbound_source_fails_closed(self):
        with patch.object(c,'strict_json',return_value={'source_sha':'UNBOUND_P17_SOURCE_SHA'}):
            with self.assertRaises(ValueError):p.binding()
    def test_only_four_explicit_timing_fields_can_differ(self):
        left=fixture.value();right=copy.deepcopy(left)
        right['reference']['kernel_ns']+=1;right['reference']['metric_prepare_ns']+=1
        for sample in right['samples']:sample['kernel_ns']+=1;sample['metric_ns']+=1
        self.assertEqual(p.semantic_result(left,fixture.plan(),fixture.CASE),p.semantic_result(right,fixture.plan(),fixture.CASE))
        right['reference']['components']=2;right['reference']['flat_count_upper_bound']=2
        self.assertNotEqual(p.semantic_result(left,fixture.plan(),fixture.CASE),p.semantic_result(right,fixture.plan(),fixture.CASE))
        right=fixture.value();right['description']['full_state_debug']='changed'
        with self.assertRaises(ValueError):p.semantic_result(right,fixture.plan(),fixture.CASE)
    def test_exact_features_missing_test_lib_and_unknown_flags(self):
        for arm,fault in (('off',None),('on',None),('on','missing_unit'),('off','observer'),('on','unknown')):
            with self.subTest(arm=arm,fault=fault),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);target=root/('target-p17-'+arm)/'release/.fingerprint'
                for package,names,actual in (('lab-engine',['lib-lab_engine.json']+(['test-lib-lab_engine.json'] if arm=='on' else []),p.features(arm=='on')),
                    ('lab-scenario',['lib-lab_scenario.json','test-bin-lab-distribution-bench.json','bin-lab-distribution-bench.json'],[])):
                    folder=target/(package+'-mock');folder.mkdir(parents=True)
                    for name in names:
                        if fault=='missing_unit' and name=='test-lib-lab_engine.json':continue
                        features=actual+([p.FEATURE] if fault=='observer' else ['experiment-unknown'] if fault=='unknown' else [])
                        (folder/name).write_text(json.dumps({'features':json.dumps(features),'rustflags':['-Ctarget-cpu=x86-64']}))
                if fault:
                    with self.assertRaises(ValueError):ci.fingerprints(root,arm,arm=='on')
                else:self.assertEqual(len(ci.fingerprints(root,arm,arm=='on')),5 if arm=='on' else 4)
    def test_frozen_corpus_and_both_control_and_failure_plans(self):
        controller=Path(__file__).resolve().parents[3]
        manifest=c.corpus(controller)
        for case_id,sha in p.PLAN_SHA.items():
            plan=Path(__file__).resolve().parent/(case_id+'.plan.json')
            self.assertEqual(c.sha(plan),sha)
            c.description(c.line(plan),manifest['cases'][int(case_id.rsplit('-',1)[1])])
if __name__=='__main__':unittest.main()
