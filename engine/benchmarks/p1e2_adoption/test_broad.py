import copy,json,tempfile,unittest
from pathlib import Path
import compare_broad as b

class BroadRecordsTests(unittest.TestCase):
    def fixtures(self):
        manifest={'schema':2,'cases':[],'case_count':2,'distinct_input_count':2,'smoke_count':1}
        rows=[]
        for index in range(2):
            meta={'case':'case-'+str(index),'origin':'derived:fixture','rolls':'Extremes','resume_policy':'stop-at-first-pause',
              'mid_turn':[[],[]],'smoke':index==0,'input_hash':index,'decision':'Turn('+str(index)+')','required_activation':['target-damaged']}
            manifest['cases'].append(meta)
            rows.append({'schema':2,**{k:meta[k] for k in ('case','origin','rolls','resume_policy','mid_turn','input_hash','decision')},
              'input':'State { input: '+str(index)+' }','activation':{'target-damaged':True},'flat_factored_full_state_agreement':True,
              'sample_scope':b.SAMPLE_SCOPE,'distribution':{'State { hidden: 1 }\nNone':.5,'State { hidden: 2 }\nSome(pending)':.5},
              'samples':{'State { hidden: 1 }\nNone':1.}})
        return manifest,rows
    def compare(self,left,right,manifest):
        with tempfile.TemporaryDirectory() as temporary:
            paths=[Path(temporary)/'baseline.jsonl',Path(temporary)/'candidate.jsonl']
            for path,rows in zip(paths,(left,right)):
                path.write_bytes(''.join(json.dumps(row)+'\n' for row in rows).encode())
            return b.compare_records(*paths,manifest)
    def test_complete_full_state_probability_roundoff_and_extra_negative_control(self):
        manifest,left=self.fixtures();right=copy.deepcopy(left)
        for rows in (left,right):rows[0]['activation']['below-half-no-berserk']=True
        right[0]['distribution']['State { hidden: 1 }\nNone']+=1e-14
        right[0]['distribution']['State { hidden: 2 }\nSome(pending)']-=1e-14
        value=self.compare(left,right,manifest)
        self.assertTrue(value['passed']);self.assertEqual(value['distinct_input_count'],2)
    def test_smoke_missing_extra_reordered_case_rejected(self):
        manifest,left=self.fixtures()
        for right in (left[:1],left+left[:1],list(reversed(left))):
            with self.assertRaises(ValueError):self.compare(left,right,manifest)
    def test_activation_vacuity_changed_input_and_resume_rejected(self):
        manifest,left=self.fixtures()
        edits=[lambda r:r['activation'].clear(),lambda r:r['activation'].update({'target-damaged':False}),
               lambda r:r.update(input='State { changed }'),lambda r:r.update(mid_turn=[['changed'],[]]),
               lambda r:r.update(flat_factored_full_state_agreement=False)]
        for edit in edits:
            right=copy.deepcopy(left);edit(right[0])
            with self.assertRaises(ValueError):self.compare(left,right,manifest)
    def test_hidden_pending_keys_probability_mass_and_sample_drift_rejected(self):
        manifest,left=self.fixtures()
        edits=[lambda r:r['distribution'].update({'State { hidden: 3 }\nNone':r['distribution'].pop('State { hidden: 1 }\nNone')}),
               lambda r:r['distribution'].update({'State { hidden: 1 }\nSome(new)':r['distribution'].pop('State { hidden: 1 }\nNone')}),
               lambda r:r['distribution'].update({'State { hidden: 1 }\nNone':.6,'State { hidden: 2 }\nSome(pending)':.4}),
               lambda r:r['distribution'].update({'State { hidden: 1 }\nNone':.4}),
               lambda r:r.update(samples={'State { hidden: 2 }\nSome(pending)':1.})]
        for edit in edits:
            right=copy.deepcopy(left);edit(right[0])
            with self.assertRaises(ValueError):self.compare(left,right,manifest)
    def test_duplicate_nonfinite_and_vacuous_manifest_rejected(self):
        for raw in ('{"a":1,"a":2}','{"a":NaN}'):
            with self.assertRaises(ValueError):b.strict(raw)
        manifest,left=self.fixtures();manifest['cases'][0]['required_activation']=[]
        with self.assertRaises(ValueError):self.compare(left,left,manifest)
    def test_optional_false_activation_not_reported_verified(self):
        manifest,rows=self.fixtures();rows[0]['activation']['optional-unseen']=False
        value=self.compare(rows,rows,manifest)
        self.assertNotIn('optional-unseen',value['verified_activation_labels'])
    def test_small_point_errors_cannot_hide_large_accumulated_tv(self):
        manifest,left=self.fixtures();right=copy.deepcopy(left)
        for rows in (left,right):rows[0]['distribution']={'State { i: '+str(i)+' }\nNone':1/4000 for i in range(4000)}
        for i,key in enumerate(right[0]['distribution']):right[0]['distribution'][key]+=(1 if i<2000 else -1)*9e-13
        with self.assertRaisesRegex(ValueError,'total variation'):self.compare(left,right,manifest)

if __name__=='__main__':unittest.main()
