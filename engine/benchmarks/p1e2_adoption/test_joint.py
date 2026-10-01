import copy,json,math,tempfile,unittest
from pathlib import Path
import compare_joint as j

class JointExportTests(unittest.TestCase):
    def write(self,root,rows=None,entries=None,edit=None):
        root.mkdir()
        entries=entries or ['State { hidden: 1, hp: 0 }\nNone']
        rows=rows or [(0,(10,)*12,.5),(0,(20,)*12,.5)]
        data=b''.join(j.ROW.pack(identifier,*hp,p) for identifier,hp,p in rows)
        plan=b'{"frozen":1}\n'
        dictionary=(json.dumps({'schema':1,'kind':'full-non-hp-state-suspension-dictionary','party_lengths':[6,6],'entries':entries})+'\n').encode()
        manifest={'schema':1,'kind':'exact-joint-hp-export','status':'complete',**j.TEXT_CONTRACT,
          'dictionary':'dictionary.json','joint':'joint.bin','selection':'selection.json','party_lengths':[6,6],
          'hp_unit_count':12,'record_bytes':36,'dictionary_entries':len(entries),'components':max(len(rows),len(entries)),
          'suspended_components':0,'unique_joint_rows':len(rows),'flat_count_upper_bound':len(rows),
          'component_mass':1.,'joint_mass':1.,'tv_bound':0.,'joint_bytes':len(data),'dictionary_bytes':len(dictionary),
          'selection_bytes':len(plan),'payload_bytes_excluding_manifest':len(data)+len(dictionary)+len(plan),
          'limits':{'max_rows':j.MAX_ROWS,'max_bytes_including_manifest':j.MAX_BYTES},
          'all_state_restored':True,'all_lazy_tags_clear':True,'actual_eq_hash_dictionary':True,'debug_injective_on_observed_keys':True,
          'kernel_ns':1,'prepare_ns':1,'export_ns_before_manifest':1}
        if edit:edit(manifest)
        (root/'manifest.json').write_bytes((json.dumps(manifest)+'\n').encode())
        (root/'dictionary.json').write_bytes(dictionary);(root/'selection.json').write_bytes(plan);(root/'joint.bin').write_bytes(data)
        return plan
    def compare(self,a_rows=None,b_rows=None,a_entries=None,b_entries=None,edit=None):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            plan=self.write(root/'original',a_rows,a_entries)
            self.write(root/'on',b_rows,b_entries,edit)
            return j.compare_exports(root/'original',root/'on',plan)
    def test_complete_joint_support_and_tiny_raw_roundoff(self):
        rows=[(0,(10,)*12,.5+1e-14),(0,(20,)*12,.5-1e-14)]
        value=self.compare(b_rows=rows)
        self.assertTrue(value['passed']);self.assertEqual(value['unique_joint_rows'],2)
        self.assertLess(value['normalized_tv'],1e-12);self.assertFalse(value['full500_complete'])
    def test_last_reserve_and_same_marginals_different_correlation_reject(self):
        a=(10,)*12;b=(20,)+(10,)*10+(20,)
        c=(10,)*11+(20,);d=(20,)+(10,)*11
        with self.assertRaises(ValueError):self.compare(a_rows=[(0,a,.5),(0,b,.5)],b_rows=[(0,c,.5),(0,d,.5)])
    def test_hidden_non_hp_state_and_suspension_changes_reject(self):
        for entry in ('State { hidden: 2, hp: 0 }\nNone','State { hidden: 1, hp: 0 }\nSome(pending)'):
            with self.assertRaises(ValueError):self.compare(b_entries=[entry])
    def test_bad_probability_mass_order_duplicates_and_dictionary_ids_reject(self):
        choices=[[(0,(10,)*12,.6),(0,(20,)*12,.4)],
          [(0,(10,)*12,.4),(0,(20,)*12,.4)],[(0,(10,)*12,0.),(0,(20,)*12,1.)],
          [(0,(10,)*12,float('nan')),(0,(20,)*12,.5)],[(0,(20,)*12,.5),(0,(10,)*12,.5)],
          [(0,(10,)*12,.5),(0,(10,)*12,.5)],[(1,(10,)*12,.5),(1,(20,)*12,.5)]]
        for rows in choices:
            with self.assertRaises(ValueError):self.compare(b_rows=rows)
    def test_per_key_tolerance_does_not_hide_accumulated_tv(self):
        a=[(0,(i,)+(10,)*11,1/4000) for i in range(4000)]
        b=[(identifier,hp,p+(9e-13 if i<2000 else -9e-13)) for i,(identifier,hp,p) in enumerate(a)]
        with self.assertRaisesRegex(ValueError,'total variation'):self.compare(a_rows=a,b_rows=b)
    def test_incomplete_truncated_plan_and_manifest_proof_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);plan=self.write(root/'out')
            with self.assertRaises(ValueError):j.read_export(root/'out',b'changed\n')
            file=root/'out/joint.bin';file.write_bytes(file.read_bytes()[:-1])
            with self.assertRaises(ValueError):j.read_export(root/'out',plan)
        for edit in (lambda v:v.update(status='partial'),lambda v:v.update(all_state_restored=False),
                     lambda v:v.update(tv_bound=.1),lambda v:v.update(hp_unit_count=4),
                     lambda v:v.update(dictionary='../secret')):
            with self.assertRaises(ValueError):self.compare(edit=edit)

if __name__=='__main__':unittest.main()
